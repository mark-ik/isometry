use std::collections::{HashMap, HashSet};

use isometry_core::{
    apply, reachable, roll, visible_tiles, Facing, IsoGeometry, Layer, MapDocument, MoveRules,
    Rng, RollRecord, SessionEvent, SightRules, TileCoord, TileKindId, Token, TokenId, TurnList,
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
}

impl EditMode {
    pub const ALL: [EditMode; 8] = [
        EditMode::Select,
        EditMode::PaintGround,
        EditMode::PaintProp,
        EditMode::Fill,
        EditMode::Raise,
        EditMode::Lower,
        EditMode::Token,
        EditMode::Play,
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

/// Runner state: the substrate document plus view-layer concerns
/// (camera, selection, editor).
pub struct UiState {
    pub map: MapDocument,
    pub geo: IsoGeometry,
    /// Board-origin offset within the pane, logical px. Snap-scrolled by
    /// whole tile steps (the tactics references scroll in steps; the
    /// smooth-pan lane waits on the netrender camera-offset composite).
    pub camera: (f32, f32),
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
    /// Dice generator. Seeded deterministically; the host reseeds with
    /// real entropy at startup.
    rng: Rng,
    /// How "roll initiative" orders the turn list.
    pub initiative_mode: InitiativeMode,
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
}

impl UiState {
    pub fn new(map: MapDocument) -> Self {
        Self {
            map,
            geo: IsoGeometry::default(),
            camera: (0.0, 0.0),
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
            net_mode: NetMode::Local,
            net_outbox: Vec::new(),
            viewer: None,
            sight_radius: SIGHT_RADIUS,
            visible: HashSet::new(),
            explored: HashSet::new(),
            roll_log: Vec::new(),
            rng: Rng::new(1),
            initiative_mode: InitiativeMode::Individual,
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
        let Some((total, dice)) = roll(expr, &mut self.rng) else {
            self.status = format!("bad roll: {expr}");
            return;
        };
        let record = RollRecord {
            by: self.roller_name(),
            expr: expr.to_owned(),
            dice,
            total,
        };
        self.status = format!("{} rolled {} = {}", record.by, record.expr, record.total);
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

    /// Whether the hovered tile changed in a way the board renders (the
    /// path preview): the host calls this read-only before paying for a
    /// state update.
    pub fn hover_needs_update(&self, cursor: (f32, f32)) -> Option<Option<TileCoord>> {
        let t = self.tile_at_cursor(cursor);
        (t != self.hover_tile && self.mode == EditMode::Play && !self.reach.is_empty())
            .then_some(t)
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
        // Editing is offline (Local) work; in a session only Play and
        // Select act on a click.
        if self.net_mode == NetMode::Remote
            && !matches!(self.mode, EditMode::Play | EditMode::Select)
        {
            return;
        }
        match self.mode {
            EditMode::Select => {
                self.selected = Some(at);
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
        let snap = GameSnapshot {
            map: snap_map,
            turns: ui.turns.clone(),
            roll_log: Vec::new(),
        };
        ui.apply_snapshot(snap);
        assert_eq!(ui.map.token(TokenId(1)).unwrap().at, (before.0 + 1, before.1));
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
        use isometry_core::TokenId;
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
        use isometry_core::TokenId;
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
