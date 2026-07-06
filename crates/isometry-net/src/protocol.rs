use isometry_core::{MapDocument, SessionEvent, TokenId, TurnList};
use serde::{Deserialize, Serialize};

/// A peer's identity within a session. For the iroh transport this wraps
/// the remote node id; the pure-sync core only needs it to route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PeerId(pub u64);

/// The replicated game state: exactly the substrate document plus the
/// turn order. View concerns (camera, undo, selection) never cross the
/// wire; each peer keeps its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameSnapshot {
    pub map: MapDocument,
    pub turns: TurnList,
}

/// The replicated unit: a map mutation or a turn-order change. The host
/// orders these into one log every peer replays; `MapDocument` and
/// `TurnList` together are the whole shared state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GameEvent {
    /// A substrate document mutation (token move, tile paint, ...).
    Map(SessionEvent),
    /// Add a token to the turn order.
    TurnAdd(TokenId),
    /// Drop a token from the turn order (free movement thereafter).
    TurnRemove(TokenId),
    /// Advance to the next turn.
    TurnAdvance,
}

/// One message on the wire. The host is the authority: clients send
/// `Intent`, the host validates and rebroadcasts `Applied` with a
/// sequence number, and every peer applies the same ordered log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NetMessage {
    /// Host to a joining client: full state as of `seq` applied events,
    /// plus the host's rolling log hash at that point so a late joiner
    /// seeds its own hash to match and converges on the tail.
    Snapshot {
        seq: u64,
        log_hash: u64,
        state: GameSnapshot,
    },
    /// Host to all: the next ordered, host-validated event.
    Applied { seq: u64, event: GameEvent },
    /// Client to host: a proposed event (may be rejected).
    Intent { event: GameEvent },
    /// Host to the proposer: the intent failed validation.
    Rejected { reason: String },
}

/// Where a produced message goes. The transport resolves this to actual
/// peers; the session core stays routing-agnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recipient {
    /// Every connected peer (used for `Applied`, so ordering is uniform).
    All,
    /// One specific peer (snapshot to a joiner, reject to a proposer).
    One(PeerId),
    /// The host (a client's `Intent`).
    Host,
}

/// A message the session wants sent, paired with its destination.
pub type Outbound = (Recipient, NetMessage);

/// FNV-1a over bytes. A fixed, std-independent hash so the log-hash
/// convergence check holds across machines and std versions (unlike
/// `DefaultHasher`, whose SipHash keys are unspecified across builds).
pub(crate) fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Starting basis for the rolling log hash.
pub(crate) const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// Fold one applied `(seq, event)` into the rolling log hash via its
/// postcard bytes — the same byte form both host and client hash, so
/// equal logs give equal hashes regardless of platform.
pub(crate) fn fold_event(hash: u64, seq: u64, event: &GameEvent) -> u64 {
    let mut h = fnv1a(hash, &seq.to_le_bytes());
    if let Ok(bytes) = postcard::to_allocvec(event) {
        h = fnv1a(h, &bytes);
    }
    h
}
