//! The durable world vocabulary: places, routes, characters, factions, laws,
//! history, and storylets.
//!
//! Split out of `world.rs` on 2026-07-24; behavior unchanged.

use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignWorld {
    #[serde(default)]
    pub factions: BTreeMap<String, WorldFaction>,
    #[serde(default)]
    pub places: BTreeMap<String, WorldPlace>,
    #[serde(default)]
    pub characters: BTreeMap<String, WorldCharacter>,
    #[serde(default)]
    pub routes: BTreeMap<String, WorldRoute>,
    #[serde(default)]
    pub laws: BTreeMap<String, WorldLaw>,
    #[serde(default)]
    pub history: Vec<HistoryEvent>,
    #[serde(default)]
    pub storylets: BTreeMap<String, StoryletProposal>,
    /// A faction's mutable numbers, keyed by faction id: banked downtime time,
    /// and the `want_<thing>` / `have_<thing>` pairs that drive radiant quests.
    /// A faction's "sheet" at a different scale, but integers only for now --
    /// enough for banking and demand, and `Eq` (unlike the float-carrying
    /// `SheetData` a token holds). Promoting it to a full sheet a system can read
    /// through its Lua is the "abilities are projections" refinement. Unlike the
    /// immutable identity in [`Self::factions`], this changes as a faction acts,
    /// so it overwrites rather than insert-onces.
    #[serde(default)]
    pub faction_sheets: BTreeMap<String, BTreeMap<String, i64>>,
    /// Who plays each faction: faction id -> the player name granted its
    /// channel. A faction is an owner name like any other (a token owned by a
    /// faction id belongs to that faction), and this grant lets a player command
    /// that faction's tokens as if their own -- the per-channel permission that
    /// makes a faction *playable* rather than only DM-run. Absent means the DM
    /// runs it. Session state, not authored content, but it lives beside
    /// `faction_sheets` because both are the mutable per-faction layer.
    #[serde(default)]
    pub faction_control: BTreeMap<String, String>,
    /// Where each traveling party sits on the overmap: party owner -> place id.
    /// A split party (C3) keeps separate positions; a single party has one entry.
    /// Session play-state, like [`Self::faction_control`], not authored content.
    #[serde(default)]
    pub party_node: BTreeMap<String, String>,
    /// Each party's travel pace, as a percent of normal time: 100 is normal, 50
    /// is fast (half the time), 200 is slow (double). Absent reads as 100. The
    /// number is all the substrate keeps; what a pace *trades* (fast loses
    /// passive Perception, slow lets you forage) is system business.
    #[serde(default)]
    pub party_pace: BTreeMap<String, i64>,
    /// The overmap a party has discovered: party owner -> the place ids it knows.
    /// The rest of the map is hidden -- unseeable, unroutable -- until revealed by
    /// travel, word of mouth, a guide, a skill check, or a map read. Fog at
    /// overmap scale, with explored memory: once known, a place stays known.
    #[serde(default)]
    pub party_known: BTreeMap<String, BTreeSet<String>>,
    /// A party's named stores: party owner -> resource -> amount. Food gathered
    /// by foraging on the road lives here; the substrate keeps the count and the
    /// system decides what a store is worth and when it runs out.
    #[serde(default)]
    pub party_resources: BTreeMap<String, BTreeMap<String, i64>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldFaction {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub claims: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldPlace {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub map: Option<String>,
    /// Optional hand-placed overmap coordinate. `None` lets the render layout
    /// place the site (a force-directed relaxation from the routes); `Some`
    /// pins it, and the overmap honors authored positions when any place sets
    /// one. Integers keep [`CampaignWorld`] `Eq` (no float in the key state).
    #[serde(default)]
    pub position: Option<(i32, i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldCharacter {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub faction: Option<String>,
    #[serde(default)]
    pub place: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldRoute {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Travel weight for the overmap: the abstract cost of taking this route.
    /// A road and a mountain pass between the same two places can differ, which
    /// is the whole point of a pointcrawl (the swamp shortcut versus the safe
    /// road). Zero (the unauthored default) reads as 1 when projected, so an
    /// unweighted route is still traversable at unit cost.
    #[serde(default)]
    pub weight: u32,
}

/// A named rule of the generated setting. `parameters` is pack vocabulary;
/// system plugins opt into keys they understand rather than the substrate
/// hardcoding what iron, fire, names, oaths, or magic mean.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldLaw {
    pub id: String,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub id: String,
    /// Pack-defined chronological tick. Equal ticks retain authored order.
    pub time: i64,
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub participants: Vec<String>,
    #[serde(default)]
    pub place: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoryletRequirements {
    /// Every tag must be carried by at least one committed faction.
    #[serde(default)]
    pub faction_tags: Vec<String>,
    /// IDs are checked against the host-private store, without exposing text.
    #[serde(default)]
    pub hidden_facts: Vec<String>,
    #[serde(default)]
    pub world_laws: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSlot {
    pub key: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StoryletEffect {
    Fact { fact: WorldFact },
    History { event: HistoryEvent },
    Item { item: ItemProposal },
    LocalMap { map: LocalMapProposal },
}

/// A quality-based narrative opportunity. Matching and casting are pure;
/// committing each effect remains an explicit host operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoryletProposal {
    pub key: String,
    pub entry: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub requirements: StoryletRequirements,
    #[serde(default)]
    pub roles: Vec<RoleSlot>,
    #[serde(default)]
    pub effects: Vec<StoryletEffect>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryletResolution {
    pub cast: BTreeMap<String, String>,
    pub effects: Vec<StoryletEffect>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoryletError {
    MissingFactionTag(String),
    MissingHiddenFact(String),
    MissingWorldLaw(String),
    UncastRole(String),
}

