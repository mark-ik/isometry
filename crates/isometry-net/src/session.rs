//! The DM-authority replication core, as two pure synchronous state
//! machines. They consume [`NetMessage`]s and emit [`Outbound`]s; the
//! transport (in-memory or iroh) is a dumb pump. No async, no I/O, no
//! networking here — that is what makes the whole protocol testable by
//! routing messages in a loop and asserting the peers converge.

use std::collections::HashMap;

use codicil::Codicil;
use isometry_campaign::{
    CampaignStore, GenerationRecord, GenerationRecordError, InventoryError, ItemId,
    ItemModifierReveal, WorldFact,
};
use isometry_core::{apply, EventError, TokenId};

use crate::protocol::{
    fold_event, GameEvent, GameSnapshot, NetMessage, Outbound, PeerId, Recipient, FNV_OFFSET,
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
    ConflictingGeneration(String),
    InvalidGeneration(GenerationRecordError),
}

const MAX_GENERATION_VALUE_DEPTH: usize = 16;

pub fn apply_game(state: &mut GameSnapshot, event: &GameEvent) -> Result<(), GameError> {
    match event {
        GameEvent::Map(e) => {
            apply(&mut state.map, e).map_err(GameError::Core)?;
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
            state.turns.advance();
            Ok(())
        }
        GameEvent::TurnSetOrder(order) => {
            state.turns.set_order(order.clone());
            Ok(())
        }
        GameEvent::Rolled(record) => {
            state.roll_log.push(record.clone());
            let overflow = state
                .roll_log
                .len()
                .saturating_sub(crate::protocol::ROLL_LOG_CAP);
            if overflow > 0 {
                state.roll_log.drain(0..overflow);
            }
            Ok(())
        }
        GameEvent::SheetSet { token, sheet } => {
            state.map.set_sheet(*token, sheet.clone());
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
        }
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
    pub fn commit_generation(
        &mut self,
        record: GenerationRecord,
    ) -> Result<Vec<Outbound>, String> {
        self.try_commit(GameEvent::Generation(record))
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
            NetMessage::Intent {
                event:
                    GameEvent::Fact(_)
                    | GameEvent::InventorySet { .. }
                    | GameEvent::ItemTransfer { .. }
                    | GameEvent::ItemModifierRevealed(_)
                    | GameEvent::Generation(_),
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "campaign authoring is committed by the DM".to_owned(),
                },
            )],
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
            NetMessage::Intent { .. } | NetMessage::Rejected { .. } | NetMessage::Hello { .. } => {}
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
