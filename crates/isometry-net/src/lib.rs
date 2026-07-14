//! Isometry's session layer: DM-authority replication of the game event
//! log over a transport seam.
//!
//! The design keeps the hard-to-test part (networking) out of the part
//! that carries the rules (replication). [`HostSession`] and
//! [`ClientSession`] are pure synchronous state machines: they take
//! [`NetMessage`]s and return [`Outbound`]s, with no async or I/O. The
//! [`sim`] module routes those in-process for tests and local play; the
//! `iroh` feature adds a QUIC transport that pumps the same messages
//! between machines.
//!
//! The model is deliberately simple because play is turn-based: the host
//! is the authority, clients send [`GameEvent`] intents, the host
//! validates and rebroadcasts an ordered [`NetMessage::Applied`] stream,
//! and every peer replays it. No rollback, no CRDTs. A per-peer rolling
//! log hash ([`HostSession::log_hash`]) makes divergence detectable.

mod protocol;
mod session;
pub mod sim;

#[cfg(feature = "campaign-p2p")]
pub mod campaign_space;

#[cfg(feature = "campaign-p2p-net")]
pub mod campaign_sync;

#[cfg(feature = "campaign-moot")]
pub mod campaign_moot;

#[cfg(feature = "campaign-murm")]
pub mod campaign_secrets;

pub use protocol::{
    ActionResolved, GameEvent, GameSnapshot, NetMessage, Outbound, PeerId, Recipient, ROLL_LOG_CAP,
};
pub use session::{apply_game, ClientSession, GameError, HostSession};

#[cfg(feature = "iroh")]
pub mod iroh_link;
