use std::collections::{BTreeMap, HashMap, HashSet};

use isometry_campaign::{
    CampaignMap, CampaignWorld, EquipmentSlot, GenValue, GenerationRecord, GeneratorChoice,
    Inventory, ItemId,
};
use isometry_core::{
    apply, distance, reachable, roll, template_tiles, visible_from, Facing, IsoGeometry, Layer,
    MapDocument, MoveRules, Rng, RollRecord, SessionEvent, SightRules, TemplateKind, TileCoord,
    TileKindId, Token, TokenId, TurnList,
};
use isometry_net::{apply_game, GameEvent, GameSnapshot, ROLL_LOG_CAP};

/// Fixed side-panel width in logical px (CSS `.side` width plus its
/// padding); the host uses it to keep drag painting off the panel.
pub const PANEL_W: f32 = 228.0;

/// Default move budget until system plugins supply speed stats (I6).
/// Fallback move budget for a sheetless token. The real number is
/// system-driven: sheet `speed` projected through conditions into
/// `MapDocument::mobility` (next-horizons B.5, answered).
const MOVE_BUDGET: u32 = 5;

/// Default token sight radius until system plugins supply per-token
/// senses. Configurable via [`UiState::sight_radius`].
const SIGHT_RADIUS: u32 = 6;

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
type Step = Vec<SessionEvent>;

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

/// Runner state: the substrate document plus view-layer concerns
/// (camera, selection, editor).
pub struct UiState {
    pub map: MapDocument,
    pub geo: IsoGeometry,
    /// Board-origin offset within the pane, logical px. Snap-scrolled by
    /// whole tile steps (the tactics references scroll in steps; the
    /// smooth-pan lane waits on the netrender camera-offset composite).
    pub camera: (f32, f32),
    /// The board pane's logical size in px, set by the host on init and
    /// resize. Drives viewport culling in `board_root`. `(0, 0)` means the
    /// host has not reported it yet, in which case culling is skipped (emit
    /// everything, the safe pre-windowing behavior).
    pub viewport: (f32, f32),
    pub selected: Option<TileCoord>,
    pub mode: EditMode,
    /// Palette selection painted by `PaintGround` / `PaintProp` / `Fill`.
    pub brush: TileKindId,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// One-line feedback under the palette.
    pub status: String,
    /// One-shot host requests (the host consumes and clears these).
    pub save_requested: bool,
    pub load_requested: bool,
    /// Local vs networked-session behavior.
    pub net_mode: NetMode,
    /// In `Remote` mode, game events the app should send to the session
    /// (the host drains these each frame). Empty in `Local` mode.
    pub net_outbox: Vec<GameEvent>,
    /// Whose eyes the board renders through. `None` is omniscient (the
    /// DM / a spectator). `Some(player)` shows fog of war from that
    /// player's tokens.
    pub viewer: Option<String>,
    /// Sight radius for fog computation.
    pub sight_radius: u32,
    /// Tiles currently in sight of the viewer's tokens (fog active only).
    pub visible: HashSet<TileCoord>,
    /// Tiles the viewer has ever seen (remembered terrain under fog).
    pub explored: HashSet<TileCoord>,
    /// The shared roll log (most recent last). Mirrored from the session
    /// snapshot in Remote mode; kept locally in Local mode.
    pub roll_log: Vec<RollRecord>,
    /// Public inventory/equipment state mirrored from an authoritative
    /// snapshot. The UI projects it; item instances remain campaign data.
    pub inventories: BTreeMap<TokenId, Inventory>,
    /// Dice generator. Seeded deterministically; the host reseeds with
    /// real entropy at startup.
    rng: Rng,
    /// How "roll initiative" orders the turn list.
    pub initiative_mode: InitiativeMode,
    /// Measure mode: the clicked anchor, the area shape, and its size.
    pub measure_anchor: Option<TileCoord>,
    pub template_kind: TemplateKind,
    pub template_size: u32,
    /// The message log (whispers sent and received), display strings.
    pub messages: Vec<String>,
    /// The `>` command line. `command_active` captures keystrokes into
    /// `command_draft` (the whisper-composer pattern); `command_results` holds
    /// the last `>find` list, shown until the next command.
    pub command_active: bool,
    pub command_draft: String,
    pub command_results: Vec<String>,
    /// Whether a whisper is being typed (keys route to the draft).
    pub composing: bool,
    /// The whisper being typed.
    pub whisper_draft: String,
    /// Who a composed whisper goes to (a player name); the DM cycles it.
    pub whisper_target: Option<String>,
    /// Whispers to send: `(target, text)`, drained by the host bridge.
    pub whisper_outbox: Vec<(String, String)>,
    /// Connected player names the DM can whisper to (set by the host
    /// bridge). Empty solo.
    pub connected_players: Vec<String>,
    /// Character sheets: the schema (host-supplied), which token's sheet
    /// is open, its precomputed derived stats (host-supplied), and the
    /// one-shot requests the host drains to bind/edit/roll.
    pub sheet_schema: SheetSchema,
    pub open_sheet: Option<TokenId>,
    /// The host's transient rules projection of the open sheet after public
    /// equipped-item modifiers. The stored map sheet remains unmodified.
    pub sheet_effective: Option<isometry_core::SheetData>,
    pub sheet_derived: BTreeMap<String, i64>,
    pub bind_sheet_request: Option<TokenId>,
    pub sheet_edit: Option<(TokenId, String, i64)>,
    pub sheet_action: Option<(TokenId, String)>,
    /// Target-pick mode: `(actor, action_key)` is waiting for the player to
    /// click a victim. An untargeted action never enters this state; it just
    /// rolls. Escape cancels.
    pub action_pick: Option<(TokenId, String)>,
    /// A committed intent the host drains, validates, and adjudicates:
    /// `(actor, target, action_key)`. The view never resolves anything itself.
    pub action_intent: Option<(TokenId, TokenId, String)>,
    /// Beats currently playing, token to beat name. Purely representational:
    /// the view sets a class, the engine's animation clock runs it, and it is
    /// cleared when the clock says nothing is animating.
    pub beats: BTreeMap<TokenId, String>,
    /// The `beat_seq` whose beats have already been staged, so a replicated
    /// action plays once rather than on every snapshot mirror.
    pub beat_seq: u64,
    /// A monster spawn awaiting its stat block: `(token, monster key)`. The host
    /// owns the system, so it is what turns a compendium row into a sheet.
    pub spawn_sheet_request: Option<(TokenId, String)>,
    /// A request to clear one condition (`(token, name)`, standing up from
    /// prone). The host rules on it, because clearing a condition means
    /// recomputing what the token can do, and the rules live there.
    pub clear_condition_request: Option<(TokenId, String)>,
    pub inventory_request: Option<InventoryRequest>,
    /// False for a joined player. The host still validates its own event path;
    /// this only keeps DM authoring controls out of player UI.
    pub can_edit_inventory: bool,
    /// Public commit-result records mirrored from a session snapshot. The W2
    /// preview table will project this ledger; content scripts never run here.
    pub generations: Vec<GenerationRecord>,
    pub campaign_maps: BTreeMap<String, CampaignMap>,
    /// Each location's clock in ticks (see `GameSnapshot::clocks`): rounds tick
    /// it automatically, the DM's pass-time verb adds the downtime, and travel
    /// pulls the destination up to the traveler.
    pub clocks: BTreeMap<String, u64>,
    /// Tokens per player (see `GameSnapshot::party_cap`), mirrored so the host
    /// can gate a recruit against it.
    pub party_cap: u32,
    pub active_map: Option<String>,
    pub world: CampaignWorld,
    /// Generator preview state is local to the host until `Commit`; players
    /// receive only the resulting public record through a snapshot.
    pub generator_open: bool,
    pub generator_preview: Option<GenerationRecord>,
    pub generator_choices: Vec<GeneratorChoice>,
    pub generator_selected: usize,
    pub generator_locks: BTreeMap<String, GenValue>,
    pub generation_request: Option<GenerationRequest>,
    /// The storylet surface (C6, "dialogue"): host-computed rows of the
    /// campaign's narrative opportunities, whether each is currently playable
    /// (its requirements met and roles cast), and the DM's request to play one.
    /// Host-only: matching reads host-private secret facts, so a joined client
    /// never receives this.
    pub storylets: Vec<StoryletRow>,
    pub storylet_open: bool,
    pub storylet_selected: usize,
    /// The key of the storylet the DM asked to play; the host drains it, commits
    /// its effects, and they replicate.
    pub storylet_request: Option<String>,
    /// Host-only downtime surface: the DM rolls a faction tick, edits the batch
    /// by striking moves, and commits the keepers. These rows are display; the
    /// host app holds the real moves and commits the un-struck ones. A joined
    /// client never rolls a tick (it reads the world and spends host entropy).
    pub faction_moves: Vec<FactionMoveRow>,
    pub downtime_open: bool,
    pub downtime_selected: usize,
    /// One-shot: the DM asked for a fresh tick; the host rolls it and fills rows.
    pub downtime_roll_request: bool,
    /// One-shot: the DM committed the kept moves; the host drains and commits.
    pub downtime_commit_request: bool,
    /// The overmap surface (C8 exploration): the party's pointcrawl. Drawn from
    /// `self.world` (projected places + routes); the party's current node comes
    /// from `world.party_node`. Clicking a node arms a travel to it, which the
    /// host adjudicates.
    pub overmap_open: bool,
    /// One-shot: the node the local party asked to travel to; the host resolves
    /// the trip (roll, time, whether they get lost) and moves the party.
    pub overmap_travel_request: Option<String>,
    /// Host-fed competing-binding projection and one-shot resolution request.
    /// The view never reads Moot stores or signs campaign operations.
    pub governance_conflict: Option<GovernanceConflict>,
    pub governance_conflict_open: bool,
    pub governance_selected: usize,
    pub governance_resolution_request: Option<GovernanceResolutionRequest>,
    /// Sprite the Token mode places.
    pub token_sprite: String,
    /// Play state: the substrate turn order.
    pub turns: TurnList,
    /// Play state: the token being moved.
    pub selected_token: Option<TokenId>,
    /// Play state: reach of the selected token (tile -> previous tile
    /// on its shortest path).
    pub reach: HashMap<TileCoord, TileCoord>,
    /// Tile under the cursor (path preview follows it in Play mode).
    pub hover_tile: Option<TileCoord>,
    /// Right-click context menu: the token it targets and the pane-space
    /// position (logical px) to anchor it at. `None` when closed.
    pub context_menu: Option<(TokenId, (f32, f32))>,
    /// The SRD compendium (host-supplied view-side rows) and its overlay
    /// state: the open flag, the grid's scroll offset, and the sort
    /// (column index, descending).
    pub bestiary: Vec<MonsterRow>,
    /// Emotes the loaded packs offer: `(beat name, menu label)`. Empty when no
    /// pack declares any, in which case the menu simply has no emotes: the app
    /// does not own this vocabulary.
    pub emotes: Vec<(String, String)>,
    pub compendium_open: bool,
    pub compendium_scroll: f32,
    pub compendium_sort: (usize, bool),
    /// The compendium's open entry page (its key), or `None` for the index.
    pub compendium_selected: Option<String>,
    /// Current filter text for the compendium index (name substring).
    pub compendium_search: String,
    /// Which compendium namespace is showing.
    pub compendium_tab: CompendiumTab,
    /// Host-supplied compendium content for the Spells and Items tabs.
    pub spells: Vec<SpellRow>,
    pub items: Vec<ItemRow>,
}

impl UiState {
    pub fn new(map: MapDocument) -> Self {
        Self {
            map,
            geo: IsoGeometry::default(),
            camera: (0.0, 0.0),
            viewport: (0.0, 0.0),
            selected: None,
            mode: EditMode::Select,
            brush: TileKindId(1),
            undo: Vec::new(),
            redo: Vec::new(),
            status: String::new(),
            save_requested: false,
            load_requested: false,
            token_sprite: "knight".to_owned(),
            turns: TurnList::new(),
            selected_token: None,
            reach: HashMap::new(),
            hover_tile: None,
            context_menu: None,
            net_mode: NetMode::Local,
            net_outbox: Vec::new(),
            viewer: None,
            sight_radius: SIGHT_RADIUS,
            visible: HashSet::new(),
            explored: HashSet::new(),
            roll_log: Vec::new(),
            inventories: BTreeMap::new(),
            rng: Rng::new(1),
            initiative_mode: InitiativeMode::Individual,
            measure_anchor: None,
            template_kind: TemplateKind::Burst,
            template_size: 3,
            messages: Vec::new(),
            command_active: false,
            command_draft: String::new(),
            command_results: Vec::new(),
            composing: false,
            whisper_draft: String::new(),
            whisper_target: None,
            whisper_outbox: Vec::new(),
            connected_players: Vec::new(),
            sheet_schema: SheetSchema::default(),
            open_sheet: None,
            sheet_effective: None,
            sheet_derived: BTreeMap::new(),
            bind_sheet_request: None,
            sheet_edit: None,
            sheet_action: None,
            action_pick: None,
            action_intent: None,
            beats: BTreeMap::new(),
            beat_seq: 0,
            spawn_sheet_request: None,
            clear_condition_request: None,
            inventory_request: None,
            can_edit_inventory: true,
            generations: Vec::new(),
            campaign_maps: BTreeMap::new(),
            clocks: BTreeMap::new(),
            party_cap: isometry_net::default_party_cap(),
            active_map: None,
            world: CampaignWorld::default(),
            storylets: Vec::new(),
            storylet_open: false,
            storylet_selected: 0,
            storylet_request: None,
            faction_moves: Vec::new(),
            downtime_open: false,
            downtime_selected: 0,
            downtime_roll_request: false,
            downtime_commit_request: false,
            overmap_open: false,
            overmap_travel_request: None,
            generator_open: false,
            generator_preview: None,
            generator_choices: Vec::new(),
            generator_selected: 0,
            generator_locks: BTreeMap::new(),
            generation_request: None,
            governance_conflict: None,
            governance_conflict_open: false,
            governance_selected: 0,
            governance_resolution_request: None,
            bestiary: Vec::new(),
            emotes: Vec::new(),
            compendium_open: false,
            compendium_scroll: 0.0,
            compendium_sort: (0, false),
            compendium_selected: None,
            compendium_search: String::new(),
            compendium_tab: CompendiumTab::Monsters,
            spells: Vec::new(),
            items: Vec::new(),
        }
    }

    /// Open the selected token's sheet, requesting the host bind a fresh
    /// one first if it has none.
    pub fn open_or_bind_sheet(&mut self) {
        let Some(id) = self.selected_token.or_else(|| self.turns.active()) else {
            self.status = "select a token first".to_owned();
            return;
        };
        if self.map.sheet(id).is_none() {
            self.bind_sheet_request = Some(id);
        }
        self.open_sheet = Some(id);
    }

    pub fn close_sheet(&mut self) {
        self.open_sheet = None;
        self.sheet_effective = None;
    }

    /// Queue a GM-side item instance from an SRD/content-pack entry for the
    /// currently open sheet. The host assigns the item id and commits it.
    pub fn request_compendium_item(&mut self, item: &ItemRow) {
        let Some(token) = self.open_sheet else {
            self.status = "open a character sheet first".to_owned();
            return;
        };
        if !self.can_edit_inventory {
            self.status = "inventory changes require the host".to_owned();
            return;
        }
        self.inventory_request = Some(InventoryRequest::AddCompendiumItem {
            token,
            template: item.key.clone(),
            name: item.name.clone(),
            category: item.category.clone(),
        });
    }

    pub fn request_equip(&mut self, item: ItemId) {
        let Some(token) = self.open_sheet else {
            return;
        };
        if self.can_edit_inventory {
            self.inventory_request = Some(InventoryRequest::Equip {
                token,
                slot: EquipmentSlot::MainHand,
                item,
            });
        }
    }

    pub fn request_unequip_main_hand(&mut self) {
        if self.can_edit_inventory {
            if let Some(token) = self.open_sheet {
                self.inventory_request = Some(InventoryRequest::Unequip {
                    token,
                    slot: EquipmentSlot::MainHand,
                });
            }
        }
    }

    pub fn request_transfer(&mut self, to: TokenId, item: ItemId) {
        if self.can_edit_inventory {
            if let Some(from) = self.open_sheet {
                if from != to {
                    self.inventory_request = Some(InventoryRequest::Transfer { from, to, item });
                }
            }
        }
    }

    /// Open the W2 host-only generator preview. The initial bundled pack has
    /// one item generator; later pack discovery will populate this selector.
    /// Open the storylet surface (the DM's dialogue/scene menu). Host-only, like
    /// generation: matching a storylet reads secret facts a client never holds.
    pub fn open_storylets(&mut self) {
        if !self.can_edit_inventory {
            self.status = "storylets are the DM's".to_owned();
        } else {
            self.storylet_open = true;
            self.storylet_selected = 0;
        }
    }

    pub fn close_storylets(&mut self) {
        self.storylet_open = false;
    }

    pub fn cycle_storylet(&mut self) {
        if !self.storylets.is_empty() {
            self.storylet_selected = (self.storylet_selected + 1) % self.storylets.len();
        }
    }

    pub fn selected_storylet(&self) -> Option<&StoryletRow> {
        self.storylets.get(self.storylet_selected)
    }

    /// Ask the host to play the selected storylet: commit its effects (facts,
    /// history, items, maps), which replicate. Only a playable one can be run.
    pub fn play_storylet(&mut self) {
        if !self.can_edit_inventory {
            self.status = "storylets are the DM's".to_owned();
            return;
        }
        let picked = self
            .selected_storylet()
            .map(|row| (row.available, row.key.clone(), row.entry.clone(), row.status.clone()));
        match picked {
            Some((true, key, entry, _)) => {
                self.storylet_request = Some(key);
                self.status = format!("playing: {entry}");
            }
            Some((false, _, _, status)) => self.status = format!("not yet: {status}"),
            None => self.status = "no storylet selected".to_owned(),
        }
    }

    /// Open the downtime surface and ask the host to roll a faction tick.
    /// Host-only, like storylets: the roll reads the world and spends entropy
    /// the host owns, so a joined client never sees it.
    pub fn open_downtime(&mut self) {
        if !self.can_edit_inventory {
            self.status = "downtime is the DM's".to_owned();
            return;
        }
        self.downtime_open = true;
        self.downtime_selected = 0;
        self.downtime_roll_request = true;
    }

    pub fn close_downtime(&mut self) {
        self.downtime_open = false;
    }

    pub fn cycle_downtime(&mut self) {
        if !self.faction_moves.is_empty() {
            self.downtime_selected = (self.downtime_selected + 1) % self.faction_moves.len();
        }
    }

    /// Roll a fresh tick, discarding the current batch and its edits.
    pub fn reroll_downtime(&mut self) {
        if !self.can_edit_inventory {
            return;
        }
        self.downtime_selected = 0;
        self.downtime_roll_request = true;
    }

    pub fn selected_downtime_move(&self) -> Option<&FactionMoveRow> {
        self.faction_moves.get(self.downtime_selected)
    }

    /// Strike or keep the selected move: the DM's edit before commit. A struck
    /// move is dropped from the tick; a kept one commits.
    pub fn toggle_strike_downtime(&mut self) {
        if let Some(row) = self.faction_moves.get_mut(self.downtime_selected) {
            row.struck = !row.struck;
        }
    }

    /// Commit the kept moves. Arms a one-shot the host drains and commits; a
    /// batch with everything struck commits nothing.
    pub fn commit_downtime(&mut self) {
        if !self.can_edit_inventory {
            self.status = "downtime is the DM's".to_owned();
            return;
        }
        let kept = self.faction_moves.iter().filter(|m| !m.struck).count();
        if kept == 0 {
            self.status = "no moves kept to commit".to_owned();
            return;
        }
        self.downtime_commit_request = true;
        self.status = format!("committing {kept} downtime move(s)");
    }

    /// Open the overmap: the party's pointcrawl of known places and routes.
    /// Anyone at the table may look at where the party is; travelling is gated
    /// downstream (the host adjudicates, and only a controller's request lands).
    pub fn open_overmap(&mut self) {
        self.overmap_open = true;
    }

    pub fn close_overmap(&mut self) {
        self.overmap_open = false;
    }

    /// Ask to travel to `node`. Arms a one-shot the host drains: it rolls the
    /// navigation, spends the time, and moves the party, or refuses if there is
    /// no route. The view never decides the outcome.
    pub fn request_travel(&mut self, node: String) {
        self.overmap_travel_request = Some(node);
    }

    pub fn open_generator(&mut self) {
        if !self.can_edit_inventory {
            self.status = "generation requires the host".to_owned();
        } else if self.generator_choices.is_empty() {
            self.status = "no generator packs loaded".to_owned();
        } else {
            self.generator_open = true;
        }
    }

    pub fn close_generator(&mut self) {
        self.generator_open = false;
        self.generator_preview = None;
        self.generation_request = None;
    }

    pub fn request_generation(&mut self) {
        if self.can_edit_inventory {
            self.generation_request = Some(GenerationRequest::Generate);
        } else {
            self.status = "generation requires the host".to_owned();
        }
    }

    pub fn cycle_generator(&mut self) {
        if !self.generator_choices.is_empty() {
            self.generator_selected = (self.generator_selected + 1) % self.generator_choices.len();
            self.generator_preview = None;
            self.generator_locks.clear();
            self.generation_request = None;
        }
    }

    pub fn selected_generator(&self) -> Option<&GeneratorChoice> {
        self.generator_choices.get(self.generator_selected)
    }

    /// Toggle the selected generator's first declared lock preset. A lock is
    /// a visible value passed to each reroll, never entropy replay.
    pub fn toggle_generator_lock(&mut self) {
        if !self.can_edit_inventory {
            self.status = "generation requires the host".to_owned();
            return;
        }
        let Some(preset) = self
            .selected_generator()
            .and_then(|choice| choice.lock_presets.first())
            .cloned()
        else {
            self.status = "selected generator has no lock presets".to_owned();
            return;
        };
        if self.generator_locks.remove(&preset.key).is_some() {
            self.status = format!("unlocked {}", preset.label);
        } else {
            self.generator_locks.insert(preset.key, preset.value);
            self.status = format!("locked {}", preset.label);
        }
    }

    pub fn commit_generation_preview(&mut self) {
        if self.can_edit_inventory && self.generator_preview.is_some() {
            self.generation_request = Some(GenerationRequest::Commit);
        }
    }

    pub fn discard_generation_preview(&mut self) {
        self.generator_preview = None;
        self.generation_request = None;
        self.status = "discarded generation preview".to_owned();
    }

    pub fn open_governance_conflict(&mut self) {
        if self
            .governance_conflict
            .as_ref()
            .is_some_and(|conflict| conflict.candidates.len() >= 2)
        {
            self.governance_selected = self.governance_selected.min(
                self.governance_conflict
                    .as_ref()
                    .map_or(0, |conflict| conflict.candidates.len() - 1),
            );
            self.governance_conflict_open = true;
        } else {
            self.status = "no competing campaign bindings".to_owned();
        }
    }

    pub fn close_governance_conflict(&mut self) {
        self.governance_conflict_open = false;
    }

    pub fn select_governance_candidate(&mut self, index: usize) {
        if self
            .governance_conflict
            .as_ref()
            .is_some_and(|conflict| index < conflict.candidates.len())
        {
            self.governance_selected = index;
        }
    }

    pub fn request_governance_adopt(&mut self) {
        let Some(conflict) = &self.governance_conflict else {
            return;
        };
        if !conflict.can_adopt {
            self.status = conflict
                .restriction
                .clone()
                .unwrap_or_else(|| "this conflict cannot be adopted".to_owned());
            return;
        }
        let Some(candidate) = conflict.candidates.get(self.governance_selected) else {
            return;
        };
        self.governance_resolution_request = Some(GovernanceResolutionRequest::Adopt {
            selected: candidate.proposal,
        });
        self.governance_conflict_open = false;
    }

    pub fn request_governance_branch(&mut self) {
        let Some(conflict) = &self.governance_conflict else {
            return;
        };
        if !conflict.can_branch {
            self.status = conflict
                .restriction
                .clone()
                .unwrap_or_else(|| "this conflict cannot be branched".to_owned());
            return;
        }
        self.governance_resolution_request = Some(GovernanceResolutionRequest::Branch {
            candidates: conflict
                .candidates
                .iter()
                .map(|candidate| candidate.proposal)
                .collect(),
        });
        self.governance_conflict_open = false;
    }

    /// Open the SRD compendium overlay.
    pub fn open_compendium(&mut self) {
        self.compendium_open = true;
    }

    pub fn close_compendium(&mut self) {
        self.compendium_open = false;
        self.compendium_search.clear();
    }

    /// Append a character to the compendium filter.
    pub fn search_char(&mut self, c: char) {
        self.compendium_search.push(c);
        self.compendium_scroll = 0.0;
    }

    /// Delete the last filter character.
    pub fn search_backspace(&mut self) {
        self.compendium_search.pop();
        self.compendium_scroll = 0.0;
    }

    /// Clear the compendium filter.
    pub fn clear_compendium_search(&mut self) {
        self.compendium_search.clear();
        self.compendium_scroll = 0.0;
    }

    /// Escape in the compendium: from a page back to the index, else close.
    pub fn compendium_escape(&mut self) {
        if self.compendium_selected.is_some() {
            self.back_to_index();
        } else {
            self.close_compendium();
        }
    }

    /// Sort the compendium by a column: the same column toggles direction, a
    /// new column starts ascending. Resets scroll.
    pub fn sort_compendium(&mut self, col: usize) {
        if self.compendium_sort.0 == col {
            self.compendium_sort.1 = !self.compendium_sort.1;
        } else {
            self.compendium_sort = (col, false);
        }
        self.compendium_scroll = 0.0;
    }

    /// Open an entry's page in the current compendium tab.
    pub fn open_entry(&mut self, key: String) {
        self.compendium_selected = Some(key);
    }

    /// Switch the compendium namespace, returning to that tab's index.
    pub fn set_compendium_tab(&mut self, tab: CompendiumTab) {
        self.compendium_tab = tab;
        self.compendium_selected = None;
        self.compendium_sort = (0, false);
        self.compendium_scroll = 0.0;
        self.compendium_search.clear();
    }

    /// Scroll the compendium grid by wheel `dy`, clamped to `max`.
    pub fn scroll_compendium(&mut self, dy: f32, max: f32) {
        self.compendium_scroll = (self.compendium_scroll + dy).clamp(0.0, max);
    }

    /// Back from a monster page to the index.
    pub fn back_to_index(&mut self) {
        self.compendium_selected = None;
    }

    /// Spawn a bestiary monster as a token on the board (a Local editor
    /// action, reusing the token-placement path). Places on the selected tile
    /// if free, else the nearest free tile, then closes the compendium so the
    /// new token is visible.
    pub fn spawn_monster(&mut self, key: &str) {
        let Some(m) = self.bestiary.iter().find(|m| m.key == key) else {
            return;
        };
        let (sprite, name, key) = (m.sprite.clone(), m.name.clone(), m.key.clone());
        let at = self.free_spawn_tile();
        let id = self.next_token_id();
        let placed = SessionEvent::TokenPlaced(Token {
            id,
            at,
            facing: Facing::South,
            sprite,
            owner: None,
        });
        // In a session the placement must land authoritatively, or the token
        // shows only on the DM's screen and is wiped by the next snapshot mirror
        // (leaving an orphan sheet behind, since the SheetSet below still
        // replicates). Route it through the authority like every other mutator;
        // apply locally only when solo.
        if !self.net_emit(GameEvent::Map(placed.clone())) {
            self.apply_step(vec![placed]);
        }
        // Ask the host to bind the stat block. Without this the monster is a
        // sprite: its hit points and AC stay in the compendium and nothing can
        // be done to it. The view does not build the sheet itself, because what
        // fields a 5e creature has is the system's business, not the board's.
        self.spawn_sheet_request = Some((id, key));
        self.status = format!("spawned {name}");
        self.compendium_open = false;
        self.compendium_selected = None;
    }

    /// A free tile to spawn onto: the selection if empty, else scanning a small
    /// block outward from it. Bounds-checked, because placing a token off-map is
    /// rejected (and on a narrow map the block can walk off the edge).
    fn free_spawn_tile(&self) -> TileCoord {
        let free = |at: TileCoord| {
            self.map.ground.in_bounds(at.0, at.1) && self.token_at(at).is_none()
        };
        let start = self.selected.filter(|&s| free(s)).unwrap_or((2, 2));
        for d in 0..64 {
            let at = (start.0 + (d % 8), start.1 + (d / 8));
            if free(at) {
                return at;
            }
        }
        let (w, h) = (self.map.ground.width() as i32, self.map.ground.height() as i32);
        for row in 0..h {
            for col in 0..w {
                if free((col, row)) {
                    return (col, row);
                }
            }
        }
        (0, 0)
    }

    /// Queue a field edit (a stepper on the open sheet); the host applies
    /// and replicates it.
    pub fn request_sheet_edit(&mut self, key: &str, delta: i64) {
        if let Some(id) = self.open_sheet {
            self.sheet_edit = Some((id, key.to_owned(), delta));
        }
    }

    /// Queue an action roll; the host evaluates it against the system.
    /// Click an action on the open sheet.
    ///
    /// An untargeted action (an ability check) rolls immediately, as it always
    /// has. A targeted one cannot: it needs a victim, so it arms target-pick
    /// mode and the next click on a token becomes the intent.
    pub fn request_action(&mut self, key: &str) {
        let Some(id) = self.open_sheet else {
            return;
        };
        let targeted = self
            .sheet_schema
            .actions
            .iter()
            .any(|(k, _, targeted)| k == key && *targeted);
        if targeted {
            let label = self
                .sheet_schema
                .actions
                .iter()
                .find(|(k, _, _)| k == key)
                .map(|(_, l, _)| l.clone())
                .unwrap_or_else(|| key.to_owned());
            self.action_pick = Some((id, key.to_owned()));
            self.status = format!("{label}: pick a target (Esc to cancel)");
        } else {
            self.sheet_action = Some((id, key.to_owned()));
        }
    }

    /// Whether the board is waiting for the player to click a victim.
    pub fn picking_target(&self) -> bool {
        self.action_pick.is_some()
    }

    /// Cancel target-pick without spending anything.
    pub fn cancel_action_pick(&mut self) {
        if self.action_pick.take().is_some() {
            self.status = "action cancelled".to_owned();
        }
    }

    /// Commit the victim. This only *asks*: the host validates reach and turn
    /// ownership and the system decides the outcome. Nothing is resolved here.
    pub fn pick_action_target(&mut self, target: TokenId) {
        let Some((actor, key)) = self.action_pick.take() else {
            return;
        };
        if actor == target {
            self.status = "cannot target yourself".to_owned();
            return;
        }
        if self.map.is_defeated(target) {
            self.status = "that one is already down".to_owned();
            return;
        }
        self.action_intent = Some((actor, target, key));
    }

    /// The DM declares time passing on the active location: the downtime verb
    /// beside the automatic round tick. In a session it routes to the host;
    /// solo it applies directly. No stored map means no clock to keep.
    pub fn pass_time(&mut self, ticks: u64) {
        let Some(active) = self.active_map.clone() else {
            self.status = "no campaign clock without a stored map".to_owned();
            return;
        };
        if self.net_emit(GameEvent::TimeAdvanced { ticks }) {
            return;
        }
        *self.clocks.entry(active.clone()).or_insert(0) += ticks;
        self.status = format!("time passes: {} on {active}", self.clock_now());
    }

    /// The active location's clock, in ticks.
    pub fn clock_now(&self) -> u64 {
        self.active_map
            .as_ref()
            .and_then(|id| self.clocks.get(id))
            .copied()
            .unwrap_or(0)
    }

    /// The transition point on the active map at `at`, if any: the door the
    /// board renders and a token walks through.
    pub fn transition_at(&self, at: TileCoord) -> bool {
        let Some(active) = &self.active_map else {
            return false;
        };
        self.campaign_maps
            .get(active)
            .map(|m| {
                m.transitions
                    .iter()
                    .any(|t| (t.at.col as i32, t.at.row as i32) == at)
            })
            .unwrap_or(false)
    }

    /// Every door tile on the active map, for the board to render.
    pub fn door_tiles(&self) -> HashSet<TileCoord> {
        let Some(active) = &self.active_map else {
            return HashSet::new();
        };
        self.campaign_maps
            .get(active)
            .map(|m| {
                m.transitions
                    .iter()
                    .map(|t| (t.at.col as i32, t.at.row as i32))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Walk `token` through the door it stands on (solo / hot-seat path).
    ///
    /// Deliberately *not* a reimplementation: it builds a scratch snapshot and
    /// runs the same `apply_game` travel logic every networked peer runs, then
    /// copies the outcome back. One crossing, one set of rules, whoever hosts.
    pub fn travel(&mut self, token: TokenId) {
        // The stored copy of the active map must be current before the
        // crossing, or the departure would be computed against a stale board.
        if let Some(id) = &self.active_map {
            if let Some(m) = self.campaign_maps.get_mut(id) {
                m.document = self.map.clone();
            }
        }
        let mut snap = GameSnapshot {
            map: self.map.clone(),
            turns: self.turns.clone(),
            roll_log: Vec::new(),
            journal: Vec::new(),
            inventories: self.inventories.clone(),
            generations: Vec::new(),
            maps: self.campaign_maps.clone(),
            active_map: self.active_map.clone(),
            world: Default::default(),
            // The clocks must cross with everything else, or travel's
            // reconciliation would run against empty time and wipe the ledger
            // on copy-back.
            clocks: self.clocks.clone(),

            party_cap: self.party_cap,
            last_beats: Vec::new(),
            beat_seq: 0,
        };
        match apply_game(&mut snap, &GameEvent::Traveled { token }) {
            Ok(()) => {
                let switched = snap.active_map != self.active_map;
                self.map = snap.map;
                self.turns = snap.turns;
                self.inventories = snap.inventories;
                self.campaign_maps = snap.maps;
        self.clocks = snap.clocks;
        self.party_cap = snap.party_cap;
                self.active_map = snap.active_map;
                if switched {
                    self.selected_token = None;
                    self.selected = None;
                    self.reach.clear();
                    self.explored.clear();
                    self.status = format!(
                        "the party moves on: {}",
                        self.active_map.as_deref().unwrap_or("?")
                    );
                } else {
                    self.status = "through the door".to_owned();
                }
                self.recompute_fog();
            }
            Err(error) => {
                self.status = format!("cannot travel: {error:?}");
            }
        }
    }

    /// Play a beat on a token for its own sake: a cheer, a shrug, a taunt.
    ///
    /// The emote system in one method. It reuses the beat the combat lane
    /// already defined, so it costs no new replication, no new rendering, and no
    /// rules at all. Unlike an action it needs no adjudication, which is why a
    /// player may throw one on their own token without asking the host.
    pub fn emote(&mut self, token: TokenId, beat: &str) {
        if self.map.token(token).is_none() || self.map.is_defeated(token) {
            return;
        }
        self.close_context_menu();
        if self.net_emit(GameEvent::Emoted {
            token,
            beat: beat.to_owned(),
        }) {
            return;
        }
        let seq = self.beat_seq.wrapping_add(1);
        self.stage_beats(seq, &[isometry_core::Beat::new(token, beat)]);
    }

    /// Stage the beats of a freshly applied action, so the board plays the
    /// exchange. Idempotent per `seq`: a re-delivered snapshot does not replay.
    pub fn stage_beats(&mut self, seq: u64, beats: &[isometry_core::Beat]) {
        if seq == self.beat_seq {
            return;
        }
        self.beat_seq = seq;
        self.beats.clear();
        for beat in beats {
            self.beats.insert(beat.token, beat.name.clone());
        }
    }

    /// Drop every playing beat. The host calls this once the engine's animation
    /// clock reports nothing is animating, which is what lets the *next* strike
    /// restart the animation instead of finding the class already set.
    pub fn clear_beats(&mut self) {
        self.beats.clear();
    }

    /// Append to the local shared log, dropping the oldest past the cap. In
    /// session the authority's log arrives with the snapshot instead.
    pub fn push_roll(&mut self, record: RollRecord) {
        self.roll_log.push(record);
        let overflow = self.roll_log.len().saturating_sub(ROLL_LOG_CAP);
        if overflow > 0 {
            self.roll_log.drain(0..overflow);
        }
    }

    /// Roll `expr`, logging it under `by` with `label` (the shared-log
    /// path the host uses for a system action; solo appends locally).
    pub fn roll_labeled(&mut self, by: &str, label: &str, expr: &str) {
        let Some((total, dice)) = roll(expr, &mut self.rng) else {
            self.status = format!("bad roll: {expr}");
            return;
        };
        let record = RollRecord {
            by: by.to_owned(),
            expr: label.to_owned(),
            dice,
            total,
        };
        self.status = format!("{by} {label} = {total}");
        if self.net_mode == NetMode::Remote {
            self.net_outbox.push(GameEvent::Rolled(record));
        } else {
            self.push_roll(record);
        }
    }

    /// Open the `>` command line (host keys route to the draft until submit or
    /// cancel). Entered by the `>` key, the same way `w` opens a whisper.
    pub fn start_command(&mut self) {
        self.command_active = true;
        self.command_draft.clear();
        self.command_results.clear();
        self.status = "> command (enter run, esc cancel)".to_owned();
    }

    pub fn command_char(&mut self, c: char) {
        if self.command_active {
            self.command_draft.push(c);
        }
    }

    pub fn command_backspace(&mut self) {
        if self.command_active {
            self.command_draft.pop();
        }
    }

    pub fn command_cancel(&mut self) {
        self.command_active = false;
        self.command_draft.clear();
        self.status = "command cancelled".to_owned();
    }

    /// Parse and dispatch the command line, then close it. Every verb routes to
    /// machinery that already exists; the command layer is just the front door.
    pub fn command_submit(&mut self) {
        let input = self.command_draft.trim().to_owned();
        self.command_active = false;
        self.command_draft.clear();
        if input.is_empty() {
            return;
        }
        match crate::command::parse(&input) {
            crate::command::Command::Spawn(query) => self.spawn_query(&query),
            crate::command::Command::Gen(kind) => self.start_generator(&kind),
            crate::command::Command::Find(query) => self.find_query(&query),
            crate::command::Command::Roll(expr) => {
                if expr.trim().is_empty() {
                    self.status = "roll what? e.g. >roll 2d6+3".to_owned();
                } else {
                    // Attribute to the actual roller (a joined player rolls as
                    // themselves, not "DM"), the same way `roll_dice` does.
                    self.roll_dice(&expr);
                }
            }
            crate::command::Command::Time(ticks) => self.pass_time(ticks),
            crate::command::Command::Help => {
                self.status = "commands: >spawn >gen >find >roll >time".to_owned();
            }
            crate::command::Command::Unknown(verb) => {
                self.status = format!("unknown command: {verb} (try >help)");
            }
        }
    }

    /// `>spawn <query>`: place a statted creature. Host/DM only, because a
    /// spawn is authoring. Resolves the query to a bestiary entry and reuses the
    /// same path the compendium spawn button takes.
    pub fn spawn_query(&mut self, query: &str) {
        if !self.can_edit_inventory {
            self.status = "spawning requires the host".to_owned();
            return;
        }
        match self.resolve_bestiary(query) {
            Some(key) => self.spawn_monster(&key),
            None => self.status = format!("no monster matches '{query}'"),
        }
    }

    /// Resolve a free-text query to a bestiary key, most specific first: an
    /// exact key, then an exact name, then a name substring, then a key
    /// substring. Deterministic first-match; `>find` is for browsing.
    fn resolve_bestiary(&self, query: &str) -> Option<String> {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return None;
        }
        let by = |pred: &dyn Fn(&MonsterRow) -> bool| {
            self.bestiary.iter().find(|m| pred(m)).map(|m| m.key.clone())
        };
        by(&|m| m.key.to_ascii_lowercase() == q)
            .or_else(|| by(&|m| m.name.to_ascii_lowercase() == q))
            .or_else(|| by(&|m| m.name.to_ascii_lowercase().contains(&q)))
            .or_else(|| by(&|m| m.key.to_ascii_lowercase().contains(&q)))
    }

    /// `>gen <kind>`: select a matching generator and open the existing
    /// generator overlay on a fresh preview. The whole reroll/lock/commit
    /// surface is already built; this is just the front door to it.
    pub fn start_generator(&mut self, kind: &str) {
        if !self.can_edit_inventory {
            self.status = "generation requires the host".to_owned();
            return;
        }
        let k = kind.trim().to_ascii_lowercase();
        if k.is_empty() {
            self.status = "generate what? e.g. >gen npc".to_owned();
            return;
        }
        // Match by the id's trailing segment (`demo:npc` -> `npc`), then by a
        // substring of the id or the friendly name.
        let idx = self.generator_choices.iter().position(|c| {
            let suffix = c.id.rsplit(':').next().unwrap_or(&c.id).to_ascii_lowercase();
            suffix == k
                || suffix.contains(&k)
                || c.name.to_ascii_lowercase().contains(&k)
        });
        match idx {
            Some(i) => {
                self.generator_selected = i;
                self.generator_preview = None;
                self.generator_locks.clear();
                self.generator_open = true;
                // Fire the first preview immediately, so `>gen npc` shows a
                // candidate the DM can reroll or commit at once.
                self.generation_request = Some(GenerationRequest::Generate);
                self.status = format!("generating {}", self.generator_choices[i].name);
            }
            None => self.status = format!("no generator matches '{kind}'"),
        }
    }

    /// `>find <query>`: a unified substring search over the compendium
    /// (monsters, items, spells), shown as a list under the command line. Pure
    /// view-side and read-only, so any peer may browse.
    pub fn find_query(&mut self, query: &str) {
        let q = query.trim().to_ascii_lowercase();
        self.command_results.clear();
        if q.is_empty() {
            self.status = "find what? e.g. >find sword".to_owned();
            return;
        }
        const CAP: usize = 12;
        let mut out = Vec::new();
        for m in &self.bestiary {
            if m.name.to_ascii_lowercase().contains(&q) || m.key.to_ascii_lowercase().contains(&q) {
                out.push(format!("monster · {} ({})", m.name, m.key));
            }
        }
        for i in &self.items {
            if i.name.to_ascii_lowercase().contains(&q) {
                out.push(format!("item · {}", i.name));
            }
        }
        for s in &self.spells {
            if s.name.to_ascii_lowercase().contains(&q) {
                out.push(format!("spell · {}", s.name));
            }
        }
        let total = out.len();
        out.truncate(CAP);
        if total > CAP {
            out.push(format!("… and {} more", total - CAP));
        }
        self.command_results = out;
        self.status = if total == 0 {
            format!("no matches for '{query}'")
        } else {
            format!("{total} match{} for '{query}'", if total == 1 { "" } else { "es" })
        };
    }

    /// Start typing a whisper (host keys route to the draft until send or
    /// cancel).
    pub fn start_compose(&mut self) {
        self.composing = true;
        self.status = "whisper (enter send, esc cancel)".to_owned();
    }

    /// Append a typed character to the whisper draft.
    pub fn compose_char(&mut self, c: char) {
        if self.composing {
            self.whisper_draft.push(c);
        }
    }

    /// Delete the last draft character.
    pub fn compose_backspace(&mut self) {
        if self.composing {
            self.whisper_draft.pop();
        }
    }

    /// Cancel composing, discarding the draft.
    pub fn compose_cancel(&mut self) {
        self.composing = false;
        self.whisper_draft.clear();
        self.status = "whisper cancelled".to_owned();
    }

    /// Send the composed whisper to the current target: log it, and (as a
    /// networked host) queue it for directed delivery.
    pub fn compose_send(&mut self) {
        let text = self.whisper_draft.trim().to_owned();
        self.composing = false;
        self.whisper_draft.clear();
        if text.is_empty() {
            return;
        }
        let target = self.whisper_target.clone().unwrap_or_else(|| "table".to_owned());
        self.messages.push(format!("to {target}: {text}"));
        self.whisper_outbox.push((target, text));
        self.status = "whisper sent".to_owned();
    }

    /// Record a whisper received from the DM.
    pub fn receive_whisper(&mut self, from: &str, text: &str) {
        self.messages.push(format!("from {from}: {text}"));
        self.status = format!("whisper from {from}");
    }

    /// Cycle the whisper target through the connected player names.
    pub fn cycle_whisper_target(&mut self) {
        let names = &self.connected_players;
        if names.is_empty() {
            self.whisper_target = None;
            return;
        }
        self.whisper_target = match &self.whisper_target {
            None => Some(names[0].clone()),
            Some(cur) => {
                let i = names.iter().position(|n| n == cur);
                match i {
                    Some(i) if i + 1 < names.len() => Some(names[i + 1].clone()),
                    _ => None,
                }
            }
        };
    }

    /// The tiles the current area template covers, aimed at the hovered
    /// tile from the anchor. Empty unless in Measure mode with an anchor.
    pub fn template_preview(&self) -> std::collections::HashSet<TileCoord> {
        if self.mode != EditMode::Measure {
            return std::collections::HashSet::new();
        }
        let Some(anchor) = self.measure_anchor else {
            return std::collections::HashSet::new();
        };
        let toward = self.hover_tile.unwrap_or(anchor);
        template_tiles(&self.map, anchor, self.template_kind, self.template_size, toward)
    }

    /// The measured distance from the anchor to the hovered tile, if both
    /// are set (Measure mode).
    pub fn measured_distance(&self) -> Option<u32> {
        match (self.measure_anchor, self.hover_tile) {
            (Some(a), Some(h)) => Some(distance(a, h)),
            _ => None,
        }
    }

    /// A short display label for a token, e.g. "knight 1".
    fn token_label(&self, id: TokenId) -> String {
        match self.map.token(id) {
            Some(t) => format!("{} {}", t.sprite, id.0),
            None => format!("token {}", id.0),
        }
    }

    /// Roll initiative and reorder the turn list. Individual mode rolls a
    /// d20 per token and sorts high-to-low; side mode rolls a d20 per side
    /// and groups them. The rolls go to the shared roll log; the new order
    /// replicates (Remote) or applies locally.
    pub fn roll_initiative(&mut self) {
        let ids: Vec<TokenId> = if self.turns.is_empty() {
            self.map.tokens.iter().map(|t| t.id).collect()
        } else {
            self.turns.entries().to_vec()
        };
        if ids.is_empty() {
            self.status = "no tokens to order".to_owned();
            return;
        }
        let mut records: Vec<RollRecord> = Vec::new();
        let order: Vec<TokenId> = match self.initiative_mode {
            InitiativeMode::Individual => {
                let mut rolled: Vec<(i32, usize, TokenId)> = ids
                    .iter()
                    .enumerate()
                    .map(|(i, &id)| {
                        let (total, dice) = roll("1d20", &mut self.rng).unwrap();
                        records.push(RollRecord {
                            by: self.token_label(id),
                            expr: "init".to_owned(),
                            dice,
                            total,
                        });
                        (total, i, id)
                    })
                    .collect();
                // High roll first; ties keep input order.
                rolled.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                rolled.into_iter().map(|(_, _, id)| id).collect()
            }
            InitiativeMode::SideBased => {
                // Group tokens by owner, preserving order within a side.
                let mut sides: Vec<(String, Vec<TokenId>)> = Vec::new();
                for &id in &ids {
                    let owner = self
                        .map
                        .token(id)
                        .and_then(|t| t.owner.clone())
                        .unwrap_or_else(|| "dm".to_owned());
                    match sides.iter_mut().find(|(o, _)| *o == owner) {
                        Some(s) => s.1.push(id),
                        None => sides.push((owner, vec![id])),
                    }
                }
                let mut rolled: Vec<(i32, usize, Vec<TokenId>)> = sides
                    .into_iter()
                    .enumerate()
                    .map(|(i, (owner, toks))| {
                        let (total, dice) = roll("1d20", &mut self.rng).unwrap();
                        records.push(RollRecord {
                            by: format!("side {owner}"),
                            expr: "init".to_owned(),
                            dice,
                            total,
                        });
                        (total, i, toks)
                    })
                    .collect();
                rolled.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                rolled.into_iter().flat_map(|(_, _, toks)| toks).collect()
            }
        };
        self.status = format!("rolled initiative ({})", self.initiative_mode.label());
        if self.net_mode == NetMode::Remote {
            for r in records {
                self.net_outbox.push(GameEvent::Rolled(r));
            }
            self.net_outbox.push(GameEvent::TurnSetOrder(order));
        } else {
            self.roll_log.extend(records);
            let overflow = self.roll_log.len().saturating_sub(ROLL_LOG_CAP);
            if overflow > 0 {
                self.roll_log.drain(0..overflow);
            }
            self.turns.set_order(order);
            self.recompute_reach();
        }
    }

    /// Reseed the dice generator (the host does this with real entropy so
    /// rolls differ per launch).
    pub fn reseed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    /// The name shown as the roller: the viewer's player name in a
    /// session, else "dm".
    fn roller_name(&self) -> String {
        self.viewer.clone().unwrap_or_else(|| "dm".to_owned())
    }

    /// Roll a dice expression (e.g. "1d20+5"). The result is shared: in a
    /// session it goes to the host as a `Rolled` event and returns via the
    /// snapshot; solo it appends to the local log. Bad expressions set a
    /// status and do nothing.
    pub fn roll_dice(&mut self, expr: &str) {
        let by = self.roller_name();
        self.roll_labeled(&by, expr, expr);
    }

    /// Whether fog of war is being applied (a viewer is set).
    pub fn fog_active(&self) -> bool {
        self.viewer.is_some()
    }

    /// The fog presentation of tile `at` for the current viewer.
    pub fn fog_level(&self, at: TileCoord) -> FogLevel {
        if !self.fog_active() {
            FogLevel::Clear
        } else if self.visible.contains(&at) {
            FogLevel::Clear
        } else if self.explored.contains(&at) {
            FogLevel::Dim
        } else {
            FogLevel::Hidden
        }
    }

    /// Whether a token should be drawn: always when omniscient, always if
    /// it is the viewer's own, otherwise only while in current sight (you
    /// see foes only when they are lit).
    /// Whether the local viewer commands a token with this `owner`: it is
    /// theirs, or it belongs to a faction whose channel they have been granted.
    /// Additive over direct ownership, so a viewer with no faction grant behaves
    /// exactly as before. The DM (no viewer) is omniscient by other paths, so
    /// this stays false for them.
    pub fn commands(&self, owner: Option<&str>) -> bool {
        let Some(viewer) = self.viewer.as_deref() else {
            return false;
        };
        let Some(owner) = owner else {
            return false;
        };
        owner == viewer || self.world.faction_controller(owner) == Some(viewer)
    }

    pub fn token_visible(&self, token: &Token) -> bool {
        if !self.fog_active() {
            return true;
        }
        self.commands(token.owner.as_deref()) || self.visible.contains(&token.at)
    }

    /// Recompute the visible set from the viewer's tokens and fold it into
    /// explored memory. No-op (and clears) when omniscient.
    pub fn recompute_fog(&mut self) {
        if self.viewer.is_none() {
            self.visible.clear();
            self.explored.clear();
            return;
        }
        // Each token sees with its *own* effective sight (system-driven via
        // conditions), so a blinded scout goes dark without dimming its allies.
        let origins: Vec<(TileCoord, u32)> = self
            .map
            .tokens
            .iter()
            .filter(|t| self.commands(t.owner.as_deref()))
            .map(|t| {
                let (_, sight) = self
                    .map
                    .effective_mobility(t.id, (MOVE_BUDGET, self.sight_radius));
                (t.at, sight)
            })
            .collect();
        let mut visible = HashSet::new();
        for (at, radius) in origins {
            let rules = SightRules {
                radius,
                opaque: &|kind| kind == "tree" || kind == "wall",
            };
            visible.extend(visible_from(&self.map, at, &rules));
        }
        self.visible = visible;
        self.explored.extend(self.visible.iter().copied());
    }

    /// Cycle the viewer for previewing fog: omniscient, then each token
    /// owner in turn, then back. Explored memory resets per viewer.
    pub fn cycle_viewer(&mut self) {
        let mut owners: Vec<String> = Vec::new();
        for t in &self.map.tokens {
            if let Some(o) = &t.owner {
                if !owners.contains(o) {
                    owners.push(o.clone());
                }
            }
        }
        let next = match &self.viewer {
            None => owners.first().cloned(),
            Some(cur) => {
                let idx = owners.iter().position(|o| o == cur);
                match idx {
                    Some(i) => owners.get(i + 1).cloned(),
                    None => None,
                }
            }
        };
        self.viewer = next;
        self.explored.clear();
        self.recompute_fog();
        self.status = match &self.viewer {
            Some(v) => format!("view: {v} (fog)"),
            None => "view: all".to_owned(),
        };
    }

    /// Mirror a replicated snapshot into the view (Remote mode): the map
    /// and turn order become the host's authoritative copy, then reach
    /// recomputes for whatever token is selected. Selection and camera
    /// are local and survive.
    pub fn apply_snapshot(&mut self, snap: GameSnapshot) {
        // Beats first: a client renders from the snapshot, so this is the only
        // place it learns a flourish happened and can play it. Keyed on
        // `beat_seq`, so mirroring the same snapshot twice does not re-strike.
        if !snap.last_beats.is_empty() {
            self.stage_beats(snap.beat_seq, &snap.last_beats);
        }
        self.map = snap.map;
        self.turns = snap.turns;
        self.roll_log = snap.roll_log;
        self.inventories = snap.inventories;
        self.generations = snap.generations;
        self.campaign_maps = snap.maps;
        self.active_map = snap.active_map;
        self.world = snap.world;
        // A joined client mirrors these too, or its panel shows the wrong split-
        // party time (a C3 omission) and its cap disagrees with the host.
        self.clocks = snap.clocks;
        self.party_cap = snap.party_cap;
        self.sheet_effective = None;
        if let Some(id) = self.selected_token {
            if self.map.token(id).is_none() {
                self.selected_token = None;
            }
        }
        if self.status == "connecting..." {
            self.status = "in session".to_owned();
        }
        self.recompute_fog();
        self.recompute_reach();
    }

    /// In Remote mode, queue a game event for the session instead of
    /// mutating locally. Returns true when it was queued (so callers
    /// skip the local path).
    fn net_emit(&mut self, event: GameEvent) -> bool {
        if self.net_mode == NetMode::Remote {
            self.net_outbox.push(event);
            true
        } else {
            false
        }
    }

    /// The board tile under a logical cursor position, from the inverse
    /// projection (flat-ground picking; raised top faces resolve to the
    /// tile behind, a known limitation until elevation-aware picking).
    pub fn tile_at_cursor(&self, cursor: (f32, f32)) -> Option<TileCoord> {
        let x = cursor.0 - PANEL_W - self.camera.0;
        let y = cursor.1 - self.camera.1;
        let at = self.geo.screen_to_tile((x, y));
        self.map.ground.in_bounds(at.0, at.1).then_some(at)
    }

    /// The token a left-press at `cursor` would start dragging: a token
    /// under the cursor while in Select mode (free-move; Play movement stays
    /// the gated click-a-reach-tile path). `None` otherwise.
    pub fn token_drag_candidate(&self, cursor: (f32, f32)) -> Option<TokenId> {
        if self.mode != EditMode::Select {
            return None;
        }
        let tile = self.tile_at_cursor(cursor)?;
        self.map.tokens.iter().find(|t| t.at == tile).map(|t| t.id)
    }

    /// Free-move token `id` to `to` (the Select-mode drag release): emits a
    /// `TokenMoved` (replicated in Remote, applied and undoable locally). A
    /// no-op if `to` is out of bounds, unchanged, or already occupied.
    pub fn drag_move_token(&mut self, id: TokenId, to: TileCoord) {
        if !self.map.ground.in_bounds(to.0, to.1) {
            return;
        }
        match self.map.token(id).map(|t| t.at) {
            Some(cur) if cur == to => return,
            None => return,
            _ => {}
        }
        if self.map.tokens.iter().any(|t| t.id != id && t.at == to) {
            return; // tile occupied
        }
        let ev = SessionEvent::TokenMoved { id, to };
        if self.net_emit(GameEvent::Map(ev.clone())) {
            return;
        }
        self.apply_step(vec![ev]);
        self.recompute_reach();
    }

    /// Open the right-click context menu on token `id`, anchored at pane
    /// position `at` (logical px). Right-click also selects the token so the
    /// menu's actions operate on it.
    pub fn open_context_menu(&mut self, id: TokenId, at: (f32, f32)) {
        self.select_token(id);
        self.context_menu = Some((id, at));
    }

    /// Close the context menu (a click elsewhere, or after an action).
    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
    }

    /// Remove a token (a context-menu action): drops it from the map, the
    /// turn order, and selection. Replicated in Remote, undoable locally.
    pub fn remove_token(&mut self, id: TokenId) {
        if !self.net_emit(GameEvent::Map(SessionEvent::TokenRemoved { id })) {
            self.apply_step(vec![SessionEvent::TokenRemoved { id }]);
        }
        self.turns.remove(id);
        if self.selected_token == Some(id) {
            self.selected_token = None;
            self.reach.clear();
        }
        self.context_menu = None;
    }

    /// Whether the hovered tile changed in a way the board renders (the
    /// path preview): the host calls this read-only before paying for a
    /// state update.
    pub fn hover_needs_update(&self, cursor: (f32, f32)) -> Option<Option<TileCoord>> {
        let t = self.tile_at_cursor(cursor);
        if t == self.hover_tile {
            return None;
        }
        // Play mode redraws for the path preview; Measure mode for the
        // template + distance readout once an anchor is set.
        let play = self.mode == EditMode::Play && !self.reach.is_empty();
        let measure = self.mode == EditMode::Measure && self.measure_anchor.is_some();
        (play || measure).then_some(t)
    }

    /// Pan by whole tiles: one step is half a tile footprint on each
    /// axis, the diamond lattice spacing.
    pub fn pan_tiles(&mut self, dc: f32, dr: f32) {
        self.camera.0 -= dc * self.geo.tile_w / 2.0;
        self.camera.1 -= dr * self.geo.tile_h / 2.0;
    }

    /// Apply a batch of events as one undoable step. Events that fail
    /// validation are skipped; the step records only what applied.
    fn apply_step(&mut self, events: Vec<SessionEvent>) {
        let mut inverses: Step = Vec::new();
        for event in &events {
            if let Ok(inverse) = apply(&mut self.map, event) {
                inverses.push(inverse);
            }
        }
        if !inverses.is_empty() {
            inverses.reverse();
            self.undo.push(inverses);
            self.redo.clear();
        }
        self.recompute_fog();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        let Some(step) = self.undo.pop() else { return };
        let mut redo_step: Step = Vec::new();
        for event in &step {
            if let Ok(inverse) = apply(&mut self.map, event) {
                redo_step.push(inverse);
            }
        }
        redo_step.reverse();
        self.redo.push(redo_step);
        self.status = format!("undo ({} left)", self.undo.len());
        self.recompute_fog();
        self.recompute_reach();
    }

    pub fn redo(&mut self) {
        let Some(step) = self.redo.pop() else { return };
        let mut undo_step: Step = Vec::new();
        for event in &step {
            if let Ok(inverse) = apply(&mut self.map, event) {
                undo_step.push(inverse);
            }
        }
        undo_step.reverse();
        self.undo.push(undo_step);
        self.status = "redo".to_owned();
        self.recompute_fog();
        self.recompute_reach();
    }

    /// The next free token id across the *whole campaign*, not just the active
    /// map: a spawn (or a Token-mode placement) must not reuse an id already
    /// resident on a stored map or held by an inventory, because inventories key
    /// on `TokenId` globally. Same discipline as travel's id minting and the
    /// generator commit's `next_snapshot_id`.
    fn next_token_id(&self) -> TokenId {
        let max = self
            .campaign_maps
            .values()
            .flat_map(|m| m.document.tokens.iter())
            .chain(self.map.tokens.iter())
            .map(|t| t.id.0)
            .chain(self.inventories.keys().map(|id| id.0))
            .max()
            .unwrap_or(0);
        TokenId(max + 1)
    }

    fn token_at(&self, at: TileCoord) -> Option<TokenId> {
        self.map.tokens.iter().find(|t| t.at == at).map(|t| t.id)
    }

    /// Whether `id` may move right now: free tokens (outside the turn
    /// list) always may; listed tokens only on their turn.
    pub fn may_move(&self, id: TokenId) -> bool {
        !self.turns.contains(id) || self.turns.active() == Some(id)
    }

    pub fn recompute_reach(&mut self) {
        self.reach.clear();
        let Some(id) = self.selected_token else { return };
        let Some(token) = self.map.token(id) else {
            self.selected_token = None;
            return;
        };
        if !self.may_move(id) {
            return;
        }
        let (budget, _) = self
            .map
            .effective_mobility(id, (MOVE_BUDGET, SIGHT_RADIUS));
        let rules = MoveRules {
            budget,
            step_up: 1,
            step_down: 2,
            passable: &|kind| kind != "water",
        };
        self.reach = reachable(&self.map, token.at, &rules, id);
    }

    /// Select a token (Play mode): highlights its reach if it may move.
    pub fn select_token(&mut self, id: TokenId) {
        self.selected_token = Some(id);
        self.recompute_reach();
        if let Some(t) = self.map.token(id) {
            let gate = if self.may_move(id) { "" } else { " (waiting)" };
            self.status = format!("{} {}{}", t.sprite, id.0, gate);
        }
    }

    /// Add or drop `id` from the turn order (the drag-in/drag-out
    /// trichotomy's click form; out of the list = free movement).
    pub fn toggle_turn(&mut self, id: TokenId) {
        let listed = self.turns.contains(id);
        let event = if listed {
            GameEvent::TurnRemove(id)
        } else {
            GameEvent::TurnAdd(id)
        };
        if self.net_emit(event) {
            return;
        }
        if listed {
            self.turns.remove(id);
        } else {
            self.turns.add(id);
        }
        self.recompute_reach();
    }

    /// Advance the turn and select whoever is up.
    pub fn end_turn(&mut self) {
        if self.net_emit(GameEvent::TurnAdvance) {
            return;
        }
        let before = self.turns.round();
        let map = &self.map;
        self.turns.advance_skipping(|id| map.is_defeated(id));
        let elapsed = self.turns.round().saturating_sub(before);
        if elapsed > 0 {
            if let Some(active) = self.active_map.clone() {
                *self.clocks.entry(active).or_insert(0) += elapsed;
            }
        }
        if let Some(active) = self.turns.active() {
            // A turn beginning refreshes its per-turn counters: actions refill,
            // the multiple-attack penalty resets. Solo path; the session mirrors
            // this through apply_game on the authority.
            self.map.clear_turn_counters(active);
            self.select_token(active);
            if let Some(t) = self.map.token(active) {
                let owner = t.owner.as_deref().unwrap_or("dm");
                self.status = format!("turn: {} {} ({owner})", t.sprite, active.0);
            }
        }
    }

    /// Rotate the selected token's facing clockwise (undoable locally,
    /// replicated in a session).
    pub fn rotate_selected(&mut self) {
        let Some(id) = self.selected_token else { return };
        let Some(t) = self.map.token(id) else { return };
        let next = match t.facing {
            Facing::North => Facing::East,
            Facing::East => Facing::South,
            Facing::South => Facing::West,
            Facing::West => Facing::North,
        };
        if self.net_emit(GameEvent::Map(SessionEvent::TokenFaced { id, facing: next })) {
            return;
        }
        self.apply_step(vec![SessionEvent::TokenFaced { id, facing: next }]);
    }

    /// A click on token `id` (dispatched by the token element).
    pub fn click_token(&mut self, id: TokenId) {
        match self.mode {
            EditMode::Play => self.select_token(id),
            // Token placement is a Local-mode editor action.
            EditMode::Token if self.net_mode == NetMode::Local => {
                self.apply_step(vec![SessionEvent::TokenRemoved { id }]);
                self.turns.remove(id);
                self.recompute_reach();
            }
            _ => {}
        }
    }

    /// The editor entry point: a click (or paint-drag) on tile `at`.
    pub fn click_tile(&mut self, at: TileCoord) {
        // Editing is offline (Local) work; in a session only Play,
        // Select, and Measure act on a click (Measure is purely local).
        if self.net_mode == NetMode::Remote
            && !matches!(
                self.mode,
                EditMode::Play | EditMode::Select | EditMode::Measure
            )
        {
            return;
        }
        match self.mode {
            EditMode::Select => {
                self.selected = Some(at);
            }
            EditMode::Measure => {
                self.measure_anchor = Some(at);
                self.status = format!("anchor ({}, {})", at.0, at.1);
            }
            EditMode::Token => {
                if let Some(id) = self.token_at(at) {
                    self.click_token(id);
                } else {
                    self.apply_step(vec![SessionEvent::TokenPlaced(Token {
                        id: self.next_token_id(),
                        at,
                        facing: Facing::South,
                        sprite: self.token_sprite.clone(),
                        owner: None,
                    })]);
                }
            }
            EditMode::Play => {
                if let Some(id) = self.token_at(at) {
                    self.select_token(id);
                } else if let Some(id) = self.selected_token {
                    if self.reach.contains_key(&at) {
                        let path = isometry_core::path_to(&self.reach, at);
                        let from = self
                            .map
                            .token(id)
                            .map(|t| t.at)
                            .unwrap_or(at);
                        let last_from = path
                            .len()
                            .checked_sub(2)
                            .and_then(|i| path.get(i).copied())
                            .unwrap_or(from);
                        let facing = facing_between(last_from, at);
                        if self.net_mode == NetMode::Remote {
                            // Send intent; the authoritative echo moves
                            // the token, so don't touch the local map.
                            self.net_outbox
                                .push(GameEvent::Map(SessionEvent::TokenMoved { id, to: at }));
                            self.net_outbox
                                .push(GameEvent::Map(SessionEvent::TokenFaced { id, facing }));
                        } else {
                            self.apply_step(vec![
                                SessionEvent::TokenMoved { id, to: at },
                                SessionEvent::TokenFaced { id, facing },
                            ]);
                            self.recompute_reach();
                            // Landing on a door is walking through it. In a
                            // session the host's sweep does this; solo does it
                            // here, through the same shared logic.
                            if self.transition_at(at) {
                                self.travel(id);
                            }
                        }
                    }
                }
            }
            EditMode::PaintGround => self.apply_step(vec![SessionEvent::TilePlaced {
                layer: Layer::Ground,
                at,
                kind: self.brush,
            }]),
            EditMode::PaintProp => self.apply_step(vec![SessionEvent::TilePlaced {
                layer: Layer::Prop,
                at,
                kind: self.brush,
            }]),
            EditMode::Fill => {
                if at.0 >= 0 && at.1 >= 0 {
                    let region = self.map.ground.flood_region((at.0 as u32, at.1 as u32));
                    let kind = self.brush;
                    let events: Vec<SessionEvent> = region
                        .into_iter()
                        .map(|(c, r)| SessionEvent::TilePlaced {
                            layer: Layer::Ground,
                            at: (c as i32, r as i32),
                            kind,
                        })
                        .collect();
                    self.status = format!("filled {} tiles", events.len());
                    self.apply_step(events);
                }
            }
            EditMode::Raise | EditMode::Lower => {
                if at.0 >= 0 && at.1 >= 0 {
                    let h = *self
                        .map
                        .elevation
                        .get(at.0 as u32, at.1 as u32)
                        .unwrap_or(&0);
                    let new = if self.mode == EditMode::Raise {
                        h.saturating_add(1).min(12)
                    } else {
                        h.saturating_sub(1)
                    };
                    if new != h {
                        self.apply_step(vec![SessionEvent::ElevationSet { at, height: new }]);
                    }
                }
            }
        }
    }

    /// Swap in a freshly loaded document; editor history and play state
    /// die with the old one.
    pub fn replace_map(&mut self, map: MapDocument) {
        self.map = map;
        self.undo.clear();
        self.redo.clear();
        self.selected = None;
        self.turns = TurnList::new();
        self.selected_token = None;
        self.reach.clear();
        self.explored.clear();
        self.recompute_fog();
    }
}

/// Facing after a step from `from` to `to` (grid-axis neighbors; equal
/// tiles keep looking South).
fn facing_between(from: TileCoord, to: TileCoord) -> Facing {
    match (to.0 - from.0, to.1 - from.1) {
        (1, _) => Facing::East,
        (-1, _) => Facing::West,
        (_, -1) => Facing::North,
        _ => Facing::South,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::demo_map;

    #[test]
    fn paint_undo_redo_round_trip() {
        let mut ui = UiState::new(demo_map());
        let pristine = ui.map.clone();
        ui.mode = EditMode::PaintGround;
        ui.brush = TileKindId(2);
        ui.click_tile((3, 3));
        ui.click_tile((4, 3));
        let painted = ui.map.clone();
        assert_ne!(painted, pristine);
        ui.undo();
        ui.undo();
        assert_eq!(ui.map, pristine);
        ui.redo();
        ui.redo();
        assert_eq!(ui.map, painted);
    }

    #[test]
    fn play_move_respects_turn_gate_and_sets_facing() {
        use isometry_core::TokenId;
        let mut ui = UiState::new(demo_map());
        ui.mode = EditMode::Play;
        // Knight at (10, 14): free token, may move.
        ui.select_token(TokenId(1));
        assert!(ui.may_move(TokenId(1)));
        assert!(!ui.reach.is_empty());
        ui.click_tile((12, 14)); // 2 east, within budget 5
        let t = ui.map.token(TokenId(1)).unwrap();
        assert_eq!(t.at, (12, 14));
        assert_eq!(t.facing, isometry_core::Facing::East);
        // Both tokens in the list: only the active one may move.
        ui.toggle_turn(TokenId(1));
        ui.toggle_turn(TokenId(2));
        assert!(ui.may_move(TokenId(1)));
        assert!(!ui.may_move(TokenId(2)));
        ui.select_token(TokenId(2));
        assert!(ui.reach.is_empty(), "waiting token gets no reach");
        let before = ui.map.token(TokenId(2)).unwrap().at;
        ui.click_tile((before.0 + 1, before.1));
        assert_eq!(ui.map.token(TokenId(2)).unwrap().at, before);
        // End turn: token 2 is up and can move now.
        ui.end_turn();
        assert!(ui.may_move(TokenId(2)));
        // The move is undoable (one step: move + facing).
        ui.select_token(TokenId(1));
        assert!(!ui.may_move(TokenId(1)) || ui.turns.active() == Some(TokenId(1)));
    }

    #[test]
    fn ending_a_turn_refreshes_the_incoming_token_per_turn_counters() {
        use isometry_core::TokenId;
        let mut ui = UiState::new(demo_map());
        ui.toggle_turn(TokenId(1));
        ui.toggle_turn(TokenId(2));
        assert_eq!(ui.turns.active(), Some(TokenId(1)));
        // The active knight has spent part of its turn: an action economy, a
        // multiple-attack tally -- the view never learns which.
        ui.map.bump_turn_counter(TokenId(1), "actions_spent", 2);

        // The goblin's turn begins. A turn-start wipes the *incoming* token's
        // counters, so the goblin's clear (they were empty) while the knight
        // keeps its spend as it waits.
        ui.end_turn();
        assert_eq!(ui.turns.active(), Some(TokenId(2)));
        assert_eq!(ui.map.turn_counter(TokenId(1), "actions_spent"), 2);

        // Back to the knight: its own turn beginning clears the ledger, so it
        // acts with a whole economy again.
        ui.end_turn();
        assert_eq!(ui.turns.active(), Some(TokenId(1)));
        assert_eq!(ui.map.turn_counter(TokenId(1), "actions_spent"), 0);
    }

    #[test]
    fn drag_move_relocates_a_token_and_is_undoable() {
        use isometry_core::TokenId;
        let mut ui = UiState::new(demo_map());
        let start = ui.map.token(TokenId(1)).unwrap().at; // (10, 14)
        ui.drag_move_token(TokenId(1), (5, 5));
        assert_eq!(ui.map.token(TokenId(1)).unwrap().at, (5, 5));
        // Occupied (goblin 2 at (15, 8)) and out-of-bounds are no-ops.
        ui.drag_move_token(TokenId(1), (15, 8));
        ui.drag_move_token(TokenId(1), (999, 999));
        assert_eq!(ui.map.token(TokenId(1)).unwrap().at, (5, 5));
        // The one real move undoes back to the start.
        ui.undo();
        assert_eq!(ui.map.token(TokenId(1)).unwrap().at, start);
    }

    #[test]
    fn drag_move_routes_out_in_remote_mode() {
        use isometry_core::TokenId;
        let mut ui = UiState::new(demo_map());
        ui.net_mode = NetMode::Remote;
        let before = ui.map.token(TokenId(1)).unwrap().at;
        ui.drag_move_token(TokenId(1), (5, 5));
        // Session mode emits an intent and leaves the local map untouched.
        assert_eq!(ui.map.token(TokenId(1)).unwrap().at, before);
        assert_eq!(ui.net_outbox.len(), 1);
    }

    #[test]
    fn token_drag_candidate_finds_a_token_in_select_mode_only() {
        use isometry_core::TokenId;
        let mut ui = UiState::new(demo_map());
        // Cursor over knight 1's tile (10, 14); default camera is (0, 0).
        let (sx, sy) = ui.geo.tile_to_screen((10, 14), 0);
        let on_token = (sx + PANEL_W + ui.camera.0, sy + ui.camera.1);
        assert_eq!(ui.mode, EditMode::Select);
        assert_eq!(ui.token_drag_candidate(on_token), Some(TokenId(1)));
        // An empty tile, or any non-Select mode, yields nothing.
        let (ex, ey) = ui.geo.tile_to_screen((0, 0), 0);
        assert_eq!(ui.token_drag_candidate((ex + PANEL_W, ey)), None);
        ui.mode = EditMode::Play;
        assert_eq!(ui.token_drag_candidate(on_token), None);
    }

    #[test]
    fn context_menu_opens_selects_and_removes() {
        use isometry_core::TokenId;
        let mut ui = UiState::new(demo_map());
        let n = ui.map.tokens.len();
        ui.open_context_menu(TokenId(1), (50.0, 60.0));
        assert_eq!(ui.context_menu, Some((TokenId(1), (50.0, 60.0))));
        assert_eq!(ui.selected_token, Some(TokenId(1)), "right-click selects");
        ui.close_context_menu();
        assert!(ui.context_menu.is_none());
        // Remove drops the token from the map, turn order, and selection.
        ui.turns.add(TokenId(1));
        ui.open_context_menu(TokenId(1), (0.0, 0.0));
        ui.remove_token(TokenId(1));
        assert_eq!(ui.map.tokens.len(), n - 1);
        assert!(ui.map.token(TokenId(1)).is_none());
        assert!(!ui.turns.contains(TokenId(1)));
        assert!(ui.selected_token.is_none());
        assert!(ui.context_menu.is_none());
    }

    #[test]
    fn remote_mode_routes_moves_as_events_not_local_mutation() {
        use isometry_core::TokenId;
        let mut ui = UiState::new(demo_map());
        ui.net_mode = NetMode::Remote;
        ui.mode = EditMode::Play;
        let before = ui.map.token(TokenId(1)).unwrap().at;
        ui.select_token(TokenId(1));
        // A move in a session emits intents and leaves the local map
        // untouched (the host authority echoes the real move back).
        ui.click_tile((before.0 + 1, before.1));
        assert_eq!(ui.map.token(TokenId(1)).unwrap().at, before);
        assert_eq!(ui.net_outbox.len(), 2, "move + facing emitted");
        // End turn and toggle also route out, not local.
        ui.net_outbox.clear();
        ui.end_turn();
        ui.toggle_turn(TokenId(2));
        assert_eq!(ui.net_outbox.len(), 2);
        // Editing is inert in a session.
        ui.net_outbox.clear();
        ui.mode = EditMode::PaintGround;
        ui.click_tile((0, 0));
        assert!(ui.net_outbox.is_empty());
        assert!(ui.can_undo() == false, "no local edit happened");

        // A snapshot mirrors the authoritative state in.
        let mut snap_map = ui.map.clone();
        snap_map.token_mut(TokenId(1)).unwrap().at = (before.0 + 1, before.1);
        let inventories = std::collections::BTreeMap::from([(TokenId(1), Inventory::default())]);
        let snap = GameSnapshot {
            map: snap_map,
            turns: ui.turns.clone(),
            roll_log: Vec::new(),
            journal: Vec::new(),
            inventories: inventories.clone(),
            generations: Vec::new(),
            maps: Default::default(),
            active_map: None,
            world: Default::default(),
            clocks: Default::default(),

            party_cap: isometry_net::default_party_cap(),
            last_beats: Vec::new(),
            beat_seq: 0,
        };
        ui.apply_snapshot(snap);
        assert_eq!(ui.map.token(TokenId(1)).unwrap().at, (before.0 + 1, before.1));
        assert_eq!(ui.inventories, inventories);
    }

    #[test]
    fn compendium_item_request_targets_the_open_sheet() {
        let mut ui = UiState::new(demo_map());
        ui.open_sheet = Some(TokenId(1));
        let item = ItemRow {
            key: "longsword".to_owned(),
            name: "Longsword".to_owned(),
            category: "Weapon".to_owned(),
            cost: "15 gp".to_owned(),
            weight: "3 lb.".to_owned(),
            detail: "1d8 slashing".to_owned(),
            desc: String::new(),
        };
        ui.request_compendium_item(&item);
        assert_eq!(
            ui.inventory_request,
            Some(InventoryRequest::AddCompendiumItem {
                token: TokenId(1),
                template: "longsword".to_owned(),
                name: "Longsword".to_owned(),
                category: "Weapon".to_owned(),
            })
        );
    }

    #[test]
    fn generator_controls_keep_locks_visible_and_queue_host_work() {
        let mut ui = UiState::new(demo_map());
        ui.generator_choices.push(GeneratorChoice {
            id: "demo:forge_item".to_owned(),
            name: "Forge item".to_owned(),
            default_args: GenValue::Text {
                value: "river".to_owned(),
            },
            lock_presets: vec![isometry_campaign::GeneratorLockPreset {
                key: "culture".to_owned(),
                label: "River-clan culture".to_owned(),
                value: GenValue::Text {
                    value: "river-clans".to_owned(),
                },
            }],
        });
        ui.open_generator();
        ui.toggle_generator_lock();
        assert_eq!(
            ui.generator_locks.get("culture"),
            Some(&GenValue::Text {
                value: "river-clans".to_owned()
            })
        );
        ui.request_generation();
        assert_eq!(ui.generation_request, Some(GenerationRequest::Generate));

        ui.toggle_generator_lock();
        assert!(!ui.generator_locks.contains_key("culture"));
    }

    #[test]
    fn governance_conflict_queues_typed_adopt_and_branch_requests() {
        let mut ui = UiState::new(demo_map());
        ui.governance_conflict = Some(GovernanceConflict {
            candidates: vec![
                GovernanceBindingRow {
                    proposal: [1; 32],
                    moot: "North table".to_owned(),
                    policy: "unanimous".to_owned(),
                    endorsements: 2,
                    required: 2,
                    claims: 1,
                },
                GovernanceBindingRow {
                    proposal: [2; 32],
                    moot: "North table".to_owned(),
                    policy: "threshold 2".to_owned(),
                    endorsements: 2,
                    required: 2,
                    claims: 1,
                },
            ],
            can_adopt: true,
            can_branch: true,
            restriction: None,
        });

        ui.open_governance_conflict();
        ui.select_governance_candidate(1);
        ui.request_governance_adopt();
        assert_eq!(
            ui.governance_resolution_request,
            Some(GovernanceResolutionRequest::Adopt { selected: [2; 32] })
        );
        assert!(!ui.governance_conflict_open);

        ui.open_governance_conflict();
        ui.request_governance_branch();
        assert_eq!(
            ui.governance_resolution_request,
            Some(GovernanceResolutionRequest::Branch {
                candidates: vec![[1; 32], [2; 32]],
            })
        );
    }

    #[test]
    fn governance_conflict_respects_host_restrictions() {
        let mut ui = UiState::new(demo_map());
        ui.governance_conflict = Some(GovernanceConflict {
            candidates: vec![
                GovernanceBindingRow {
                    proposal: [1; 32],
                    moot: "First table".to_owned(),
                    policy: "unanimous".to_owned(),
                    endorsements: 1,
                    required: 1,
                    claims: 1,
                },
                GovernanceBindingRow {
                    proposal: [2; 32],
                    moot: "Other table".to_owned(),
                    policy: "unanimous".to_owned(),
                    endorsements: 1,
                    required: 1,
                    claims: 1,
                },
            ],
            can_adopt: false,
            can_branch: false,
            restriction: Some("no shared founding electorate".to_owned()),
        });
        ui.open_governance_conflict();
        ui.request_governance_adopt();
        assert!(ui.governance_resolution_request.is_none());
        assert_eq!(ui.status, "no shared founding electorate");
    }

    #[test]
    fn transfer_request_keeps_source_and_target_explicit() {
        let mut ui = UiState::new(demo_map());
        ui.open_sheet = Some(TokenId(1));
        ui.request_transfer(TokenId(2), ItemId::new("token-1.item-0"));
        assert_eq!(
            ui.inventory_request,
            Some(InventoryRequest::Transfer {
                from: TokenId(1),
                to: TokenId(2),
                item: ItemId::new("token-1.item-0"),
            })
        );
    }

    #[test]
    fn local_roll_appends_to_log_and_is_reproducible() {
        let mut ui = UiState::new(demo_map());
        ui.reseed(99);
        ui.roll_dice("1d20+3");
        assert_eq!(ui.roll_log.len(), 1);
        let rec = &ui.roll_log[0];
        assert_eq!(rec.by, "dm");
        assert_eq!(rec.dice.len(), 1);
        assert_eq!(rec.total, rec.dice[0] as i32 + 3);
        // A bad expression sets a status and adds nothing.
        ui.roll_dice("nonsense");
        assert_eq!(ui.roll_log.len(), 1);
        assert!(ui.status.starts_with("bad roll"));
    }

    #[test]
    fn roll_initiative_individual_and_side() {
        let mut ui = UiState::new(demo_map());
        ui.reseed(5);
        let ids: Vec<_> = ui.map.tokens.iter().map(|t| t.id).collect();
        for id in &ids {
            ui.turns.add(*id);
        }
        // Individual: one roll per token, order preserved as a set.
        ui.roll_initiative();
        assert_eq!(ui.turns.entries().len(), ids.len());
        assert_eq!(ui.roll_log.len(), ids.len());
        let mut sorted = ui.turns.entries().to_vec();
        sorted.sort();
        let mut expect = ids.clone();
        expect.sort();
        assert_eq!(sorted, expect, "same tokens, reordered");

        // Side-based: tokens grouped by owner, so exactly one boundary
        // between the two sides (A knights, B goblins).
        ui.initiative_mode = InitiativeMode::SideBased;
        ui.roll_initiative();
        let owners: Vec<String> = ui
            .turns
            .entries()
            .iter()
            .map(|id| ui.map.token(*id).unwrap().owner.clone().unwrap())
            .collect();
        let boundaries = owners.windows(2).filter(|w| w[0] != w[1]).count();
        assert_eq!(boundaries, 1, "two sides act in blocks");
    }

    #[test]
    fn compose_whisper_builds_draft_and_logs() {
        let mut ui = UiState::new(demo_map());
        ui.whisper_target = Some("alice".to_owned());
        ui.start_compose();
        assert!(ui.composing);
        for c in "hi".chars() {
            ui.compose_char(c);
        }
        ui.compose_backspace();
        ui.compose_char('e');
        ui.compose_char('y');
        assert_eq!(ui.whisper_draft, "hey");
        ui.compose_send();
        assert!(!ui.composing);
        assert_eq!(ui.messages, vec!["to alice: hey".to_string()]);
        assert_eq!(
            ui.whisper_outbox,
            vec![("alice".to_string(), "hey".to_string())]
        );
        ui.receive_whisper("dm", "watch out");
        assert_eq!(ui.messages[1], "from dm: watch out");
    }

    #[test]
    fn remote_roll_routes_out_not_local() {
        let mut ui = UiState::new(demo_map());
        ui.net_mode = NetMode::Remote;
        ui.viewer = Some("A".to_owned());
        ui.roll_dice("2d6");
        assert!(
            ui.roll_log.is_empty(),
            "remote rolls come back via snapshot"
        );
        assert_eq!(ui.net_outbox.len(), 1);
    }

    #[test]
    fn fog_hides_out_of_sight_and_remembers_explored() {
        let mut ui = UiState::new(demo_map());
        // Knights are owner "A" near (10,14)/(9,15); goblins "B" near the
        // northeast hill. As player A, the goblins are out of sight.
        ui.viewer = Some("A".to_owned());
        ui.recompute_fog();
        let knight = ui.map.token(TokenId(1)).unwrap().at;
        let goblin = ui.map.token(TokenId(2)).unwrap().at;
        assert_eq!(ui.fog_level(knight), FogLevel::Clear);
        assert_eq!(ui.fog_level(goblin), FogLevel::Hidden);
        assert!(ui.token_visible(ui.map.token(TokenId(1)).unwrap()));
        assert!(!ui.token_visible(ui.map.token(TokenId(2)).unwrap()));

        // Explored memory: a tile seen once stays remembered (Dim) after
        // the token that saw it moves away.
        let seen_far = ui
            .visible
            .iter()
            .copied()
            .find(|&t| t != knight && (t.0 - knight.0).abs() + (t.1 - knight.1).abs() >= 3)
            .expect("some far-but-visible tile");
        // Move the knight to the opposite side so seen_far leaves sight.
        ui.mode = EditMode::Play;
        ui.apply_step(vec![SessionEvent::TokenMoved {
            id: TokenId(1),
            to: (0, 0),
        }]);
        ui.apply_step(vec![SessionEvent::TokenMoved {
            id: TokenId(3),
            to: (1, 0),
        }]);
        assert_eq!(
            ui.fog_level(seen_far),
            FogLevel::Dim,
            "a tile seen earlier is remembered, not black"
        );

        // Omniscient clears fog entirely.
        ui.viewer = None;
        ui.recompute_fog();
        assert_eq!(ui.fog_level(goblin), FogLevel::Clear);
        assert!(!ui.fog_active());
    }

    #[test]
    fn token_mode_places_and_removes() {
        let mut ui = UiState::new(demo_map());
        ui.mode = EditMode::Token;
        let n = ui.map.tokens.len();
        ui.click_tile((2, 2));
        assert_eq!(ui.map.tokens.len(), n + 1);
        let placed = ui.token_at((2, 2)).unwrap();
        assert!(placed.0 > 2, "fresh id past the demo tokens");
        ui.click_tile((2, 2));
        assert_eq!(ui.map.tokens.len(), n);
        ui.undo(); // undo the removal: token back
        assert_eq!(ui.map.tokens.len(), n + 1);
        assert_eq!(ui.token_at((2, 2)), Some(placed));
    }

    #[test]
    fn fill_is_one_undo_step() {
        let mut ui = UiState::new(demo_map());
        let pristine = ui.map.clone();
        ui.mode = EditMode::Fill;
        ui.brush = TileKindId(3);
        ui.click_tile((0, 0));
        assert_ne!(ui.map, pristine);
        ui.undo();
        assert_eq!(ui.map, pristine);
    }

    #[test]
    fn spawn_in_a_session_routes_through_the_authority_not_the_local_map() {
        // The bug the adversarial review caught: a hosted DM's `>spawn` mutated
        // the local map directly, so the token never replicated and was wiped by
        // the next snapshot mirror (leaving an orphan sheet). It must emit an
        // authoritative TokenPlaced instead.
        let mut ui = UiState::new(demo_map());
        ui.net_mode = NetMode::Remote;
        ui.bestiary = vec![MonsterRow {
            key: "goblin".to_owned(),
            name: "Goblin".to_owned(),
            cr: 0.25,
            cr_label: "1/4".to_owned(),
            kind: "humanoid".to_owned(),
            size: "small".to_owned(),
            alignment: "ne".to_owned(),
            hp: 7,
            hit_dice: "2d6".to_owned(),
            ac: 15,
            speed_ft: 30,
            xp: 50,
            abilities: [8, 14, 10, 10, 8, 8],
            actions: Vec::new(),
            sprite: "goblin".to_owned(),
        }];
        let before = ui.map.tokens.len();

        ui.spawn_query("goblin");

        // The local map is untouched; the placement is queued for the authority.
        assert_eq!(ui.map.tokens.len(), before, "no local mutation in a session");
        let placed = ui.net_outbox.iter().any(|e| {
            matches!(e, GameEvent::Map(SessionEvent::TokenPlaced(_)))
        });
        assert!(placed, "the spawn must replicate as an authoritative event");
        // And the stat-block bind is queued for the same id.
        assert!(ui.spawn_sheet_request.is_some());
    }

    #[test]
    fn the_storylet_surface_is_dm_only_and_plays_only_the_ready() {
        let mut ui = UiState::new(demo_map());
        // A joined player cannot open or play storylets (matching reads secrets).
        ui.can_edit_inventory = false;
        ui.open_storylets();
        assert!(!ui.storylet_open, "a client must not open the storylet surface");

        // The DM can. A locked storylet cannot be played; a ready one arms a
        // request the host will commit.
        ui.can_edit_inventory = true;
        ui.storylets = vec![
            StoryletRow {
                key: "locked".to_owned(),
                entry: "The cult stirs.".to_owned(),
                available: false,
                status: "needs a faction tagged 'cult'".to_owned(),
                cast: Vec::new(),
            },
            StoryletRow {
                key: "ready".to_owned(),
                entry: "A stranger greets you.".to_owned(),
                available: true,
                status: "ready".to_owned(),
                cast: Vec::new(),
            },
        ];
        ui.open_storylets();
        assert!(ui.storylet_open);

        ui.storylet_selected = 0; // the locked one
        ui.play_storylet();
        assert_eq!(ui.storylet_request, None, "a locked storylet cannot be played");

        ui.storylet_selected = 1; // the ready one
        ui.play_storylet();
        assert_eq!(ui.storylet_request.as_deref(), Some("ready"));
    }

    #[test]
    fn the_downtime_surface_is_dm_only_and_commits_only_the_kept() {
        let mut ui = UiState::new(demo_map());
        // A joined player cannot open downtime (the roll reads the world and
        // spends host entropy), so nothing is armed.
        ui.can_edit_inventory = false;
        ui.open_downtime();
        assert!(!ui.downtime_open, "a client must not open the downtime surface");
        assert!(!ui.downtime_roll_request);

        // The DM can: opening arms a roll request the host fills with rows.
        ui.can_edit_inventory = true;
        ui.open_downtime();
        assert!(ui.downtime_open && ui.downtime_roll_request);
        ui.downtime_roll_request = false; // the host consumed it and filled rows
        ui.faction_moves = vec![
            FactionMoveRow {
                faction: "tide".to_owned(),
                verb: "court".to_owned(),
                text: "Bran swore to the Tide Court.".to_owned(),
                has_change: true,
                struck: false,
            },
            FactionMoveRow {
                faction: "ash".to_owned(),
                verb: "raid".to_owned(),
                text: "The Ash Company raided a rival.".to_owned(),
                has_change: false,
                struck: false,
            },
        ];

        // Strike the raid; it will not commit.
        ui.downtime_selected = 1;
        ui.toggle_strike_downtime();
        assert!(ui.faction_moves[1].struck);
        ui.commit_downtime();
        assert!(ui.downtime_commit_request, "one kept move arms the commit");

        // Strike everything and commit refuses: an empty tick is no tick.
        ui.downtime_commit_request = false;
        ui.faction_moves.iter_mut().for_each(|m| m.struck = true);
        ui.commit_downtime();
        assert!(!ui.downtime_commit_request, "nothing kept, nothing to commit");
    }

    #[test]
    fn the_overmap_surface_opens_and_arms_a_travel_request() {
        let mut ui = UiState::new(demo_map());
        assert!(!ui.overmap_open);
        // Anyone may look at the overmap (unlike the DM-only downtime surface).
        ui.open_overmap();
        assert!(ui.overmap_open);
        // Clicking a place arms a one-shot the host resolves; the view decides
        // nothing about the trip.
        ui.request_travel("forest".to_owned());
        assert_eq!(ui.overmap_travel_request.as_deref(), Some("forest"));
        ui.close_overmap();
        assert!(!ui.overmap_open);
    }

    #[test]
    fn a_viewer_commands_a_faction_only_once_granted_its_channel() {
        let mut ui = UiState::new(demo_map());
        ui.viewer = Some("B".to_owned());

        // Ungranted, a faction's token is not B's to command.
        assert!(!ui.commands(Some("tide")));
        // Grant B the Tide Court's channel (as the replicated world would carry).
        ui.world.faction_control.insert("tide".to_owned(), "B".to_owned());
        assert!(ui.commands(Some("tide")), "the grant extends command to the faction");

        // Direct ownership is unchanged, and a stranger's token stays off-limits.
        assert!(ui.commands(Some("B")));
        assert!(!ui.commands(Some("A")));
        assert!(!ui.commands(Some("ash")), "an unrelated faction is not B's");
        assert!(!ui.commands(None), "a DM token is nobody's to a player");
    }

    #[test]
    fn apply_snapshot_mirrors_the_clock_and_the_cap() {
        // A joined client mirrors the host snapshot into its UiState. Dropping
        // clocks (a C3 omission the C5 review caught) shows the wrong split-party
        // time on clients; dropping party_cap desyncs the limit.
        let mut ui = UiState::new(demo_map());
        assert_eq!(ui.party_cap, 4);
        let mut snap = GameSnapshot {
            map: demo_map(),
            turns: TurnList::new(),
            roll_log: Vec::new(),
            journal: Vec::new(),
            inventories: Default::default(),
            generations: Vec::new(),
            maps: Default::default(),
            active_map: None,
            world: Default::default(),
            clocks: Default::default(),
            party_cap: 2,
            last_beats: Vec::new(),
            beat_seq: 0,
        };
        snap.clocks.insert("field".to_owned(), 7);
        ui.apply_snapshot(snap);
        assert_eq!(ui.party_cap, 2, "the cap must mirror");
        assert_eq!(ui.clocks.get("field"), Some(&7), "the clock must mirror");
    }

    #[test]
    fn a_spawn_tile_stays_on_the_board_on_a_narrow_map() {
        // free_spawn_tile's outward scan could walk off a map narrower than its
        // stride, yielding an off-board tile that fails placement. It must clamp.
        let mut ui = UiState::new(MapDocument::new("slot", 3, 3));
        // Pack the whole 3x3 but one cell, forcing the scan to the survivor.
        for row in 0..3 {
            for col in 0..3 {
                if (col, row) != (2, 2) {
                    ui.map.tokens.push(Token {
                        id: TokenId(100 + (row * 3 + col) as u32),
                        at: (col, row),
                        facing: Facing::South,
                        sprite: "goblin".to_owned(),
                        owner: None,
                    });
                }
            }
        }
        let at = ui.free_spawn_tile();
        assert!(
            ui.map.ground.in_bounds(at.0, at.1),
            "spawn tile {at:?} is off the 3x3 board"
        );
        assert_eq!(at, (2, 2), "the one free in-bounds cell");
    }
}
