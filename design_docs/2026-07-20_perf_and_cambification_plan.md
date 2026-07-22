# Performance and cambification

**Date:** 2026-07-20
**Status:** ACTIVE. The performance regressions found by the audit are FIXED
(this session); the cambification lanes are proposals awaiting a go.
**Trigger:** Mark reported "an odd lag that wasn't present a few days ago" and
asked for an audit: duplications/inefficiencies lowering fps, plus candidates
for cambification (promotion into the Cambium catalog, or obviation by adopting
what Cambium already does better).

## The lag: what the audit found

The timeline pointed at the exploration-mode wiring (07-16..19) and the
sprigging paint-leaf pipeline (07-19). Both held regressions. Severity order:

### Fixed this session

1. **The overmap swatch was built every frame, even with the surface closed.**
   `redraw()` evaluated `overmap_swatch(state)` as a match scrutinee *before*
   the `overmap_open` guard, so every frame paid the world->graph projection
   (`overmap_for`: clones every place and route) plus the force-directed layout
   (120 iterations x O(n^2)) for nothing. Combat beats redraw at ~60fps, so a
   session with a real campaign world paid this continuously during any
   animation. Scales with world size, which is why the demo barely shows it and
   a real session does. Fix: the swatch is only built while the surface is open.

2. **The painted leaf was re-registered every frame while the overmap was
   open, defeating the leaf-tier retention gate.** A fresh `GraphCanvas` is
   born `dirty`, so inserting a new one per frame made `render_into` repaint
   the leaf every frame -- the exact cost the retained design exists to avoid.
   Fix: the App keeps `last_overmap_swatch` and re-registers only when the
   swatch model actually changed (`GraphCanvasSwatch: PartialEq`).

3. **`custom_leaf_boxes()` walked the whole box tree every frame** to size
   leaves, even when no leaf was registered (the ordinary board frame). Fix:
   the walk and `render_into` run only while a leaf is live.

4. **Four full `CampaignWorld` clones per dispatch.** The pump tail runs after
   every click/key/drag-step, and `pump_overmap`, `pump_overmap_read`,
   `pump_storylets`, and `pump_faction_turn` all cloned `state.world` *before*
   checking whether they had anything to do. `pump_faction_turn` was worst: its
   only pre-clone gate was `can_edit`, which is always true for a solo host, so
   the clone ran on literally every dispatch. `after_dispatch` also cloned the
   journal unconditionally. A `CampaignWorld` is BTreeMaps of Strings
   throughout (places, routes, characters, factions, storylets, party state),
   so these clones are allocation storms that scale with the campaign. Fix:
   every pump reads its cheap request flags first and clones only for a live
   request; `after_dispatch` checks `save_requested || load_requested` before
   touching the journal. (`pump_overmap_orders` and `pump_sheets` already had
   the correct shape.)

### Noted, not yet fixed (candidates, in value order)

5. **`emit_host_event` per event makes reveal bursts O(N) full round-trips.**
   A map read revealing N places emits N `NodeRevealed` events, each paying
   snapshot clone -> `HostSession::with_history` -> `apply_snapshot` -> fog +
   reach recompute. A batched `emit_host_events(Vec<GameEvent>)` (one session,
   one apply) is the fix. Bounded bursts today, so noted rather than fixed.
6. **The storylet surface refreshes every row on every dispatch while open**
   (world clone + `resolve_storylet` per storylet). Refresh-on-change (compare
   a world revision, or only on request) is the upgrade.
7. **While the overmap is open, the force layout runs per redraw** (host
   swatch build) **and per view rebuild** (`overmap_overlay`). At tens of
   places this is microseconds and fine; if worlds reach hundreds of places,
   memoize `Overmap::layout` keyed on topology (node + edge ids), not per call.
8. **Hover crossings over `on_hover` elements rebuild the view** (the
   dispatch-driven rebuild is how Cambium hover works). Bounded today: only
   the overmap's node targets register `on_hover`. Worth remembering when
   adopting hover-rich components.
9. **`UiState.messages` grows unbounded** (renders only the last 5, but the
   Vec never truncates). Cap it like `ROLL_LOG_CAP`.

### The structural ceiling (pre-existing, not the new lag)

`after_dispatch` requests a redraw unconditionally, every redraw re-emits and
re-translates the full paint list, and every state update rebuilds the full
view. Measured at demo scale (debug build): median animation frame ~18ms,
restyle-burst frames 100-220ms, boot cascade ~190ms. These predate the lag
report and are the baseline to attack only if play at real scale needs it
(dirty-region emit, memoized subtrees, release builds for play sessions).

### Measurement notes

Debug build, demo board. Combat-selftest steady state: median ~18ms scene,
run-to-run averages 41-51ms (noise from restyle bursts; the before/after delta
of the fixes is below this noise floor at demo scale because the fixed costs
scale with world size, which the demo lacks). The mechanisms above were each
verified by reading the actual call paths; the fixes hold under the full test
suite (core 58, campaign 28, net 42, system 48, views 34 green).

## Cambification

Already adopted: `data_grid` (compendium), `summary_body` (downtime),
`graph_canvas_swatch` + the sprigging leaf pipeline (overmap), `on_wheel`.

### Obviation lane: adopt Cambium where it already does it better

| Isometry hand-roll | Cambium component | Notes |
| --- | --- | --- |
| `widgets::tab_strip` (compendium nav) | `tab_bar` / `tabs::tab_strip` | Name-collides with the catalog component today; the catalog one adds keyboard activation (`TabActivation`) and ARIA. |
| Mode row, pace row, stance row | `segmented_control` | Exact shape (one-of-N choice row); pace/stance pickers and the Select/Paint/... mode row are three consumers at once. |
| Context menu (`board.rs::context_menu_overlay` + host-side outside-click dismissal in `main.rs`) | `command_menu` | Brings Escape dismissal, outside-click, disabled-with-reason rows, submenus; deletes the hand-rolled dismissal branch in the winit host. |
| `search_field` (display-only) + the `>` command line + the whisper composer (all host-routed key capture) | `caret_text_field` / `styled_field` | Real caret editing replaces three bespoke key-capture lanes in `genet::key()`. Biggest UX upgrade of the lane. |
| `record_card` + `stat_row`/`stat_list` | `summary_body` (title/eyebrow/facts) or `detail_panel` (`DetailRow`/`DetailSection`) | The facts vec is exactly the stat-list shape; one of the two components covers each consumer. |
| Turn list / roll log / messages panes | `sectioned_list` | Moderate value; brings selection + row kinds. |
| `overlay_panel` | keep the layout, adopt `overlay_surface` semantics | The catalog surface owns Escape/outside-click/roles; isometry surfaces currently hand-roll or lack dismissal. |
| Hand-copied `.graph-canvas-swatch*` CSS in `theme.rs` | `GRAPH_CANVAS_SWATCH_CSS` | Adopt the exported structural constant; keep only palette overrides host-side. |

### Shared projection and catalog lanes

1. **`Overmap::layout` moves through Scenograph P4, not Sprigging.** The
   2026-07-21 projection proofs plan makes Isometry the second portable-scene
   consumer and explicitly deletes this hand-rolled force layout. Sprigging
   remains the retained paint-leaf layer; it does not grow a competing placement
   engine.
2. **Visible node labels landed in `graph_canvas_swatch`.** Cambium 0.3.0 ships
   `with_node_labels` and `with_expand`; Isometry can now delete its duplicate
   label projection and the hidden no-op Expand route as a local adoption slice.

**Constraint:** isometry's committed manifest pins published `cambium = 0.3.0`
and `sprigging = 0.2.0` (local checkouts only override via the gitignored
`.cargo/config.toml`). The release and Isometry bump landed 2026-07-22. Future
catalog promotions follow the same order: land upstream, release, then adopt.
Never commit an Isometry build that needs unpublished catalog API.

### File-size note

`isometry-views/src/state.rs` is ~3.1k lines and `isometry-genet/src/main.rs`
~3.9k. No stated ceiling in this repo, but both are past the point where the
mere-style split (per-surface state modules; host concerns out of main) would
pay for itself. Candidate seams: overmap/story/downtime/generator state blocks;
the pump family; the selftest family.

## Done conditions

- [x] Lag mechanisms identified with call-path evidence (items 1-4) and fixed.
- [x] Full test suite green after the fixes.
- [ ] Batched `emit_host_events` for reveal bursts (item 5).
- [ ] Storylet refresh-on-change (item 6).
- [ ] Obviation lane: adopt `segmented_control`, `tab_bar`, `command_menu`,
      `caret_text_field`, `GRAPH_CANVAS_SWATCH_CSS` (each its own small PR-
      sized change, in that order of value).
- [ ] Projection lane: consume the Scenograph scene contract in P4 and delete
      `Overmap::layout`.
- [ ] Catalog lane: adopt Cambium 0.3.0 node labels and remove Isometry's
      duplicate label projection and no-op Expand route.

## Progress

- **2026-07-22:** Sprigging 0.2.0, Cambium 0.3.0, and Cambium Nematic 0.3.0
  published; Isometry bumped to the published Cambium/Sprigging pair. Cambium
  Winit 0.3.0 remains source-only until `genet-layout` has a standalone
  crates.io release.
