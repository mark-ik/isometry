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

*Items 5, 6, and 9 were fixed on 2026-07-24; see Progress. Kept as written for
the record of what was found.*

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

## Findings

### 2026-07-24: the scaled receipt

The demo-scale caveat above is now retired. `isometry_views::synth_world`
(`crates/isometry-views/src/demo.rs`) builds a campaign at real size -- sites on
a sparse grid, one notable per site, factions, laws, and storylets in a mix that
casts early, casts late, and never casts -- and
`crates/isometry-views/tests/scaled_world.rs` measures against it.

Release build, median of nine batches, storylets at half the place count:

| places | storylets | world clone | swatch build | gate compare | storylet refresh |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 5 | 2 | 7.5 us | 4.7 us | 0.10 us | 0.8 us |
| 50 | 25 | 73 us | 58 us | 1.0 us | 4.8 us |
| 150 | 75 | 273 us | 250 us | 3.6 us | 13.9 us |
| 400 | 200 | 623 us | 1050 us | 11 us | 42 us |

What the columns settle:

- **The four request-flag gates (item 4) avoid ~2.5ms per dispatch** at 400
  places: four clones at ~623us each, on every click, key, and drag-step.
- **The `overmap_open` guard (item 1) avoids ~1.05ms per frame.** At 60fps that
  is ~6% of the frame budget, paid continuously, for a surface that is closed.
- **The swatch-changed gate (item 2) costs ~11us to avoid that ~1050us**, about
  100x, and that is its *worst* case: the compare is measured against an equal
  swatch, which is the unchanged-frame steady state and the one path with no
  early exit. An earlier draft compared against a differing swatch, exited on the
  first node, and reported 0.01us; that number flattered the gate by three
  orders of magnitude and is not the one to quote.
- **Storylet refresh (item 6, still open) costs ~42us per dispatch** while the
  surface is open. Real, an order of magnitude under the clones, and the reason
  item 6 ranks below item 5 rather than above it.

Cost is superlinear in places (2.7x places from 150 to 400 gives 4.2x swatch
build), so these are floor figures for a campaign that outgrows 400 sites.

The receipt is `#[ignore]`d and needs `--release`; debug timings are dominated
by unoptimized BTreeMap walks and misreport the ratios:

```sh
cargo test -p isometry-views --release --test scaled_world -- --ignored --nocapture
```

Six sibling tests in that file always run, and they are the more durable half.
They assert the swatch-changed gate is *sound* rather than fast: an unchanged
world must rebuild an equal swatch (or the gate silently stops gating and the
per-frame repaint returns unannounced), and a moved party, a revealed place, or
a hover must each change it (or the overmap paints a stale map). Layout
determinism is what makes the whole retention design work, and nothing else in
the suite pins it.

### 2026-07-24: the storylet clone, not the storylet resolve

Item 6 named two costs, "world clone + `resolve_storylet` per storylet", and
proposed a fix for the second. The receipt says the first was 15x larger:

| | before | after |
| --- | ---: | ---: |
| world clone | 686 us | gone |
| storylet resolve | 47 us | gone while unchanged |
| inputs compare | -- | 45 us |
| **per dispatch, surface open** | **733 us** | **45 us** |

Gating only the resolve while keeping the clone would have saved ~6%. The clone
was also unnecessary: `pump_storylets` cloned the world up front for both its
paths, and the play path only ever asked `world.storylets.is_empty()`. The rows
build fine from a borrow. Removing it and gating on the inputs is ~16x.

Two shape choices worth keeping:

- **Compare the inputs, not a revision counter.** A counter is a new invariant
  every mutation site must honor, and one missed bump leaves the surface
  silently stale -- a worse failure than the cost it saves. Comparing what the
  rows were built from cannot go stale.
- **The row builder is now a free, pure `storylet_rows(&world, &secret_ids)`.**
  The gate is sound only if the rows depend on nothing but its cached inputs, so
  the builder takes exactly those and nothing else. Adding a third input will
  not compile past the call site, which is a better guarantee than a comment.

### 2026-07-24: Cambium node labels cannot land yet

The catalog lane called this "a clean delete-and-adopt". Half of it is: the
Expand chip is now off at the model (`with_expand(false)`) instead of hidden
with `display: none`, which had left a tabbable, screen-reader-announced button
wired to a closure that did nothing. `GRAPH_CANVAS_SWATCH_CSS` is adopted, with
Isometry keeping colors plus the 4px radius its chrome uses.

The label half cannot. `graph_canvas_swatch` renders every visible label with
one flat class:

```rust
// cambium 0.3.0 and 0.3.1, graph_canvas.rs -- illustrative excerpt
el("span", node.label.clone())
    .attr("class", "graph-canvas-swatch-label")
```

Isometry's overlay carries three states (`overmap-label-here` green and bold,
`overmap-label-hover`, plain), and on a pointcrawl the party's location is the
label layer's whole job. The `selected` / `hovered` classes the swatch does emit
go on the node *buttons*, which sit in a different container, so no CSS
relationship reaches the labels. Verified against both the published 0.3.0 and
the local 0.3.1 source, so this is not a version-skew artifact.

Adopting as-is would be a visible regression. The order is the one this plan
already sets: land per-node label state classes upstream, release, then adopt.
Until then the overlay stays.

### 2026-07-24: the obviation lane needs a state bridge decided first

Scoped before starting, because it is larger than "swap the call". The catalog's
selection components are typed over Cambium's own state, not the consumer's:

```rust
// cambium 0.3.0, selection_bar.rs -- illustrative
pub fn segmented_control(
    state: &SelectionState,
    items: &[SelectionItem],
) -> impl View<SelectionState, (), GenetCtx, Element = GenetElement>
```

Isometry's rows are `View<UiState, ()>` and act directly (`clickable` -> a
request flag the host pumps). Adopting therefore needs three things per surface,
not one:

1. A `SelectionState` field on `UiState` per row (mode, pace, stance).
2. `lens` to project `UiState -> SelectionState`. Cambium re-exports `lens`,
   `map_state`, and `map_action` from xilem_core, so the adapter exists.
3. **A bridge from the component's state change back to a domain action.** The
   component moves `SelectionState.selected`; it does not know that pace is a
   replicated world event. Something has to notice and dispatch.

Point 3 is the same question for all three rows, so it was answered once rather
than three times.

**RULED 2026-07-24 (Mark): pump-side compare.** `SelectionState.selected` is a
request the host reconciles each dispatch, the shape `pump_overmap` and the
storylet gate already use. One convention across the codebase beats a second
event path. The alternative considered and declined was `map_action` (the
segment emitting a domain action directly): closer to the component's intent,
but it would run alongside the request-flag convention rather than replacing it.

Each pump reads its cheap flag before doing anything, per the item-4 fix.

The same question governs `command_menu` (dismissal state) and
`caret_text_field` (a `TextInput` replacing three bespoke key-capture lanes in
`genet::key()`, which is also where the focus and key-routing done-conditions
live). Deciding the bridge once unblocks all of them; deciding it per surface is
how the lane ends up with three conventions.

### 2026-07-24: what actually blocks the node-label adoption

The code blocker is gone. Cambium's `graph_canvas_swatch` now puts the same
`selected` / `focused` / `hovered` modifiers on a visible label that it already
put on the node button, with a test that fails if a label stops carrying node
state. That is the whole reason Isometry was hand-rolling its own label layer.

What remains is **one release step, and it is not Claude's to take**:

1. ~~Teach the swatch to emit per-node label state.~~ Done, cambium 0.3.2.
2. **Publish cambium 0.3.2 to crates.io.** 0.3.1 is already published, so the
   next version is the vehicle. This is an outward-facing, irreversible
   action and needs Mark.
3. Delete Isometry's `overmap-label*` overlay and switch to
   `with_node_labels(true)`. No manifest change is needed: Isometry's
   `cambium = "0.3.0"` is a caret range, so a published 0.3.2 is picked up.

Step 3 is deliberately *not* done yet, and not merely out of caution. Isometry
builds locally against the path-overridden cambium, so the adoption would
compile and look correct here while a clean checkout resolved to the published
0.3.1, whose labels have no state classes. That failure is silent: the labels
render, just without the "here" emphasis. A build error would be safer than
what adopting early actually produces, which is why the plan's land-release-
adopt order exists.

### 2026-07-24: the selection bridge, built and proven

The mode, pace, and stance rows are now one `segmented_control` each, on the
ruled pump-side compare. The shape that made it work:

- **The rows are a mirror, never the truth.** Truth stays in `ui.mode` and in
  the world. `UiState::sync_selection_rows` pushes it into the rows wherever it
  can change (`apply_snapshot`, opening a surface), so by the time
  `pump_selection_rows` runs, a disagreement can only be the user having moved
  a control. That one-way push is what makes a two-way binding tractable;
  without it the compare cannot tell which side moved and fires spuriously
  every dispatch.
- **Order tables are the single source of row order.** `PACE_PCTS` and
  `STANCE_KEYS` are indexed by the selection index, so the row's order and its
  meaning cannot drift apart in separate literals.
- **Mode commits locally, pace and stance only ask.** Mode is view state.
  Pace and stance are world state, so the bridge sets the existing request
  flags and the adjudicated host path is unchanged. A catalog component did not
  get to shortcut the authority.

`SelectionState::select_one` is private in Cambium, so the sync writes the
public `active` / `selected` fields; rebuilding the state instead would drop
`focus_active` mid-keyboard-interaction. Worth a `pub fn set_selection` upstream
if a third consumer wants it.

### 2026-07-24: the leaf-gate generalization is not yet worth writing

Scoped and dropped. `OVERMAP_LEAF_KEY` is the only key ever inserted into the
host's `LeafRegistry` (`main.rs:433`), so an "any registered leaf changed?"
rewrite would generalize a fan-out with one member and no way to exercise the
other paths. The second leaf is real but unscheduled (`appearance_preview`,
chisel Path-B, in the campaign-packs plan). Revisit when it lands; the current
gate is ~25 lines and reads clearly.

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
| ~~Hand-copied `.graph-canvas-swatch*` CSS in `theme.rs`~~ | `GRAPH_CANVAS_SWATCH_CSS` | **Adopted 2026-07-24.** Structure from the constant, `GRAPH_CANVAS_SWATCH_PALETTE` keeps colors plus the 4px radius the surrounding chrome uses. |

### Shared projection and catalog lanes

1. **`Overmap::layout` moves through Scenograph P4, not Sprigging.** The
   2026-07-21 projection proofs plan makes Isometry the second portable-scene
   consumer and explicitly deletes this hand-rolled force layout. Sprigging
   remains the retained paint-leaf layer; it does not grow a competing placement
   engine.
2. **Visible node labels landed in `graph_canvas_swatch`.** Cambium 0.3.0 ships
   `with_node_labels` and `with_expand`; Isometry can now delete its duplicate
   label projection and the hidden no-op Expand route as a local adoption slice.
   *Revised 2026-07-24: the Expand half landed, the label half is blocked on an
   upstream change. See Findings.*

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

**Done 2026-07-24.** Also corrected: the repo *does* state a ceiling, 600 LOC
per file in `CLAUDE.md`, and the note missed the second-largest file entirely
(`isometry-system/src/lib.rs`, 3672). All three are split:

| file | before | after | modules |
| --- | ---: | ---: | --- |
| `isometry-genet/src/main.rs` | 4012 | 500 | `render`, `input`, `dispatch`, `host`, `sheets`, `adjudicate`, `storylets`, `overmap`, `generators`, `selftest`, `catalog` |
| `isometry-views/src/state.rs` | 3161 | 377 | `state/{rows,surfaces,play,session,interaction,tests}` |
| `isometry-system/src/lib.rs` | 3672 | 413 | `sys/{system_core,system_actions,generator,lua_read,lua_write,srd}`, `tests` |

Mechanical: no behavior changed, and the split is verifiable as such because the
suite is untouched and green (`--all-features`, 18 targets). Method bodies moved
into `impl` blocks in sibling modules, which needed `pub(crate)` on items that
now cross a module boundary and were previously private within one file.

Every non-test source file is now under the ceiling. What remains over it:

| file | lines | note |
| --- | ---: | --- |
| `isometry-net/tests/replication.rs` | 1970 | test file |
| `isometry-net/src/session.rs` | 1325 | **not yet split**, next candidate |
| `isometry-system/src/tests.rs` | 1285 | test file |
| `isometry-net/src/campaign_space.rs` | 1271 | **not yet split** |
| `isometry-campaign/src/world.rs` | 860 | **not yet split** |
| `isometry-views/src/state/tests.rs` | 768 | test file |

The test files were moved wholesale rather than distributed across the new
modules; splitting them follows the code they cover, and is worth doing when a
module's tests are what someone is actually reading.

## Done conditions

- [x] Lag mechanisms identified with call-path evidence (items 1-4) and fixed.
- [x] Full test suite green after the fixes.
- [x] Scaled receipt: a synthetic campaign world, gate-soundness tests that
      always run, and measured figures for items 1, 2, 4, and 6 (2026-07-24).
- [x] Batched `emit_host_events` for reveal bursts (item 5, 2026-07-24).
- [x] Storylet refresh-on-change (item 6, 2026-07-24) -- and the world clone it
      turned out to be hiding, which was the larger half.
- [x] `UiState.messages` capped at `MESSAGES_CAP` (item 9, 2026-07-24).
- [x] Adopt `GRAPH_CANVAS_SWATCH_CSS` and drop the no-op Expand route
      (2026-07-24).
- [x] Split the three oversized files (2026-07-24); see the file-size note.
- [x] Obviation lane, `segmented_control`: the mode, pace, and stance rows
      migrated on the ruled pump-side bridge (2026-07-24).
- [ ] Obviation lane, remaining: `tab_bar`, `command_menu`, `caret_text_field`.
      The bridge is settled and proven, so these are now mechanical.
- [x] Split `isometry-net/src/session.rs`, `campaign_space.rs`, and
      `isometry-campaign/src/world.rs` (2026-07-24). Every non-test source file
      in the repo is now under the 600-LOC ceiling.
- [x] Projection lane: consume the Scenograph scene contract in P4 and delete
      `Overmap::layout`.
- [x] Catalog lane, upstream half: Cambium's visible labels now carry the
      node's `selected` / `focused` / `hovered` state (2026-07-24, cambium
      0.3.2, 142 tests green).
- [ ] Catalog lane, adoption: **blocked on publishing cambium 0.3.2** to
      crates.io. Nothing else stands in the way; see Findings.

## Progress

- **2026-07-22:** Sprigging 0.2.0, Cambium 0.3.0, and Cambium Nematic 0.3.0
  published; Isometry bumped to the published Cambium/Sprigging pair. Cambium
  Winit 0.3.0 remains source-only until `genet-layout` has a standalone
  crates.io release.
- **2026-07-24:** Scaled receipt landed (`synth_world` + `tests/scaled_world.rs`);
  see Findings. Items 1, 2, and 4 now have measured figures instead of
  call-path argument alone, and item 6 has a size. The leaf-gate generalization
  was scoped and dropped for want of a second leaf.
- **2026-07-24:** Items 5, 6, and 9 fixed. `emit_host_events(Vec<GameEvent>)`
  applies a burst to one `HostSession` for one fog + reach recompute, with
  `emit_host_event` delegating to it so no call site changed; the map read is
  its first consumer. `pump_storylets` lost its world clone entirely and gates
  the rebuild on a `(world, secret_ids)` compare through the new pure
  `storylet_rows`. `UiState::push_message` caps the whisper log at
  `MESSAGES_CAP` (50).
- **2026-07-24:** The three oversized files split (see the file-size note):
  `main.rs` 4012 -> 500, `state.rs` 3161 -> 377, `isometry-system/src/lib.rs`
  3672 -> 413. Mechanical, suite unchanged and green. The obviation lane's
  state-bridge question was scoped and ruled (pump-side compare) before any
  surface was migrated.
- **2026-07-24:** `segmented_control` adopted for the mode, pace, and stance
  rows, with `sync_selection_rows` + `pump_selection_rows` as the bridge and
  tests pinning the mirror. `campaign_space.rs` and `world.rs` split; every
  non-test source file is now under the ceiling. Cambium 0.3.2 teaches visible
  labels their node state, which leaves only the publish step between here and
  the label adoption. Suite green at `--all-features`, 18 targets.
- **2026-07-24:** Catalog lane half-landed: `with_expand(false)` and
  `GRAPH_CANVAS_SWATCH_CSS` adopted, node labels blocked upstream. Suite green
  at `--all-features`: core 56, campaign 28, net 10 + 42, system 48, views 40 +
  6, voxel 7, genet 5, graphshell 2 (244 total, plus the ignored receipt). The
  "core 58" in the measurement notes above was already stale when quoted.
