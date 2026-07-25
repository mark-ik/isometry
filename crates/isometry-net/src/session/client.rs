//! The client replica: apply the host's ordered log, propose intents.
//!
//! Never mutates game state on its own. The authority orders, the client
//! replays, which is the whole reason this needs no rollback or CRDT.
//!
//! Split out of `session.rs` on 2026-07-24; behavior unchanged.

use super::*;

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
    pub(crate) fn default() -> Self {
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
    pub(crate) fn drain_pending(&mut self) {
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
