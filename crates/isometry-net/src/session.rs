//! The DM-authority replication core, as two pure synchronous state
//! machines. They consume [`NetMessage`]s and emit [`Outbound`]s; the
//! transport (in-memory or iroh) is a dumb pump. No async, no I/O, no
//! networking here — that is what makes the whole protocol testable by
//! routing messages in a loop and asserting the peers converge.

use std::collections::HashMap;

use codicil::Codicil;
use isometry_campaign::{
    CampaignStore, FactionMove, GenerationRecord, GenerationRecordError, InventoryError, ItemId,
    ItemInstance, ItemModifierReveal, MapScale, StoryletEffect, StoryletProposal, WorldError,
    WorldEvent, WorldFact,
};
use isometry_core::{apply, EventError, TileCoord, TokenId};

use crate::protocol::{
    fold_event, ActionIntent, GameEvent, GameSnapshot, NetMessage, Outbound, PeerId, Recipient,
    FNV_OFFSET,
};

/// Apply one [`GameEvent`] to the replicated state, or reject it
/// unchanged. Turn ops that name a token validate its existence so a
/// stale intent can't desync the order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameError {
    Core(EventError),
    ConflictingFact(String),
    Inventory(InventoryError),
    UnknownItem(ItemId),
    DuplicateItem(ItemId),
    SameInventoryTransfer(TokenId),
    /// A resolution addressed a token with no sheet, so it could not be applied
    /// whole. Rejected rather than half-applied.
    UnsheetedTarget(TokenId),
    /// A travel event for a token not standing on a transition point.
    NotOnTransition(TokenId),
    ConflictingGeneration(String),
    InvalidGeneration(GenerationRecordError),
    UnknownMap(String),
    ConflictingMap(String),
    World(WorldError),
}

const MAX_GENERATION_VALUE_DEPTH: usize = 16;

/// Hand a fresh set of beats to every peer's board. Bumping the sequence is what
/// makes two identical consecutive strikes play twice rather than once.
fn play_beats(state: &mut GameSnapshot, beats: Vec<isometry_core::Beat>) {
    state.last_beats = beats;
    state.beat_seq = state.beat_seq.wrapping_add(1);
}

/// Append a roll to the shared log, dropping the oldest past the cap.
fn push_roll(state: &mut GameSnapshot, record: &isometry_core::RollRecord) {
    state.roll_log.push(record.clone());
    let overflow = state
        .roll_log
        .len()
        .saturating_sub(crate::protocol::ROLL_LOG_CAP);
    if overflow > 0 {
        state.roll_log.drain(0..overflow);
    }
}

pub fn apply_game(state: &mut GameSnapshot, event: &GameEvent) -> Result<(), GameError> {
    match event {
        GameEvent::Map(e) => {
            apply(&mut state.map, e).map_err(GameError::Core)?;
            sync_active_map(state);
            Ok(())
        }
        GameEvent::TurnAdd(id) => {
            require_token(state, *id)?;
            state.turns.add(*id);
            Ok(())
        }
        GameEvent::TurnRemove(id) => {
            state.turns.remove(*id);
            Ok(())
        }
        GameEvent::TurnAdvance => {
            // The fallen do not get a turn. Deterministic and replicated: the
            // skip is computed from state every peer already has, so nobody has
            // to be told about it separately.
            let before = state.turns.round();
            let map = &state.map;
            state.turns.advance_skipping(|id| map.is_defeated(id));
            // A turn beginning wipes the token's per-turn counters: its action
            // economy refills, its multiple-attack penalty resets. The substrate
            // clears the named ledger without knowing what any counter meant.
            if let Some(active) = state.turns.active() {
                state.map.clear_turn_counters(active);
            }
            // A completed round is elapsed time: tick the location's clock by
            // however many rounds the wrap crossed. Time is a campaign feature,
            // so a bare board with no stored map keeps no clock.
            let elapsed = state.turns.round().saturating_sub(before);
            if elapsed > 0 {
                if let Some(active) = state.active_map.clone() {
                    *state.clocks.entry(active).or_insert(0) += elapsed;
                }
            }
            Ok(())
        }
        GameEvent::TurnSetOrder(order) => {
            state.turns.set_order(order.clone());
            Ok(())
        }
        GameEvent::Rolled(record) => {
            push_roll(state, record);
            Ok(())
        }
        GameEvent::ActionResolved(res) => {
            require_token(state, res.actor)?;
            require_token(state, res.target)?;
            // Every delta must address a token that actually has a sheet. A
            // resolution that would half-apply is rejected whole, so a peer
            // either takes all of an action or none of it and the hashes cannot
            // drift apart.
            if res
                .deltas
                .iter()
                .any(|d| state.map.sheet(d.token).is_none())
            {
                return Err(GameError::UnsheetedTarget(res.target));
            }
            for delta in &res.deltas {
                state.map.apply_delta(delta);
            }
            // Forced movement is truth, so it lands here, in the ordered log,
            // where every peer applies the identical tile. A stagger beat never
            // reaches this function at all.
            for (token, to) in &res.displaced {
                if let Some(t) = state.map.tokens.iter_mut().find(|t| t.id == *token) {
                    t.at = *to;
                }
            }
            for token in &res.defeated {
                state.map.set_defeated(*token, true);
            }
            for (token, name, value) in &res.conditions {
                state.map.set_condition(*token, name, *value);
            }
            for (token, mobility) in &res.mobility {
                state.map.set_mobility(*token, *mobility);
            }
            // Allegiance: a convinced creature changes sides. The host already
            // ruled the owner and the cap; every peer applies the same change,
            // and each peer's fog recomputes from it (a new ally feeds your
            // sight, an ex-ally stops feeding it).
            for (token, owner) in &res.owner_changes {
                if let Some(t) = state.map.tokens.iter_mut().find(|t| t.id == *token) {
                    t.owner = owner.clone();
                }
            }
            // The action's per-turn spend: the acting peer's rules decided it,
            // and every peer folds the same integer deltas into the shared
            // ledger. Applied verbatim, like the sheet deltas -- the authority
            // never reruns the afford rule (that gate lives where the Lua ran).
            for (token, key, delta) in &res.turn_counters {
                state.map.bump_turn_counter(*token, key, *delta);
            }
            push_roll(state, &res.attack);
            if let Some(damage) = &res.damage {
                push_roll(state, damage);
            }
            play_beats(state, res.beats.clone());
            sync_active_map(state);
            Ok(())
        }
        GameEvent::Emoted { token, beat } => {
            require_token(state, *token)?;
            play_beats(state, vec![isometry_core::Beat::new(*token, beat.clone())]);
            Ok(())
        }
        GameEvent::StanceSet { token, stance } => {
            require_token(state, *token)?;
            state.map.set_stance(*token, stance);
            sync_active_map(state);
            Ok(())
        }
        GameEvent::ConditionSet {
            token,
            condition,
            value,
            mobility,
        } => {
            require_token(state, *token)?;
            state.map.set_condition(*token, condition, *value);
            state.map.set_mobility(*token, *mobility);
            sync_active_map(state);
            Ok(())
        }
        GameEvent::SheetSet { token, sheet } => {
            state.map.set_sheet(*token, sheet.clone());
            sync_active_map(state);
            Ok(())
        }
        GameEvent::Fact(fact) => {
            if !fact.id.is_empty() {
                if let Some(existing) = state.journal.iter().find(|entry| entry.id == fact.id) {
                    return if existing == fact {
                        Ok(())
                    } else {
                        Err(GameError::ConflictingFact(fact.id.clone()))
                    };
                }
            }
            state.journal.push(fact.clone());
            Ok(())
        }
        GameEvent::InventorySet { token, inventory } => {
            require_token(state, *token)?;
            inventory.validate().map_err(GameError::Inventory)?;
            for (owner, other) in &state.inventories {
                if owner != token {
                    if let Some(id) = inventory
                        .items
                        .keys()
                        .find(|id| other.items.contains_key(*id))
                    {
                        return Err(GameError::DuplicateItem(id.clone()));
                    }
                }
            }
            state.inventories.insert(*token, inventory.clone());
            Ok(())
        }
        GameEvent::ItemTransfer { from, to, item } => transfer_item(state, *from, *to, item),
        GameEvent::ItemModifierRevealed(reveal) => {
            apply_item_modifier_reveal(state, reveal)?;
            Ok(())
        }
        GameEvent::Generation(record) => {
            record
                .validate(MAX_GENERATION_VALUE_DEPTH)
                .map_err(GameError::InvalidGeneration)?;
            if let Some(existing) = state.generations.iter().find(|entry| entry.id == record.id) {
                return if existing == record {
                    Ok(())
                } else {
                    Err(GameError::ConflictingGeneration(record.id.clone()))
                };
            }
            state.generations.push(record.clone());
            Ok(())
        }
        GameEvent::MapStored(map) => {
            if map.id.trim().is_empty() {
                return Err(GameError::UnknownMap(map.id.clone()));
            }
            if let Some(existing) = state.maps.get(&map.id) {
                return if existing == map {
                    Ok(())
                } else {
                    Err(GameError::ConflictingMap(map.id.clone()))
                };
            }
            state.maps.insert(map.id.clone(), map.clone());
            Ok(())
        }
        GameEvent::MapActivated { id } => {
            let map = state
                .maps
                .get(id)
                .ok_or_else(|| GameError::UnknownMap(id.clone()))?;
            state.map = map.document.clone();
            state.active_map = Some(id.clone());
            state.turns = isometry_core::TurnList::new();
            for token in &state.map.tokens {
                state.turns.add(token.id);
            }
            Ok(())
        }
        GameEvent::World(event) => state.world.apply(event).map_err(GameError::World),
        GameEvent::Traveled { token } => travel(state, *token),
        GameEvent::TimeAdvanced { ticks } => {
            let active = state
                .active_map
                .clone()
                .ok_or_else(|| GameError::UnknownMap("<no active map>".to_owned()))?;
            *state.clocks.entry(active).or_insert(0) += ticks;
            Ok(())
        }
        GameEvent::TravelResolved {
            party,
            to,
            ticks,
            roll,
            lost: _,
            exhaustion,
            encounter,
            forage,
        } => {
            // The party arrives: its overmap position is world state, and
            // arriving discovers the place and what is one step on.
            state.world.party_node.insert(party.clone(), to.clone());
            state.world.discover_around(party, to);
            // Arriving advances the destination site's clock by the travel time,
            // so a place reached later is later there -- the C3 clock, reached
            // across the overmap instead of through a door. A bare waypoint (no
            // site) keeps no clock, so its leg is not banked anywhere yet.
            if let Some(map) = state.world.places.get(to).and_then(|p| p.map.clone()) {
                *state.clocks.entry(map).or_insert(0) += ticks;
            }
            // The march's toll: every party member gains exhaustion, a graded
            // condition, worsened to at least the level the march exacted (a
            // short leg after a long one does not refresh you). The party is the
            // tokens sharing its owner.
            if *exhaustion > 0 {
                let members: Vec<_> = state
                    .map
                    .tokens
                    .iter()
                    .filter(|t| t.owner.as_deref() == Some(party.as_str()))
                    .map(|t| t.id)
                    .collect();
                for id in members {
                    if state.map.condition_value(id, "exhaustion") < *exhaustion {
                        state.map.set_condition(id, "exhaustion", *exhaustion);
                    }
                }
            }
            // Food the party gathered on the road joins its stores.
            if *forage != 0 {
                state.world.add_party_resource(party, "food", *forage);
            }
            // A peril on the road drops the party onto the destination's tactical
            // map to fight, rather than arriving in peace: the same map switch a
            // door makes (C2). A bare waypoint with no site is a safe arrival.
            if *encounter {
                if let Some(map) = state.world.places.get(to).and_then(|p| p.map.clone()) {
                    if state.maps.contains_key(&map) {
                        state.active_map = Some(map);
                    }
                }
            }
            push_roll(state, roll);
            sync_active_map(state);
            Ok(())
        }
    }
}

/// Walk one token through the transition point it stands on. Everything is
/// derived from replicated state, so all peers land it identically.
fn travel(state: &mut GameSnapshot, token: TokenId) -> Result<(), GameError> {
    require_token(state, token)?;
    let at = state.map.token(token).map(|t| t.at).unwrap_or_default();
    let active_id = state
        .active_map
        .clone()
        .ok_or_else(|| GameError::UnknownMap("<no active map>".to_owned()))?;
    // The door is the tile the traveler stands on.
    let transition = state
        .maps
        .get(&active_id)
        .and_then(|m| {
            m.transitions
                .iter()
                .find(|t| (t.at.col as i32, t.at.row as i32) == at)
        })
        .cloned()
        .ok_or(GameError::NotOnTransition(token))?;
    let target = state
        .maps
        .get(&transition.target_map)
        .ok_or_else(|| GameError::UnknownMap(transition.target_map.clone()))?;

    // Destination: the target's named entry door, else its first spawn zone,
    // else the origin corner; then the first free tile scanning outward, the
    // same deterministic walk spawning already uses.
    let anchor: TileCoord = transition
        .target_entry
        .as_ref()
        .and_then(|entry| target.transitions.iter().find(|t| &t.id == entry))
        .map(|t| (t.at.col as i32, t.at.row as i32))
        .or_else(|| {
            target
                .spawn_zones
                .first()
                .and_then(|z| z.cells.first())
                .map(|c| (c.col as i32, c.row as i32))
        })
        .unwrap_or((1, 1));
    let (w, h) = (target.document.ground.width(), target.document.ground.height());
    let occupied: Vec<TileCoord> = target.document.tokens.iter().map(|t| t.at).collect();
    let mut landing = anchor;
    for d in 0..64 {
        let cand = (anchor.0 + (d % 8), anchor.1 + (d / 8));
        if cand.0 >= 0
            && cand.1 >= 0
            && (cand.0 as u32) < w
            && (cand.1 as u32) < h
            && !occupied.contains(&cand)
        {
            landing = cand;
            break;
        }
    }

    // Ids are per-map, so an arrival can collide with a resident. Mint the
    // next id above every token on every map (inventories key on TokenId
    // globally, so global uniqueness is what keeps them sound).
    let collides = target.document.tokens.iter().any(|t| t.id == token);
    let new_id = if collides {
        let max = state
            .maps
            .values()
            .flat_map(|m| m.document.tokens.iter())
            .chain(state.map.tokens.iter())
            .map(|t| t.id.0)
            .chain(state.inventories.keys().map(|id| id.0))
            .max()
            .unwrap_or(0);
        TokenId(max + 1)
    } else {
        token
    };

    // Depart: the traveler and everything it carries leaves the active map.
    let Some(pos) = state.map.tokens.iter().position(|t| t.id == token) else {
        return Err(GameError::Core(EventError::UnknownToken(token)));
    };
    let mut traveler = state.map.tokens.remove(pos);
    let sheet = state.map.sheets.remove(&token);
    let conditions = state.map.conditions.remove(&token);
    let mobility = state.map.mobility.remove(&token);
    let was_defeated = state.map.defeated.remove(&token);
    state.turns.remove(token);
    sync_active_map(state);

    // Arrive.
    traveler.id = new_id;
    traveler.at = landing;
    let target = state
        .maps
        .get_mut(&transition.target_map)
        .expect("target existed above");
    target.document.tokens.push(traveler);
    if let Some(sheet) = sheet {
        target.document.sheets.insert(new_id, sheet);
    }
    if let Some(conditions) = conditions {
        target.document.conditions.insert(new_id, conditions);
    }
    if let Some(mobility) = mobility {
        target.document.mobility.insert(new_id, mobility);
    }
    if was_defeated {
        target.document.defeated.insert(new_id);
    }
    if new_id != token {
        if let Some(inventory) = state.inventories.remove(&token) {
            state.inventories.insert(new_id, inventory);
        }
    }

    // Nobody arrives before they left: the destination's clock catches up to
    // the traveler's. This is the whole of split-party reconciliation: while
    // parties are apart their locations' clocks drift freely (simultaneity is
    // presentation), and the moment anyone crosses, the two timelines agree.
    let source_time = state.clocks.get(&active_id).copied().unwrap_or(0);
    let dest = state
        .clocks
        .entry(transition.target_map.clone())
        .or_insert(0);
    *dest = (*dest).max(source_time);

    // The board follows the last player out: when no player-owned token
    // remains on the active map, the target activates, exactly as a manual
    // `MapActivated` would (fresh board, fresh turn order).
    if !state.map.tokens.iter().any(|t| t.owner.is_some()) {
        let doc = state
            .maps
            .get(&transition.target_map)
            .expect("target existed above")
            .document
            .clone();
        state.map = doc;
        state.active_map = Some(transition.target_map.clone());
        state.turns = isometry_core::TurnList::new();
        for t in &state.map.tokens {
            state.turns.add(t.id);
        }
    }
    Ok(())
}

fn sync_active_map(state: &mut GameSnapshot) {
    if let Some(id) = &state.active_map {
        if let Some(map) = state.maps.get_mut(id) {
            map.document = state.map.clone();
        }
    }
}

fn transfer_item(
    state: &mut GameSnapshot,
    from: TokenId,
    to: TokenId,
    item: &ItemId,
) -> Result<(), GameError> {
    require_token(state, from)?;
    require_token(state, to)?;
    if from == to {
        return Err(GameError::SameInventoryTransfer(from));
    }
    let source = state
        .inventories
        .get(&from)
        .ok_or_else(|| GameError::UnknownItem(item.clone()))?;
    if !source.items.contains_key(item) {
        return Err(GameError::UnknownItem(item.clone()));
    }
    if state
        .inventories
        .get(&to)
        .is_some_and(|target| target.items.contains_key(item))
    {
        return Err(GameError::DuplicateItem(item.clone()));
    }
    let moved = state
        .inventories
        .get_mut(&from)
        .expect("source inventory was checked")
        .take(item)
        .map_err(GameError::Inventory)?;
    state
        .inventories
        .entry(to)
        .or_default()
        .insert(moved)
        .map_err(GameError::Inventory)
}

fn apply_item_modifier_reveal(
    state: &mut GameSnapshot,
    reveal: &ItemModifierReveal,
) -> Result<(), GameError> {
    let item = state
        .inventories
        .values_mut()
        .find_map(|inventory| inventory.item_mut(&reveal.item))
        .ok_or_else(|| GameError::UnknownItem(reveal.item.clone()))?;
    item.attach_modifier(reveal.modifier.clone())
        .map_err(GameError::Inventory)
}

fn require_token(state: &GameSnapshot, id: TokenId) -> Result<(), GameError> {
    if state.map.token(id).is_some() {
        Ok(())
    } else {
        Err(GameError::Core(EventError::UnknownToken(id)))
    }
}

/// The host's authoritative session. Owns the canonical state and the
/// ordered log; validates every intent before it becomes `Applied`.
pub struct HostSession {
    state: GameSnapshot,
    /// The host-private GM layer. It never enters a public snapshot or event.
    campaign: CampaignStore,
    /// The durable, append-only authority history. The public snapshot is a
    /// materialized view of this log; checkpoints keep both for fast restore.
    history: Codicil<GameEvent>,
    /// Count of applied events; also the seq stamped on the next one.
    seq: u64,
    log_hash: u64,
    /// Player name each connected peer announced (via `Hello`), so the
    /// DM can whisper by name.
    peer_names: HashMap<PeerId, String>,
    /// Client action requests awaiting adjudication. They sit here because this
    /// crate is deliberately rules-blind: it can validate that you own the token
    /// you are swinging, but it has no `System` and so cannot say whether you
    /// hit. The host *app* drains these, resolves them with its rules plugin, and
    /// commits the outcome back through `local_event`.
    pending_actions: Vec<ActionIntent>,
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

    /// Complete interrupted reveals after restoring a snapshot and campaign
    /// store. An identical public fact finalizes; an absent one is retried.
    pub fn reconcile_pending_reveals(&mut self) -> Result<Vec<Outbound>, String> {
        let pending: Vec<WorldFact> = self.campaign.pending_world_facts().collect();
        let mut out = Vec::new();
        for fact in pending {
            if let Some(existing) = self.state.journal.iter().find(|entry| entry.id == fact.id) {
                if existing != &fact {
                    return Err(format!("conflicting public campaign fact: {}", fact.id));
                }
            } else {
                out.extend(self.try_commit(GameEvent::Fact(fact.clone()))?);
            }
            self.campaign.finish_reveal(&fact.id);
        }
        let pending_modifiers: Vec<ItemModifierReveal> =
            self.campaign.pending_item_modifier_reveals().collect();
        for reveal in pending_modifiers {
            let item = self
                .state
                .inventories
                .values()
                .find_map(|inventory| inventory.items.get(&reveal.item))
                .ok_or_else(|| {
                    format!("missing public item for modifier reveal: {}", reveal.item.0)
                })?;
            if let Some(existing) = item
                .modifiers
                .iter()
                .find(|modifier| modifier.id == reveal.modifier.id)
            {
                if existing != &reveal.modifier {
                    return Err(format!(
                        "conflicting public item modifier: {}",
                        reveal.modifier.id
                    ));
                }
            } else {
                out.extend(self.try_commit(GameEvent::ItemModifierRevealed(reveal.clone()))?);
            }
            self.campaign.finish_item_modifier_reveal(&reveal.id);
        }
        Ok(out)
    }

    /// A message arrived from `from`: `Intent` proposes an event,
    /// `Hello` announces the player's name; anything else is ignored (a
    /// misbehaving client cannot corrupt the authority).
    pub fn on_message(&mut self, from: PeerId, msg: NetMessage) -> Vec<Outbound> {
        match msg {
            // Campaign reveals are DM-committed only (`local_event`); a
            // client cannot make a hidden record public by proposing it.
            // A resolution is a *verdict*, and a peer cannot pronounce its own.
            // Accepting this as an intent would let a client choose whether it
            // hit and how much damage it dealt. The rules run on the sequencer;
            // a client asks, it does not decide. (The ask itself, an action
            // intent a client can send, is the next step: it needs a message the
            // host app can drain and resolve with its system plugin, since this
            // crate is deliberately rules-blind.)
            NetMessage::Intent {
                event: GameEvent::ActionResolved(_),
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "actions are adjudicated by the host".to_owned(),
                },
            )],
            // Travel is a verdict too: a client cannot pronounce where its party
            // arrived, how long it took, or whether it got lost.
            NetMessage::Intent {
                event: GameEvent::TravelResolved { .. },
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "travel is adjudicated by the host".to_owned(),
                },
            )],
            // The DM keeps the clock: a player does not declare hours passing.
            NetMessage::Intent {
                event: GameEvent::TimeAdvanced { .. },
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "the DM keeps the clock".to_owned(),
                },
            )],
            // Travel is ruled by the host's own sweep (it watches for tokens
            // standing on doors after every applied move), so a client walks
            // through a door by walking; it never asks in words.
            NetMessage::Intent {
                event: GameEvent::Traveled { .. },
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "travel is ruled by the host".to_owned(),
                },
            )],
            // A condition is a rules ruling with numbers attached; a client
            // proposing one would be pronouncing what `prone` means. Standing up
            // travels as an action intent instead, so the host's rules answer.
            NetMessage::Intent {
                event: GameEvent::ConditionSet { .. },
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "conditions are ruled by the host".to_owned(),
                },
            )],
            NetMessage::Intent {
                event:
                    GameEvent::Fact(_)
                    | GameEvent::InventorySet { .. }
                    | GameEvent::ItemTransfer { .. }
                    | GameEvent::ItemModifierRevealed(_)
                    | GameEvent::Generation(_)
                    | GameEvent::MapStored(_)
                    | GameEvent::MapActivated { .. }
                    | GameEvent::World(_),
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "campaign authoring is committed by the DM".to_owned(),
                },
            )],
            // An emote needs no adjudication (there is no verdict to forge), but
            // it does need ownership: waving is harmless, and puppeteering the
            // DM's monsters is not. A player emotes their own tokens.
            NetMessage::Intent {
                event: GameEvent::Emoted { token, .. },
            } if !self.peer_owns(from, token) => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "you can only emote your own tokens".to_owned(),
                },
            )],
            // A stance is a declaration, not a verdict, so a player sets it on its
            // own tokens (and only its own), exactly like an emote.
            NetMessage::Intent {
                event: GameEvent::StanceSet { token, .. },
            } if !self.peer_owns(from, token) => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "you can only set the stance of your own tokens".to_owned(),
                },
            )],
            // A player asking to act. Two things are checkable without any rules
            // at all, so they are checked here: the actor exists, and it is
            // yours. Everything else -- reach, turn, whether it hits, what it
            // costs -- is the rules system's, so the request is queued for the
            // host app to adjudicate and commit.
            NetMessage::Action(intent) => {
                if !self.peer_owns(from, intent.actor) {
                    return vec![(
                        Recipient::One(from),
                        NetMessage::Rejected {
                            reason: "you can only act with your own tokens".to_owned(),
                        },
                    )];
                }
                self.pending_actions.push(intent);
                Vec::new()
            }
            NetMessage::Intent { event } => match self.try_commit(event) {
                Ok(out) => out,
                Err(reason) => vec![(Recipient::One(from), NetMessage::Rejected { reason })],
            },
            NetMessage::Hello { name } => {
                self.peer_names.insert(from, name);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Whether the peer's announced player name may command `token`: either it
    /// owns the token directly, or the token is owned by a faction whose channel
    /// the player has been granted. A DM-controlled token (`owner: None`)
    /// belongs to nobody, so no client owns it.
    fn peer_owns(&self, peer: PeerId, token: TokenId) -> bool {
        let Some(name) = self.peer_names.get(&peer).map(String::as_str) else {
            return false;
        };
        let Some(owner) = self.state.map.token(token).and_then(|t| t.owner.as_deref()) else {
            return false;
        };
        // A faction is an owner name like any other; playing it means holding its
        // channel, so the grant extends command to the faction's tokens.
        owner == name || self.state.world.faction_controller(owner) == Some(name)
    }

    /// The DM whispers to the player named `to`. Returns a directed
    /// message to that peer (empty if nobody by that name is connected).
    pub fn whisper(&self, from: &str, to: &str, text: &str) -> Vec<Outbound> {
        self.peer_names
            .iter()
            .find(|(_, name)| name.as_str() == to)
            .map(|(&peer, _)| {
                vec![(
                    Recipient::One(peer),
                    NetMessage::Whisper {
                        from: from.to_owned(),
                        text: text.to_owned(),
                    },
                )]
            })
            .unwrap_or_default()
    }

    /// The player names currently connected (whisper targets).
    pub fn peer_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.peer_names.values().cloned().collect();
        names.sort();
        names.dedup();
        names
    }

    fn commit(&mut self, event: GameEvent) -> Vec<Outbound> {
        self.try_commit(event).unwrap_or_default()
    }

    fn try_commit(&mut self, event: GameEvent) -> Result<Vec<Outbound>, String> {
        apply_game(&mut self.state, &event).map_err(|e| format!("{e:?}"))?;
        let appended = self.history.append(event.clone());
        self.seq = appended.0 + 1;
        self.log_hash = fold_event(self.log_hash, self.seq, &event);
        Ok(vec![(
            Recipient::All,
            NetMessage::Applied {
                seq: self.seq,
                event,
            },
        )])
    }
}

/// A client's replica. Applies the host's ordered `Applied` stream and
/// proposes its own moves as `Intent`. Never mutates state optimistically
/// — the authority orders, the client replays — so it cannot diverge.
pub struct ClientSession {
    state: Option<GameSnapshot>,
    /// Seq of the last applied event; the snapshot sets the baseline.
    applied: u64,
    log_hash: u64,
    /// Applied events that arrived ahead of a gap, kept until the gap
    /// fills (QUIC is ordered per-stream, but a reconnect or a
    /// multi-stream transport could interleave; this keeps replay exact).
    pending: Vec<(u64, GameEvent)>,
    /// Whispers received from the DM, oldest first: `(from, text)`.
    inbox: Vec<(String, String)>,
}

impl Default for ClientSession {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientSession {
    pub fn new() -> Self {
        Self {
            state: None,
            applied: 0,
            log_hash: FNV_OFFSET,
            pending: Vec::new(),
            inbox: Vec::new(),
        }
    }

    /// Announce this player's name to the host (sent on connect), so the
    /// DM can whisper to it.
    pub fn hello(&self, name: &str) -> Outbound {
        (
            Recipient::Host,
            NetMessage::Hello {
                name: name.to_owned(),
            },
        )
    }

    /// Whispers received so far.
    pub fn inbox(&self) -> &[(String, String)] {
        &self.inbox
    }

    /// Take and clear received whispers (the bridge drains these to the
    /// UI).
    pub fn drain_inbox(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.inbox)
    }

    /// The replicated state, once the snapshot has arrived.
    pub fn state(&self) -> Option<&GameSnapshot> {
        self.state.as_ref()
    }

    pub fn applied(&self) -> u64 {
        self.applied
    }

    pub fn log_hash(&self) -> u64 {
        self.log_hash
    }

    /// Propose an event to the host. Returned as an `Outbound` to
    /// `Recipient::Host`; the client does not apply it until the host
    /// echoes it back as `Applied`.
    pub fn intent(&self, event: GameEvent) -> Outbound {
        (Recipient::Host, NetMessage::Intent { event })
    }

    /// Ask the host to resolve an action. The client never decides the outcome,
    /// so this carries no roll, no damage and no verdict: only the request.
    pub fn action(&self, intent: ActionIntent) -> Outbound {
        (Recipient::Host, NetMessage::Action(intent))
    }

    /// Handle a message from the host. Returns any follow-on outbound
    /// (none today; the shape leaves room for acks).
    pub fn on_message(&mut self, msg: NetMessage) -> Vec<Outbound> {
        match msg {
            NetMessage::Snapshot {
                seq,
                log_hash,
                state,
            } => {
                // Seed from the host's hash at snapshot time, so folding
                // the tail forward lands on the same value the host holds
                // — late joiners and from-start peers all converge.
                self.state = Some(state);
                self.applied = seq;
                self.log_hash = log_hash;
                self.drain_pending();
            }
            NetMessage::Applied { seq, event } => {
                self.pending.push((seq, event));
                self.drain_pending();
            }
            NetMessage::Whisper { from, text } => {
                self.inbox.push((from, text));
            }
            NetMessage::Intent { .. }
            | NetMessage::Rejected { .. }
            | NetMessage::Hello { .. }
            | NetMessage::Action(_) => {}
        }
        Vec::new()
    }

    /// Apply every buffered event whose seq is next, in order.
    fn drain_pending(&mut self) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        loop {
            let Some(idx) = self
                .pending
                .iter()
                .position(|(seq, _)| *seq == self.applied + 1)
            else {
                break;
            };
            let (seq, event) = self.pending.swap_remove(idx);
            // A client trusts the host's ordering; an apply error here
            // means the streams diverged, so drop it loudly in debug.
            if apply_game(state, &event).is_ok() {
                self.applied = seq;
                self.log_hash = fold_event(self.log_hash, seq, &event);
            } else {
                debug_assert!(false, "host-ordered event failed to apply on client");
            }
        }
    }
}
