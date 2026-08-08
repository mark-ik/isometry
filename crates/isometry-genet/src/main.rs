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
//! input automation), `ISOMETRY_OVERMAP_SELFTEST=1` (overmap capture), and
//! `ISOMETRY_OVERMAP_SOURCE_TIME_SELFTEST=1` (historical-overmap capture).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use codicil::Codicil;
use isometry_campaign::{
    CampaignStore, CampaignWorld, EntropyTape, FactionMove, GenValue, GeneratorRequest, ItemId,
    ItemInstance, MapScale, WorldEvent, WorldFact,
};
use isometry_core::{
    Facing, FieldValue, MapDocument, Rng, SessionEvent, SheetData, TileCoord, Token, TokenId,
};
use isometry_net::{
    ActionIntent, ActionResolved, GameEvent, GameSnapshot, HostSession, RequestId, apply_game,
};
use isometry_system::{
    ActionError, GeneratorCatalog, GeneratorLimits, System, monster_sheet, sheet_with_conditions,
    sheet_with_turn_counters, srd_5e, srd_bestiary, srd_items, srd_spells,
};
use isometry_views::{
    ActionRow, EditMode, FactionMoveRow, GenerationRequest, InventoryRequest, ItemRow, MonsterRow,
    NetMode, PANEL_W, SheetSchema, SpellRow, StoryletRow, UiChild, UiState, board_css, board_root,
    demo_map, synth_map,
};

mod adjudicate;
mod campaign_store;
mod catalog;
mod dispatch;
mod generators;
mod host;
mod input;
mod net;
mod overmap;
mod render;
mod selection_rows;
mod selftest;
mod sheets;
mod source_time;
mod storylets;

use cambium::{GenetAppRunner, HoverEvent, HoverPhase, PointerClick, Propagation};
use cambium_winit::{ime_event_from_winit, key_event_from_winit, modifiers_from_winit};
use campaign_store::{CampaignCheckpoint, CampaignRepository};
use catalog::{bestiary_of, items_of, schema_of, spells_of};
use genet_layout::{
    Applied, IncrementalLayout, InteractionState, LeafPaintSource, ScrollOffsets, SourceNodeId,
    VisualAffinity, VisualCaret,
};
use genet_scripted_dom::{NodeId, ScriptedDom};
use genet_winit_host::SurfaceHost;
use layout_dom_api::{DomMutation, LayoutDom as _, LayoutDomMut as _, LocalName, Namespace};
use net::{NetBridge, Role};
use netrender::{ColorLoad, ExternalTexturePlacement, NetrenderOptions};
use paint_list_api::{DeviceIntSize, PaintCmd, PaintList as _};
use sprigging::{ColorF, LeafRegistry, RenderedLeaves, Size};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{
    ElementState, KeyEvent as WinitKeyEvent, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey as WinitNamedKey};
use winit::window::{Window, WindowId};

type Runner = GenetAppRunner<UiState, fn(&UiState) -> UiChild, UiChild>;

fn command_field_node(runner: &Runner) -> Option<NodeId> {
    let node = runner.focus()?;
    let dom = runner.dom();
    let dom = dom.borrow();
    if dom.element_name(node)?.local.as_ref() != "input" {
        return None;
    }
    let parent = dom.parent(node)?;
    (dom.attribute(parent, &Namespace::from(""), &LocalName::from("class")) == Some("cmd-line"))
        .then_some(node)
}

/// Logical px per wheel notch, used to normalize trackpad pixel deltas.
const WHEEL_NOTCH_PX: f32 = 48.0;
/// Board pan in diagonal tile steps per wheel notch (over the board pane).
const WHEEL_BOARD_TILES: f32 = 2.0;

/// Bridges the neutral Sprigging leaf cache to the layout engine's paint splice:
/// `emit_paint_list_with_leaves` asks this for each `<custom-leaf>`'s commands.
struct RenderedLeafSource<'a>(&'a RenderedLeaves);

impl LeafPaintSource for RenderedLeafSource<'_> {
    fn leaf_commands(&self, key: u64) -> Option<&[PaintCmd]> {
        self.0.get(key)
    }
}

/// The overmap palette: the party's place reads warm-green, the rest of the
/// discovered map cool-gray. The host owns the palette so no product-specific
/// node kind leaks into Cambium.
fn overmap_node_color(kind: &isometry_views::OvermapNodeKind) -> ColorF {
    match kind {
        isometry_views::OvermapNodeKind::Here => ColorF {
            r: 0.62,
            g: 0.83,
            b: 0.54,
            a: 1.0,
        },
        isometry_views::OvermapNodeKind::Elsewhere => ColorF {
            r: 0.72,
            g: 0.75,
            b: 0.82,
            a: 1.0,
        },
    }
}

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
    /// Public state immediately before the first entry in `history`. It is the
    /// required replay origin for a truthful source-time projection.
    history_origin: Option<GameSnapshot>,
    /// The log length last attached to the view's source-time adapter. Keeps
    /// ordinary dispatches from cloning/reinstalling an unchanged history.
    source_history_len: Option<usize>,
    /// Whether the current source-time availability has been projected into the
    /// view. This distinguishes an unavailable source from a stale projection
    /// that still needs clearing after a checkpoint load.
    source_history_attached: bool,
    /// Retained layout session in logical coordinates: hit-test target
    /// and incremental-apply subject.
    layout: Option<IncrementalLayout<NodeId>>,
    layout_size: (f32, f32),
    /// Cambium's retained custom-paint leaves, keyed by leaf key. The overmap's
    /// painted graph (nodes + edges) is registered here when the surface is
    /// open; paint splices each leaf's commands at its `<custom-leaf>` box.
    leaves: LeafRegistry<u64>,
    /// The leaf-tier paint cache: each registered leaf's last-rendered command
    /// buffer, reused across frames when the leaf and its box are unchanged.
    rendered_leaves: RenderedLeaves,
    /// The overmap swatch the registered leaf was last built from. Compared per
    /// frame while the surface is open so the leaf is re-registered (and thus
    /// repainted) only when the model actually changed; also the "any leaf is
    /// live" gate for the per-frame leaf-box walk.
    last_overmap_swatch:
        Option<cambium::GraphCanvasSwatch<String, isometry_views::OvermapNodeKind>>,
    /// The exact inputs the storylet rows were last built from: the world and
    /// the host's secret-fact ids. Compared per dispatch while the surface is
    /// open, so the rows re-resolve only when an answer could have changed. Kept
    /// across a close so reopening is free; the compare catches anything that
    /// moved while it was shut.
    last_storylet_inputs: Option<(CampaignWorld, Vec<String>)>,
    /// Origin of the CSS animation clock. `tick_animations` takes seconds
    /// since an arbitrary but monotonic zero; the process start is that zero.
    clock: Instant,
    /// Host entropy for adjudication. Every die an action rolls comes from here,
    /// so a fixed seed replays a session's combat exactly; peers never roll.
    action_rng: Rng,
    /// Nonces for this process's *own* asks: the DM's swings and solo play,
    /// which arrive over no connection and so carry [`PeerId::HOST`]. A joined
    /// player's ask is numbered by its own `ClientSession` and attributed by the
    /// host, so it never passes through here.
    own_requests: u64,
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
    /// The view node a cambium `on_hover` handler last saw the pointer enter, so
    /// a crossing dispatches one leave to it and one enter to the next. Drives
    /// the overmap's painted hover emphasis (and any future hoverable widget).
    hover_target_node: Option<NodeId>,
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
    /// `ISOMETRY_OVERMAP_SELFTEST`: seed a small overmap, place the party, and
    /// open the overmap surface for a capture (C8).
    overmap_selftest: bool,
    overmap_fired: bool,
    /// `ISOMETRY_OVERMAP_SOURCE_TIME_SELFTEST`: after the normal overmap seed,
    /// capture its first durable source prefix. This stays opt-in so the C8
    /// screenshot mode remains a live exploration proof.
    overmap_source_time_selftest: bool,
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
    /// The real faction moves behind the downtime surface's display rows,
    /// index-aligned with them. The DM strikes rows in the view; on commit the
    /// host keeps the moves whose row survived. Rolled from `generation_tape`.
    faction_turn_batch: Vec<FactionMove>,
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
        m.tokens
            .iter()
            .filter(|t| t.owner.as_deref() == Some(owner))
            .count()
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
    let occupied: std::collections::HashSet<TileCoord> = map.tokens.iter().map(|t| t.at).collect();
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
        history_origin: None,
        source_history_len: None,
        source_history_attached: false,
        layout: None,
        layout_size: (0.0, 0.0),
        leaves: LeafRegistry::new(),
        rendered_leaves: RenderedLeaves::new(),
        last_overmap_swatch: None,
        last_storylet_inputs: None,
        clock: Instant::now(),
        // A fixed seed keeps a solo session reproducible and makes the headed
        // verification deterministic. A real table seeds this per session.
        action_rng: Rng::new(0x15D_0BE),
        own_requests: 0,
        beats_playing: false,
        sheet,
        cursor: (0.0, 0.0),
        modifiers: ModifiersState::empty(),
        lmb_down: false,
        last_drag: None,
        drag_token: None,
        last_hover: None,
        last_focus: None,
        hover_target_node: None,
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
        overmap_selftest: std::env::var_os("ISOMETRY_OVERMAP_SELFTEST").is_some(),
        overmap_fired: false,
        overmap_source_time_selftest: std::env::var_os("ISOMETRY_OVERMAP_SOURCE_TIME_SELFTEST")
            .is_some(),
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
        faction_turn_batch: Vec::new(),
        pack_emotes,
    };
    event_loop.run_app(&mut app).expect("run app");
}
