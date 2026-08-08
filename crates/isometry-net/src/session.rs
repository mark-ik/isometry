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
    RequestId, FNV_OFFSET, PROTOCOL_VERSION,
};

mod apply;
mod client;
mod host;
mod messages;

// The 2026-07-24 split moved the bodies into the four modules above while this
// file kept the shared imports they read through `use super::*`. Re-exported
// here so the session layer's surface is unchanged: `lib.rs` still publishes
// exactly these four names.
pub use apply::{apply_game, GameError};
pub use client::ClientSession;
pub use host::HostSession;

