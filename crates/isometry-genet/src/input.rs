//! Pointer and keyboard input, translated into app intent.
//!
//! Hit testing goes through the laid-out box tree, so what the user clicks is
//! what the engine actually painted. `key` is also where the three bespoke
//! key-capture lanes live (search field, the `>` command line, the whisper
//! composer); the obviation lane replaces them with `caret_text_field`.
//!
//! Split out of `main.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl App {
    /// A wheel notch over the board pane snap-pans the board (wheel = pan,
    /// the tactics-canvas convention). Over the side panel it is inert: the
    /// panel fits the default window, and genet has no `overscroll-behavior`
    /// to keep a near-full panel's scroll from chaining into the whole-
    /// document viewport (which would drag the board), so true panel-scroll
    /// for short windows is a follow-on. `nx`/`ny` are wheel notches.
    pub(crate) fn wheel(&mut self, nx: f32, ny: f32) {
        if self.cursor.0 <= PANEL_W {
            return;
        }
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.pan_tiles(-nx * WHEEL_BOARD_TILES, -ny * WHEEL_BOARD_TILES));
        }
    }

    /// Drive `:hover` restyles on target change (engine `set_interaction`;
    /// `Unchanged` when nothing interaction-sensitive matched), and dispatch
    /// cambium enter/leave to whatever `on_hover` handler sits under the cursor
    /// (the overmap's painted node emphasis rides this).
    pub(crate) fn hover(&mut self) {
        // Phase 1: hit-test and the engine-level `:hover` restyle. Returns the
        // view node carrying an `on_hover` handler under the cursor, if any.
        let target = {
            let (Some(runner), Some(layout)) = (self.runner.as_ref(), self.layout.as_mut()) else {
                return;
            };
            let (x, y) = self.cursor;
            let dom = runner.dom();
            let dom_ref = dom.borrow();
            let hit = layout.hit_test(&*dom_ref, x, y, &ScrollOffsets::default());
            let hovered = hit.map(|n| layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n));
            let focused = runner
                .focus()
                .map(|n| layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n));
            let target = hit.and_then(|n| runner.hover_target(n));
            if (hovered, focused) != (self.last_hover, self.last_focus) {
                self.last_hover = hovered;
                self.last_focus = focused;
                let state = InteractionState {
                    hovered: hovered.map(SourceNodeId),
                    focused: focused.map(SourceNodeId),
                    ..Default::default()
                };
                if layout.set_interaction(&*dom_ref, &state) != Applied::Unchanged {
                    drop(dom_ref);
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            target
        };
        // Phase 2: view-level enter/leave. `dispatch_hover` no-ops on a stale
        // node, so a target that has since been removed is harmless. Leave the
        // old before entering the new, since a single hovered slot is shared.
        if target == self.hover_target_node {
            return;
        }
        let previous = self.hover_target_node;
        self.hover_target_node = target;
        if let Some(runner) = self.runner.as_mut() {
            if let Some(prev) = previous {
                runner.dispatch_hover(prev, HoverEvent::new(HoverPhase::Leave, (0.0, 0.0), (0.0, 0.0)));
            }
            if let Some(now) = target {
                runner.dispatch_hover(now, HoverEvent::new(HoverPhase::Enter, (0.0, 0.0), (0.0, 0.0)));
            }
        }
        if previous.is_some() || target.is_some() {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    /// Hit-test the cursor against the retained layout: the node plus
    /// its stable opaque id (the drag-dedupe key).
    pub(crate) fn cursor_hit(&self) -> Option<(NodeId, u64)> {
        let (Some(runner), Some(layout)) = (self.runner.as_ref(), self.layout.as_ref()) else {
            return None;
        };
        let (x, y) = self.cursor;
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        layout
            .hit_test(&*dom_ref, x, y, &ScrollOffsets::default())
            .map(|n| (n, layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n)))
    }

    pub(crate) fn click(&mut self) {
        let hit = self.cursor_hit();
        if self.profile {
            eprintln!(
                "[isometry] click at {:?} hit {:?}",
                self.cursor,
                hit.map(|h| h.1)
            );
        }
        let Some((node, id)) = hit else { return };
        self.last_drag = Some(id);
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        runner.dispatch_click(
            node,
            PointerClick {
                local: (0.0, 0.0),
                prop: Propagation::new(),
            },
        );
        if self.profile {
            runner.update(|ui| {
                eprintln!(
                    "[isometry] post-dispatch mode={:?} selected={:?} status={:?}",
                    ui.mode, ui.selected, ui.status
                );
            });
        }
        self.after_dispatch();
    }


    pub(crate) fn key(&mut self, event: &WinitKeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        // Escape backs out of target-pick before anything else reads it, so an
        // armed attack is always cancellable without spending a turn.
        if runner.state().picking_target()
            && matches!(event.logical_key, WinitKey::Named(WinitNamedKey::Escape))
        {
            runner.update(|ui| ui.cancel_action_pick());
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
            return;
        }
        // While the > command line is open, keys go to its draft. Wins over the
        // whisper composer below so it is never shadowed.
        if runner.state().command_active {
            match &event.logical_key {
                WinitKey::Named(WinitNamedKey::Escape) => {
                    runner.update(|ui| ui.command_cancel());
                }
                WinitKey::Named(WinitNamedKey::Enter) => {
                    runner.update(|ui| ui.command_submit());
                }
                WinitKey::Named(WinitNamedKey::Backspace) => {
                    runner.update(|ui| ui.command_backspace());
                }
                WinitKey::Named(WinitNamedKey::Space) => {
                    runner.update(|ui| ui.command_char(' '));
                }
                WinitKey::Character(c) => {
                    let s = c.to_string();
                    runner.update(|ui| {
                        for ch in s.chars() {
                            ui.command_char(ch);
                        }
                    });
                }
                _ => {}
            }
            self.after_dispatch();
            return;
        }
        // While composing a whisper, keys go to the draft.
        if runner.state().composing {
            match &event.logical_key {
                WinitKey::Named(WinitNamedKey::Escape) => {
                    runner.update(|ui| ui.compose_cancel());
                }
                WinitKey::Named(WinitNamedKey::Enter) => {
                    runner.update(|ui| ui.compose_send());
                }
                WinitKey::Named(WinitNamedKey::Backspace) => {
                    runner.update(|ui| ui.compose_backspace());
                }
                WinitKey::Named(WinitNamedKey::Space) => {
                    runner.update(|ui| ui.compose_char(' '));
                }
                WinitKey::Character(c) => {
                    let s = c.to_string();
                    runner.update(|ui| {
                        for ch in s.chars() {
                            ui.compose_char(ch);
                        }
                    });
                }
                _ => {}
            }
            self.after_dispatch();
            return;
        }
        // While the compendium is open, keys filter the index.
        if runner.state().compendium_open {
            match &event.logical_key {
                WinitKey::Named(WinitNamedKey::Escape) => {
                    runner.update(|ui| ui.compendium_escape());
                }
                WinitKey::Named(WinitNamedKey::Backspace) => {
                    runner.update(|ui| ui.search_backspace());
                }
                WinitKey::Named(WinitNamedKey::Space) => {
                    runner.update(|ui| ui.search_char(' '));
                }
                WinitKey::Character(c) => {
                    let s = c.to_string();
                    runner.update(|ui| {
                        for ch in s.chars() {
                            ui.search_char(ch);
                        }
                    });
                }
                _ => {}
            }
            self.after_dispatch();
            return;
        }
        match &event.logical_key {
            WinitKey::Character(c) if c.as_str() == ">" => {
                // The command sigil opens the > line, the way `w` opens a
                // whisper. The draft starts empty; the ">" is the prompt.
                runner.update(|ui| ui.start_command());
                self.after_dispatch();
                return;
            }
            WinitKey::Character(c) if c.as_str() == "w" && !self.modifiers.control_key() => {
                runner.update(|ui| ui.start_compose());
                self.after_dispatch();
                return;
            }
            WinitKey::Character(c) if c.as_str() == "r" && !self.modifiers.control_key() => {
                runner.update(|ui| ui.rotate_selected());
                self.after_dispatch();
                return;
            }
            WinitKey::Character(c) if c.as_str() == "f" && !self.modifiers.control_key() => {
                // Cycle the fog viewer: omniscient, then each side. Lets
                // the DM preview a player's view (and drives single-window
                // fog verification without a session).
                runner.update(|ui| ui.cycle_viewer());
                self.after_dispatch();
                return;
            }
            WinitKey::Named(WinitNamedKey::Enter) => {
                if self.profile {
                    eprintln!("[isometry] key: Enter -> end_turn");
                }
                runner.update(|ui| ui.end_turn());
                self.after_dispatch();
                return;
            }
            _ => {}
        }
        if self.modifiers.control_key() {
            match &event.logical_key {
                WinitKey::Character(c) if c.as_str() == "z" => {
                    runner.update(|ui| ui.undo());
                    self.after_dispatch();
                    return;
                }
                WinitKey::Character(c) if c.as_str() == "y" => {
                    runner.update(|ui| ui.redo());
                    self.after_dispatch();
                    return;
                }
                _ => {}
            }
        }
        let pan = match event.logical_key {
            WinitKey::Named(WinitNamedKey::ArrowLeft) => Some((-1.0, 1.0)),
            WinitKey::Named(WinitNamedKey::ArrowRight) => Some((1.0, -1.0)),
            WinitKey::Named(WinitNamedKey::ArrowUp) => Some((-1.0, -1.0)),
            WinitKey::Named(WinitNamedKey::ArrowDown) => Some((1.0, 1.0)),
            _ => None,
        };
        if let Some((dc, dr)) = pan {
            runner.update(|ui| ui.pan_tiles(dc, dr));
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }
}
