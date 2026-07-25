//! Direct board manipulation: cursor hits, drags, the context menu, undo.
//!
//! The mouse-facing half of the state. Coordinates arrive in logical px and
//! leave as `TileCoord` through the iso math, so a view never does the
//! projection itself.
//!
//! Split out of `state.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl UiState {
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
        // A fresh menu starts at the top rather than wherever the last one was
        // left, and with no submenu hanging open.
        self.context_menu_state.selected = 0;
        self.context_menu_state.submenu = None;
        self.context_menu_state.submenu_selected = 0;
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
    pub(crate) fn apply_step(&mut self, events: Vec<SessionEvent>) {
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
    pub(crate) fn next_token_id(&self) -> TokenId {
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

    pub(crate) fn token_at(&self, at: TileCoord) -> Option<TokenId> {
        self.map.tokens.iter().find(|t| t.at == at).map(|t| t.id)
    }

    /// Whether `id` may move right now: free tokens (outside the turn
    /// list) always may; listed tokens only on their turn.
    pub fn may_move(&self, id: TokenId) -> bool {
        !self.turns.contains(id) || self.turns.active() == Some(id)
    }

    pub fn recompute_reach(&mut self) {
        self.reach.clear();
        let Some(id) = self.selected_token else {
            return;
        };
        let Some(token) = self.map.token(id) else {
            self.selected_token = None;
            return;
        };
        if !self.may_move(id) {
            return;
        }
        let (budget, _) = self.map.effective_mobility(id, (MOVE_BUDGET, SIGHT_RADIUS));
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
        let Some(id) = self.selected_token else {
            return;
        };
        let Some(t) = self.map.token(id) else { return };
        let next = match t.facing {
            Facing::North => Facing::East,
            Facing::East => Facing::South,
            Facing::South => Facing::West,
            Facing::West => Facing::North,
        };
        if self.net_emit(GameEvent::Map(SessionEvent::TokenFaced {
            id,
            facing: next,
        })) {
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
                        let from = self.map.token(id).map(|t| t.at).unwrap_or(at);
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
