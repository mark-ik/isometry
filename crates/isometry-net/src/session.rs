//! The DM-authority replication core, as two pure synchronous state
//! machines. They consume [`NetMessage`]s and emit [`Outbound`]s; the
//! transport (in-memory or iroh) is a dumb pump. No async, no I/O, no
//! networking here — that is what makes the whole protocol testable by
//! routing messages in a loop and asserting the peers converge.

use std::collections::HashMap;

use isometry_core::{apply, EventError, TokenId};

use crate::protocol::{
    fold_event, GameEvent, GameSnapshot, NetMessage, Outbound, PeerId, Recipient, FNV_OFFSET,
};

/// Apply one [`GameEvent`] to the replicated state, or reject it
/// unchanged. Turn ops that name a token validate its existence so a
/// stale intent can't desync the order.
pub fn apply_game(state: &mut GameSnapshot, event: &GameEvent) -> Result<(), EventError> {
    match event {
        GameEvent::Map(e) => {
            apply(&mut state.map, e)?;
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
            let overflow = state.roll_log.len().saturating_sub(crate::protocol::ROLL_LOG_CAP);
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
            state.journal.push(fact.clone());
            Ok(())
        }
    }
}

fn require_token(state: &GameSnapshot, id: TokenId) -> Result<(), EventError> {
    if state.map.token(id).is_some() {
        Ok(())
    } else {
        Err(EventError::UnknownToken(id))
    }
}

/// The host's authoritative session. Owns the canonical state and the
/// ordered log; validates every intent before it becomes `Applied`.
pub struct HostSession {
    state: GameSnapshot,
    /// Count of applied events; also the seq stamped on the next one.
    seq: u64,
    log_hash: u64,
    /// Player name each connected peer announced (via `Hello`), so the
    /// DM can whisper by name.
    peer_names: HashMap<PeerId, String>,
}

impl HostSession {
    pub fn new(state: GameSnapshot) -> Self {
        Self {
            state,
            seq: 0,
            log_hash: FNV_OFFSET,
            peer_names: HashMap::new(),
        }
    }

    pub fn state(&self) -> &GameSnapshot {
        &self.state
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

    /// A message arrived from `from`: `Intent` proposes an event,
    /// `Hello` announces the player's name; anything else is ignored (a
    /// misbehaving client cannot corrupt the authority).
    pub fn on_message(&mut self, from: PeerId, msg: NetMessage) -> Vec<Outbound> {
        match msg {
            // Campaign facts are DM-committed only (`local_event`); a
            // client cannot make something true by proposing it.
            NetMessage::Intent {
                event: GameEvent::Fact(_),
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "campaign facts are committed by the DM".to_owned(),
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
        self.seq += 1;
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
            NetMessage::Intent { .. }
            | NetMessage::Rejected { .. }
            | NetMessage::Hello { .. } => {}
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
