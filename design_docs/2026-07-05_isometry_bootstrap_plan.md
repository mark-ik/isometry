# Isometry bootstrap plan

**Date:** 2026-07-05
**Status:** active plan. I0 + I1 landed 2026-07-05/06 (all three probes
verified, receipts in Findings). I2 landed 2026-07-06 (editor, undo,
save/load, pixel-sprite tokens); I2 residue: a "new empty map" entry
point (the app always opens the demo board today). I3 (tokens and local
play) is next.
**Thesis:** a pixel-art isometric P2P VTT is buildable on the Strophos
stack with the woodshed consumer pattern, and the GBA tactics aesthetic
(fixed camera, battle-scale maps) keeps every known engine risk inside
measured territory. This plan records the founding decisions, the engine
probes that gate the render approach, and the phase ladder to a playable
session.

## Decisions on file (2026-07-05 founding session)

1. **Standalone repo, woodshed pattern.** Isometry consumes xilem-serval,
   serval-layout, and netrender as git deps on the mark-ik remotes with
   the `[patch.crates-io]` mirror at the workspace root (stylo,
   stylo_atoms, taffy, ipc-channel; copy the current set from
   `repos/woodshed/Cargo.toml` when wiring I1, it is the maintained
   model). Local serval checkouts override via a gitignored
   `.cargo/config.toml`. Isometry becomes the third serval consumer app
   after meerkat and woodshed, pressuring lanes the other two do not:
   image sprites, drag and drop, pointer capture, form-heavy sheet UI.
2. **DM-authority sessions over p2p transport.** The DM's app is the
   authority; players connect over iroh and receive an ordered event
   log the host validates and rebroadcasts. Late joiners get a snapshot
   plus the log tail. Turn-based play makes rollback netcode and CRDTs
   unnecessary; that machinery is out of scope by doctrine (CLAUDE.md
   don'ts).
3. **Tile-as-DOM-element, viewport-windowed.** The map lives in
   isometry-core state at any size; the xilem_serval view function
   projects only the visible tile range plus a margin, the same move
   `Orrery::frame` makes in meerkat (cull before creating elements).
   Map size is therefore a state question, not an engine question; the
   live DOM stays in the low thousands of elements regardless.
4. **Tileset-as-stylesheet.** Tile kinds are CSS classes; a tileset ships
   sprites, a manifest naming the class vocabulary, and optionally CSS.
   Keep variants class-shaped rather than per-element custom properties
   so stylo style sharing collapses identical tiles to one computed
   style.
5. **Aesthetic contract (Knight of Lodis, FFTA).** Fixed camera, one
   locked isometric angle, rotation permanently out of scope. 2:1
   diamond tiles (32x16 default footprint, parameterized). Quantized
   height steps. Low internal resolution integer-scaled with
   nearest-neighbor. Sprites draw two facings and mirror for the other
   two. Battle-scale maps (15x15 to 30x30) are the design center.
6. **Substrate/system split.** Substrate: tiles, tokens, turn list,
   facing, elevation, area templates, fog, dice mechanics as rollers.
   System plugin: schemas for characters and items plus rhai scripts for
   derived stats and roll formulas. Whether elevation grants a bonus is
   the plugin's business; that it exists and renders is the substrate's.
7. **Licensing lane.** 5e SRD is CC-BY-4.0, Pathfinder 2e is ORC; both
   are shippable as first-party system plugins with attribution. No
   other publisher content.

## Engine probes (gate the render approach, ride I1)

Receipts from the 2026-07-04 meerkat perf work say settled frames are
near-free (per-surface tile state, dirty_tiles=0), single-tile edits are
local (tile-dep hashing in `netrender/src/tile_cache/mod.rs`), and the
scaling costs are camera pan (world-space AABB hashes dirty every tile),
per-edit full-scene paint emission, and animation churn. The probes turn
those into go/no-go measurements at Isometry's actual scale:

- **P1, pixelated sampling.** Verify `image-rendering: pixelated` (or an
  equivalent nearest-neighbor path) survives serval's knockout set end to
  end at integer scale factors. Done when a test sprite renders crisp at
  2x/3x/4x in a serval host window. If knocked out, un-knock in serval
  rather than working around (standards-correct over host hacks).
- **P2, element count.** A synthetic 30x30x3-layer board (roughly 2,700
  tile elements) plus 20 tokens: measure first build, settled frame,
  single-tile swap, and a continuous pan, debug and release. Done when
  settled cost is near zero, edits stay under a few ms, and the pan
  number is recorded (expected bad until the pan fix, see below).
- **P3, depth sort with elevation.** z-index derived from (row + col +
  height step) with sprites overhanging tiles behind them. Done when a
  stacked cliff with a token on top and one behind renders in correct
  painter order, verified by screenshot.

**Known dependency, camera pan.** Panning re-rasters the full viewport
per frame today. The approved escape hatch (composite the cached surface
texture at the camera offset during the gesture, re-render on settle,
Mark 2026-07-04) lives on the serval/netrender side and is not landed.
Isometry's fixed-camera aesthetic tolerates snap-scroll in the interim;
the tactics references scroll in steps anyway. Track, do not block on it.

## Crate ladder

- `isometry-core` (landed, I0): grids, iso projection math, map document,
  session events. Pure Rust, serde only.
- `isometry-views` (I1): xilem_serval view functions plus the CSS sheets.
  Board view windows the visible tile range; chrome is panels and menus.
- `isometry-serval` (I1): native winit host, borrowed from the meerkat
  main.rs harness patterns and `woodshed-serval` (do not import either).
- `isometry-net` (I4): iroh transport, host authority, snapshot and tail.
  Event semantics stay in core; this crate only moves bytes.
- `isometry-web` (post-I6): browser player client via the serval-web
  lane, generalized from `serval/examples/serval_web_smoke`.

## Phases (done-conditions, not dates)

### I0: repo bootstrap (landed 2026-07-05)

Workspace, repo docs (CLAUDE.md, DOC_POLICY, DOC_README,
PROJECT_DESCRIPTION), and the isometry-core seed: `TileGrid`, iso
projection with round-trip tests, `MapDocument` with serde round-trip,
`SessionEvent` apply with tests. **Done when** `cargo test -p
isometry-core` passes. Landed; see Progress.

### I1: serval host renders a board

Wire the serval/netrender git deps and patch mirror; stand up
`isometry-views` and `isometry-serval`; render a static hand-authored
board from a `MapDocument` with a placeholder tileset. Run probes P1-P3
and record receipts in Findings. **Done when** a serval host window shows
a correctly depth-sorted 30x30 board with crisp pixels and the three
probe receipts are logged.

### I2: map editor

Tile palette, layer select, height brush (raise/lower), prop placement,
flood fill, save/load (serde JSON first, format review later), undo via
the event log. **Done when** a Lodis-scale map can be authored from an
empty board, saved, reloaded identically, and every edit is undoable.

### I3: tokens and local play

Token placement, drag movement with path preview, facing, a turn list
with drag-in/drag-out (free movement mode when out), hot-seat play on one
machine. **Done when** two hot-seat players can run a skirmish on an
authored map with turns and facing tracked.

### I4: sessions

`isometry-net` on iroh: DM hosts, players join by ticket, event log
replicates, late join via snapshot plus tail, per-player fog. **Done
when** two machines on different networks complete a session with a
mid-session join and no state divergence (event log hashes match).

### I5: table furniture

Initiative modes (individual speed order and side-based, a system-level
choice over the same turn list), dice roller with modifier expressions,
measurement and area templates, GM whispers. **Done when** a 5e-shaped
encounter runs end to end without leaving the app.

### I6: system plugins

Schema plus rhai plugin architecture; one system first (pick 5e SRD or
PF2e at phase start, not now). Character sheets render from schema;
rolls use plugin formulas. **Done when** a character sheet for the chosen
system is created, bound to a token, and drives its rolls in a session.

## Design space on file (alternatives and open questions)

- **Bevy or Godot instead of the serval stack.** Bevy: game-native
  (bevy_ecs_tilemap, matchbox), weak at form-heavy chrome, feeds nothing
  back into Strophos. Godot: fastest to playable, same objection
  stronger. Not chosen for the woodshed reasons. Revisit trigger: serval
  churn blocking Isometry shipping for an extended stretch, or probe
  P2/P3 failing without a landable engine fix.
- **Transport.** iroh chosen for QUIC hole-punching and ticket-shaped
  joins. Browser player client needs a transport story on wasm (iroh
  browser support was experimental as of the decision); candidates:
  iroh-on-wasm when stable, a WebRTC lane (matchbox-style), or a thin
  relay the DM app runs. Decide inside I4 planning for native, revisit
  for isometry-web.
- **Co-DM concurrent map editing.** Would motivate CRDTs (loro or
  automerge). Deliberately deferred; single-author maps plus the event
  log cover the product until a real co-DM need surfaces.
- **Animation policy.** Tile-cache receipts say animation churn is the
  settled-frame enemy. Default posture: sparse animation (selected token
  idle, cursor pulse), palette-cycle effects confined to few elements.
  Ambient full-map water shimmer needs its own probe before adoption.
- **Format of record for maps and packs.** JSON via serde first; a
  campaign-pack container (zip with manifest) decided in I2 or when
  sharing becomes real.
- **Megamaps.** Roll20-scale dungeon crawls (100x100+) are out of the
  design center. If they become a target: retained segment emission on
  the serval side converts per-edit cost from O(scene) to O(edit)
  (designed, deprioritized per the 2026-07-04 receipts), and ground
  chunking is the app-side fallback.

## Findings

- 2026-07-06 (probe P2 closed, release + synthetic receipts, 1100x720):
  release demo board (24x24, ~650 elements): first frame 13.7ms scene +
  19.1ms raster, snap-pan frames ~4.3ms scene + ~1.3ms raster. Release
  synthetic (`ISOMETRY_SYNTH=1`, 30x30 all-layers + 20 tokens, ~2,700
  elements): first frame 27.6ms + 6.2ms, pan ~16ms + ~3.7ms. Debug
  synthetic pan ~166ms scene (debug is roughly 10x release on this
  path). Settled frames remain zero-cost (event-driven). Verdict:
  battle-scale boards are comfortably real-time in release at the
  DOM-per-tile design, pan included, before any of the listed
  optimizations (viewport windowing, retained emission, camera-offset
  composite).
- 2026-07-06 (P1 fix landed, both repos): serval-layout paint emission
  now reads computed `image-rendering` (commit in serval,
  `paint_emit.rs`); netrender carries a `nearest` flag on
  `SceneImage`/`ScenePattern`, hashes it (and `clamp_to_uv`, a
  pre-existing hash gap) into tile deps, and maps it to vello's
  nearest-neighbor sampler; the paint translator sets it from the paint
  list (`crisp-edges` and `pixelated` both lower to nearest). Token
  sprites are 8x12 data-URI PNGs at 3x under
  `image-rendering: pixelated`.
- 2026-07-06 (engine gap, found by the sprite tokens): serval-layout's
  retained `IncrementalLayout` never decoded CSS background/border
  images; every `emit_paint_list` passed a fresh empty
  `BackgroundImagePlane`, so `background-image` painted only on the
  one-shot layout path. Fixed engine-side: the session owns the plane,
  builds it from the cascade, and rebuilds it after applies that can
  change which URL applies (structural batches, class/id flips,
  inline-style edits mentioning background/border-image), with a
  URL-keyed decode cache so rebuilds decode nothing twice.
  Geometry-only inline-style edits (the pan case) skip the rebuild.
  Receipt: knight sprite crisp at 3x in
  `scry-shots/2026-07-06_isometry_sprites_zoom.png`, nearest-neighbor
  edges, gold pauldrons intact; P1 verified end to end.
- 2026-07-06 (engine gap, found by the I2 click probe): absolute inset
  sizing (`position: absolute; left: 0; right: 0; top: 0; bottom: 0`
  with auto width/height) is not honored by serval-layout. The app root
  sized to content (228px, the side panel), `hit_test` returned None
  everywhere right of it, while paint still drew the overflowing board,
  so the bug presented as "panel clicks work, board clicks vanish."
  Two engine-side notes worth their own serval issue: inset sizing for
  absolutes, and the paint/hit divergence on content overflowing an
  undersized `overflow: hidden` box (paint did not clip where hit-test
  pruned). App-side fix: the woodshed root idiom
  (`width: 100%; height: 100%`).
- 2026-07-06 (I2 editor receipts, scripted input drive): mode and brush
  buttons dispatch; a five-tile water drag-paint applies through the
  per-tile dedupe; Raise applies twice; Ctrl+Z pops one step (7 steps
  became 6, matching 5 paints + 2 raises); Save writes
  `maps/demo_skirmish.json`. Fill and paint/undo/redo round-trips are
  covered headlessly by `isometry-views` state tests.
- 2026-07-06 (harness): screen-grab captures lost twice to overlapping
  windows during concurrent desktop use (the known scry-shots gotcha).
  The host now self-captures: `ISOMETRY_CAPTURE_DIR=<dir>` overwrites
  `<dir>/isometry_capture.png` with every presented frame via
  netrender_device texture readback, immune to window occlusion.
- 2026-07-05 (I1, probe P1, code-grounded): `image-rendering: pixelated`
  is knocked out at three seams. serval-layout's paint emission hardcodes
  `ImageRendering::Auto` at every image site
  (`repos/serval/components/serval-layout/paint_emit.rs:1115` and
  siblings); paint_list_render's translator never reads the field; the
  vello image quality is never selected from it. `paint_list_api`
  already carries the `Pixelated` variant
  (`repos/netrender/paint_list_api/src/primitives.rs:217`). Verdict:
  engine-side fix (read computed style at emit, thread through the
  translator, map to the nearest-neighbor sampler), standards-correct
  over host hacks. Until it lands the placeholder tileset uses clip-path
  diamonds; `clip-path: polygon()/circle()/ellipse()` is supported
  (`paint_emit.rs:3191`).
- 2026-07-05 (I1, probe P2 receipts, debug build, 24x24 demo board,
  roughly 650 elements, 1100x720 window): first frame scene 146-155ms,
  first raster 26-44ms; snap-pan frames (camera as one inline-style
  attribute change on the board container) scene ~65ms, raster ~10-11ms;
  settled frames cost zero because the host is event-driven
  (`ControlFlow::Wait`, no redraw without input). The ~65ms pan scene
  cost is view rebuild + full-scene emit, the expected O(scene) lane;
  candidates when it matters: memoized row views, retained segment
  emission, the netrender camera-offset composite. Release numbers and
  the 30x30x3 synthetic are still to run.
- 2026-07-05 (I1, probe P3, screenshot receipt
  `scry-shots/2026-07-05_isometry_i1_board.png`): terraced hill with
  cliff columns, tokens standing on elevation, trees and path sorting
  correctly in front of and behind terrain; depth as plain z-index from
  `depth_key` holds. Cosmetic note: terraces read as dark rings under
  the placeholder tileset; a real tileset resolves it.
- 2026-07-05 (I1, infrastructure): the serval GitHub tip (48c08ea) was
  mid-flight broken (the `invalidate.rs` fix sat uncommitted in the
  local working tree), the known transient-concurrent-work pattern. The
  gitignored `.cargo/config.toml` override to the sibling checkouts (the
  woodshed/mere pattern) is in place; committed manifests stay on git
  deps and resolve once serval main is pushed green.
- 2026-07-05 (founding session, code-grounded): netrender tile
  invalidation hashes primitives per world-space tile
  (`repos/netrender/netrender/src/tile_cache/mod.rs`), so camera pan
  dirties all visible tiles while single-tile edits stay local. Meerkat
  receipts (debug, ~108-node session): steady chrome 4.3ms, structural
  frame 24.3ms chrome + 45ms raster, settled surfaces dirty_tiles=0.
  Full receipts in
  `repos/serval/docs/2026-07-03_shell_paint_emission_raster_plan.md` and
  the mere render perf plan
  (`repos/mere/design_docs/mere_docs/implementation_strategy/2026-06-24_meerkat_render_perf_plan.md`).

## Progress

- 2026-07-05: repo created. I0 landed: workspace, CLAUDE.md, doc set,
  isometry-core seed (grid, iso, map, event modules with tests).
- 2026-07-05 (later): I1 largely landed. isometry-views (board view fn,
  demo map, placeholder tileset CSS) + isometry-serval (winit host,
  woodshed harness shape, `ISOMETRY_PROFILE=1` frame timers). Demo board
  renders with hover, click-select, and arrow-key snap pan; P2/P3
  receipts in Findings. Residue for the next session: the P1 engine fix
  in serval/netrender, a 30x30x3 synthetic P2 run, release-build
  numbers, and a click-to-select edit-cost receipt.
- 2026-07-06: P1 engine fix landed in serval + netrender (see Findings);
  pixel-sprite tokens replace the colored rects. I2 largely landed:
  core `apply` returns inverse events and `TileGrid::flood_region`
  (undo primitive, 14 core tests); editor modes
  (Select/Paint/Prop/Fill/Raise/Lower), tile-kind palette bound to the
  tileset classes, undo/redo/save/load, drag painting with per-tile
  dedupe, Ctrl+Z / Ctrl+Y; `ISOMETRY_SYNTH=1` stress board;
  `ISOMETRY_CAPTURE_DIR` self-capture. Scripted-drive receipts: paint,
  raise, undo, save, and load all applied, and the painted stroke
  survived the save/load round-trip on screen. Release + synthetic P2
  numbers landed (see Findings), closing I1's residue. Serval-side,
  this phase surfaced and fixed two engine gaps (pixelated sampling,
  retained bg-image decode) and documented two more for later (absolute
  inset sizing; paint/hit divergence on clipped overflow). Remaining
  I2 residue: a "new empty map" entry point.
