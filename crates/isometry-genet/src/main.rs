//! Isometry's genet desktop host (bootstrap plan I1).
//!
//! A winit window presenting the board screen over live state:
//! `GenetAppRunner` diffs `isometry_views::board_root` into a
//! `ScriptedDom`, a retained `IncrementalLayout` lays it out at logical
//! size (incremental `apply` for attribute-only batches, so a camera pan
//! stays off the rebuild path), paint emission lowers to a
//! `netrender::Scene`, and `genet-winit-host`'s `SurfaceHost` rasterizes
//! and composites onto the backbuffer. Borrowed from the woodshed-genet
//! harness shape.
//!
//! Sessions (I4): `--host` binds an iroh session and prints a join
//! ticket; `--join <ticket>` dials it. `--campaign <name>` restores that
//! campaign's durable checkpoint before a host accepts peers. In a session the view is Remote —
//! play routes through the host authority (`net` module bridges the
//! async session to this sync loop). Env hooks: `ISOMETRY_PROFILE=1`
//! (frame timers + net trace), `ISOMETRY_CAPTURE_DIR` (self-capture),
//! `ISOMETRY_SYNTH=1` (stress board), `ISOMETRY_NET_SELFTEST=1` (fire one
//! end-turn after warm-up to verify the session round-trip without OS
//! input automation).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use codicil::Codicil;
use isometry_campaign::{
    CampaignStore, EntropyTape, GenValue, GeneratorRequest, ItemId, ItemInstance, MapScale,
    WorldFact,
};
use isometry_core::FieldValue;
use isometry_net::{apply_game, GameEvent, GameSnapshot, HostSession};
use isometry_system::{
    srd_5e, srd_bestiary, srd_items, srd_spells, GeneratorCatalog, GeneratorLimits, System,
};
use isometry_views::{
    board_css, board_root, demo_map, synth_map, ActionRow, GenerationRequest, InventoryRequest,
    ItemRow, MonsterRow, NetMode, SheetSchema, SpellRow, UiChild, UiState, PANEL_W,
};

mod campaign_store;
mod net;
use campaign_store::{CampaignCheckpoint, CampaignRepository};
use layout_dom_api::{DomMutation, LayoutDomMut as _};
use net::{NetBridge, Role};
use netrender::{ColorLoad, ExternalTexturePlacement, NetrenderOptions};
use paint_list_api::{DeviceIntSize, PaintList as _};
use genet_layout::{Applied, IncrementalLayout, InteractionState, ScrollOffsets, SourceNodeId};
use genet_scripted_dom::{NodeId, ScriptedDom};
use genet_winit_host::SurfaceHost;
use winit::application::ApplicationHandler;
use winit::event::{
    ElementState, KeyEvent as WinitKeyEvent, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey as WinitNamedKey};
use winit::window::{Window, WindowId};
use xilem_serval::{PointerClick, Propagation, GenetAppRunner};

type Runner = GenetAppRunner<UiState, fn(&UiState) -> UiChild, UiChild>;

/// Logical px per wheel notch, used to normalize trackpad pixel deltas.
const WHEEL_NOTCH_PX: f32 = 48.0;
/// Board pan in diagonal tile steps per wheel notch (over the board pane).
const WHEEL_BOARD_TILES: f32 = 2.0;

struct App {
    window: Option<Arc<Window>>,
    host: Option<SurfaceHost>,
    runner: Option<Runner>,
    /// GM-only state saved beside the public map through Muniment. It never
    /// enters the view, map JSON, or replicated snapshot.
    campaign: CampaignStore,
    /// Public campaign facts. The view does not render the journal yet, but
    /// the checkpoint must retain it for replay and host handoff.
    journal: Vec<WorldFact>,
    /// The host's append-only Codicil history. It is empty for local editing
    /// until a session begins, then mirrors the authority actor.
    history: Codicil<GameEvent>,
    /// Retained layout session in logical coordinates: hit-test target
    /// and incremental-apply subject.
    layout: Option<IncrementalLayout<NodeId>>,
    layout_size: (f32, f32),
    sheet: String,
    cursor: (f32, f32),
    modifiers: ModifiersState,
    /// Left button held: paint-capable modes keep applying on entry
    /// into each new tile (drag painting).
    lmb_down: bool,
    /// Opaque id of the last element a held drag dispatched to, so one
    /// tile gets one application per crossing, not one per pixel.
    last_drag: Option<u64>,
    /// A token grabbed by a left-press (Select mode); the release moves it
    /// to the tile under the cursor. `None` when no token drag is active.
    drag_token: Option<isometry_core::TokenId>,
    last_hover: Option<u64>,
    last_focus: Option<u64>,
    profile: bool,
    /// `ISOMETRY_CAPTURE_DIR`: overwrite `<dir>/isometry_capture.png`
    /// with every presented frame, read back from the app's own texture.
    /// Screen grabs lose to overlapping windows; this cannot.
    capture_dir: Option<std::path::PathBuf>,
    /// What session, if any, this process runs (from `--host`/`--join`),
    /// consumed once at `resumed`.
    net_intent: Option<NetIntent>,
    /// `--as <player>`: the fog viewer this process plays as. `None` is
    /// omniscient (the DM / a spectator).
    viewer_arg: Option<String>,
    /// `--campaign <name>`: restore this named campaign checkpoint at boot.
    campaign_arg: Option<String>,
    /// The live session bridge in networked mode.
    net: Option<NetBridge>,
    /// Last session version pulled into the UI; a bump means redraw.
    last_net_version: u64,
    /// `ISOMETRY_NET_SELFTEST=1`: fire one end-turn from inside the app a
    /// few seconds after a session starts, so the UI→net→republish→UI
    /// round-trip is verifiable without OS input automation (Windows
    /// foreground-lock makes driving one of two windows unreliable).
    net_selftest: bool,
    /// Session start instant, for the self-test delay.
    started: Option<Instant>,
    selftest_fired: bool,
    /// The loaded game system (owns the Lua interpreter); character
    /// sheets evaluate through it.
    system: Option<System>,
    /// Last open sheet, so derived stats recompute only on change.
    last_sheet_open: Option<isometry_core::TokenId>,
    /// Entropy remains host-owned even while a preview is uncommitted. Each
    /// accepted record carries its exact draw; no peer evaluates the pack.
    generation_tape: EntropyTape,
    generation_ordinal: u64,
    generator_catalog: GeneratorCatalog,
}

/// Parsed session role from the command line.
enum NetIntent {
    Host,
    Join(String),
}

fn document_slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// The public, reviewable map document.
fn map_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("maps").join(format!("{}.json", document_slug(name)))
}

/// The private GM store paired with a map. Muniment's redb backend makes the
/// slot durable; it is intentionally outside the map's shareable JSON.
fn campaign_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("campaigns").join(format!("{}.redb", document_slug(name)))
}

/// Search the bundled example, a project-local pack root, and user-selected
/// roots. Entries may be pack directories or directories containing packs.
fn generator_pack_roots() -> Vec<std::path::PathBuf> {
    let mut roots = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../isometry-system/examples/packs/demo")];
    let local = std::path::PathBuf::from("packs");
    if local.is_dir() {
        roots.push(local);
    }
    if let Some(paths) = std::env::var_os("ISOMETRY_PACK_DIRS") {
        roots.extend(std::env::split_paths(&paths));
    }
    roots
}

impl App {
    fn scale_factor(&self) -> f64 {
        self.window.as_ref().map_or(1.0, |w| w.scale_factor())
    }

    fn redraw(&mut self) {
        let (Some(window), Some(host), Some(runner)) = (
            self.window.as_ref(),
            self.host.as_ref(),
            self.runner.as_ref(),
        ) else {
            return;
        };
        let size = window.inner_size();
        let (pw, ph) = (size.width.max(1), size.height.max(1));
        let scale = window.scale_factor() as f32;
        let (lw, lh) = (pw as f32 / scale, ph as f32 / scale);

        let t0 = std::time::Instant::now();
        let scene = {
            let dom = runner.dom();
            let mut muts: Vec<DomMutation<NodeId>> = Vec::new();
            dom.borrow_mut().drain_mutations(&mut muts);
            let dom_ref = dom.borrow();
            let sheets: Vec<&str> = vec![self.sheet.as_str()];
            let structural = muts
                .iter()
                .any(|m| !matches!(m, DomMutation::AttributeChanged { .. }));
            let size_changed = self.layout_size != (lw, lh);
            match self.layout.as_mut() {
                Some(layout) if !structural && !size_changed => {
                    if !muts.is_empty() {
                        let _ = layout.apply(&*dom_ref, &sheets, &muts);
                    }
                }
                _ => {
                    let mut layout = IncrementalLayout::new(&*dom_ref, &sheets, lw, lh);
                    if let Some(prev) = self.layout.as_ref() {
                        layout.set_element_scroll(prev.element_scroll().clone());
                    }
                    self.layout = Some(layout);
                    self.layout_size = (lw, lh);
                }
            }
            let layout = self.layout.as_ref().expect("layout just ensured");
            let list = layout.emit_paint_list(
                &*dom_ref,
                &ScrollOffsets::default(),
                DeviceIntSize::new(lw as i32, lh as i32),
            );
            let translated = paint_list_render::translate_paint_cmd_stream(
                list.viewport(),
                list.commands(),
                list.fonts(),
                list.images(),
            );
            translated.scene
        };
        let t_scene = t0.elapsed();

        let t1 = std::time::Instant::now();
        let (tex, view) = host.core().rasterize_scaled(
            &scene,
            pw,
            ph,
            ColorLoad::Clear(wgpu::Color::BLACK),
            scale,
        );
        if let Some(dir) = &self.capture_dir {
            let rgba = host
                .core()
                .renderer()
                .wgpu_device
                .read_rgba8_texture(&tex, pw, ph);
            let path = dir.join("isometry_capture.png");
            if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| {
                let file = std::fs::File::create(&path)?;
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), pw, ph);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let mut writer = enc.write_header().map_err(std::io::Error::other)?;
                writer
                    .write_image_data(&rgba)
                    .map_err(std::io::Error::other)?;
                Ok(())
            }) {
                eprintln!("[isometry] capture failed: {e}");
            }
        }
        let Some(frame) = host.acquire() else { return };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        host.renderer().compose_external_texture(
            &view,
            &target,
            host.format(),
            pw,
            ph,
            ExternalTexturePlacement::new([0.0, 0.0, pw as f32, ph as f32]),
        );
        frame.present();
        if self.profile {
            eprintln!(
                "[isometry] scene {:.2}ms raster+present {:.2}ms",
                t_scene.as_secs_f64() * 1000.0,
                t1.elapsed().as_secs_f64() * 1000.0,
            );
        }
    }

    /// A wheel notch over the board pane snap-pans the board (wheel = pan,
    /// the tactics-canvas convention). Over the side panel it is inert: the
    /// panel fits the default window, and genet has no `overscroll-behavior`
    /// to keep a near-full panel's scroll from chaining into the whole-
    /// document viewport (which would drag the board), so true panel-scroll
    /// for short windows is a follow-on. `nx`/`ny` are wheel notches.
    fn wheel(&mut self, nx: f32, ny: f32) {
        if self.cursor.0 <= PANEL_W {
            return;
        }
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.pan_tiles(-nx * WHEEL_BOARD_TILES, -ny * WHEEL_BOARD_TILES));
        }
    }

    /// Drive `:hover` restyles on target change (engine `set_interaction`;
    /// `Unchanged` when nothing interaction-sensitive matched).
    fn hover(&mut self) {
        let (Some(runner), Some(layout)) = (self.runner.as_ref(), self.layout.as_mut()) else {
            return;
        };
        let (x, y) = self.cursor;
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        let hovered = layout
            .hit_test(&*dom_ref, x, y, &ScrollOffsets::default())
            .map(|n| layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n));
        let focused = runner
            .focus()
            .map(|n| layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n));
        if (hovered, focused) == (self.last_hover, self.last_focus) {
            return;
        }
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

    /// Hit-test the cursor against the retained layout: the node plus
    /// its stable opaque id (the drag-dedupe key).
    fn cursor_hit(&self) -> Option<(NodeId, u64)> {
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

    fn click(&mut self) {
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

    /// Consume one-shot state requests (save/load) and repaint: the
    /// tail of every dispatch.
    fn after_dispatch(&mut self) {
        let mut save: Option<(std::path::PathBuf, String, String, GameSnapshot)> = None;
        let mut load: Option<(std::path::PathBuf, String)> = None;
        let journal = self.journal.clone();
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                if std::mem::take(&mut ui.save_requested) {
                    match serde_json::to_string_pretty(&ui.map) {
                        Ok(json) => {
                            let name = ui.map.name.clone();
                            save = Some((
                                map_path(&name),
                                json,
                                name,
                                GameSnapshot {
                                    map: ui.map.clone(),
                                    turns: ui.turns.clone(),
                                    roll_log: ui.roll_log.clone(),
                                    journal: journal.clone(),
                                    inventories: ui.inventories.clone(),
                                    generations: ui.generations.clone(),
                                    maps: ui.campaign_maps.clone(),
                                    active_map: ui.active_map.clone(),
                                    world: ui.world.clone(),
                                },
                            ));
                        }
                        Err(e) => ui.status = format!("save failed: {e}"),
                    }
                }
                if std::mem::take(&mut ui.load_requested) {
                    load = Some((map_path(&ui.map.name), ui.map.name.clone()));
                }
            });
        }
        if let Some((path, json, name, local_public)) = save {
            let map_result =
                std::fs::create_dir_all("maps").and_then(|_| std::fs::write(&path, json));
            let campaign = self
                .net
                .as_ref()
                .and_then(NetBridge::campaign)
                .unwrap_or_else(|| self.campaign.clone());
            let public = self
                .net
                .as_ref()
                .and_then(NetBridge::latest)
                .unwrap_or(local_public);
            let history = self
                .net
                .as_ref()
                .and_then(NetBridge::history)
                .unwrap_or_else(|| self.history.clone());
            let checkpoint = CampaignCheckpoint::new(public, campaign, history);
            let campaign_result = CampaignRepository::open(campaign_path(&name))
                .and_then(|repository| repository.save_checkpoint(&checkpoint));
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| {
                    ui.status = match (map_result.as_ref(), campaign_result.as_ref()) {
                        (Ok(()), Ok(())) => format!("saved {}", path.display()),
                        (Err(error), Ok(())) => {
                            format!("checkpoint saved, map export failed: {error}")
                        }
                        (Err(error), Err(_)) => format!("map save failed: {error}"),
                        (Ok(()), Err(error)) => {
                            format!("map saved, private campaign save failed: {error}")
                        }
                    };
                });
            }
        }
        if let Some((path, name)) = load {
            let checkpoint = CampaignRepository::open(campaign_path(&name))
                .and_then(|repository| repository.load_checkpoint());
            if let Ok(Some(checkpoint)) = checkpoint {
                self.campaign = checkpoint.private;
                self.journal = checkpoint.public.journal.clone();
                self.history = checkpoint.history;
                if let Some(runner) = self.runner.as_mut() {
                    runner.update(|ui| {
                        ui.apply_snapshot(checkpoint.public);
                        ui.status = format!("loaded checkpoint {}", path.display());
                    });
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
                return;
            }
            let checkpoint_error = checkpoint.err();
            let loaded = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|json| {
                    serde_json::from_str::<isometry_core::MapDocument>(&json)
                        .map_err(|e| e.to_string())
                });
            match loaded {
                Ok(map) => {
                    let name = map.name.clone();
                    let campaign = CampaignRepository::open(campaign_path(&name))
                        .and_then(|repository| repository.load_private());
                    if let Ok(campaign) = campaign.as_ref() {
                        self.campaign = campaign.clone();
                    }
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| {
                            ui.replace_map(map);
                            ui.status = match (campaign, checkpoint_error) {
                                (Ok(_), None) => format!("loaded {}", path.display()),
                                (Err(error), _) => {
                                    format!("map loaded, private campaign state failed: {error}")
                                }
                                (_, Some(error)) => format!(
                                    "loaded legacy map after checkpoint read failed: {error}"
                                ),
                            };
                        });
                    }
                }
                Err(error) => {
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| ui.status = format!("load failed: {error}"));
                    }
                }
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        self.pump_sheets();
        self.pump_generators();
        self.pump_net();
    }

    /// In networked mode: ship the UI's queued game events to the
    /// session, and pull the latest replicated snapshot into the view
    /// when the session advanced. No-op when solo.
    fn pump_net(&mut self) {
        if self.net.is_none() {
            return;
        }
        // Drain the outbox and submit each event.
        let mut events = Vec::new();
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| events = std::mem::take(&mut ui.net_outbox));
        }
        // Drain queued whispers (host-side) too.
        let mut whispers = Vec::new();
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| whispers = std::mem::take(&mut ui.whisper_outbox));
        }
        let mut received = Vec::new();
        let mut players = Vec::new();
        let mut campaign_outcomes = Vec::new();
        let mut failure = None;
        if let Some(net) = self.net.as_mut() {
            // Armillary keeps the network runtime off the winit kernel. Drain
            // its typed updates before reading any mirror state.
            net.poll();
            if !events.is_empty() && self.profile {
                eprintln!("[isometry] pump: submitting {} event(s)", events.len());
            }
            for event in events {
                net.submit(event);
            }
            for (to, text) in whispers {
                net.whisper(to, text);
            }
            // Deliver received whispers into the message log, and refresh
            // the whisper-target list from connected players.
            received = net.take_whispers();
            players = net.players();
            campaign_outcomes = net.take_campaign_outcomes();
            failure = net.take_failure();
        }
        if !received.is_empty()
            || !players.is_empty()
            || !campaign_outcomes.is_empty()
            || failure.is_some()
        {
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| {
                    for (from, text) in &received {
                        ui.receive_whisper(from, text);
                    }
                    ui.connected_players = players;
                    if let Some(outcome) = campaign_outcomes.last() {
                        ui.status = match &outcome.value {
                            Ok(()) => {
                                format!("committed campaign draft (request {})", outcome.request)
                            }
                            Err(error) => format!(
                                "campaign commit failed (request {}): {error}",
                                outcome.request
                            ),
                        };
                    }
                    if let Some(error) = &failure {
                        ui.status = error.clone();
                    }
                });
                if !received.is_empty() || !campaign_outcomes.is_empty() || failure.is_some() {
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
        }
        // Mirror in a new snapshot when the session version bumped.
        let version = self.net.as_ref().map(|n| n.version()).unwrap_or(0);
        if version != self.last_net_version {
            self.last_net_version = version;
            let snap = self.net.as_ref().and_then(|n| n.latest());
            if let (Some(snap), Some(runner)) = (snap, self.runner.as_mut()) {
                self.journal = snap.journal.clone();
                if let Some(history) = self.net.as_ref().and_then(NetBridge::history) {
                    self.history = history;
                }
                runner.update(|ui| ui.apply_snapshot(snap));
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
        }
    }

    /// Drain the UI's sheet requests (bind / edit / action) and evaluate
    /// them through the game system: bind a default sheet, apply a field
    /// edit, or roll an action; then recompute the open sheet's derived
    /// stats. Cheap-checks first so a normal frame does no work.
    fn pump_sheets(&mut self) {
        if self.system.is_none() {
            return;
        }
        let (bind, edit, action, inventory_request, open) = match self.runner.as_ref() {
            Some(r) => {
                let s = r.state();
                (
                    s.bind_sheet_request,
                    s.sheet_edit.clone(),
                    s.sheet_action.clone(),
                    s.inventory_request.clone(),
                    s.open_sheet,
                )
            }
            None => return,
        };
        let open_changed = open != self.last_sheet_open;
        let effective_missing = self
            .runner
            .as_ref()
            .is_some_and(|runner| open.is_some() && runner.state().sheet_effective.is_none());
        if bind.is_none()
            && edit.is_none()
            && action.is_none()
            && inventory_request.is_none()
            && !open_changed
            && !effective_missing
        {
            return;
        }
        self.last_sheet_open = open;
        let system = self.system.as_mut().expect("system present");
        let Some(runner) = self.runner.as_mut() else {
            return;
        };

        // Bind a fresh default sheet.
        if let Some(tok) = bind {
            let sheet = system.default_sheet();
            runner.update(|ui| {
                ui.bind_sheet_request = None;
                ui.map.set_sheet(tok, sheet.clone());
                if ui.net_mode == NetMode::Remote {
                    ui.net_outbox.push(GameEvent::SheetSet {
                        token: tok,
                        sheet: sheet.clone(),
                    });
                }
            });
        }

        // Apply a field edit (clamped non-negative), then replicate.
        if let Some((tok, key, delta)) = edit {
            let mut updated = None;
            runner.update(|ui| {
                ui.sheet_edit = None;
                if let Some(sheet) = ui.map.sheets.get_mut(&tok) {
                    let cur = sheet.int(&key).unwrap_or(0);
                    sheet.set_int(&key, (cur + delta).max(0));
                    updated = Some(sheet.clone());
                }
            });
            if let Some(sheet) = updated {
                runner.update(|ui| {
                    if ui.net_mode == NetMode::Remote {
                        ui.net_outbox
                            .push(GameEvent::SheetSet { token: tok, sheet });
                    }
                });
            }
        }

        // Item instances are minted and equipped on the host. The request only
        // carries pack/template data; the authoritative inventory replacement
        // is what enters the replicated log.
        if let Some(request) = inventory_request {
            let mut event = None;
            runner.update(|ui| {
                ui.inventory_request = None;
                match request {
                    InventoryRequest::AddCompendiumItem {
                        token,
                        template,
                        name,
                        category,
                    } => {
                        if ui.map.token(token).is_none() {
                            ui.status = "cannot add item to a missing token".to_owned();
                            return;
                        }
                        let inventory = ui.inventories.entry(token).or_default();
                        let mut ordinal = inventory.items.len();
                        let id = loop {
                            let id = ItemId::new(format!("token-{}.item-{ordinal}", token.0));
                            if !inventory.items.contains_key(&id) {
                                break id;
                            }
                            ordinal += 1;
                        };
                        let appearance_layers = if category == "Weapon" {
                            vec![format!("weapon:{template}")]
                        } else {
                            Vec::new()
                        };
                        let item = ItemInstance {
                            id,
                            template: format!("srd5e:{template}"),
                            name: name.clone(),
                            quantity: 1,
                            tags: vec![category.to_lowercase()],
                            modifiers: Vec::new(),
                            appearance_layers,
                        };
                        if inventory.insert(item).is_ok() {
                            event = Some(GameEvent::InventorySet {
                                token,
                                inventory: inventory.clone(),
                            });
                            ui.status = format!("added {name}");
                        }
                    }
                    InventoryRequest::Equip { token, slot, item } => {
                        if let Some(inventory) = ui.inventories.get_mut(&token) {
                            if inventory.equip(slot, item).is_ok() {
                                event = Some(GameEvent::InventorySet {
                                    token,
                                    inventory: inventory.clone(),
                                });
                                ui.status = "equipped item".to_owned();
                            }
                        }
                    }
                    InventoryRequest::Unequip { token, slot } => {
                        if let Some(inventory) = ui.inventories.get_mut(&token) {
                            inventory.equipped.remove(&slot);
                            event = Some(GameEvent::InventorySet {
                                token,
                                inventory: inventory.clone(),
                            });
                            ui.status = "unequipped item".to_owned();
                        }
                    }
                    InventoryRequest::Transfer { from, to, item } => {
                        let destination_has_item = ui
                            .inventories
                            .get(&to)
                            .is_some_and(|inventory| inventory.items.contains_key(&item));
                        if !destination_has_item {
                            let moved = ui
                                .inventories
                                .get_mut(&from)
                                .and_then(|inventory| inventory.take(&item).ok());
                            if let Some(moved) = moved {
                                if ui.inventories.entry(to).or_default().insert(moved).is_ok() {
                                    event = Some(GameEvent::ItemTransfer { from, to, item });
                                    ui.status = "transferred item".to_owned();
                                }
                            }
                        }
                    }
                }
                ui.sheet_effective = None;
            });
            if let Some(event) = event {
                runner.update(|ui| {
                    if ui.net_mode == NetMode::Remote {
                        ui.net_outbox.push(event);
                    }
                });
            }
        }

        // Roll an action: evaluate its dice expression against the sheet.
        if let Some((tok, key)) = action {
            let (sheet, inventory) = {
                let state = runner.state();
                (
                    state.map.sheet(tok).cloned(),
                    state.inventories.get(&tok).cloned(),
                )
            };
            if let Some(sheet) = sheet {
                let effective = system.effective_sheet(&sheet, inventory.as_ref());
                if let Some(expr) = system.action_expr(&key, &effective) {
                    let by = sheet.text("name").unwrap_or("?").to_owned();
                    let label = system
                        .actions
                        .iter()
                        .find(|a| a.key == key)
                        .map(|a| a.label.clone())
                        .unwrap_or(key);
                    runner.update(|ui| {
                        ui.sheet_action = None;
                        ui.roll_labeled(&by, &label, &expr);
                    });
                }
            }
        }

        // Recompute derived stats for the open sheet.
        if let Some(tok) = open {
            let (sheet, inventory) = {
                let state = runner.state();
                (
                    state.map.sheet(tok).cloned(),
                    state.inventories.get(&tok).cloned(),
                )
            };
            if let Some(sheet) = sheet {
                let effective = system.effective_sheet(&sheet, inventory.as_ref());
                let derived = system.derived(&effective);
                runner.update(|ui| {
                    ui.sheet_effective = Some(effective);
                    ui.sheet_derived = derived;
                });
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// Evaluate or commit a host-owned generator preview. The view asks for a
    /// one-shot action; this desktop layer loads the declared pack and owns the
    /// entropy tape, while `isometry-net` remains scripting-agnostic.
    fn pump_generators(&mut self) {
        let mut action = None;
        let mut locks = Default::default();
        let mut can_edit = false;
        let mut remote = false;
        let mut existing_ids = Vec::new();
        let mut preview = None;
        let mut choice = None;
        let mut local_snapshot = None;
        let journal = self.journal.clone();
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                action = ui.generation_request.take();
                locks = ui.generator_locks.clone();
                can_edit = ui.can_edit_inventory;
                remote = ui.net_mode == NetMode::Remote;
                choice = ui.selected_generator().cloned();
                existing_ids = ui
                    .generations
                    .iter()
                    .map(|record| record.id.clone())
                    .collect();
                if action == Some(GenerationRequest::Commit) {
                    preview = ui.generator_preview.take();
                    local_snapshot = Some(GameSnapshot {
                        map: ui.map.clone(),
                        turns: ui.turns.clone(),
                        roll_log: ui.roll_log.clone(),
                        journal: journal.clone(),
                        inventories: ui.inventories.clone(),
                        generations: ui.generations.clone(),
                        maps: ui.campaign_maps.clone(),
                        active_map: ui.active_map.clone(),
                        world: ui.world.clone(),
                    });
                }
            });
        }
        let Some(action) = action else {
            return;
        };
        if !can_edit {
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| ui.status = "generation requires the host".to_owned());
            }
            return;
        }
        match action {
            GenerationRequest::Generate => {
                let Some(choice) = choice else {
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| ui.status = "no generator selected".to_owned());
                    }
                    return;
                };
                let mut ordinal = self.generation_ordinal;
                let record_id = loop {
                    ordinal = ordinal.wrapping_add(1);
                    let generator_slug: String = choice
                        .id
                        .chars()
                        .map(|c| if c.is_alphanumeric() { c } else { '.' })
                        .collect();
                    let id = format!("generated.{generator_slug}.{ordinal}");
                    if !existing_ids.iter().any(|existing| existing == &id) {
                        break id;
                    }
                };
                self.generation_ordinal = ordinal;
                let request = GeneratorRequest {
                    generator: choice.id,
                    args: choice.default_args,
                    locks,
                };
                let result = self.generator_catalog.generate(
                    record_id,
                    &request,
                    &mut self.generation_tape,
                    GeneratorLimits::default(),
                );
                if let Some(runner) = self.runner.as_mut() {
                    runner.update(|ui| match result {
                        Ok(record) => {
                            ui.generator_preview = Some(record);
                            ui.status = "generated preview".to_owned();
                        }
                        Err(error) => ui.status = format!("generation failed: {error}"),
                    });
                }
            }
            GenerationRequest::Commit => {
                let Some(record) = preview else {
                    return;
                };
                if matches!(record.proposal, GenValue::Campaign { .. }) {
                    let item_owner = local_snapshot.as_ref().and_then(|snapshot| {
                        snapshot
                            .turns
                            .active()
                            .or_else(|| snapshot.map.tokens.first().map(|token| token.id))
                    });
                    if remote {
                        if let Some(net) = self.net.as_mut() {
                            let request = net.commit_campaign(record, item_owner);
                            if let Some(runner) = self.runner.as_mut() {
                                runner.update(|ui| match request {
                                    Some(request) => {
                                        ui.status = format!(
                                            "committing campaign draft (request {})",
                                            request
                                        )
                                    }
                                    None => {
                                        ui.status = "campaign authority actor stopped".to_owned()
                                    }
                                });
                            }
                        }
                    } else if let Some(snapshot) = local_snapshot {
                        let mut host = HostSession::with_history(
                            snapshot,
                            self.campaign.clone(),
                            self.history.clone(),
                        );
                        match host.commit_campaign(record.clone(), item_owner) {
                            Ok(_) => {
                                self.campaign = host.campaign().clone();
                                self.history = host.history().clone();
                                self.journal = host.state().journal.clone();
                                let snapshot = host.state().clone();
                                if let Some(runner) = self.runner.as_mut() {
                                    runner.update(|ui| {
                                        ui.apply_snapshot(snapshot);
                                        ui.status = "committed campaign draft".to_owned();
                                    });
                                }
                            }
                            Err(error) => {
                                if let Some(runner) = self.runner.as_mut() {
                                    runner.update(|ui| {
                                        ui.generator_preview = Some(record);
                                        ui.status = format!("campaign commit failed: {error}");
                                    });
                                }
                            }
                        }
                    }
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                    return;
                }
                let mut events = vec![GameEvent::Generation(record.clone())];
                match &record.proposal {
                    GenValue::LocalMap { map } => match map.lower(MapScale::Local) {
                        Ok(mut campaign_map) => {
                            campaign_map.id = format!("{}.map", record.id);
                            let id = campaign_map.id.clone();
                            events.push(GameEvent::MapStored(campaign_map));
                            events.push(GameEvent::MapActivated { id });
                        }
                        Err(error) => {
                            if let Some(runner) = self.runner.as_mut() {
                                runner.update(|ui| {
                                    ui.generator_preview = Some(record.clone());
                                    ui.status = format!("generated map is invalid: {error}");
                                });
                            }
                            return;
                        }
                    },
                    GenValue::WorldFact { fact } => events.push(GameEvent::Fact(fact.clone())),
                    GenValue::Item { item } => {
                        let target = local_snapshot.as_ref().and_then(|snapshot| {
                            snapshot
                                .turns
                                .active()
                                .or_else(|| snapshot.map.tokens.first().map(|token| token.id))
                        });
                        let Some(target) = target else {
                            if let Some(runner) = self.runner.as_mut() {
                                runner.update(|ui| {
                                    ui.generator_preview = Some(record.clone());
                                    ui.status =
                                        "generated item needs a character on the map".to_owned();
                                });
                            }
                            return;
                        };
                        let mut inventory = local_snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.inventories.get(&target))
                            .cloned()
                            .unwrap_or_default();
                        let instance = ItemInstance {
                            id: ItemId::new(format!("{}.item", record.id)),
                            template: item.template.clone(),
                            name: item.name.clone(),
                            quantity: 1,
                            tags: item.tags.clone(),
                            modifiers: Vec::new(),
                            appearance_layers: Vec::new(),
                        };
                        if let Err(error) = inventory.insert(instance) {
                            if let Some(runner) = self.runner.as_mut() {
                                runner.update(|ui| {
                                    ui.generator_preview = Some(record.clone());
                                    ui.status = format!("generated item is invalid: {error:?}");
                                });
                            }
                            return;
                        }
                        events.push(GameEvent::InventorySet {
                            token: target,
                            inventory,
                        });
                    }
                    _ => {}
                }
                if remote {
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| {
                            ui.net_outbox.extend(events);
                            ui.status = "committing generated result".to_owned();
                        });
                    }
                } else if let Some(mut snapshot) = local_snapshot {
                    let result = events
                        .iter()
                        .try_for_each(|event| apply_game(&mut snapshot, event));
                    match result {
                        Ok(()) => {
                            self.journal = snapshot.journal.clone();
                            if let Some(runner) = self.runner.as_mut() {
                                runner.update(|ui| {
                                    ui.apply_snapshot(snapshot);
                                    ui.status = "committed generated result".to_owned();
                                });
                            }
                        }
                        Err(error) => {
                            if let Some(runner) = self.runner.as_mut() {
                                runner.update(|ui| {
                                    ui.generator_preview = Some(record);
                                    ui.status = format!("generation commit failed: {error:?}");
                                });
                            }
                        }
                    }
                }
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// The env-gated self-test: after a warm-up, emit one end-turn as if
    /// the user pressed it, exercising the full session round-trip.
    fn maybe_selftest(&mut self) {
        if !self.net_selftest || self.selftest_fired {
            return;
        }
        let ready = self
            .started
            .map(|t| t.elapsed() > Duration::from_secs(3))
            .unwrap_or(false);
        if ready {
            self.selftest_fired = true;
            eprintln!("[isometry] selftest: firing end_turn");
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| ui.end_turn());
            }
            self.pump_net();
        }
    }

    fn key(&mut self, event: &WinitKeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
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
        if self.net.is_some() {
            self.started = Some(Instant::now());
        }
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
        // In a session, poll the bridge ~10Hz so remote changes (a peer's
        // move) reach the view without local input driving the loop.
        if self.net.is_some() {
            self.maybe_selftest();
            self.pump_net();
            self.pump_sheets();
            self.pump_generators();
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(100),
            ));
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
            WindowEvent::KeyboardInput { event, .. } => self.key(&event),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

/// Parse `--host` or `--join <ticket>` from the command line.
fn parse_net_intent() -> Option<NetIntent> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--host") {
        Some(NetIntent::Host)
    } else if let Some(i) = args.iter().position(|a| a == "--join") {
        args.get(i + 1).map(|t| NetIntent::Join(t.clone()))
    } else {
        None
    }
}

/// The view-facing schema (plain labels) for a loaded system, so the
/// board renders a sheet without depending on isometry-system.
/// Translate the system's bestiary into the view-side compendium rows.
fn bestiary_of() -> Vec<MonsterRow> {
    srd_bestiary()
        .into_iter()
        .map(|m| {
            let cr_label = m.cr_label();
            MonsterRow {
                key: m.key,
                name: m.name,
                cr: m.challenge_rating,
                cr_label,
                kind: m.kind,
                size: m.size,
                alignment: m.alignment,
                hp: m.hit_points,
                hit_dice: m.hit_dice,
                ac: m.armor_class,
                speed_ft: m.speed_ft,
                xp: m.xp,
                abilities: m.abilities,
                actions: m
                    .actions
                    .into_iter()
                    .map(|a| ActionRow {
                        name: a.name,
                        to_hit: a.to_hit,
                        damage: a.damage,
                        desc: a.desc,
                    })
                    .collect(),
                sprite: m.sprite,
            }
        })
        .collect()
}

fn spells_of() -> Vec<SpellRow> {
    srd_spells()
        .into_iter()
        .map(|s| {
            let level_label = s.level_label();
            SpellRow {
                key: s.key,
                name: s.name,
                level: s.level,
                level_label,
                school: s.school,
                casting_time: s.casting_time,
                range: s.range,
                components: s.components,
                duration: s.duration,
                desc: s.desc,
            }
        })
        .collect()
}

fn items_of() -> Vec<ItemRow> {
    srd_items()
        .into_iter()
        .map(|i| ItemRow {
            key: i.key,
            name: i.name,
            category: i.category,
            cost: i.cost,
            weight: i.weight,
            detail: i.detail,
            desc: i.desc,
        })
        .collect()
}

fn schema_of(system: &System) -> SheetSchema {
    SheetSchema {
        fields: system
            .fields
            .iter()
            .map(|f| {
                (
                    f.key.clone(),
                    f.label.clone(),
                    matches!(f.default, FieldValue::Int(_)),
                )
            })
            .collect(),
        derived: system
            .derived
            .iter()
            .map(|d| (d.key.clone(), d.label.clone()))
            .collect(),
        actions: system
            .actions
            .iter()
            .map(|a| (a.key.clone(), a.label.clone()))
            .collect(),
    }
}

/// Parse `--as <player>` from the command line.
fn parse_viewer() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == "--as")
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Parse `--campaign <name>` for checkpoint restore. The name shares the map
/// slug convention, so `--campaign "Demo Skirmish"` resolves its paired store.
fn parse_campaign() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == "--campaign")
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let generator_catalog = GeneratorCatalog::discover(generator_pack_roots());
    let mut app = App {
        window: None,
        host: None,
        runner: None,
        campaign: CampaignStore::new(),
        journal: Vec::new(),
        history: Codicil::new(),
        layout: None,
        layout_size: (0.0, 0.0),
        sheet: board_css(),
        cursor: (0.0, 0.0),
        modifiers: ModifiersState::empty(),
        lmb_down: false,
        last_drag: None,
        drag_token: None,
        last_hover: None,
        last_focus: None,
        profile: std::env::var_os("ISOMETRY_PROFILE").is_some(),
        capture_dir: std::env::var_os("ISOMETRY_CAPTURE_DIR").map(Into::into),
        net_intent: parse_net_intent(),
        viewer_arg: parse_viewer(),
        campaign_arg: parse_campaign(),
        net: None,
        last_net_version: 0,
        net_selftest: std::env::var_os("ISOMETRY_NET_SELFTEST").is_some(),
        started: None,
        selftest_fired: false,
        system: None,
        last_sheet_open: None,
        generation_tape: EntropyTape::from_seed(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos() as u64)
                .unwrap_or(1),
        ),
        generation_ordinal: 0,
        generator_catalog,
    };
    event_loop.run_app(&mut app).expect("run app");
}
