//! The winit `ApplicationHandler`: window lifecycle and event routing.
//!
//! Resume builds the window, engine, and layout; `about_to_wait` drives the
//! self-tests and animation ticks; `window_event` routes input into the
//! translations in `input`.
//!
//! Split out of `main.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Isometry")
                        .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 820.0)),
                )
                .expect("create window"),
        );
        let size = window.inner_size();
        let host = SurfaceHost::boot(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            NetrenderOptions {
                tile_cache_size: Some(1024),
                enable_vello: true,
                ..Default::default()
            },
        )
        .expect("boot genet host");
        // `ISOMETRY_SYNTH=<n>` loads an n x n synthetic stress board (n>1,
        // default 30 = the probe P2 board) instead of the demo skirmish;
        // large n exercises viewport windowing.
        let map = match std::env::var("ISOMETRY_SYNTH") {
            Ok(v) => {
                let n = v
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|&n| n > 1)
                    .unwrap_or(30);
                synth_map(n, n)
            }
            Err(_) => demo_map(),
        };
        let can_restore = !matches!(self.net_intent.as_ref(), Some(NetIntent::Join(_)));
        let mut restore_status = None;
        let mut restored_public = None;
        if can_restore {
            if let Some(name) = self.campaign_arg.take() {
                match CampaignRepository::open(campaign_path(&name))
                    .and_then(|repository| repository.load_checkpoint())
                {
                    Ok(Some(checkpoint)) => {
                        self.campaign = checkpoint.private;
                        self.journal = checkpoint.public.journal.clone();
                        self.history = checkpoint.history;
                        restored_public = Some(checkpoint.public);
                        restore_status = Some(format!("restored campaign {name}"));
                    }
                    Ok(None) => restore_status = Some(format!("campaign {name} has no checkpoint")),
                    Err(error) => {
                        restore_status = Some(format!("campaign restore failed: {error}"))
                    }
                }
            }
        }
        let mut ui = UiState::new(map);
        if let Some(snapshot) = restored_public {
            ui.apply_snapshot(snapshot);
        }
        ui.generator_choices = self.generator_catalog.choices();
        for diagnostic in self.generator_catalog.diagnostics() {
            eprintln!("[isometry] content pack: {diagnostic}");
        }
        if let Some(status) = restore_status {
            ui.status = status;
        }
        // Start with the board roughly centered in the pane, and every
        // token in the turn order (a skirmish ready to play; drop
        // tokens out via the panel for free movement).
        ui.camera = (420.0, 140.0);
        // Seed the pane size so the view can window tile emission to the
        // viewport (the host keeps it current on resize).
        let scale = window.scale_factor() as f32;
        ui.viewport = (
            (size.width as f32 / scale - PANEL_W).max(0.0),
            size.height as f32 / scale,
        );
        let ids: Vec<_> = ui.map.tokens.iter().map(|t| t.id).collect();
        for id in ids {
            ui.turns.add(id);
        }

        // Session setup: host publishes this board; a client starts from
        // an empty view and fills in on the first snapshot. Either way the
        // view is Remote, so play routes through the session.
        match self.net_intent.take() {
            Some(NetIntent::Host) => {
                self.net_is_host = true;
                ui.net_mode = NetMode::Remote;
                let snapshot = GameSnapshot {
                    map: ui.map.clone(),
                    turns: ui.turns.clone(),
                    roll_log: Vec::new(),
                    journal: self.journal.clone(),
                    inventories: ui.inventories.clone(),
                    generations: ui.generations.clone(),
                    maps: ui.campaign_maps.clone(),
                    active_map: ui.active_map.clone(),
                    world: ui.world.clone(),
                    clocks: ui.clocks.clone(),

                    party_cap: ui.party_cap,
                    last_beats: Vec::new(),
                    beat_seq: 0,
                };
                self.net = Some(NetBridge::spawn(Role::Host {
                    state: snapshot,
                    campaign: self.campaign.clone(),
                    history: self.history.clone(),
                }));
            }
            Some(NetIntent::Join(ticket)) => {
                ui.net_mode = NetMode::Remote;
                ui.can_edit_inventory = false;
                ui.status = "connecting...".to_owned();
                let name = self
                    .viewer_arg
                    .clone()
                    .unwrap_or_else(|| "player".to_owned());
                self.net = Some(NetBridge::spawn(Role::Client { ticket, name }));
            }
            None => {}
        }
        // Boot clock. The net selftest waits on it, and so does the combat
        // selftest, which runs solo (there is no session to wait for).
        self.started = Some(Instant::now());
        // Fog viewer from `--as`. Applies in any mode: a client sees
        // through its player's tokens, and a solo run can preview a side.
        if let Some(v) = self.viewer_arg.take() {
            ui.viewer = Some(v);
            ui.recompute_fog();
        }
        // Seed the dice generator with real entropy so rolls differ per
        // launch (the clock is plenty for a friendly table).
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        ui.reseed(seed);

        // Load the game system (5e SRD) and hand the view its schema so it
        // can render sheets without knowing any rules.
        let system = srd_5e();
        ui.sheet_schema = schema_of(&system);
        ui.bestiary = bestiary_of();
        ui.emotes = self.pack_emotes.clone();
        ui.spells = spells_of();
        ui.items = items_of();
        self.system = Some(system);

        let dom = Rc::new(RefCell::new(ScriptedDom::new()));
        let runner = Runner::new(dom, board_root as fn(&UiState) -> UiChild, ui);
        self.window = Some(window);
        self.host = Some(host);
        self.runner = Some(runner);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.maybe_combat_selftest();
        self.maybe_travel_selftest();
        self.maybe_cmd_selftest();
        self.maybe_convince_selftest();
        self.maybe_storylet_selftest();
        self.maybe_overmap_selftest();
        if (self.travel_selftest && !self.travel_fired)
            || (self.cmd_selftest && !self.cmd_fired)
            || (self.convince_selftest && !self.convince_fired)
            || (self.storylet_selftest && !self.storylet_fired)
            || (self.overmap_selftest && !self.overmap_fired)
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(100),
            ));
        }
        // A still board parks on `Wait`, which blocks until input arrives, so an
        // armed selftest would never reach its own deadline. Tick until it fires.
        if self.combat_selftest && !(self.combat_swings == 0 && self.combat_emoted) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(100),
            ));
        }
        // In a session, poll the bridge ~10Hz so remote changes (a peer's
        // move) reach the view without local input driving the loop.
        if self.net.is_some() {
            self.maybe_selftest();
            self.pump_net();
            self.pump_sheets();
            self.pump_generators();
            self.pump_storylets();
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(100),
            ));
        }
        // While a beat is playing, drive frames. `has_active_animations` is
        // clock-based and settles on its own, so the loop drops back to `Wait`
        // the moment the last animation ends: the board is idle-cheap again
        // without app state tracking "am I animating".
        let animating = self
            .layout
            .as_ref()
            .is_some_and(IncrementalLayout::has_active_animations);
        if animating {
            self.beats_playing = true;
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
            ));
        } else if self.beats_playing {
            // The last beat just ended. Drop the classes so the *next* strike is
            // a genuine change and restarts the animation; leaving them set
            // would restyle nothing and the second swing would stand still.
            self.beats_playing = false;
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| ui.clear_beats());
            }
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(host) = self.host.as_mut() {
                    host.resize(size.width.max(1), size.height.max(1));
                }
                // Keep the view's pane size current so windowing culls to
                // the actual viewport.
                let scale = self.scale_factor() as f32;
                let vw = (size.width as f32 / scale - PANEL_W).max(0.0);
                let vh = size.height as f32 / scale;
                if let Some(runner) = self.runner.as_mut() {
                    runner.update(|ui| ui.viewport = (vw, vh));
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (nx, ny) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    // Trackpad pixel deltas: approximate notches off the same
                    // per-notch px the panel scrolls by.
                    MouseScrollDelta::PixelDelta(p) => {
                        let s = self.scale_factor() as f32;
                        (
                            p.x as f32 / s / WHEEL_NOTCH_PX,
                            p.y as f32 / s / WHEEL_NOTCH_PX,
                        )
                    }
                };
                self.wheel(nx, ny);
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale_factor();
                self.cursor = ((position.x / scale) as f32, (position.y / scale) as f32);
                self.hover();
                // Play-mode path preview: rebuild only when the hovered
                // tile changed and a reach highlight is showing.
                if let Some(runner) = self.runner.as_mut() {
                    if let Some(t) = runner.state().hover_needs_update(self.cursor) {
                        runner.update(|ui| ui.hover_tile = t);
                        if let Some(window) = self.window.as_ref() {
                            window.request_redraw();
                        }
                    }
                }
                // Drag painting: while the button is held in a paint
                // mode, entering a tile applies the brush there. The
                // panel strip is excluded so a drag can never spam its
                // buttons.
                if self.lmb_down && self.cursor.0 > PANEL_W {
                    let drags = self
                        .runner
                        .as_mut()
                        .map(|r| {
                            let mut d = false;
                            r.update(|ui| d = ui.mode.drags());
                            d
                        })
                        .unwrap_or(false);
                    if drags {
                        if let Some((_, id)) = self.cursor_hit() {
                            if self.last_drag != Some(id) {
                                self.click();
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.lmb_down = true;
                self.click();
                // A left-click off the menu dismisses it (a menu item's own
                // handler already closed it; this catches clicks elsewhere).
                if self
                    .runner
                    .as_ref()
                    .is_some_and(|r| r.state().context_menu.is_some())
                {
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| ui.close_context_menu());
                    }
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
                // A press on a token (Select mode) starts a drag; the
                // release moves it to the tile under the cursor.
                self.drag_token = self
                    .runner
                    .as_ref()
                    .and_then(|r| r.state().token_drag_candidate(self.cursor));
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.lmb_down = false;
                self.last_drag = None;
                if let Some(id) = self.drag_token.take() {
                    // Move the grabbed token to the release tile if it moved.
                    let to = self.runner.as_ref().and_then(|r| {
                        let ui = r.state();
                        let cur = ui.map.token(id)?.at;
                        let to = ui.tile_at_cursor(self.cursor)?;
                        (to != cur).then_some(to)
                    });
                    if let Some(to) = to {
                        if let Some(runner) = self.runner.as_mut() {
                            runner.update(|ui| ui.drag_move_token(id, to));
                        }
                        self.after_dispatch();
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                // Right-click a token opens its context menu at the cursor.
                let target = self.runner.as_ref().and_then(|r| {
                    let ui = r.state();
                    let tile = ui.tile_at_cursor(self.cursor)?;
                    ui.map.tokens.iter().find(|t| t.at == tile).map(|t| t.id)
                });
                if let Some(id) = target {
                    let pos = (self.cursor.0 - PANEL_W, self.cursor.1);
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| ui.open_context_menu(id, pos));
                    }
                    self.after_dispatch();
                }
            }
            WindowEvent::Ime(ime) => self.ime(&ime),
            WindowEvent::KeyboardInput { event, .. } => self.key(&event),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}
