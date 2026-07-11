# Isometry bootstrap plan

**Date:** 2026-07-05
**Status:** active plan. I0-I6 landed 2026-07-05/07 (probes verified,
receipts in Findings). I4 = DM-authority sessions over iroh (replication
core, real-QUIC loopback, windowed `--host`/`--join`, per-player fog).
I5 = table furniture: dice roller, initiative modes, measurement + area
templates, GM whispers (four committed pieces; receipts in
scry-shots/2026-07-06_isometry_{dice,init,measure,whisper}_*.png).
I6 = system plugins: schema-driven character sheets plus Lua (piccolo)
rules, 5e SRD first; the substrate/system split proven end to end
(receipts in
scry-shots/2026-07-07_isometry_{sheet_open,attack_rng,str14_attack}.png).
Residue: a "new empty map" entry point; drag-to-reorder on the turn
list; serval `.side` wheel-scroll (taller window meanwhile);
cross-machine run (unavailable here). The bootstrap arc is complete;
isometry-web (browser player client) and campaign packs are the next
horizons, each their own plan.
**Thesis:** a pixel-art isometric P2P VTT is buildable on the Merely
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
   **Amended 2026-07-08** (see 2026-07-08_campaign_packs_plan.md, decision
   11): the fixed camera is now the 2D mode of a 2D/2.5D/3D lens ladder, not
   a permanent invariant, and facings move toward eight (flanking). The rest
   of this contract stands.
6. **Substrate/system split.** Substrate: tiles, tokens, turn list,
   facing, elevation, area templates, fog, dice mechanics as rollers.
   System plugin: schemas for characters and items plus Lua scripts
   (piccolo) for derived stats and roll formulas. Whether elevation grants a bonus is
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

### I3: tokens and local play (landed 2026-07-06)

Token placement, drag movement with path preview, facing, a turn list
with drag-in/drag-out (free movement mode when out), hot-seat play on one
machine. **Done when** two hot-seat players can run a skirmish on an
authored map with turns and facing tracked. Landed: Token mode
(place/remove, sprite swatches), Play mode (select, BFS reach with
elevation/occupancy rules, hover path preview, move + facing as one
undoable step, r to rotate), TurnList in core with panel rows, in/out
toggles, End turn / Enter, gold active marker, green selection marker.
In/out is click-toggle rather than drag (drag-to-reorder deferred);
move budget is a constant 5 until system plugins supply speed (I6).

### I4: sessions (landed 2026-07-06)

`isometry-net` on iroh: DM hosts, players join by ticket, event log
replicates, late join via snapshot plus tail, per-player fog. **Done
when** two machines on different networks complete a session with a
mid-session join and no state divergence (event log hashes match).
Landed: `isometry-net` with `HostSession`/`ClientSession` pure-sync
replication over a transport seam; the replicated unit is `GameEvent`
(map `SessionEvent`s + turn ops), the host orders it, a portable FNV
log hash makes convergence checkable. Late join carries snapshot + the
host's hash so joiners converge on the tail. The `iroh` feature binds a
QUIC transport (one bi-stream per peer, postcard frames, ticket
mint/parse). isometry-serval gets `--host`/`--join` over a background
tokio runtime; in a session the view is Remote (play routes through the
authority, no optimistic mutation). Verified as far as one machine
allows: 5 replication tests, a real-QUIC loopback (mid-session join,
convergence on state + hash), a two-window render (client shows host's
board over QUIC), and a focus-free self-test of the host UI→net→UI
round-trip; per-player fog of war (client-side render fog, LOS in core;
see the fog Finding). **Deferred:** cross-machine/cross-network run
(physically unavailable here; the loopback binds two real endpoints and
does the mid-session-join + convergence the done-condition names).

### I5: table furniture (landed 2026-07-06)

Initiative modes (individual speed order and side-based, a system-level
choice over the same turn list), dice roller with modifier expressions,
measurement and area templates, GM whispers. **Done when** a 5e-shaped
encounter runs end to end without leaving the app. Landed as four
committed pieces: (1) a seedable dice roller (core `dice`: xorshift Rng +
NdS+M parser) with a shared roll log replicated as `GameEvent::Rolled`;
(2) initiative modes (`roll_initiative` orders the turn list by d20 per
token or per side, via `TurnList::set_order` / `GameEvent::TurnSetOrder`);
(3) measurement + area templates (core `template`: Chebyshev distance,
burst/line/cone tile sets; a Measure mode previews them); (4) directed GM
whispers (net `Hello`/`Whisper`, a host key-capture composer, a message
log). The mechanical furniture for a 5e encounter is present; the rules
that consume it (HP, AC, hit resolution) are system-plugin work in I6, so
"end to end" completes when a system lands.

### I6: system plugins (landed 2026-07-07)

Schema plus Lua (piccolo) plugin architecture; one system first (5e SRD
chosen). Character sheets render from schema; rolls use plugin formulas.
**Done when** a character sheet for the chosen system is created, bound to
a token, and drives its rolls in a session. Landed as a new
`isometry-system` crate holding the plugin lane: a `System` bundles
field/derived/action schemas with a sandboxed `piccolo::Lua`
(`Lua::core()`, no io/os); the 5e SRD system is data plus a Lua script
(`ab_mod`, `m_str`..`m_cha`, `a_attack`). Core stays rules-free:
`SheetData` (a system-tagged `FieldValue` map) lives in `isometry-core`,
binds to tokens on the `MapDocument`, and replicates as
`GameEvent::SheetSet`. Views render a system-agnostic sheet overlay from a
plain `SheetSchema` plus host-precomputed derived stats; the host
(`pump_sheets`) owns the Lua and keeps it off the render path (derived
recompute on edit, action formulas evaluated on click, all via
`runner.update`). Receipts: a 5e sheet bound to a token shows Lua-derived
modifiers and rolls its Attack action
(`scry-shots/2026-07-07_isometry_sheet_open.png`); rolls vary over five
clicks (`_attack_rng.png`); bumping STR 10->14 via the stepper re-derives
`STR mod +0 -> +2` and shifts Attack from 1d20+2 to 1d20+4
(`_str14_attack.png`) -- the full edit -> Lua re-derive -> action loop.

## Design space on file (alternatives and open questions)

- **Bevy or Godot instead of the serval stack.** Bevy: game-native
  (bevy_ecs_tilemap, matchbox), weak at form-heavy chrome, feeds nothing
  back into Merely. Godot: fastest to playable, same objection
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

- 2026-07-07 (I6 system plugins): the substrate/system split holds under a
  real scripting engine. Rules live in a new `isometry-system` crate; the
  core learns only `SheetData` (a system-tagged `FieldValue` map on the
  `MapDocument`, replicated as `GameEvent::SheetSet`) and never what a
  modifier means. piccolo (pure-Rust Lua, `Lua::core()` sandbox: no io/os)
  fits the woodshed posture: no C toolchain, no runtime deps. Two engine
  facts worth recording: (1) piccolo's `//` truncates toward zero rather
  than flooring (Lua 5.4 floors), so `ab_mod` normalizes the remainder by
  hand to stay correct for negative modifiers; (2) the `try_enter` closure
  is higher-ranked (`for<'gc>`), so no method-lifetime borrow may cross
  into it: the Rust->Lua boundary copies the sheet into an owned `Vec` and
  interns every key as a `'gc` Lua string before `table.set`. The layering
  earned its keep: views stay rules-blind, rendering the sheet overlay from
  a plain `SheetSchema` plus host-precomputed derived stats, while the host
  (`pump_sheets`) owns the `Lua` and evaluates off the render path (derived
  on edit, actions on click, via `runner.update`). The int-only Rust<->Lua
  boundary (build a table, call a global, read an `i64`) keeps the seam
  small.
- 2026-07-06 (I5 shape): the table furniture reused the seams the earlier
  phases established. Randomness is data, not shared state: the roller
  resolves a roll with its own Rng and the RESULT crosses the wire (a
  `RollRecord`), the friendly-table trust model again (like fog and the
  DM-authority log); a portable xorshift Rng keeps it dep-free and
  test-seedable. Initiative is "just" a reorder of the turn list, so
  modes change how the order is BUILT, not how `advance` walks it.
  Templates are the same core-geometry shape as visibility/movement.
  Whispers are the one thing that is NOT the broadcast log: directed
  `Recipient::One`, verified by the sim (reaches only the named player,
  never touches the replicated log). Text input avoided the serval
  `text_field`/focus lane: the host captures keystrokes into a compose
  buffer directly, simpler and fully in our control. Panel growth forced
  a taller default window (820) + `.side { overflow-y: auto }`; the
  wheel-scroll of `.side` did not visibly engage in a quick test (serval
  scroll-container wiring is a later check), so the height bump is the
  load-bearing fit for now.
- 2026-07-06 (I4 architecture): the session layer keeps networking out
  of the code that carries the rules. `HostSession`/`ClientSession` are
  pure synchronous state machines (consume `NetMessage`, emit
  `Outbound`); the `sim` module routes them in-process for tests, the
  `iroh` feature pumps them over QUIC. This made the whole protocol
  testable without a network (5 tests: from-start convergence,
  late-join snapshot+tail, invalid-intent rejection, turn replication,
  atomic move+facing) and isolated the one part that can't be
  cross-machine-verified here behind a feature flag.
- 2026-07-06 (iroh 0.98 API, confirmed against mere + docs.rs):
  `Endpoint::builder(presets::N0).alpns(vec![..]).bind()` (server),
  `Endpoint::bind(presets::N0)` (client); `endpoint.connect(addr, alpn)
  -> Connection`; accept is double-await
  (`accept().await? .await?`); `open_bi`/`accept_bi -> (SendStream,
  RecvStream)`; `conn.remote_id() -> EndpointId`. Tickets via
  `iroh_tickets::endpoint::EndpointTicket` with the loopback-addr
  rewrite (the mere-transport pattern) so same-machine peers dial with
  no network. Deadlock avoided by having the **host open** the stream
  and write the snapshot first (QUIC opens lazily; the party with data
  opens). Compiled clean first try; loopback converges over real QUIC.
  Did NOT couple to mere's `murm/transport` (it's mere-internal, bound
  to mere identity); harvested the pattern, kept isometry standalone.
- 2026-07-06 (verification limit + focus-free hook): driving one of two
  overlapping same-title windows via OS input (SetForegroundWindow /
  AppActivate + SendKeys) is unreliable — Windows foreground-lock meant
  a host keypress never reached the app (confirmed: no `key:` trace
  line). So live two-window propagation is shown by composition, and a
  new `ISOMETRY_NET_SELFTEST=1` hook fires one end-turn from inside the
  app to verify the host UI→net→republish→UI round-trip deterministically
  (traced: end_turn → pump submit → run_host apply → seq 0->1 republish
  → board shows knight 3 active). Pattern to keep for net verification.
- 2026-07-06 (I3 receipts, scripted drive + self-captures in
  scry-shots/2026-07-06_isometry_i3_*.png): token select shows the BFS
  reach (water impassable, elevation step limits honored around the
  hill terraces), hovered path tints inside it, click moves the token
  with facing from the final step (mirrored sprite) as one undoable
  step, Enter advances the turn and the gold marker + panel row follow
  (knight 1 to knight 3 to goblin 2). Turn gating (listed tokens move
  only on their turn, unlisted move freely) and token place/remove are
  covered by isometry-views state tests; TurnList and pathfinding by
  isometry-core tests (21 total).
- 2026-07-06 (engine gap, found by sprite mirroring): serval conjugates
  CSS transforms at the box origin; the spec default `transform-origin`
  is `50% 50%`, so `scaleX(-1)` reflects an element out of its own box
  (and its hit region with it, which is how it surfaced: flipped tokens
  missed clicks). App-side workaround: pre-translate by the width
  (`translateX(24px) scaleX(-1)`). Worth a serval issue alongside the
  inset-sizing and paint/hit-divergence notes.
- 2026-07-06 (CSS lesson, not an engine bug): state tints (reach, path,
  selected, hover) must sit after the tile-kind rules in the sheet;
  equal specificity resolves by source order, and kind rules earlier in
  the sheet silently win otherwise. Tileset authors inherit this rule:
  kind classes first, state classes last.
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

- 2026-07-07 (I6): system plugins landed. New `isometry-system` crate
  (`System` = field/derived/action schemas + a sandboxed `piccolo::Lua`;
  `srd_5e()` builds the 5e SRD system as data plus a Lua script). Core:
  `SheetData`/`FieldValue` + `MapDocument` sheet binding (1 test). Net:
  `GameEvent::SheetSet` replication. Views: system-agnostic sheet overlay
  (fields with steppers, Lua-derived modifiers, action buttons),
  `open_or_bind_sheet` / `request_sheet_edit` / `request_action`,
  `roll_labeled`. Serval: host loads `srd_5e()`, hands views a
  `SheetSchema`, and `pump_sheets` binds/edits/rolls and recomputes derived
  via Lua off the render path; dice reseeded with clock entropy. +3 system,
  +1 core tests. Receipts in
  scry-shots/2026-07-07_isometry_{sheet_open,attack_rng,str_mid,str14_attack}.png:
  a 5e sheet bound to knight 1 rolls Attack (1d20+2), rolls vary over five
  clicks, and STR 10->14 re-derives STR mod +0->+2 and shifts Attack to
  1d20+4.
- 2026-07-06 (I5): table furniture landed as four committed pieces.
  Dice (core `dice`: Rng + roller; net `Rolled` + shared roll log;
  panel dice buttons + log). Initiative (`TurnList::set_order`,
  `GameEvent::TurnSetOrder`, `roll_initiative` individual/side; panel
  toggle + Roll init). Measure (core `template`: distance +
  burst/line/cone; Measure mode + highlight + readout). Whispers (net
  `Hello`/`Whisper` directed routing; host key-capture composer +
  message log; NetBridge whisper channel + inbox + player list). +5
  core, +1 net, +4 views tests; receipts in scry-shots. Panel outgrew
  720, so the window is 820 and `.side` is `overflow-y: auto`.
- 2026-07-06 (I4 fog): per-player fog of war landed. Core
  `visibility` module (radius Bresenham LOS + opacity, 4 tests); views
  fog state (viewer, visible/explored sets, three-state fog_level,
  token_visible, recompute_fog, cycle_viewer) with shroud rendering and
  enemy-token hiding (1 test); serval `--as <player>` + `f` viewer
  cycle. Client-side render fog (host still sends full state). Receipts
  in scry-shots/2026-07-06_isometry_fog_*.png.
- 2026-07-06 (I4): sessions landed. `isometry-net` crate (protocol +
  HostSession/ClientSession + sim + iroh_link behind the `iroh`
  feature); isometry-views gains a Remote net-mode (play/turn actions
  route as `GameEvent`s, render from the replicated snapshot);
  isometry-serval gains `--host`/`--join` over a background tokio
  bridge. Receipts: 5 replication tests, real-QUIC loopback, two-window
  client-renders-host-board, focus-free host round-trip self-test.
  Session smoke example for a manual two-process demo. Residue noted in
  the I4 phase (per-player fog, new-empty-map, drag-reorder).
- 2026-07-06 (later): I3 landed. Core: `TurnList` (active-stable
  removal), `reachable`/`path_to` BFS with `MoveRules` (budget,
  climb/drop steps, passability, occupancy). Views: Play and Token
  modes, reach/path tinting, turn panel, markers, sprite mirroring.
  Host: hover-tile preview updates gated by a read-only state check
  (`runner.update` rebuilds the tree, so per-pixel updates are out),
  r/Enter keys. Receipts + two new engine notes in Findings. Demo
  starts with all four tokens in the turn order.

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
