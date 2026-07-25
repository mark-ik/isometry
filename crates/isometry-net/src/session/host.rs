//! The DM's authority: the session that owns the truth.
//!
//! Holds the snapshot, the campaign store, and the append-only history. Every
//! commit path funnels through here so there is one place where an event
//! becomes ordered fact.
//!
//! Split out of `session.rs` on 2026-07-24; behavior unchanged.

use super::*;

/// The host's authoritative session. Owns the canonical state and the
/// ordered log; validates every intent before it becomes `Applied`.
pub struct HostSession {
    pub(super) state: GameSnapshot,
    /// The host-private GM layer. It never enters a public snapshot or event.
    pub(super) campaign: CampaignStore,
    /// The durable, append-only authority history. The public snapshot is a
    /// materialized view of this log; checkpoints keep both for fast restore.
    pub(super) history: Codicil<GameEvent>,
    /// Count of applied events; also the seq stamped on the next one.
    pub(super) seq: u64,
    pub(super) log_hash: u64,
    /// Player name each connected peer announced (via `Hello`), so the
    /// DM can whisper by name.
    pub(super) peer_names: HashMap<PeerId, String>,
    /// Client action requests awaiting adjudication. They sit here because this
    /// crate is deliberately rules-blind: it can validate that you own the token
    /// you are swinging, but it has no `System` and so cannot say whether you
    /// hit. The host *app* drains these, resolves them with its rules plugin, and
    /// commits the outcome back through `local_event`.
    pub(super) pending_actions: Vec<ActionIntent>,
}

impl HostSession {
    pub fn new(state: GameSnapshot) -> Self {
        Self::with_campaign(state, CampaignStore::new())
    }

    /// Restore a host from its public session state and private GM state.
    pub fn with_campaign(state: GameSnapshot, campaign: CampaignStore) -> Self {
        Self::with_history(state, campaign, Codicil::new())
    }

    /// Restore a host from its materialized state and ordered Codicil history.
    /// Sequence and convergence hash derive from the log rather than from a
    /// separately persisted counter.
    pub fn with_history(
        state: GameSnapshot,
        campaign: CampaignStore,
        history: Codicil<GameEvent>,
    ) -> Self {
        let mut log_hash = FNV_OFFSET;
        for (index, event) in history.entries().iter().enumerate() {
            log_hash = fold_event(log_hash, index as u64 + 1, event);
        }
        Self {
            state,
            campaign,
            seq: history.len() as u64,
            log_hash,
            history,
            peer_names: HashMap::new(),
            pending_actions: Vec::new(),
        }
    }

    /// Drain the client action requests awaiting adjudication.
    ///
    /// The host app calls this, resolves each with its rules system, and commits
    /// the outcome. Nothing here has been decided: these are asks.
    pub fn take_action_intents(&mut self) -> Vec<ActionIntent> {
        std::mem::take(&mut self.pending_actions)
    }

    pub fn state(&self) -> &GameSnapshot {
        &self.state
    }

    pub fn campaign(&self) -> &CampaignStore {
        &self.campaign
    }

    pub fn campaign_mut(&mut self) -> &mut CampaignStore {
        &mut self.campaign
    }

    pub fn history(&self) -> &Codicil<GameEvent> {
        &self.history
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn log_hash(&self) -> u64 {
        self.log_hash
    }

    /// A peer connected: hand it the current snapshot so it starts from
    /// this exact state, then the `Applied` tail carries it forward.
    /// Because snapshot + tail share the same `seq`, a late joiner and an
    /// original peer converge.
    pub fn on_connect(&self, peer: PeerId) -> Vec<Outbound> {
        vec![(
            Recipient::One(peer),
            NetMessage::Snapshot {
                seq: self.seq,
                log_hash: self.log_hash,
                state: self.state.clone(),
            },
        )]
    }

    /// The host itself proposes an event (the DM plays too). Validates
    /// and, on success, returns the broadcast to every peer.
    pub fn local_event(&mut self, event: GameEvent) -> Vec<Outbound> {
        self.commit(event)
    }

    /// Commit a secret reveal without losing it if the public fact is rejected.
    /// A crash while pending is recovered with [`Self::reconcile_pending_reveals`].
    pub fn reveal_secret(&mut self, id: &str) -> Result<Vec<Outbound>, String> {
        let fact = self
            .campaign
            .begin_reveal(id)
            .ok_or_else(|| format!("unknown or pending campaign secret: {id}"))?;
        match self.try_commit(GameEvent::Fact(fact)) {
            Ok(out) => {
                self.campaign.finish_reveal(id);
                Ok(out)
            }
            Err(error) => {
                self.campaign.abort_reveal(id);
                Err(error)
            }
        }
    }

    /// Reveal a generated item modifier through the same durable two-phase
    /// protocol as a secret fact. It becomes public only after its inventory
    /// event commits to the shared log.
    pub fn reveal_item_modifier(&mut self, id: &str) -> Result<Vec<Outbound>, String> {
        let reveal = self
            .campaign
            .begin_item_modifier_reveal(id)
            .ok_or_else(|| format!("unknown or pending item modifier: {id}"))?;
        match self.try_commit(GameEvent::ItemModifierRevealed(reveal)) {
            Ok(out) => {
                self.campaign.finish_item_modifier_reveal(id);
                Ok(out)
            }
            Err(error) => {
                self.campaign.abort_item_modifier_reveal(id);
                Err(error)
            }
        }
    }

    /// Commit a validated generator result in commit-result mode. The record
    /// is public and replayable, while applying it to game state stays a
    /// separate, type-specific DM operation.
    pub fn commit_generation(&mut self, record: GenerationRecord) -> Result<Vec<Outbound>, String> {
        self.try_commit(GameEvent::Generation(record))
    }

    /// Resolve one committed storylet against public world data and private
    /// fact IDs, then commit all effects through ordinary replicated events.
    /// A cloned snapshot prevalidates the batch so a bad late effect cannot
    /// leave a half-applied storylet.
    pub fn commit_storylet(
        &mut self,
        key: &str,
        item_owner: Option<TokenId>,
    ) -> Result<Vec<Outbound>, String> {
        let storylet = self
            .state
            .world
            .storylets
            .get(key)
            .cloned()
            .ok_or_else(|| format!("unknown storylet: {key}"))?;
        let resolved = self
            .state
            .world
            .resolve_storylet(&storylet, self.campaign.secret_ids())
            .map_err(|error| format!("storylet does not match: {error:?}"))?;
        let mut events = Vec::new();
        for (index, effect) in resolved.effects.into_iter().enumerate() {
            match effect {
                StoryletEffect::Fact { fact } => events.push(GameEvent::Fact(fact)),
                StoryletEffect::History { event } => {
                    events.push(GameEvent::World(WorldEvent::History(event)))
                }
                StoryletEffect::LocalMap { map } => {
                    let map = map
                        .lower(MapScale::Local)
                        .map_err(|error| format!("storylet map is invalid: {error}"))?;
                    events.push(GameEvent::MapStored(map));
                }
                StoryletEffect::Item { item } => {
                    let owner = item_owner
                        .ok_or_else(|| "storylet item effect needs an owner".to_owned())?;
                    let mut inventory = self
                        .state
                        .inventories
                        .get(&owner)
                        .cloned()
                        .unwrap_or_default();
                    // A storylet re-lights while its requirements hold, so it can
                    // be played more than once. A fixed `storylet.{key}.{index}`
                    // id would collide on the second grant and fail the whole
                    // commit; disambiguate so each play yields a fresh instance,
                    // the way the Fact/History effects already replay cleanly.
                    let mut id = ItemId::new(format!("storylet.{key}.{index}"));
                    let mut nonce = 1;
                    while inventory.items.contains_key(&id) {
                        id = ItemId::new(format!("storylet.{key}.{index}.{nonce}"));
                        nonce += 1;
                    }
                    inventory
                        .insert(ItemInstance {
                            id,
                            template: item.template,
                            name: item.name,
                            quantity: 1,
                            tags: item.tags,
                            modifiers: Vec::new(),
                            appearance_layers: Vec::new(),
                        })
                        .map_err(|error| format!("storylet item is invalid: {error:?}"))?;
                    events.push(GameEvent::InventorySet {
                        token: owner,
                        inventory,
                    });
                }
            }
        }
        let mut preview = self.state.clone();
        for event in &events {
            apply_game(&mut preview, event)
                .map_err(|error| format!("storylet effect rejected: {error:?}"))?;
        }
        let mut out = Vec::new();
        for event in events {
            out.extend(self.try_commit(event)?);
        }
        Ok(out)
    }

    /// Commit a downtime faction tick: a batch of moves the DM has previewed
    /// and edited. Each move flattens to ordinary world events (its history line
    /// and its change), which commit through the same path a DM edit does, so
    /// the whole batch lands in the ordered log and replicates to every peer.
    /// Staged on a clone first, so one rejected move cannot half-apply the tick.
    pub fn commit_faction_turn(&mut self, moves: Vec<FactionMove>) -> Result<Vec<Outbound>, String> {
        // A faction empties its bank when it acts: the moves it earned are the
        // time it spent, so the same banked time cannot buy a busy tick twice.
        // Collected before the moves are consumed; a faction with nothing banked
        // gets no spend event, so a sheetless world is untouched.
        let acted: std::collections::BTreeSet<String> =
            moves.iter().map(|m| m.faction.clone()).collect();
        let mut events: Vec<GameEvent> = moves
            .into_iter()
            .flat_map(FactionMove::into_events)
            .map(GameEvent::World)
            .collect();
        for faction in acted {
            let Some(sheet) = self.state.world.faction_sheet(&faction) else {
                continue;
            };
            if sheet.get("banked_time").copied().unwrap_or(0) == 0 {
                continue;
            }
            let mut spent = sheet.clone();
            spent.insert("banked_time".to_owned(), 0);
            events.push(GameEvent::World(WorldEvent::FactionSheet {
                faction,
                sheet: spent,
            }));
        }
        let mut preview = self.state.clone();
        for event in &events {
            apply_game(&mut preview, event)
                .map_err(|error| format!("faction move rejected: {error:?}"))?;
        }
        let mut out = Vec::new();
        for event in events {
            out.extend(self.try_commit(event)?);
        }
        Ok(out)
    }

    /// Offer a batch of radiant quests: faction-demand storylets the DM chose to
    /// make playable. Each enters the world as an ordinary storylet proposal, so
    /// it shows up in the storylet surface (C6) and can be played while its
    /// patron faction stands. Staged on a clone, like the faction tick.
    pub fn commit_radiant_quests(
        &mut self,
        quests: Vec<StoryletProposal>,
    ) -> Result<Vec<Outbound>, String> {
        let events: Vec<GameEvent> = quests
            .into_iter()
            .map(|quest| GameEvent::World(WorldEvent::Storylet(quest)))
            .collect();
        let mut preview = self.state.clone();
        for event in &events {
            apply_game(&mut preview, event)
                .map_err(|error| format!("radiant quest rejected: {error:?}"))?;
        }
        let mut out = Vec::new();
        for event in events {
            out.extend(self.try_commit(event)?);
        }
        Ok(out)
    }

    /// Accept an inspectable campaign draft. Public world/maps/rewards enter
    /// the ordered log; hidden facts enter only the host-private store. Both
    /// sides are staged before either is changed.
    pub fn commit_campaign(
        &mut self,
        record: GenerationRecord,
        item_owner: Option<TokenId>,
    ) -> Result<Vec<Outbound>, String> {
        let isometry_campaign::GenValue::Campaign { campaign: draft } = record.proposal.clone()
        else {
            return Err("generation record is not a campaign draft".to_owned());
        };
        draft
            .validate()
            .map_err(|error| format!("invalid campaign draft: {error:?}"))?;

        let mut private = self.campaign.clone();
        for secret in &draft.secrets {
            if let Some(existing) = private.secret(&secret.id) {
                if existing != secret {
                    return Err(format!(
                        "conflicting private campaign secret: {}",
                        secret.id
                    ));
                }
            } else {
                private.insert_secret(secret.clone());
            }
        }

        let mut events = vec![GameEvent::Generation(record)];
        events.extend(
            draft
                .public_world_events()
                .into_iter()
                .map(GameEvent::World),
        );
        for draft_map in &draft.maps {
            let map = draft_map
                .map
                .lower(draft_map.scale)
                .map_err(|error| format!("campaign map is invalid: {error}"))?;
            events.push(GameEvent::MapStored(map));
        }
        if !draft.rewards.is_empty() {
            let owner =
                item_owner.ok_or_else(|| "campaign reward needs a character owner".to_owned())?;
            let mut inventory = self
                .state
                .inventories
                .get(&owner)
                .cloned()
                .unwrap_or_default();
            for (index, item) in draft.rewards.iter().enumerate() {
                inventory
                    .insert(ItemInstance {
                        id: ItemId::new(format!("campaign.{}.reward.{index}", draft.id)),
                        template: item.template.clone(),
                        name: item.name.clone(),
                        quantity: 1,
                        tags: item.tags.clone(),
                        modifiers: Vec::new(),
                        appearance_layers: Vec::new(),
                    })
                    .map_err(|error| format!("campaign reward is invalid: {error:?}"))?;
            }
            events.push(GameEvent::InventorySet {
                token: owner,
                inventory,
            });
        }
        events.push(GameEvent::MapActivated {
            id: draft.starting_map.clone(),
        });

        let mut preview = self.state.clone();
        for event in &events {
            apply_game(&mut preview, event)
                .map_err(|error| format!("campaign draft rejected: {error:?}"))?;
        }
        let mut out = Vec::new();
        for event in events {
            out.extend(self.try_commit(event)?);
        }
        self.campaign = private;
        Ok(out)
    }

}
