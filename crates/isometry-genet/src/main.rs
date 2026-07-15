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
use isometry_core::{Facing, FieldValue, MapDocument, Rng, SessionEvent, SheetData, TileCoord, Token, TokenId};
use isometry_net::{
    apply_game, ActionIntent, ActionResolved, GameEvent, GameSnapshot, HostSession,
};
use isometry_system::{
    monster_sheet, sheet_with_conditions, srd_5e, srd_bestiary, srd_items, srd_spells, ActionError,
    GeneratorCatalog, GeneratorLimits, System,
};
use isometry_views::{
    board_css, board_root, demo_map, synth_map, ActionRow, EditMode, GenerationRequest,
    InventoryRequest, ItemRow, MonsterRow, NetMode, SheetSchema, SpellRow, StoryletRow, UiChild,
    UiState, PANEL_W,
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
use cambium::{GenetAppRunner, PointerClick, Propagation};

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
    /// Origin of the CSS animation clock. `tick_animations` takes seconds
    /// since an arbitrary but monotonic zero; the process start is that zero.
    clock: Instant,
    /// Host entropy for adjudication. Every die an action rolls comes from here,
    /// so a fixed seed replays a session's combat exactly; peers never roll.
    action_rng: Rng,
    /// True while a beat is on screen, so the beats can be cleared the moment
    /// the engine's clock reports the last one finished. Without this the class
    /// would still be set when the next strike lands, and an unchanged class
    /// restyles nothing, so the second swing would never animate.
    beats_playing: bool,
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
    /// True when this process is the authority. Only the host adjudicates, so a
    /// client must *ask* rather than resolve: otherwise two machines would each
    /// roll their own dice for the same swing.
    net_is_host: bool,
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
    /// Emotes the loaded packs offer, handed to the view at boot. The app owns
    /// no beat vocabulary of its own.
    pack_emotes: Vec<(String, String)>,
    /// Tokens whose standing-on-a-door state has already produced a travel
    /// event, so the sweep emits once per crossing rather than once per poll.
    travel_emitted: Vec<TokenId>,
    /// `ISOMETRY_TRAVEL_SELFTEST`: register two campaign maps joined by a door
    /// and walk the knight through it. Focus-free, like the others.
    travel_selftest: bool,
    travel_fired: bool,
    /// `ISOMETRY_CMD_SELFTEST`: drive the `>` command line: spawn, find, and a
    /// full `>gen npc` generate/commit into a statted NPC.
    cmd_selftest: bool,
    cmd_fired: bool,
    /// `ISOMETRY_CONVINCE_SELFTEST`: a bard wins a goblin over, then hits the
    /// party cap on the next one. Proves allegiance + the cap + fog.
    convince_selftest: bool,
    convince_fired: bool,
    /// `ISOMETRY_STORYLET_SELFTEST`: seed two storylets (one ready, one locked),
    /// open the surface, play the ready one, and confirm its fact commits.
    storylet_selftest: bool,
    storylet_fired: bool,
    /// `ISOMETRY_COMBAT_SELFTEST`: drive a short adjudicated exchange on boot.
    combat_selftest: bool,
    /// Swings left to throw, when the last one landed, and whether the winner
    /// has taken its bow.
    combat_swings: u8,
    last_swing: Option<Instant>,
    combat_emoted: bool,
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
/// How many tokens a player owns across the *whole* campaign: the active board
/// plus every stored map. The party cap is a limit on a person's followers, and
/// a split party (C3) has them spread over several maps, so counting only the
/// active board would let the cap be dodged by recruiting on each map in turn.
fn owner_token_count(ui: &UiState, owner: &str) -> u32 {
    let on = |m: &isometry_core::MapDocument| {
        m.tokens.iter().filter(|t| t.owner.as_deref() == Some(owner)).count()
    };
    let active = on(&ui.map);
    let stored: usize = ui
        .campaign_maps
        .iter()
        // The active map is also mirrored into the stored registry, so skip it
        // there to avoid double-counting its tokens.
        .filter(|(id, _)| Some(id.as_str()) != ui.active_map.as_deref())
        .map(|(_, m)| on(&m.document))
        .sum();
    (active + stored) as u32
}

/// A free tile to place a generated token on, scanning outward from (2, 2) past
/// anything occupied. The host commit path holds only a snapshot, so this is the
/// snapshot twin of the view's `free_spawn_tile`.
fn free_snapshot_tile(map: &MapDocument) -> TileCoord {
    let occupied: std::collections::HashSet<TileCoord> =
        map.tokens.iter().map(|t| t.at).collect();
    let free = |at: TileCoord| map.ground.in_bounds(at.0, at.1) && !occupied.contains(&at);
    // Prefer a free interior tile scanning outward from (2, 2), but never leave
    // the board: a narrow or short map has no col/row 2..17, and placing a token
    // off-map fails the whole commit (TokenPlaced rejects out-of-bounds).
    for d in 0..256 {
        let at = (2 + (d % 16), 2 + (d / 16));
        if free(at) {
            return at;
        }
    }
    // The window missed (a small or packed map): take any free in-bounds tile.
    let (w, h) = (map.ground.width() as i32, map.ground.height() as i32);
    for row in 0..h {
        for col in 0..w {
            if free((col, row)) {
                return (col, row);
            }
        }
    }
    (0, 0) // board is full or empty; (0,0) is in-bounds for any non-empty map
}

/// The next free token id across the whole campaign, so a generated NPC never
/// collides with a resident of another stored map (inventories key on `TokenId`
/// globally). Same discipline as travel's id minting.
fn next_snapshot_id(snapshot: &GameSnapshot) -> TokenId {
    let max = snapshot
        .maps
        .values()
        .flat_map(|m| m.document.tokens.iter())
        .chain(snapshot.map.tokens.iter())
        .map(|t| t.id.0)
        .chain(snapshot.inventories.keys().map(|id| id.0))
        .max()
        .unwrap_or(0);
    TokenId(max + 1)
}

/// A DM-facing reason a storylet is not yet playable.
fn describe_storylet_error(error: &isometry_campaign::StoryletError) -> String {
    use isometry_campaign::StoryletError::*;
    match error {
        MissingFactionTag(tag) => format!("needs a faction tagged '{tag}'"),
        MissingHiddenFact(id) => format!("needs the secret '{id}' to be true"),
        MissingWorldLaw(id) => format!("needs the law '{id}'"),
        UncastRole(role) => format!("no character fits the role '{role}'"),
    }
}

fn generator_pack_roots() -> Vec<std::path::PathBuf> {
    // The `core` pack ships the default beat vocabulary (strike, recoil, fall,
    // cheer...). It is a pack like any other, so a campaign overrides a beat
    // simply by declaring the same name: the app owns no choreography.
    let mut roots = vec![
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../isometry-system/examples/packs/core"),
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../isometry-system/examples/packs/demo"),
    ];
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
                    // A fresh session cascades with its animation clock still at
                    // zero, so any @keyframes it starts is stamped `start_time =
                    // 0`. Our clock must share that origin, or the very next tick
                    // hands the engine a `now` seconds past the animation's end
                    // and a 420ms beat expires before its first frame. Rebasing
                    // here keeps the two in one timebase.
                    self.clock = Instant::now();
                }
            }
            // Advance the CSS animation clock. A transition or @keyframes run
            // *starts* on the restyle that sets its class (the `apply` above);
            // this re-interpolates it at the current time. On a still board the
            // animation set is empty and this returns `Applied::Unchanged`, so
            // an idle surface pays nothing for the clock existing.
            if let Some(layout) = self.layout.as_mut() {
                let now_s = self.clock.elapsed().as_secs_f64();
                let _ = layout.tick_animations(&*dom_ref, now_s);
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
                                    clocks: ui.clocks.clone(),

                                    party_cap: ui.party_cap,
                                    last_beats: Vec::new(),
                                    beat_seq: 0,
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
        self.pump_storylets();
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
                        // Campaign drafts and storylets share this one-shot
                        // outcome channel, so the text stays neutral to fit both.
                        ui.status = match &outcome.value {
                            Ok(()) => format!("committed (request {})", outcome.request),
                            Err(error) => {
                                format!("commit failed (request {}): {error}", outcome.request)
                            }
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
                // Pull the authoritative host-private campaign too, or storylet
                // availability (which reads secret_ids) resolves against a stale
                // copy: a mid-session reveal would leave a secret-gated storylet
                // wrongly locked, or a removed secret wrongly playable.
                if let Some(campaign) = self.net.as_ref().and_then(NetBridge::campaign) {
                    self.campaign = campaign;
                }
                runner.update(|ui| ui.apply_snapshot(snap));
                // The host's door sweep: any token now standing on a transition
                // point of the active map walks through it. Clients never ask in
                // words; they walk, the move replicates, and this notices. The
                // emitted list keeps one crossing from being ruled twice while
                // its echo is still in flight.
                if self.net_is_host {
                    let on_doors: Vec<TokenId> = {
                        let ui = runner.state();
                        ui.map
                            .tokens
                            .iter()
                            .filter(|t| ui.transition_at(t.at))
                            .map(|t| t.id)
                            .collect()
                    };
                    for token in &on_doors {
                        if !self.travel_emitted.contains(token) {
                            runner.update(|ui| {
                                ui.net_outbox.push(GameEvent::Traveled { token: *token })
                            });
                        }
                    }
                    self.travel_emitted = on_doors;
                }
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
        let (bind, edit, action, inventory_request, open, intent, spawn_sheet, clear_condition) =
            match self.runner.as_ref() {
                Some(r) => {
                    let s = r.state();
                    (
                        s.bind_sheet_request,
                        s.sheet_edit.clone(),
                        s.sheet_action.clone(),
                        s.inventory_request.clone(),
                        s.open_sheet,
                        s.action_intent.clone(),
                        s.spawn_sheet_request.clone(),
                        s.clear_condition_request.clone(),
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
            && intent.is_none()
            && spawn_sheet.is_none()
            && clear_condition.is_none()
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

        // A spawned monster becomes statted: the compendium stat block reaches a
        // real sheet, which is what makes it a thing that can be fought rather
        // than a sprite standing on a diamond.
        if let Some((token, key)) = spawn_sheet {
            let sheet = srd_bestiary()
                .iter()
                .find(|m| m.key == key)
                .map(monster_sheet);
            runner.update(|ui| {
                ui.spawn_sheet_request = None;
                let Some(sheet) = sheet else {
                    ui.status = format!("no stat block for {key}");
                    return;
                };
                ui.map.set_sheet(token, sheet.clone());
                if ui.net_mode == NetMode::Remote {
                    ui.net_outbox.push(GameEvent::SheetSet { token, sheet });
                }
            });
        }

        // Clear one condition: the ask half of standing up. The rules recompute
        // what the token can do without it; if no condition remains, the
        // mobility override clears entirely and the sheet's base numbers stand.
        if let Some((token, name)) = clear_condition {
            let (sheet, remaining) = {
                let s = runner.state();
                let mut set = s.map.conditions.get(&token).cloned().unwrap_or_default();
                set.remove(&name);
                (s.map.sheet(token).cloned(), set)
            };
            let mobility = match (&sheet, remaining.is_empty()) {
                (_, true) => None,
                (Some(sheet), false) => {
                    let conditioned = sheet_with_conditions(sheet, remaining.iter());
                    system.mobility_for(&conditioned, true)
                }
                (None, false) => None,
            };
            runner.update(|ui| {
                ui.clear_condition_request = None;
                let event = GameEvent::ConditionSet {
                    token,
                    condition: name.clone(),
                    on: false,
                    mobility,
                };
                if ui.net_mode == NetMode::Remote {
                    ui.net_outbox.push(event);
                } else {
                    ui.map.set_condition(token, &name, false);
                    ui.map.set_mobility(token, mobility);
                    ui.recompute_fog();
                    ui.recompute_reach();
                    ui.status = format!("cleared {name}");
                }
            });
        }

        // Where does an action get adjudicated? On the authority, always.
        //
        // A joined player *asks*: the intent goes over the wire as an
        // `ActionIntent` carrying no roll and no verdict, and the host's rules
        // system answers. If a client resolved its own swing it would be rolling
        // its own dice and choosing its own damage, which is precisely what the
        // host refuses elsewhere.
        //
        // Everyone else (the DM, and solo play) *is* the authority, so they fall
        // through to the resolver below.
        let mut intent = intent;
        if let Some((actor, target, key)) = intent.clone() {
            if self.net.is_some() && !self.net_is_host {
                if let Some(net) = self.net.as_ref() {
                    net.submit_action(ActionIntent {
                        actor,
                        target,
                        action_key: key,
                    });
                }
                runner.update(|ui| {
                    ui.action_intent = None;
                    ui.status = "asking the host...".to_owned();
                });
                intent = None;
            }
        }

        // The host also adjudicates its players' requests, through this same
        // path: one resolver, one entropy source, one set of rules, whoever asked.
        let mut queued: Vec<(TokenId, TokenId, String)> = Vec::new();
        if self.net_is_host {
            if let Some(net) = self.net.as_mut() {
                queued = net
                    .take_action_intents()
                    .into_iter()
                    .map(|i| (i.actor, i.target, i.action_key))
                    .collect();
            }
        }
        for pending in intent.into_iter().chain(queued) {
            self.adjudicate(pending);
        }

        // Recompute derived stats for the open sheet.
        let Some(system) = self.system.as_mut() else {
            return;
        };
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
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

    /// Resolve one action request and commit its outcome. The only path from
    /// "I swing at that goblin" to the goblin being hurt, taken by the DM's own
    /// swings and by a player's request alike.
    fn adjudicate(&mut self, (actor, target, key): (TokenId, TokenId, String)) {
        let Some(system) = self.system.as_mut() else {
            return;
        };
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        // The host validates what the substrate can see (both tokens exist, it
        // is the actor's turn, the victim is in reach), and the system decides
        // everything else. Only the resolved outcome is replicated, so peers
        // apply it without rerunning a line of Lua.
        {
            let (actor_sheet, actor_inv, target_sheet, target_inv, tiles, turn_ok) = {
                let s = runner.state();
                let tiles = match (s.map.token(actor), s.map.token(target)) {
                    (Some(a), Some(t)) => Some((a.at, t.at)),
                    _ => None,
                };
                // An empty turn order means free play (the editor, a hot-seat
                // skirmish before initiative); once initiative exists, only the
                // active token may act.
                let turn_ok = s.turns.active().map_or(true, |active| active == actor);
                (
                    s.map.sheet(actor).cloned(),
                    s.inventories.get(&actor).cloned(),
                    s.map.sheet(target).cloned(),
                    s.inventories.get(&target).cloned(),
                    tiles,
                    turn_ok,
                )
            };

            let outcome: Result<_, String> = (|| {
                let (actor_at, target_at) = tiles.ok_or_else(|| "no such target".to_owned())?;
                if !turn_ok {
                    return Err("not your turn".to_owned());
                }
                let actor_sheet = actor_sheet.ok_or_else(|| "attacker has no sheet".to_owned())?;
                let target_sheet =
                    target_sheet.ok_or_else(|| "target has no sheet".to_owned())?;
                // Equipment counts: resolve against the effective sheets, so a
                // magic sword's bonus lands and armour raises the AC it is
                // compared against. Conditions ride along as boolean fields, so
                // the rules can read `t.prone` and the resolver can tell "apply
                // prone" from "already prone".
                let (actor_conds, target_conds) = {
                    let s = runner.state();
                    (
                        s.map.conditions.get(&actor).cloned().unwrap_or_default(),
                        s.map.conditions.get(&target).cloned().unwrap_or_default(),
                    )
                };
                let actor_eff = sheet_with_conditions(
                    &system.effective_sheet(&actor_sheet, actor_inv.as_ref()),
                    actor_conds.iter(),
                );
                let target_eff = sheet_with_conditions(
                    &system.effective_sheet(&target_sheet, target_inv.as_ref()),
                    target_conds.iter(),
                );
                system
                    .resolve_action(
                        &key,
                        actor,
                        &actor_eff,
                        actor_at,
                        target,
                        &target_eff,
                        target_at,
                        &mut self.action_rng,
                    )
                    .map_err(|e| match e {
                        ActionError::OutOfRange { range, distance } => {
                            format!("out of reach ({distance} tiles, reach {range})")
                        }
                        ActionError::SelfTarget => "cannot target yourself".to_owned(),
                        ActionError::AlreadyDefeated => "that one is already down".to_owned(),
                        ActionError::NotTargeted(key) => format!("{key} needs no target"),
                        ActionError::UnknownAction(key) => format!("no such action: {key}"),
                        // A script or dice-expression fault is the system's bug,
                        // not the player's; name it rather than hiding it.
                        ActionError::ScriptFailed(f) => format!("system script failed: {f}"),
                        ActionError::BadDice(expr) => format!("system rolled bad dice: {expr}"),
                    })
            })();

            let label = system
                .actions
                .iter()
                .find(|a| a.key == key)
                .map(|a| a.label.clone())
                .unwrap_or_else(|| key.clone());

            runner.update(|ui| {
                ui.action_intent = None;
                let resolution = match outcome {
                    Ok(r) => r,
                    Err(reason) => {
                        // A refused intent changes nothing at all: no dice, no
                        // deltas, no turn spent.
                        ui.status = reason;
                        return;
                    }
                };

                // Where does a shove actually land? The rules said how hard and
                // which way; the *board* rules on the rest, because a wall, a map
                // edge, or another body stops a push short and the system does
                // not know the map. This is truth, so it is decided once and
                // replicated, unlike the stagger beat riding alongside it.
                let mut displaced = Vec::new();
                if let Some((step, tiles)) = resolution.push {
                    if let Some(from) = ui.map.token(target).map(|t| t.at) {
                        let occupied: Vec<TileCoord> =
                            ui.map.tokens.iter().map(|t| t.at).collect();
                        let (w, h) = (ui.map.ground.width(), ui.map.ground.height());
                        let landing = isometry_core::push_path(from, step, tiles, |at| {
                            at.0 >= 0
                                && at.1 >= 0
                                && (at.0 as u32) < w
                                && (at.1 as u32) < h
                                && !occupied.contains(&at)
                        });
                        if let Some(to) = landing {
                            displaced.push((target, to));
                        }
                    }
                }
                // A landed recruit becomes an owner change, ruled here because
                // owners and the party cap are the map's, not the rules'. The
                // winner's side is the actor's owner; a player's party has a cap
                // (the DM, owner None, is uncapped); a creature already on that
                // side needs nothing.
                let mut owner_changes = Vec::new();
                let mut recruit_note = "";
                if let Some(recruited) = resolution.recruited {
                    let new_owner = ui.map.token(actor).and_then(|t| t.owner.clone());
                    let current = ui.map.token(recruited).and_then(|t| t.owner.clone());
                    if current == new_owner {
                        recruit_note = " (already at your side)";
                    } else if let Some(owner) = new_owner.clone() {
                        // Count the owner's *whole* party, not just the active
                        // map: a split party (C3) keeps tokens on stored maps,
                        // and the cap is a limit on people, not on this board.
                        let owned = owner_token_count(ui, &owner);
                        if owned >= ui.party_cap {
                            recruit_note = " but your party is full";
                        } else {
                            owner_changes.push((recruited, Some(owner)));
                            recruit_note = " and it joins you";
                        }
                    } else {
                        owner_changes.push((recruited, None));
                        recruit_note = " and it joins you";
                    }
                }

                let victim = ui
                    .map
                    .token(target)
                    .and_then(|_| ui.map.sheet(target))
                    .and_then(|s| s.text("name").map(str::to_owned))
                    .unwrap_or_else(|| "target".to_owned());
                let down = !resolution.defeated.is_empty();
                ui.status = if resolution.recruited.is_some() {
                    // A social action; "hits for 0 damage" would read wrong.
                    let verb = if owner_changes.is_empty() { "sways" } else { "wins over" };
                    format!(
                        "{} {verb} {victim} ({}){recruit_note}",
                        resolution.attack.by, resolution.attack.total
                    )
                } else if resolution.hit {
                    let dmg = resolution.damage.as_ref().map_or(0, |d| d.total);
                    let felled = if down { " and drops it" } else { "" };
                    format!(
                        "{} hits {victim} ({}) for {dmg}{felled}",
                        resolution.attack.by, resolution.attack.total
                    )
                } else {
                    format!(
                        "{} misses {victim} ({})",
                        resolution.attack.by, resolution.attack.total
                    )
                };

                let event = GameEvent::ActionResolved(ActionResolved {
                    actor,
                    target,
                    action_key: key.clone(),
                    label,
                    attack: resolution.attack,
                    hit: resolution.hit,
                    damage: resolution.damage,
                    deltas: resolution.deltas,
                    beats: resolution.beats,
                    defeated: resolution.defeated,
                    displaced,
                    conditions: resolution.conditions,
                    mobility: resolution.mobility,
                    owner_changes,
                });
                if ui.net_mode == NetMode::Remote {
                    // In session the authority applies it and mirrors the
                    // snapshot back, which is where the beats get staged (so a
                    // joined player sees the exchange too, not just the DM).
                    //
                    // But fold this recruit's owner change into the local map
                    // *now*, before the mirror returns: pump_sheets adjudicates a
                    // whole batch of queued intents in one pass, and the cap
                    // above counts `ui.map`. Without this, two same-owner
                    // recruits in one batch would both see the pre-recruit count
                    // and both pass, blowing past the cap. The authority applies
                    // the identical change on mirror-back, so this is idempotent.
                    if let GameEvent::ActionResolved(res) = &event {
                        for (token, owner) in &res.owner_changes {
                            if let Some(t) = ui.map.tokens.iter_mut().find(|t| t.id == *token) {
                                t.owner = owner.clone();
                            }
                        }
                        if !res.owner_changes.is_empty() {
                            ui.recompute_fog();
                            ui.recompute_reach();
                        }
                    }
                    ui.net_outbox.push(event);
                } else if let GameEvent::ActionResolved(res) = &event {
                    // Solo: no authority to route through, so apply it here.
                    for delta in &res.deltas {
                        ui.map.apply_delta(delta);
                    }
                    for (token, to) in &res.displaced {
                        if let Some(t) = ui.map.tokens.iter_mut().find(|t| t.id == *token) {
                            t.at = *to;
                        }
                    }
                    for token in &res.defeated {
                        ui.map.set_defeated(*token, true);
                    }
                    for (token, name, on) in &res.conditions {
                        ui.map.set_condition(*token, name, *on);
                    }
                    for (token, mobility) in &res.mobility {
                        ui.map.set_mobility(*token, *mobility);
                    }
                    for (token, owner) in &res.owner_changes {
                        if let Some(t) = ui.map.tokens.iter_mut().find(|t| t.id == *token) {
                            t.owner = owner.clone();
                        }
                    }
                    if !res.conditions.is_empty()
                        || !res.displaced.is_empty()
                        || !res.owner_changes.is_empty()
                    {
                        // A condition, a shove, or a change of allegiance all
                        // change what can be seen and reached; recompute so the
                        // board tells the truth now, not on the next click.
                        ui.recompute_fog();
                        ui.recompute_reach();
                    }
                    ui.push_roll(res.attack.clone());
                    if let Some(damage) = &res.damage {
                        ui.push_roll(damage.clone());
                    }
                    let seq = ui.beat_seq.wrapping_add(1);
                    ui.stage_beats(seq, &res.beats);
                }
                ui.sheet_effective = None;
            });
        }

    }

    /// Evaluate or commit a host-owned generator preview. The view asks for a
    /// one-shot action; this desktop layer loads the declared pack and owns the
    /// entropy tape, while `isometry-net` remains scripting-agnostic.
    /// Build a replicated snapshot from the view's current state, for a host
    /// operation that needs to prevalidate against a clone (storylet commit).
    fn snapshot_of(&self, ui: &UiState) -> GameSnapshot {
        GameSnapshot {
            map: ui.map.clone(),
            turns: ui.turns.clone(),
            roll_log: ui.roll_log.clone(),
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
        }
    }

    /// The storylet surface (C6). While the DM has it open, resolve each
    /// campaign storylet against the current world and the host-private secrets
    /// and hand the view rows; when the DM plays one, commit its effects.
    ///
    /// Host-only: matching reads secret facts, and committing is authoring. A
    /// client's `open_storylets`/`play_storylet` are gated on `can_edit_inventory`
    /// upstream, so this never runs for a joined player.
    fn pump_storylets(&mut self) {
        let (open, request, world, can_edit) = match self.runner.as_ref() {
            Some(r) => {
                let s = r.state();
                (s.storylet_open, s.storylet_request.clone(), s.world.clone(), s.can_edit_inventory)
            }
            None => return,
        };
        if !can_edit || (!open && request.is_none()) {
            return;
        }

        // Refresh the rows while the surface is open: which storylets are
        // playable now (requirements met, roles cast) and which are still locked.
        if open {
            let secret_ids: Vec<String> = self.campaign.secret_ids().map(str::to_owned).collect();
            let rows: Vec<StoryletRow> = world
                .storylets
                .iter()
                .map(|(key, storylet)| {
                    match world.resolve_storylet(storylet, secret_ids.iter().map(String::as_str)) {
                        Ok(resolved) => StoryletRow {
                            key: key.clone(),
                            entry: storylet.entry.clone(),
                            available: true,
                            status: "ready".to_owned(),
                            cast: resolved
                                .cast
                                .into_iter()
                                .map(|(role, character_id)| {
                                    let name = world
                                        .characters
                                        .get(&character_id)
                                        .map(|c| c.name.clone())
                                        .unwrap_or(character_id);
                                    (role, name)
                                })
                                .collect(),
                        },
                        Err(error) => StoryletRow {
                            key: key.clone(),
                            entry: storylet.entry.clone(),
                            available: false,
                            status: describe_storylet_error(&error),
                            cast: Vec::new(),
                        },
                    }
                })
                .collect();
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| {
                    // Only replace on change, to keep the DM's selection stable.
                    if ui.storylets != rows {
                        ui.storylets = rows;
                        if ui.storylet_selected >= ui.storylets.len() {
                            ui.storylet_selected = 0;
                        }
                    }
                });
            }
        }

        // Play a storylet: commit its effects. A storylet Item effect wants a
        // recipient, so pass the active token (else the first), like campaign.
        let Some(key) = request else {
            return;
        };
        let (item_owner, snapshot) = match self.runner.as_ref() {
            Some(r) => {
                let s = r.state();
                let owner = s.turns.active().or_else(|| s.map.tokens.first().map(|t| t.id));
                (owner, self.snapshot_of(s))
            }
            None => return,
        };
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.storylet_request = None);
        }
        if world.storylets.is_empty() {
            return;
        }
        let remote = matches!(self.runner.as_ref().map(|r| r.state().net_mode), Some(NetMode::Remote));
        if remote {
            if let Some(net) = self.net.as_mut() {
                let request = net.commit_storylet(key.clone(), item_owner);
                if let Some(runner) = self.runner.as_mut() {
                    runner.update(|ui| {
                        ui.status = match request {
                            Some(request) => format!("playing storylet (request {request})"),
                            None => "storylet authority actor stopped".to_owned(),
                        };
                    });
                }
            }
        } else {
            let mut host = HostSession::with_history(
                snapshot,
                self.campaign.clone(),
                self.history.clone(),
            );
            match host.commit_storylet(&key, item_owner) {
                Ok(_) => {
                    self.campaign = host.campaign().clone();
                    self.history = host.history().clone();
                    self.journal = host.state().journal.clone();
                    let snapshot = host.state().clone();
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| {
                            ui.apply_snapshot(snapshot);
                            ui.status = format!("played storylet: {key}");
                        });
                    }
                }
                Err(error) => {
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| ui.status = format!("storylet failed: {error}"));
                    }
                }
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

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
                        clocks: ui.clocks.clone(),

                        party_cap: ui.party_cap,
                        last_beats: Vec::new(),
                        beat_seq: 0,
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
                    GenValue::Npc { npc } => {
                        // Lower a generated NPC into a *statted* creature. The
                        // proposal is thin (key, name, tags); its key doubles as
                        // a bestiary slug, so a generated "Skreek" is a goblin's
                        // stat block under a generated name. That reuse is what
                        // makes `>gen npc` end in something fightable rather than
                        // a nameless sprite. A key with no bestiary match falls
                        // back to a plain default sheet.
                        let snapshot = local_snapshot.as_ref();
                        let (at, id) = match snapshot {
                            Some(s) => (free_snapshot_tile(&s.map), next_snapshot_id(s)),
                            None => ((2, 2), TokenId(1)),
                        };
                        let monster = srd_bestiary().into_iter().find(|m| m.key == npc.key);
                        let (sprite, mut sheet) = match monster {
                            Some(m) => (m.sprite.clone(), monster_sheet(&m)),
                            None => {
                                let sheet = self
                                    .system
                                    .as_ref()
                                    .map(System::default_sheet)
                                    .unwrap_or_else(|| SheetData::new("5e-srd"));
                                ("knight".to_owned(), sheet)
                            }
                        };
                        // The generated name over the base creature's.
                        sheet.set_text("name", npc.name.clone());
                        events.push(GameEvent::Map(SessionEvent::TokenPlaced(Token {
                            id,
                            at,
                            facing: Facing::South,
                            sprite,
                            owner: None,
                        })));
                        events.push(GameEvent::SheetSet { token: id, sheet });
                        // A fightable NPC joins initiative.
                        events.push(GameEvent::TurnAdd(id));
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

    /// `ISOMETRY_COMBAT_SELFTEST=1`: a focus-free proof of the adjudication
    /// loop. It stats both duelists, stands the goblin in reach, and swings.
    ///
    /// The app drives itself rather than being driven by synthetic clicks,
    /// because SendKeys loses the foreground race on a machine someone is
    /// actually using and silently types into their editor. Same rationale as
    /// `ISOMETRY_NET_SELFTEST`.
    /// `ISOMETRY_TRAVEL_SELFTEST=1`: prove C2 end to end in the app. The demo
    /// board becomes the stored map `field` with a door; a `hut` map waits on
    /// the other side; the knight (the whole party: everyone else is demoted to
    /// DM furniture) walks onto the door through the normal Play-mode click
    /// path, and the board follows it through.
    fn maybe_travel_selftest(&mut self) {
        if !self.travel_selftest || self.travel_fired {
            return;
        }
        let ready = self
            .started
            .is_some_and(|t| t.elapsed() > Duration::from_secs(2));
        if !ready {
            return;
        }
        self.travel_fired = true;
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        runner.update(|ui| {
            // The party is the knight alone; the rest is the DM's furniture.
            for t in ui.map.tokens.iter_mut() {
                if t.id != TokenId(1) {
                    t.owner = None;
                }
            }
            // The field: the live board, stored, with a door beside the knight.
            let field = isometry_campaign::CampaignMap {
                id: "field".to_owned(),
                scale: isometry_campaign::MapScale::Local,
                document: ui.map.clone(),
                spawn_zones: Vec::new(),
                transitions: vec![isometry_campaign::MapTransition {
                    id: "field-gate".to_owned(),
                    at: isometry_campaign::MapPoint { col: 12, row: 14 },
                    target_map: "hut".to_owned(),
                    target_entry: Some("hut-door".to_owned()),
                }],
                encounter_anchors: Vec::new(),
            };
            // The hut: a small stone room with one resident.
            let mut hut_doc = isometry_core::MapDocument::new("hut", 10, 10);
            let floor = hut_doc.intern_tile_kind("stone");
            for r in 0..10 {
                for c in 0..10 {
                    hut_doc.ground.set(c, r, floor);
                }
            }
            hut_doc.tokens.push(isometry_core::Token {
                id: TokenId(7),
                at: (6, 6),
                facing: isometry_core::Facing::South,
                sprite: "goblin".to_owned(),
                owner: None,
            });
            let hut = isometry_campaign::CampaignMap {
                id: "hut".to_owned(),
                scale: isometry_campaign::MapScale::Local,
                document: hut_doc,
                spawn_zones: Vec::new(),
                transitions: vec![isometry_campaign::MapTransition {
                    id: "hut-door".to_owned(),
                    at: isometry_campaign::MapPoint { col: 2, row: 2 },
                    target_map: "field".to_owned(),
                    target_entry: Some("field-gate".to_owned()),
                }],
                encounter_anchors: Vec::new(),
            };
            ui.campaign_maps.insert("field".to_owned(), field);
            ui.campaign_maps.insert("hut".to_owned(), hut);
            ui.active_map = Some("field".to_owned());
            // Time passes in the field before anyone crosses: the DM declares
            // a rest, so the two locations' clocks drift apart.
            ui.pass_time(4);
            eprintln!(
                "[isometry] travel selftest: on {:?}, knight@{:?}, door at (12, 14) | clocks {:?}",
                ui.active_map,
                ui.map.token(TokenId(1)).map(|t| t.at),
                ui.clocks,
            );
            // Walk through the door via the normal Play-mode click path.
            ui.mode = EditMode::Play;
            ui.select_token(TokenId(1));
            ui.click_tile((12, 14));
            eprintln!(
                "[isometry] travel selftest: {} | active {:?} board '{}' | knight here: {:?} | field still holds knight: {:?} | clocks {:?}",
                ui.status,
                ui.active_map,
                ui.map.name,
                ui.map.tokens.iter().find(|t| t.sprite == "knight").map(|t| t.at),
                ui.campaign_maps["field"].document.token(TokenId(1)).is_some(),
                ui.clocks,
            );
        });
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// `ISOMETRY_CMD_SELFTEST=1` (pair with `ISOMETRY_GEN_SEED` for a fixed
    /// NPC): drive the whole `>` command surface once, focus-free.
    fn maybe_cmd_selftest(&mut self) {
        if !self.cmd_selftest || self.cmd_fired {
            return;
        }
        if !self.started.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            return;
        }
        self.cmd_fired = true;
        let before = self
            .runner
            .as_ref()
            .map(|r| r.state().map.tokens.len())
            .unwrap_or(0);

        // >spawn: a statted goblin, resolved from a free-text query.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.spawn_query("gobl"));
        }
        self.pump_sheets(); // binds the stat block

        // >find: a unified compendium search.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.find_query("sword"));
            eprintln!(
                "[isometry] cmd selftest: find 'sword' -> {} results, first: {:?}",
                runner.state().command_results.len(),
                runner.state().command_results.first(),
            );
        }

        // >gen npc: open the generator, generate a preview, commit it.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.start_generator("npc"));
        }
        self.pump_generators(); // Generate -> preview
        let previewed = self
            .runner
            .as_ref()
            .and_then(|r| r.state().generator_preview.clone());
        eprintln!("[isometry] cmd selftest: gen npc preview = {previewed:?}");
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.commit_generation_preview());
        }
        self.pump_generators(); // Commit -> lower into a statted token
        self.pump_sheets();

        if let Some(runner) = self.runner.as_ref() {
            let ui = runner.state();
            let newest = ui.map.tokens.last();
            eprintln!(
                "[isometry] cmd selftest: tokens {} -> {} | newest {:?} sheet name {:?} hp {:?} | status: {}",
                before,
                ui.map.tokens.len(),
                newest.map(|t| (t.id.0, t.sprite.clone(), t.at)),
                newest.and_then(|t| ui.map.sheet(t.id)).and_then(|s| s.text("name").map(str::to_owned)),
                newest.and_then(|t| ui.map.sheet(t.id)).and_then(|s| s.int("hp_current")),
                ui.status,
            );
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// `ISOMETRY_CONVINCE_SELFTEST=1`: a bard recruits a goblin, then meets the
    /// party cap on the next. Focus-free.
    /// `ISOMETRY_STORYLET_SELFTEST=1`: seed a ready storylet and a locked one,
    /// open the surface, play the ready one, and confirm its fact committed.
    fn maybe_storylet_selftest(&mut self) {
        if !self.storylet_selftest || self.storylet_fired {
            return;
        }
        if !self.started.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            return;
        }
        self.storylet_fired = true;

        use isometry_campaign::{StoryletEffect, StoryletProposal, StoryletRequirements, WorldFact};
        // A ready storylet (no requirements, no roles) and a locked one (needs a
        // faction that does not exist).
        let ready = StoryletProposal {
            key: "gate-greeting".to_owned(),
            entry: "A stranger greets you at the gate.".to_owned(),
            tags: Vec::new(),
            requirements: StoryletRequirements::default(),
            roles: Vec::new(),
            effects: vec![StoryletEffect::Fact {
                fact: WorldFact {
                    id: "gate-met".to_owned(),
                    kind: "event".to_owned(),
                    text: "The party met a stranger at the gate.".to_owned(),
                    tags: Vec::new(),
                },
            }],
        };
        let locked = StoryletProposal {
            key: "cult-rises".to_owned(),
            entry: "The eel cult stirs in the deep.".to_owned(),
            tags: Vec::new(),
            requirements: StoryletRequirements {
                faction_tags: vec!["cult".to_owned()],
                ..Default::default()
            },
            roles: Vec::new(),
            effects: Vec::new(),
        };
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                ui.world.storylets.insert("gate-greeting".to_owned(), ready.clone());
                ui.world.storylets.insert("cult-rises".to_owned(), locked.clone());
                ui.open_storylets();
            });
        }
        // Compute the rows.
        self.pump_storylets();
        if let Some(runner) = self.runner.as_ref() {
            for row in &runner.state().storylets {
                eprintln!(
                    "[isometry] storylet selftest: {} available={} status={:?} entry={:?}",
                    row.key, row.available, row.status, row.entry
                );
            }
        }
        // Select the ready one and play it.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                let idx = ui.storylets.iter().position(|r| r.key == "gate-greeting").unwrap_or(0);
                ui.storylet_selected = idx;
                ui.play_storylet();
            });
        }
        self.pump_storylets();
        let committed = self.journal.iter().any(|f| f.id == "gate-met");
        let status = self
            .runner
            .as_ref()
            .map(|r| r.state().status.clone())
            .unwrap_or_default();
        eprintln!(
            "[isometry] storylet selftest: played | journal has 'gate-met': {committed} | status: {status}"
        );
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn maybe_convince_selftest(&mut self) {
        if !self.convince_selftest || self.convince_fired {
            return;
        }
        if !self.started.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            return;
        }
        self.convince_fired = true;

        let Some(system) = self.system.as_mut() else {
            return;
        };
        // A silver-tongued bard: CHA 18 (+4) and proficiency, so the pitch is
        // 1d20+6 against a goblin's low resolve.
        let mut bard = system.default_sheet();
        bard.set_text("name", "Bard");
        bard.set_int("cha", 18);
        bard.set_int("prof", 2);
        let goblin = |will: i64| {
            let mut s = srd_bestiary()
                .iter()
                .find(|m| m.key == "goblin")
                .map(monster_sheet)
                .unwrap_or_else(|| system.default_sheet());
            s.set_int("will", will);
            s
        };
        let (g1, g2) = (goblin(4), goblin(4));

        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                // Knight 1 is player A's; make it the bard. Goblins 2 and 4 are
                // the DM's furniture (owner None) standing in talking range.
                ui.map.set_sheet(TokenId(1), bard.clone());
                ui.map.set_sheet(TokenId(2), g1.clone());
                ui.map.set_sheet(TokenId(4), g2.clone());
                let anchor = ui.map.token(TokenId(1)).map(|t| t.at).unwrap_or((10, 14));
                for (id, dx) in [(TokenId(2), 2), (TokenId(4), 3)] {
                    if let Some(g) = ui.map.tokens.iter_mut().find(|t| t.id == id) {
                        g.at = (anchor.0 + dx, anchor.1);
                        g.owner = None; // DM furniture, up for grabs
                    }
                }
                // A owns knight 1 and knight 3 on this board, plus one companion
                // stashed on a *stored* map (a split party, C3). The cap counts
                // the whole campaign, so that third token matters: with cap 4, A
                // can take exactly one goblin before the party fills.
                let mut away = isometry_core::MapDocument::new("waystation", 6, 6);
                away.tokens.push(isometry_core::Token {
                    id: TokenId(50),
                    at: (2, 2),
                    facing: isometry_core::Facing::South,
                    sprite: "knight".to_owned(),
                    owner: Some("A".to_owned()),
                });
                ui.campaign_maps.insert(
                    "waystation".to_owned(),
                    isometry_campaign::CampaignMap {
                        id: "waystation".to_owned(),
                        scale: isometry_campaign::MapScale::Local,
                        document: away,
                        spawn_zones: Vec::new(),
                        transitions: Vec::new(),
                        encounter_anchors: Vec::new(),
                    },
                );
                ui.party_cap = 4;
                ui.viewer = Some("A".to_owned());
                ui.recompute_fog();
                let a_active = ui.map.tokens.iter().filter(|t| t.owner.as_deref() == Some("A")).count();
                eprintln!(
                    "[isometry] convince selftest: A owns {a_active} here + 1 stored = 3 global, cap {}",
                    ui.party_cap
                );
            });
        }

        // First pitch: goblin 2 joins A (A goes 2 -> 3 tokens, at the cap).
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.action_intent = Some((TokenId(1), TokenId(2), "convince".to_owned())));
        }
        self.pump_sheets();
        // Second pitch: goblin 4 would make 4 > cap 3, so it fails to hold.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.action_intent = Some((TokenId(1), TokenId(4), "convince".to_owned())));
        }
        self.pump_sheets();

        if let Some(runner) = self.runner.as_ref() {
            let ui = runner.state();
            let owner = |id| ui.map.token(id).and_then(|t| t.owner.clone());
            let a_here = ui.map.tokens.iter().filter(|t| t.owner.as_deref() == Some("A")).count();
            let a_global = a_here
                + ui.campaign_maps
                    .values()
                    .flat_map(|m| m.document.tokens.iter())
                    .filter(|t| t.owner.as_deref() == Some("A"))
                    .count();
            eprintln!(
                "[isometry] convince selftest: goblin2 owner {:?} | goblin4 owner {:?} | A owns {a_here} here, {a_global} global (cap 4) | status: {}",
                owner(TokenId(2)),
                owner(TokenId(4)),
                ui.status,
            );
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn maybe_combat_selftest(&mut self) {
        if !self.combat_selftest || (self.combat_swings == 0 && self.combat_emoted) {
            return;
        }
        // Wait 2s for the first swing, then one per second: long enough for a
        // 420ms beat to finish and be cleared, so the *next* swing has to
        // genuinely restart the animation rather than find its class still set.
        let due = match self.last_swing {
            None => self
                .started
                .is_some_and(|t| t.elapsed() > Duration::from_secs(2)),
            Some(last) => last.elapsed() > Duration::from_millis(1000),
        };
        if !due {
            return;
        }
        let first = self.last_swing.is_none();
        self.last_swing = Some(Instant::now());

        // The swings are spent: the winner celebrates. An emote is the same beat
        // primitive, with no resolution behind it and nothing to adjudicate.
        if self.combat_swings == 0 {
            self.combat_emoted = true;
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| ui.emote(TokenId(1), "cheer"));
                eprintln!(
                    "[isometry] combat selftest: emote | beats = {:?}",
                    runner.state().beats
                );
            }
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
            return;
        }
        self.combat_swings -= 1;
        let swings_left = self.combat_swings;

        let Some(system) = self.system.as_mut() else {
            return;
        };
        let mut knight = system.default_sheet();
        knight.set_text("name", "Knight");
        knight.set_int("str", 18); // +4
        knight.set_int("prof", 3); // so the swing is 1d20+7 against AC 15
        let Some(goblin) = srd_bestiary().iter().find(|m| m.key == "goblin").map(monster_sheet)
        else {
            eprintln!("[isometry] combat selftest: no goblin in the bestiary");
            return;
        };
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        runner.update(|ui| {
            if first {
                // Stand the goblin within reach of the knight, and stat them
                // both. After that the board carries its own state: each swing
                // hits whatever hit points the last one left behind.
                if let Some(at) = ui.map.token(TokenId(1)).map(|t| t.at) {
                    if let Some(g) = ui.map.tokens.iter_mut().find(|t| t.id == TokenId(2)) {
                        g.at = (at.0 + 1, at.1);
                    }
                }
                ui.map.set_sheet(TokenId(1), knight.clone());
                ui.map.set_sheet(TokenId(2), goblin.clone());
                ui.open_sheet = Some(TokenId(1));
                ui.recompute_fog();
                eprintln!(
                    "[isometry] combat selftest: knight@{:?} vs goblin@{:?} | goblin hp {:?}, ac {:?} (1d20+7 to hit)",
                    ui.map.token(TokenId(1)).map(|t| t.at),
                    ui.map.token(TokenId(2)).map(|t| t.at),
                    ui.map.sheet(TokenId(2)).and_then(|s| s.int("hp_current")),
                    ui.map.sheet(TokenId(2)).and_then(|s| s.int("ac")),
                );
            }
            // Trip first (a condition: prone halves speed, truth on every
            // peer), then attacks. The prone goblin keeps its tile, unlike the
            // shove run: a condition changes what it can DO, not where it is.
            // The fixed tape rolls 11 then 22: the first swing misses whatever
            // it is, so the trip goes second, where it connects.
            let action = if swings_left == 2 { "trip" } else { "attack" };
            ui.action_intent = Some((TokenId(1), TokenId(2), action.to_owned()));
        });
        self.pump_sheets();
        if let Some(runner) = self.runner.as_ref() {
            let ui = runner.state();
            eprintln!(
                "[isometry] combat selftest: {} | goblin hp {:?} conds {:?} mobility {:?} | beats = {:?}",
                ui.status,
                ui.map.sheet(TokenId(2)).and_then(|s| s.int("hp_current")),
                ui.map.conditions.get(&TokenId(2)),
                ui.map.effective_mobility(TokenId(2), (5, 6)),
                ui.beats,
            );
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn key(&mut self, event: &WinitKeyEvent) {
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
        if (self.travel_selftest && !self.travel_fired)
            || (self.cmd_selftest && !self.cmd_fired)
            || (self.convince_selftest && !self.convince_fired)
            || (self.storylet_selftest && !self.storylet_fired)
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
            .map(|a| (a.key.clone(), a.label.clone(), a.target.is_some()))
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
    // Choreography is pack data: the stylesheet the packs supply is appended to
    // the app's, and the emote menu is built from whichever beats they marked
    // emotable. A table with no packs still plays a correct game; it just plays
    // it without flourishes, which is safe precisely because no rule may read a
    // beat.
    let (pack_beats, beat_diagnostics) = generator_catalog.choreography();
    for diagnostic in &beat_diagnostics {
        eprintln!("[isometry] choreography: {diagnostic}");
    }
    let mut sheet = board_css();
    for beat in &pack_beats {
        sheet.push('\n');
        sheet.push_str(&beat.css);
    }
    let pack_emotes: Vec<(String, String)> = pack_beats
        .iter()
        .filter_map(|b| b.emote.clone().map(|label| (b.name.clone(), label)))
        .collect();
    let mut app = App {
        window: None,
        host: None,
        runner: None,
        campaign: CampaignStore::new(),
        journal: Vec::new(),
        history: Codicil::new(),
        layout: None,
        layout_size: (0.0, 0.0),
        clock: Instant::now(),
        // A fixed seed keeps a solo session reproducible and makes the headed
        // verification deterministic. A real table seeds this per session.
        action_rng: Rng::new(0x15D_0BE),
        beats_playing: false,
        sheet,
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
        net_is_host: false,
        viewer_arg: parse_viewer(),
        campaign_arg: parse_campaign(),
        net: None,
        last_net_version: 0,
        net_selftest: std::env::var_os("ISOMETRY_NET_SELFTEST").is_some(),
        travel_selftest: std::env::var_os("ISOMETRY_TRAVEL_SELFTEST").is_some(),
        travel_fired: false,
        cmd_selftest: std::env::var_os("ISOMETRY_CMD_SELFTEST").is_some(),
        cmd_fired: false,
        convince_selftest: std::env::var_os("ISOMETRY_CONVINCE_SELFTEST").is_some(),
        convince_fired: false,
        storylet_selftest: std::env::var_os("ISOMETRY_STORYLET_SELFTEST").is_some(),
        storylet_fired: false,
        combat_selftest: std::env::var_os("ISOMETRY_COMBAT_SELFTEST").is_some(),
        combat_swings: 4,
        last_swing: None,
        combat_emoted: false,
        travel_emitted: Vec::new(),
        started: None,
        selftest_fired: false,
        system: None,
        last_sheet_open: None,
        // `ISOMETRY_GEN_SEED` fixes the generator tape so `>gen` previews and
        // rerolls are reproducible (headed verification, and a table that wants
        // a deterministic session); otherwise the wall clock seeds it as before.
        generation_tape: EntropyTape::from_seed(
            std::env::var("ISOMETRY_GEN_SEED")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or_else(|| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_nanos() as u64)
                        .unwrap_or(1)
                }),
        ),
        generation_ordinal: 0,
        generator_catalog,
        pack_emotes,
    };
    event_loop.run_app(&mut app).expect("run app");
}
