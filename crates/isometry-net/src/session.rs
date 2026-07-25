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

mod apply;
mod client;
mod host;
mod messages;

/// Apply one [`GameEvent`] to the replicated state, or reject it
/// unchanged. Turn ops that name a token validate its existence so a
/// stale intent can't desync the order.
#[derive(Clone, Debug, PartialEq, Eq)]

