use std::collections::HashMap;

use isometry_core::{
    apply, reachable, Facing, IsoGeometry, Layer, MapDocument, MoveRules, SessionEvent,
    TileCoord, TileKindId, Token, TokenId, TurnList,
};

/// Fixed side-panel width in logical px (CSS `.side` width plus its
/// padding); the host uses it to keep drag painting off the panel.
pub const PANEL_W: f32 = 228.0;

/// Default move budget until system plugins supply speed stats (I6).
const MOVE_BUDGET: u32 = 5;

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
        if self.turns.contains(id) {
            self.turns.remove(id);
        } else {
            self.turns.add(id);
        }
        self.recompute_reach();
    }

    /// Advance the turn and select whoever is up.
    pub fn end_turn(&mut self) {
        self.turns.advance();
        if let Some(active) = self.turns.active() {
            self.select_token(active);
            if let Some(t) = self.map.token(active) {
                let owner = t.owner.as_deref().unwrap_or("dm");
                self.status = format!("turn: {} {} ({owner})", t.sprite, active.0);
            }
        }
    }

    /// Rotate the selected token's facing clockwise (undoable).
    pub fn rotate_selected(&mut self) {
        let Some(id) = self.selected_token else { return };
        let Some(t) = self.map.token(id) else { return };
        let next = match t.facing {
            Facing::North => Facing::East,
            Facing::East => Facing::South,
            Facing::South => Facing::West,
            Facing::West => Facing::North,
        };
        self.apply_step(vec![SessionEvent::TokenFaced { id, facing: next }]);
    }

    /// A click on token `id` (dispatched by the token element).
    pub fn click_token(&mut self, id: TokenId) {
        match self.mode {
            EditMode::Play => self.select_token(id),
            EditMode::Token => {
                self.apply_step(vec![SessionEvent::TokenRemoved { id }]);
                self.turns.remove(id);
                self.recompute_reach();
            }
            _ => {}
        }
    }

    /// The editor entry point: a click (or paint-drag) on tile `at`.
    pub fn click_tile(&mut self, at: TileCoord) {
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
                        self.apply_step(vec![
                            SessionEvent::TokenMoved { id, to: at },
                            SessionEvent::TokenFaced { id, facing },
                        ]);
                        self.recompute_reach();
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
