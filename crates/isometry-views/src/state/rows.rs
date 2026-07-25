//! The row and mode types the views render and the host fills.
//!
//! Plain data: a `MonsterRow` is what the compendium draws, not what the system
//! plugin knows. Keeping them here means a view never reaches into system or
//! campaign types, and the substrate stays free of any one game system.
//!
//! Split out of `state.rs` on 2026-07-24; behavior unchanged.

use super::*;

/// The system's sheet schema as plain data, so the view renders a sheet
/// without knowing any rules. The host (which owns the system plugin)
/// fills this in; the view stays system-agnostic.
#[derive(Clone, Debug, Default)]
pub struct SheetSchema {
    /// Editable fields: `(key, label, is_int)`.
    pub fields: Vec<(String, String, bool)>,
    /// Derived display stats: `(key, label)`.
    pub derived: Vec<(String, String)>,
    /// Rollable actions: `(key, label, targeted)`. A targeted action names a
    /// victim and is adjudicated; an untargeted one just produces a number.
    pub actions: Vec<(String, String, bool)>,
}

/// How initiative builds the turn order (a system choice over the same
/// turn list; `advance` just walks whatever order results).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitiativeMode {
    /// Each token rolls its own d20; the order sorts high to low.
    Individual,
    /// Each side rolls one d20; sides are ordered high to low and their
    /// tokens grouped, so a whole side acts before the next.
    SideBased,
}

impl InitiativeMode {
    pub fn label(self) -> &'static str {
        match self {
            InitiativeMode::Individual => "individual",
            InitiativeMode::SideBased => "side",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            InitiativeMode::Individual => InitiativeMode::SideBased,
            InitiativeMode::SideBased => InitiativeMode::Individual,
        }
    }
}

/// How a tile presents under fog of war for the current viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogLevel {
    /// In sight now: full render.
    Clear,
    /// Seen before, not in sight now: remembered terrain, dimmed, no
    /// live tokens.
    Dim,
    /// Never seen: not rendered at all.
    Hidden,
}

/// What a click on a tile does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditMode {
    Select,
    /// Paint the brush kind on the ground layer.
    PaintGround,
    /// Paint the brush kind on the prop layer.
    PaintProp,
    /// Flood-fill the clicked ground region with the brush kind.
    Fill,
    Raise,
    Lower,
    /// Place/remove tokens (toggle on click).
    Token,
    /// Hot-seat play: select a token, move within its reach.
    Play,
    /// Measure distance and preview area templates from a clicked anchor.
    Measure,
}

impl EditMode {
    pub const ALL: [EditMode; 9] = [
        EditMode::Select,
        EditMode::PaintGround,
        EditMode::PaintProp,
        EditMode::Fill,
        EditMode::Raise,
        EditMode::Lower,
        EditMode::Token,
        EditMode::Play,
        EditMode::Measure,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EditMode::Select => "Select",
            EditMode::PaintGround => "Paint",
            EditMode::PaintProp => "Prop",
            EditMode::Fill => "Fill",
            EditMode::Raise => "Raise",
            EditMode::Lower => "Lower",
            EditMode::Token => "Token",
            EditMode::Play => "Play",
            EditMode::Measure => "Measure",
        }
    }

    /// Modes where holding the button and dragging keeps applying.
    pub fn drags(self) -> bool {
        matches!(
            self,
            EditMode::PaintGround | EditMode::PaintProp | EditMode::Raise | EditMode::Lower
        )
    }
}

/// One undoable editor step: the inverses of the events it applied, in
/// reverse application order (so replaying the list undoes the step).
pub(crate) type Step = Vec<SessionEvent>;

/// Whether this app owns its state or mirrors a networked session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetMode {
    /// Solo / hot-seat: mutations apply locally with undo (the editor).
    Local,
    /// In a session: Play moves and turn changes become [`GameEvent`]s
    /// routed to the host authority, and the map/turns render from the
    /// replicated snapshot (no optimistic mutation). Editing is a
    /// Local-mode, offline activity, so editor actions are inert here.
    Remote,
}

/// One monster action, view-side.
#[derive(Clone)]
pub struct ActionRow {
    pub name: String,
    pub to_hit: Option<i32>,
    pub damage: Option<String>,
    pub desc: String,
}

/// A compendium row: a monster reduced to what the index shows, the page
/// displays, and the board spawns. The host fills these from the system's
/// bestiary, so the view names no rules (like [`SheetSchema`]).
#[derive(Clone)]
pub struct MonsterRow {
    pub key: String,
    pub name: String,
    pub cr: f32,
    pub cr_label: String,
    pub kind: String,
    pub size: String,
    pub alignment: String,
    pub hp: i32,
    pub hit_dice: String,
    pub ac: i32,
    pub speed_ft: i32,
    pub xp: i32,
    pub abilities: [i32; 6],
    pub actions: Vec<ActionRow>,
    pub sprite: String,
}

/// Which compendium namespace is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompendiumTab {
    Monsters,
    Spells,
    Items,
}

impl CompendiumTab {
    pub const ALL: [CompendiumTab; 3] = [Self::Monsters, Self::Spells, Self::Items];
    pub fn label(self) -> &'static str {
        match self {
            Self::Monsters => "Monsters",
            Self::Spells => "Spells",
            Self::Items => "Items",
        }
    }
}

/// A compendium spell row (host-supplied, view-side).
#[derive(Clone)]
pub struct SpellRow {
    pub key: String,
    pub name: String,
    pub level: u8,
    pub level_label: String,
    pub school: String,
    pub casting_time: String,
    pub range: String,
    pub components: String,
    pub duration: String,
    pub desc: String,
}

/// A compendium item row (host-supplied, view-side).
#[derive(Clone)]
pub struct ItemRow {
    pub key: String,
    pub name: String,
    pub category: String,
    pub cost: String,
    pub weight: String,
    pub detail: String,
    pub desc: String,
}

/// A host-authoritative inventory mutation requested by the view. The host
/// mints item ids and commits `GameEvent::InventorySet`; a player client never
/// gets the authoring controls in the first place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryRequest {
    AddCompendiumItem {
        token: TokenId,
        template: String,
        name: String,
        category: String,
    },
    Equip {
        token: TokenId,
        slot: EquipmentSlot,
        item: ItemId,
    },
    Unequip {
        token: TokenId,
        slot: EquipmentSlot,
    },
    Transfer {
        from: TokenId,
        to: TokenId,
        item: ItemId,
    },
}

/// A one-shot request from the generator preview surface. The desktop host
/// evaluates/commits it; views never load packs or run Lua.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationRequest {
    Generate,
    Commit,
}

/// One narrative opportunity as the DM sees it. Host-projected: the app resolves
/// the storylet's requirements (including host-private secret facts) and casting
/// once, and hands the view only the result. `cast` is role -> character name;
/// `status` explains a `!available` row (a missing faction, an uncast role).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryletRow {
    pub key: String,
    pub entry: String,
    pub available: bool,
    pub status: String,
    pub cast: Vec<(String, String)>,
}

/// One rolled faction move as the DM sees it in the downtime surface. Display
/// only: the real `FactionMove` (its world events) lives in the host app, which
/// commits the ones the DM keeps. `struck` is the DM's edit -- a kept move
/// commits, a struck one drops from the tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactionMoveRow {
    pub faction: String,
    pub verb: String,
    pub text: String,
    pub has_change: bool,
    pub struck: bool,
}

/// One host-projected candidate in an unresolved campaign-governance
/// conflict. Labels are presentation data; signed proposal ids remain the
/// request identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernanceBindingRow {
    pub proposal: [u8; 32],
    pub moot: String,
    pub policy: String,
    pub endorsements: u32,
    pub required: u32,
    pub claims: u32,
}

/// A conflict the collaboration actor has determined is eligible for an
/// explicit adopt-or-branch decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernanceConflict {
    pub candidates: Vec<GovernanceBindingRow>,
    pub can_adopt: bool,
    pub can_branch: bool,
    pub restriction: Option<String>,
}

/// One-shot intent drained by the collaboration host. The host constructs,
/// signs, and publishes the durable resolution proposal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GovernanceResolutionRequest {
    Adopt { selected: [u8; 32] },
    Branch { candidates: Vec<[u8; 32]> },
}

