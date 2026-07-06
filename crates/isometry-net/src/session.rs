//! The DM-authority replication core, as two pure synchronous state
//! machines. They consume [`NetMessage`]s and emit [`Outbound`]s; the
//! transport (in-memory or iroh) is a dumb pump. No async, no I/O, no
//! networking here — that is what makes the whole protocol testable by
//! routing messages in a loop and asserting the peers converge.

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
        GameEvent::Rolled(record) => {
            state.roll_log.push(record.clone());
            let overflow = state.roll_log.len().saturating_sub(crate::protocol::ROLL_LOG_CAP);
            if overflow > 0 {
                state.roll_log.drain(0..overflow);
            }
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
}

impl HostSession {
    pub fn new(state: GameSnapshot) -> Self {
        Self {
            state,
            seq: 0,
            log_hash: FNV_OFFSET,
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

    /// A message arrived from `from`. Only `Intent` is meaningful to the
    /// host; anything else is ignored (a well-behaved client never sends
    /// it, and a misbehaving one cannot corrupt the authority).
    pub fn on_message(&mut self, from: PeerId, msg: NetMessage) -> Vec<Outbound> {
        match msg {
            NetMessage::Intent { event } => match self.try_commit(event) {
                Ok(out) => out,
                Err(reason) => vec![(Recipient::One(from), NetMessage::Rejected { reason })],
            },
            _ => Vec::new(),
        }
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
        }
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
            NetMessage::Intent { .. } | NetMessage::Rejected { .. } => {}
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
