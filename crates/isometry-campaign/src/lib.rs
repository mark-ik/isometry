//! Isometry's campaign-state layer (worldbuilding plan W0).
//!
//! Two stores, by design: the **host-private campaign store** holds the
//! GM layer (secret facts, hidden modifiers, unrevealed history), and the
//! **shared log** carries only public projections. Session convergence is
//! a rolling hash over byte-identical event logs, so a secret must never
//! be serialized into a `GameSnapshot` or `GameEvent`; a reveal is an
//! ordinary event that *publishes* a fact. This crate is pure data:
//! no I/O, no net, no substrate geometry. The substrate stores and
//! displays these objects; system plugins interpret them.

mod fact;
mod collaboration;
mod generator;
mod item;
mod map;
mod pack;
mod store;
mod world;

pub use fact::{RevealCondition, SecretFact, Visibility, WorldFact};
pub use collaboration::{CampaignProposal, CampaignProposalError, CampaignProposalMode};
pub use generator::{
    CastRoleRequest, EntropyTape, GenValue, GenValueError, GenerationRecord, GenerationRecordError,
    GeneratorFixture, GeneratorRequest, ItemProposal, MapPatchProposal, NpcProposal,
};
pub use item::{
    EquipmentSlot, HiddenItemModifier, Inventory, InventoryError, ItemId, ItemInstance,
    ItemModifier, ItemModifierKind, ItemModifierReveal,
};
pub use map::{
    CampaignMap, EncounterAnchor, LocalMapProposal, MapCellProposal, MapPoint, MapProposalError,
    MapScale, MapTransition, SpawnZone, MAX_GENERATED_MAP_EDGE,
};
pub use pack::{
    BeatEntry, ContentPackError, ContentPackManifest, GeneratorChoice, GeneratorEntry,
    GeneratorLockPreset, CONTENT_PACK_FORMAT,
};
pub use store::CampaignStore;
pub use world::{
    CampaignDraft, CampaignWorld, DraftMap, HistoryEvent, RoleSlot, StoryletEffect, StoryletError,
    StoryletProposal, StoryletRequirements, StoryletResolution, WorldCharacter, WorldError,
    WorldEvent, WorldFaction, WorldLaw, WorldPlace, WorldRoute,
};
