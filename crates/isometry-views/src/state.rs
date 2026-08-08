use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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

use cambium::{CommandState, SelectionItem, SelectionState, Slider, TabStrip, TextInput};

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

/// How many whispers [`UiState::messages`] keeps. Matches `ROLL_LOG_CAP`'s
/// magnitude and stays far above the five the pane shows, so scrollback is
/// still there when the pane grows one.
pub const MESSAGES_CAP: usize = 50;

/// Point a selection row at `index` without disturbing its keyboard focus.
///
/// `SelectionState::select_one` is private to Cambium, and rebuilding the state
/// would drop `focus_active` mid-interaction, so this writes the two public
/// fields the component reads.
fn point_selection(state: &mut SelectionState, index: usize) {
    state.active = index;
    state.selected = vec![index];
}

/// Travel pace as a percent of normal, in the order the row presents them.
/// The index into this array *is* the selection index, so the array is the
/// single place the row's order is decided.
pub const PACE_PCTS: [i64; 3] = [50, 100, 200];

/// Marching stance keys, in row order. The empty key is "no stance": walking.
pub const STANCE_KEYS: [&str; 4] = ["scout", "search", "forage", ""];

/// The pace row's labels, paired with [`PACE_PCTS`] by index.
pub fn pace_items() -> Vec<SelectionItem> {
    ["Fast", "Normal", "Slow"]
        .into_iter()
        .map(SelectionItem::new)
        .collect()
}

/// The stance row's labels, paired with [`STANCE_KEYS`] by index.
pub fn stance_items() -> Vec<SelectionItem> {
    ["Scout", "Search", "Forage", "Walk"]
        .into_iter()
        .map(SelectionItem::new)
        .collect()
}

/// The mode row's labels, paired with [`EditMode::ALL`] by index.
pub fn mode_items() -> Vec<SelectionItem> {
    EditMode::ALL
        .iter()
        .map(|mode| SelectionItem::new(mode.label()))
        .collect()
}

mod interaction;
mod play;
mod rows;
mod session;
mod source_time;
mod surfaces;

use rows::Step;
pub use rows::*;

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
    /// The message log (whispers sent and received), display strings. Bounded
    /// by [`MESSAGES_CAP`]; push through [`UiState::push_message`].
    pub messages: Vec<String>,
    /// The `>` command line. `command_active` captures keystrokes into
    /// `command_draft` (the whisper-composer pattern); `command_results` holds
    /// the last `>find` list, shown until the next command.
    pub command_active: bool,
    pub command_draft: TextInput,
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
    /// One-shot: the pace the party chose (a percent of normal time); the host
    /// records it, and the next trip travels at it.
    pub overmap_pace_request: Option<i64>,
    /// One-shot: the exploration stance the party's navigator took (empty clears);
    /// the host sets it on the lead token, and the travel rule reads it.
    pub overmap_stance_request: Option<String>,
    /// One-shot: the party asked to study its maps. The host rolls the reader's
    /// literacy and, on a pass, reveals the places just beyond the known ones -- a
    /// dull-witted party learns nothing.
    pub overmap_read_request: bool,
    /// The overmap node the pointer is over, if any. The host drives it from
    /// hover enter/leave; the painted graph leaf lifts the hovered node so the
    /// place under the cursor stands out before it is clicked.
    pub overmap_hover: Option<String>,
    /// The local relation cell currently inspected in the overmap. It is a
    /// view selection, not a campaign-route edit.
    pub overmap_selected_relation: Option<String>,
    /// The relation cell under the pointer, used to lift its Swatch target.
    pub overmap_hovered_relation: Option<String>,
    /// Locally hidden relation cells. Hiding a route changes only this
    /// projection; travel and campaign geography still retain the route.
    pub overmap_hidden_relations: BTreeSet<String>,
    /// Local visual placement overrides made by pulling overmap nodes. These
    /// belong to this UI projection only: the campaign's places and travel graph
    /// remain untouched until an explicit world-authoring feature chooses to
    /// persist coordinates.
    pub overmap_position_overrides: BTreeMap<String, (f32, f32)>,
    /// Pointer position at the start of one captured overmap-node gesture. It
    /// distinguishes an ordinary click-to-travel from a drag-to-reposition.
    pub overmap_drag_start: Option<(String, (f32, f32))>,
    /// The node moved past drag slop in the active gesture. The following click
    /// is consumed so releasing a pull never also requests travel.
    pub overmap_dragged_node: Option<String>,
    /// The host-supplied origin and authority history, kept separate from the
    /// live world so historical Swatch selection cannot write truth backwards.
    overmap_source: Option<isometry_net::GameSourceHistory>,
    /// `None` means the live tail. A prefix cursor selects a replayed snapshot.
    overmap_source_cursor: Option<u64>,
    /// Disposable historical projection; `self.world` remains live at all times.
    overmap_source_snapshot: Option<GameSnapshot>,
    /// Cambium's normalized source-time scrubber state.
    pub overmap_source_slider: Slider,
    /// The catalog `segmented_control` state behind the mode, pace, and stance
    /// rows. Cambium's selection components own their own state, so these are
    /// the inner states a `lens` projects into.
    ///
    /// They are *not* the source of truth: mode lives in [`Self::mode`], pace
    /// and stance in the world. [`Self::sync_selection_rows`] pushes truth into
    /// them whenever it changes, and the host pumps notice a user-driven
    /// divergence and dispatch it back. That one-way sync plus a compare is
    /// what keeps a two-way binding from fighting itself.
    pub mode_selection: SelectionState,
    pub pace_selection: SelectionState,
    pub stance_selection: SelectionState,
    /// The catalog `tab_strip` state behind the compendium's namespace nav, on
    /// the same terms: a mirror of [`Self::compendium_tab`], never the truth.
    /// The catalog strip brings roving-tabindex arrow-key switching and the ARIA
    /// tabs roles the hand-rolled nav had neither of.
    pub compendium_tabs: TabStrip,
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
    /// Interaction state for the catalog `command_menu` the context menu is
    /// rendered by (its highlighted row and open submenu). Opening resets it,
    /// so a fresh menu never starts on the row the last one ended on.
    pub context_menu_state: CommandState,
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
            context_menu_state: CommandState::default()
                .with_id("token-menu")
                .with_label("Token actions"),
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
            command_draft: TextInput::default(),
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
            overmap_pace_request: None,
            overmap_stance_request: None,
            overmap_read_request: false,
            overmap_hover: None,
            overmap_selected_relation: None,
            overmap_hovered_relation: None,
            overmap_hidden_relations: BTreeSet::new(),
            overmap_position_overrides: BTreeMap::new(),
            overmap_drag_start: None,
            overmap_dragged_node: None,
            overmap_source: None,
            overmap_source_cursor: None,
            overmap_source_snapshot: None,
            overmap_source_slider: Slider::new(1.0)
                .with_steps(1.0 / 63.0, 10.0 / 63.0)
                .with_label("Overmap history"),
            mode_selection: SelectionState::single(0).with_id("mode-row"),
            pace_selection: SelectionState::single(1).with_id("pace-row"),
            stance_selection: SelectionState::single(3).with_id("stance-row"),
            compendium_tabs: TabStrip::new(0).with_label("Compendium namespaces"),
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
mod tests;
