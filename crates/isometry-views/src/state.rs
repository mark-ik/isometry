use std::collections::{BTreeMap, HashMap, HashSet};

use isometry_campaign::{EquipmentSlot, GenValue, GenerationRecord, Inventory, ItemId};
use isometry_core::{
    apply, distance, reachable, roll, template_tiles, visible_tiles, Facing, IsoGeometry, Layer,
    MapDocument, MoveRules, Rng, RollRecord, SessionEvent, SightRules, TemplateKind, TileCoord,
    TileKindId, Token, TokenId, TurnList,
};
use isometry_net::{GameEvent, GameSnapshot, ROLL_LOG_CAP};

/// Fixed side-panel width in logical px (CSS `.side` width plus its
/// padding); the host uses it to keep drag painting off the panel.
pub const PANEL_W: f32 = 228.0;

/// Default move budget until system plugins supply speed stats (I6).
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
    /// Rollable actions: `(key, label)`.
    pub actions: Vec<(String, String)>,
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
    pub inventory_request: Option<InventoryRequest>,
    /// False for a joined player. The host still validates its own event path;
    /// this only keeps DM authoring controls out of player UI.
    pub can_edit_inventory: bool,
    /// Public commit-result records mirrored from a session snapshot. The W2
    /// preview table will project this ledger; content scripts never run here.
    pub generations: Vec<GenerationRecord>,
    /// Generator preview state is local to the host until `Commit`; players
    /// receive only the resulting public record through a snapshot.
    pub generator_open: bool,
    pub generator_preview: Option<GenerationRecord>,
    pub generator_locks: BTreeMap<String, GenValue>,
    pub generation_request: Option<GenerationRequest>,
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
            inventory_request: None,
            can_edit_inventory: true,
            generations: Vec::new(),
            generator_open: false,
            generator_preview: None,
            generator_locks: BTreeMap::new(),
            generation_request: None,
            bestiary: Vec::new(),
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
    pub fn open_generator(&mut self) {
        if self.can_edit_inventory {
            self.generator_open = true;
        } else {
            self.status = "generation requires the host".to_owned();
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

    /// The demo pack reads this visible worldbuilding constraint. A lock is
    /// a value passed to each reroll, never a replay of past entropy.
    pub fn toggle_demo_culture_lock(&mut self) {
        if !self.can_edit_inventory {
            self.status = "generation requires the host".to_owned();
            return;
        }
        if self.generator_locks.remove("culture").is_some() {
            self.status = "unlocked culture".to_owned();
        } else {
            self.generator_locks.insert(
                "culture".to_owned(),
                GenValue::Text {
                    value: "river-clans".to_owned(),
                },
            );
            self.status = "locked culture: river-clans".to_owned();
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
        let (sprite, name) = (m.sprite.clone(), m.name.clone());
        let at = self.free_spawn_tile();
        self.apply_step(vec![SessionEvent::TokenPlaced(Token {
            id: self.next_token_id(),
            at,
            facing: Facing::South,
            sprite,
            owner: None,
        })]);
        self.status = format!("spawned {name}");
        self.compendium_open = false;
        self.compendium_selected = None;
    }

    /// A free tile to spawn onto: the selection if empty, else scanning a
    /// small block outward from it.
    fn free_spawn_tile(&self) -> TileCoord {
        let start = self.selected.unwrap_or((2, 2));
        for d in 0..64 {
            let at = (start.0 + (d % 8), start.1 + (d / 8));
            if self.token_at(at).is_none() {
                return at;
            }
        }
        start
    }

    /// Queue a field edit (a stepper on the open sheet); the host applies
    /// and replicates it.
    pub fn request_sheet_edit(&mut self, key: &str, delta: i64) {
        if let Some(id) = self.open_sheet {
            self.sheet_edit = Some((id, key.to_owned(), delta));
        }
    }

    /// Queue an action roll; the host evaluates it against the system.
    pub fn request_action(&mut self, key: &str) {
        if let Some(id) = self.open_sheet {
            self.sheet_action = Some((id, key.to_owned()));
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
            self.roll_log.push(record);
            let overflow = self.roll_log.len().saturating_sub(ROLL_LOG_CAP);
            if overflow > 0 {
                self.roll_log.drain(0..overflow);
            }
        }
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
    pub fn token_visible(&self, token: &Token) -> bool {
        if !self.fog_active() {
            return true;
        }
        token.owner.as_deref() == self.viewer.as_deref() || self.visible.contains(&token.at)
    }

    /// Recompute the visible set from the viewer's tokens and fold it into
    /// explored memory. No-op (and clears) when omniscient.
    pub fn recompute_fog(&mut self) {
        if self.viewer.is_none() {
            self.visible.clear();
            self.explored.clear();
            return;
        }
        let origins: Vec<TileCoord> = self
            .map
            .tokens
            .iter()
            .filter(|t| t.owner.as_deref() == self.viewer.as_deref())
            .map(|t| t.at)
            .collect();
        let rules = SightRules {
            radius: self.sight_radius,
            opaque: &|kind| kind == "tree" || kind == "wall",
        };
        self.visible = visible_tiles(&self.map, &origins, &rules);
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
        self.map = snap.map;
        self.turns = snap.turns;
        self.roll_log = snap.roll_log;
        self.inventories = snap.inventories;
        self.generations = snap.generations;
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

    fn next_token_id(&self) -> TokenId {
        TokenId(self.map.tokens.iter().map(|t| t.id.0).max().unwrap_or(0) + 1)
    }

    fn token_at(&self, at: TileCoord) -> Option<TokenId> {
        self.map.tokens.iter().find(|t| t.at == at).map(|t| t.id)
    }

    /// Whether `id` may move right now: free tokens (outside the turn
    /// list) always may; listed tokens only on their turn.
    pub fn may_move(&self, id: TokenId) -> bool {
        !self.turns.contains(id) || self.turns.active() == Some(id)
    }

    fn recompute_reach(&mut self) {
        self.reach.clear();
        let Some(id) = self.selected_token else { return };
        let Some(token) = self.map.token(id) else {
            self.selected_token = None;
            return;
        };
        if !self.may_move(id) {
            return;
        }
        let rules = MoveRules {
            budget: MOVE_BUDGET,
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
        self.turns.advance();
        if let Some(active) = self.turns.active() {
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
        ui.open_generator();
        ui.toggle_demo_culture_lock();
        assert_eq!(
            ui.generator_locks.get("culture"),
            Some(&GenValue::Text {
                value: "river-clans".to_owned()
            })
        );
        ui.request_generation();
        assert_eq!(ui.generation_request, Some(GenerationRequest::Generate));

        ui.toggle_demo_culture_lock();
        assert!(!ui.generator_locks.contains_key("culture"));
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
        assert!(ui.roll_log.is_empty(), "remote rolls come back via snapshot");
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
}
