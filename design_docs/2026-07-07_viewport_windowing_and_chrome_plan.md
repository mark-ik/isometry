# Viewport Windowing + Chrome Plan

**Date:** 2026-07-07
**Status:** active plan (next after I6). The "quick wins" lane from the
next-horizons landscape ([2026-07-07_next_horizons_landscape.md](2026-07-07_next_horizons_landscape.md),
sequence items 1 and 2). All phases are app-side with no genet or external
dependency, and reuse capabilities the engine already ships. Done-conditions,
not time estimates. **W1-W4 all landed 2026-07-07**: windowing, board
wheel-pan, token drag-move, token context menu. Deferred halves, tracked in
their phases: W2 panel-scroll, W3 turn-reorder + Play-gated drag, W4 hover
tooltip + tile menus.

## Why this lane first

The audit corrected the honest ceiling: the shipped interactive board tops
out around 30x30-40x40, because `ground_tiles`/`prop_tiles` iterate the
entire grid on every update, so DOM count and per-update CPU are O(scene)
and a full rebuild fires on every edit and every pan (bootstrap decision #3
designed windowing but never built it). Everything scale-related (REGION
maps, browser maps) is gated on fixing that. Alongside it, the GUI audit
found several high-value interactions the engine already supports that the
app has not wired: wheel scroll, pointer drag, and overlay-placed popups.
This lane raises the ceiling and closes those gaps.

## Phase W1: viewport windowing

Cull tile and prop emission to the visible range plus a margin, so rendered
element count is bounded by the viewport rather than the grid.

- Compute the visible tile range from the camera offset, pane size, and the
  iso projection (invert `iso_to_screen` at the pane corners to a tile-space
  AABB, pad by one tile ring plus the max elevation stack height so tall
  raised tiles at the top edge are not clipped).
- In `ground_tiles`/`prop_tiles`, iterate only that range instead of the
  full grid. Fog shrouds, markers, reach/path highlights, and templates cull
  the same way.
- Keep the model untouched: windowing is a render-emission change only, so
  save/load, replication, undo, and the grids are unaffected.

**Done when:** a 60x60x3 board pans at battle-scale frame cost (rendered
element count roughly constant as the board grows, verified under
`ISOMETRY_PROFILE=1`); the 24x24 demo renders visually identical to today; a
100x100 board is authorable and pannable (frame cost bounded by viewport,
not the 30k-element extrapolation). Receipt: profile numbers for 30x30,
60x60, 100x100 in scry-shots or the Progress log.

## Phase W2: wheel to scroll

The engine has `scroll_at` with CSS scroll chaining; the host window match
in `main.rs` has arms for CursorMoved/MouseInput/KeyboardInput/Resized but
no MouseWheel arm, so the `.side` panel cannot wheel-scroll (the I5 fix was
a taller 820 window as a workaround).

**Landed 2026-07-07 as board wheel-pan; panel-scroll deferred.** A
`MouseWheel` arm now routes by region. Over the board pane it snap-pans the
board (`pan_tiles`, the tactics-canvas wheel-is-pan convention); the region
gate keeps it from document-scrolling the chrome. Verified before/after
(testing/isometry/images/2026-07-07_isometry_w2_board_pan.png): the board
pans, the panel stays put.

Panel-scroll was the original intent, but `scroll_at` over a near-full panel
chains through `.side` into the whole-document viewport (dragging the board),
and genet has no `overscroll-behavior` to contain it (only a WPT
expectations entry). Since the panel fits the retained 820 window, panel-wheel
is left inert rather than shipping that jank. **Deferred:** true panel-scroll
for short windows, blocked on genet scroll-isolation (`overscroll-behavior:
contain` or an equivalent container fix).

## Phase W3: pointer-drag interactions

**Landed 2026-07-07 as token drag-move (Select mode); turn-reorder deferred.**
A left-press on a token in Select mode grabs it
(`UiState::token_drag_candidate`); the release free-moves it to the tile under
the cursor (`UiState::drag_move_token`, the same `TokenMoved` path a click
uses, so replication and undo are unchanged; occupied and out-of-bounds
releases are no-ops). Host wiring lives in the MouseInput down/up arms, using
the existing `tile_at_cursor`. Done at the host level, not via `on_pointer`,
because `on_pointer` reports element-local coords and a board drag needs the
target *tile*, which the host already hit-tests. Verified by unit tests (13
views tests green): move, occupancy, out-of-bounds, undo, Remote-routing, and
Select-only candidate detection. The in-app synthetic drag was not driven to a
capture: synthetic OS pointer input proved unreliable here (intermittent
delivery plus a coordinate-click hazard), so the thin host wiring is inspected
rather than captured. **Deferred:** drag-to-reorder turns (the click-toggle
in/out stays) and Play-mode gated drag (Play movement stays click-a-reach-
tile).

The original spec:

`on_pointer` (Down/Move/Up with pointer capture, local coords, element size)
already ships. Wire two drags:

- **Token drag-move:** in Select/Play, press-drag a token to a new tile,
  emitting the same move/place path a click currently does (so replication,
  turn-gating, and undo are unchanged). Play mode still honors the reach/turn
  gate.
- **Drag-to-reorder turns:** replace the click-toggle in/out on the turn
  list with press-drag reorder (the bootstrap deferred drag reorder as I5
  residue). Reorder emits the existing `TurnSetOrder` event.

**Done when:** dragging a token relocates it (replicated and undoable, gate
respected in Play); dragging a turn row reorders initiative. Receipt: a
before/after capture of each.

## Phase W4: overlay chrome

**Landed 2026-07-07 as the token context menu; tooltip deferred.**
Right-clicking a token opens a menu at the cursor: `UiState::open_context_menu`
selects the token and stores its pane position, and the host's right
MouseInput arm resolves the token via `tile_at_cursor`. The menu is an
absolutely-positioned card (the sheet-overlay pattern, `context_menu_overlay`
in `board.rs`, `.context-menu` CSS) titled by the token, with Sheet / End turn
/ Remove / Close; each action reuses an existing method plus
`close_context_menu`, and a left-click off the menu dismisses it.
`remove_token` drops the token from the map, turn order, and selection
(replicated, undoable). Verified: the menu-state and remove logic by unit test
(14 views tests green), and the render by a seeded-state capture
(testing/isometry/images/2026-07-07_isometry_w4_context_menu.png), since
synthetic right-click input was as unreliable here as the W3 drag. Placement
is a plain cursor anchor, not `overlay_at`/`anchor_point_clamped`; edge-clamp
is a follow-on. **Deferred:** the hover tooltip and tile (non-token) menus.

The original spec:

`overlay_at` + `anchor_point_clamped` (overflow-aware flip and clamp) already
ship. Add:

- **Tooltips:** hover a token or tile to show a small info popup (name,
  owner, elevation, or sheet summary if bound).
- **Context menu:** right-click a token or tile to open a radial or list
  menu with at least the common actions (end turn, open sheet, remove token,
  set facing), placed with the clamp so it never leaves the pane.

**Done when:** hovering a token shows a tooltip; right-clicking a token opens
a context menu with at least one action that works end to end. Receipt: a
capture of each.

## Follow-ons (noted, not in this lane's done-conditions)

These are larger and tracked here so they are not lost, but they are not
gating this lane:

- **Path-B VFX surface.** xilem-serval `external_texture` + netrender
  `install_external_texture`/`DrawExternalTexture` already pass e2e (the
  meerkat orrery pattern). A host-side texture producer would unlock
  particle/spell VFX, water shimmer, palette-cycle, and soft fog at any z.
  This is the highest-leverage juice, but it is a producer to build, not a
  wiring gap. Spin its own plan when VFX is prioritized.
- **Animation tick loop.** CSS transitions landed (RepaintOnly path) but the
  host is `ControlFlow::Wait` and pumps `tick_animations` only in networked
  sessions. A dedicated tick loop enables idle-bob and cursor-pulse.
- **Camera / animation interplay (probe during W1).** Windowing may leverage
  genet's CSS-transitions work (repos/genet/docs/2026-07-05_css_transitions_plan.md).
  Two angles: (a) tiles entering/leaving the windowed viewport fade or slide
  in via the RepaintOnly transition path rather than popping; (b) a camera
  pan animates a transform on the board container instead of re-emitting
  every tile per frame. Caveat from the audit: camera pan currently
  re-rasters the full viewport (the tile cache hashes world-space AABBs) and
  transitions ride the RepaintOnly path, so a transition-driven pan may still
  hit raster cost even if it avoids the view rebuild. Worth a quick probe in
  W1; the deeper smooth-pan fix remains the camera-offset composite below.
- **Camera-offset composite.** The one genet/netrender ask that would make
  smooth pan/zoom cheap (folds into open question B8: snap-pan vs smooth).
- **Layered HUD polish** using the confirmed backdrop-filter/box-shadow/z
  stack.

## Findings

- 2026-07-07 (audit basis): the four ceiling/capability facts this plan acts
  on were verified in the landscape pass against the actual code. Windowing
  gap: `ground_tiles`/`prop_tiles` iterate the whole grid; the rebuild-per-
  update model is the defining render constraint (only engine-side
  hover/focus restyles escape a full rebuild). Wheel gap: no MouseWheel arm
  in the host window match. Drag and overlay primitives (`on_pointer`,
  `overlay_at`, `anchor_point_clamped`) are present and app-consumable. See
  the landscape doc section 6 and the workflow transcript for file:line.

## Progress

- 2026-07-07 (W1 landed): viewport windowing. `UiState` gains a `viewport`
  field (pane logical px); the genet host seeds it from the window size and
  keeps it current on resize; `board.rs` culls both whole-grid emitters
  (`ground_tiles`, `prop_tiles`) through `in_view`, with generous asymmetric
  margins so elevation columns and standing sprites are never clipped, and an
  "emit all" fallback until the host reports a size. `synth_map` is now
  parametric (`ISOMETRY_SYNTH=<n>` for an n x n stress board), and a
  profile-gated line reports the emitted element count. Verified: emission is
  bounded by the viewport, not the board. A size sweep at the default camera
  gave 60x60 -> 5361 elements, 100x100 -> 4249, 200x200 -> 3718 (a 200x200
  board of 40k tiles emits fewer elements and costs less than a 60x60, so
  cost is O(viewport) not O(board); before windowing a 200x200 would emit
  ~50k+ and take seconds). The 24x24 demo renders pixel-identical (nothing
  visible clipped); 100x100 windows to a correct pane-full region. Receipts:
  scry-shots/2026-07-07_isometry_w1_{demo,100}.png. Note: the absolute cost
  of a full dense pane (~4-5k elements, ~80ms release) is a separate raster
  concern (the camera-offset composite and tile caching in the follow-ons),
  not a scaling one.
