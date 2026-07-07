# Isometry Next-Horizons Landscape

**Date:** 2026-07-07
**Status:** landscape synthesis (pre-plan). Grounds the six next-horizon
lanes after bootstrap I0-I6. Findings come from a multi-agent research +
codebase-audit pass (web prior-art plus in-repo verification with
file:line citations); load-bearing factual strands (SRD licensing, serval
capabilities) were adversarially verified. Where verification corrected a
survey claim, the correction is used. This doc is reference and decision
substrate; commitments live in the per-lane plans it sequences.

## Purpose

The bootstrap arc is done: editor, hot-seat play, iroh sessions, fog,
table furniture, and system plugins all landed. This maps what comes
next across six lanes Mark named (Lua docs, map scales + traversal,
isometry-web, rulesets/packs, generators/commands, serval GUI), states
the real constraints, and recommends a build order.

## 1. Lua plugin documentation

A system plugin today is compiled Rust (`srd_5e()`) that builds a
`SheetSchema` plus an inlined Lua script. An author-facing guide has to
teach four things in order, because each is a wall an author hits:

1. **The schema model.** A system supplies declared fields, `DerivedDef`s,
   and `ActionDef`s. `FieldValue` is `Int | Text | Bool` only.
   `default_sheet` inserts exactly the declared fields, and the sheet view
   iterates them. Consequence: everything is a pre-declared flat scalar.
   A collection (skills, conditions, inventory) has to be exploded into
   many flat keys named ahead of time.
2. **Derived and action functions.** These are Lua that the host runs via
   `call_int`: it executes `func(character)` and reads back one `i64`.
   `action_expr` returns `"{base}{bonus:+}"` (a hardcoded base die plus the
   single signed int Lua returned, e.g. `1d20+7`). An action yields one
   to-hit-style line for the table to read; damage dice, a second coupled
   roll, and pass/fail outcomes are not expressible yet.
3. **The int-only Rust/Lua boundary (the load-bearing constraint).** The
   host marshals one `character` table (the roller's own flat sheet) in
   and reads one integer out. There is no target actor, no equipment
   sub-table, no map context. So no derived value can depend on gear, no
   action can resolve against a defender's AC/DC, and nothing returns a
   string or bool. What is expressible: integer arithmetic over declared
   int fields, with lookup tables held as Lua literals in the script
   (proficiency math, ability-mod floor-division, descending-AC matrices
   all fit).
4. **The piccolo posture.** piccolo 0.3 as `Lua::core` (sandboxed,
   pure-Rust, wasm-friendly, no I/O). Marshalling is owned-copy per call,
   so scripts hold no cross-call state and mutate no host data. Nested
   access (`character.equipped.armor`) resolves to nil today. This sandbox
   is also what lets the same plugin run unchanged in the browser (see 3).

**Home decision (small spike):** whether author docs are a `design_docs/`
file, a crate-shipped `docs/` guide, or generated from the schema. Settle
it and write the guide against the *widened* ABI (see 4), so it is written
once rather than rewritten after the ABI grows.

## 2. Map editor scales + traversal

### Scale model

Every mainstream VTT (Foundry Scenes, Roll20 Pages, Owlbear Scenes,
TaleSpire Boards) and every GBA-tactics reference (FFT/FFTA world nodes,
Tactics Ogre map-dots, FE chapter select) models scale tiers as discrete
separate documents you swap between, never a seamless zoom from battle to
overworld. The overworld is a navigation layer; a node triggers a hard
swap into a separate battle document. Adopt that.

| Tier | Structure | Ceiling | Renderer |
| --- | --- | --- | --- |
| LOCAL (battle) | One `MapDocument`, as today | ~30x30-40x40 today (see below) | Existing iso pipeline |
| REGION (town/dungeon) | Another `MapDocument` at coarser tile scale, genuinely isometric | Same element budget; slice-and-swap beyond it | Existing iso pipeline |
| WORLD (travel) | A waypoint graph (pointcrawl): nodes = sites, edges = weighted/directed routes. Not a tile grid | No tile ceiling | New; thin non-tile view |

REGION as a normal `MapDocument` inherits the editor, fog, tokens,
replication, depth-sort, and native-isometric look for free, and keeps the
substrate/system split pure. Highest-leverage structural move.

### The honest tile/token ceiling today

The ceiling is the renderer, not the model. The model is cheap (~5
bytes/tile; a 1000x1000 map is ~5 MB). But viewport windowing was designed
in the bootstrap (decision #3) and never built: `ground_tiles`/`prop_tiles`
iterate the entire grid every update, so live DOM count and per-update CPU
are O(scene), and a full rebuild happens on every edit and every pan.

Measured P2 receipts (release, 1100x720):

- 24x24 demo (~650 elements): pan ~4.3 ms scene + ~1.3 ms raster.
- 30x30x3 + 20 tokens (~2,700 elements): first frame 27.6 ms scene + 6.2 ms
  raster; pan ~16 ms scene + ~3.7 ms raster. Debug is ~10x release.

Scene cost is ~6 us/element. So the smooth-pan interactive ceiling is
~2,700-4,000 rendered elements, roughly the 30x30-40x40 design center.
Elevation multiplies element count (each raised tile emits up to 13 ground
diamonds), so terrain height often sets the budget before footprint does.
A 100x100 region would be ~30k elements and ~180 ms frames, unusable until
windowing lands. **Bottom line: the shipped ceiling is ~30x30-40x40, and
the bootstrap's "low thousands regardless" text describes the
designed-but-unbuilt windowing, not the shipped state.** Confirming the
exact cutoff at 60x60 and 100x100 under `ISOMETRY_PROFILE=1` is a cheap
spike.

### Transition points and multi-map campaigns

Model a transition point as a substrate primitive: a tile-kind class or
prop carrying a target-map reference (Foundry Scene Regions, FFT world
nodes, TaleSpire board links). The container is the planned campaign pack,
extended with a scene-link table plus a DM board menu to jump directly
(the TaleSpire "up to 450 boards + board menu" model). The swap replicates
as one ordered host event; clients follow; the existing snapshot+tail
covers late join. This gives multi-locale campaigns without a WORLD graph
and without new transport.

### World traversal (pointcrawl)

If a WORLD tier is wanted (see B), use a waypoint graph as a new
`isometry-core` module (pure geometry + turns, so it fits "keep core
pure"). Rationale from the survey: all overworld models reduce to a graph
of nodes + edges; a hexcrawl is a 6-regular graph and collapses to a
pointcrawl once players route rationally. Both aesthetic anchors implement
their world layer as a node-graph (FFTA places 24 location nodes; Tactics
Ogre uses selectable map-dots). Pathfinding reuses the battle grid's
machinery: `reachable`/`path_to` is already a weighted BFS over grid
neighbors, and generalizing "neighbor" from grid-adjacency to graph-edges
is the whole change. The substrate/system split holds exactly as elevation
does: the edge carries an abstract cost/time weight; what a travel tick
costs (5e forced-march, PF2e hexploration, getting-lost rolls) is
system-plugin business. World tempo is a small party-level tick counter,
not `TurnList` (which is initiative-shaped). Battle maps are authored per
node.

## 3. isometry-web (browser player client)

### Recommended architecture

A player-only browser client for v1: the DM stays native and owns
authority, Lua, file I/O, and inbound connections; the browser is a pure
replica (Snapshot + Applied tail in, Intent out). Package it as a
standalone workspace mirroring serval's `[patch.crates-io]` (patches do not
inherit through git deps, and wasm needs the per-target WebGPU netrender
manifest), exactly like `serval_web_smoke`.

### What is already proven

- **Serval renders in a browser today.** `serval_web_smoke` drives the full
  chain on wasm32/WebGPU (xilem-serval view to ScriptedDom to serval-layout
  to PaintList to netrender to canvas), PASS in Chrome 2026-07-04. Same
  pipeline `isometry-serval` uses natively. The three known walls (Instant,
  system fonts, RGBA8-vs-BGRA) are already fixed in the libraries.
- **The reusable stack is wasm-portable by construction.** `isometry-views`
  depends only on `isometry-core`, `isometry-net`, `xilem-serval`. A grep
  finds zero `std::fs`/`thread`/`Instant`/`SystemTime`/`std::net` in
  core/views/system. So `board_root`, `UiState`, all edit/play modes, the
  sheet overlay, dice, templates, and the 5e system cross unchanged.
- **The protocol seam is clean.** `HostSession`/`ClientSession` are pure
  synchronous state machines. A browser client reuses `ClientSession` + the
  postcard wire verbatim and supplies only a new pump.

### The transport decision and the one risk

Use the iroh-wasm browser/relay path, reusing the existing `NetMessage`
protocol and ticket flow. The DM-authority model is a hub (players talk
only to the host), so the browser needs one relayed bidirectional channel
to the DM, and iroh's relay path also solves DM-behind-NAT reachability.
matchbox/WebRTC gives browser-to-browser channels that are unneeded here; a
DM-run WebSocket relay reintroduces the reachability problem.

**Transport is the single load-bearing risk.** The current `iroh_link.rs`
is native-locked (tokio rt-multi-thread; wasm is single-threaded) and uses
direct-dial + loopback-rewrite; iroh-in-browser is relay-only (no UDP hole
punching from the sandbox). isometry-web needs a wasm `ClientNet` pumping
the same frames over a relay on a single-threaded executor. **This gates
the lane behind an iroh-wasm transport spike:** prove iroh 0.98 builds and
runs in-browser single-threaded and measure relay round-trip for the
Intent to Applied echo. Secondary: a browser client exposes the full
replicated `GameSnapshot` to devtools (fog is client-side render only),
acceptable under the friendly-table trust model but a wider surface that
deserves an explicit call.

## 4. Additional rulesets + campaign packs

### How many free rulesets exist

The verified open pool is deeper than Isometry needs: roughly 18-20
genuinely shippable rulesets. The binding constraint is authoring effort
and grid-fit, not legal availability.

| System | License | Grid fit | Notes |
| --- | --- | --- | --- |
| D&D 5.1 SRD | CC-BY-4.0 (irrevocable) | High | Re-extract attribution string from the official PDF (not machine-readable) |
| D&D 5.2.1 SRD (2024) | CC-BY-4.0 | High | Low-marginal-cost extension of the 5.1 pack |
| Pathfinder 2e Remaster | ORC (irrevocable) | Highest | No SRD download; extract from Core books. Archives of Nethys is NOT a redistribution source |
| Level Up A5E | CC-BY / OGL / ORC | High | No Reserved Material; best content-per-effort; use the CC-BY lane |
| Basic Roleplaying (BRP) | ORC (irrevocable) | Medium | Free ORC Content Document; clean non-d20 pack |
| Knave 2e | CC-BY-4.0 | Low-med | Cleaner than Cairn (BY, not BY-SA) |
| Basic Fantasy 4e | CC-BY-SA-4.0 | Low-med | ShareAlike copyleft; low authoring cost |
| Pathfinder 1e (PRD) | OGL 1.0a | Highest | 100% OGC in the curated PRD; crunch-heavy |
| Cepheus / Traveller SRD | OGL 1.0a | Medium | Non-fantasy grid tactics (deckplans) |
| Fate, Blades, Dungeon World | CC-BY | Gridless | Clean licenses, poor diamond-grid fit |

Legal flags to bank:

- **Shadowdark's 3PL explicitly bars video games and apps.** Reference-only
  at best; shipping its content through a VTT may fall outside the license.
- **OSE's 2026 update is not going CC-BY.** It re-bases on the 5.1 CC-BY
  SRD plus a bespoke compatibility license. Do not plan around an
  "OSE CC-BY SRD."
- **Draw Steel's Creator License is forward-amendable with
  grandfathering,** not freely revocable (a milder concern than the survey
  first read).

### Best first packs after 5e

1. **Pathfinder 2e Remaster (ORC)**: deepest grid-tactical showcase, forces
   the schema to generalize.
2. **D&D 5.2.1 (CC-BY)**: near-free once 5.1 exists, moves to 2024 rules.
3. **Level Up A5E (CC-BY)**: best content-per-effort.
4. **Basic Roleplaying (ORC)**: clean non-d20 proof the schema stretches.
5. **One rules-light OSR CC pack** (Knave 2e preferred over copyleft Cairn).

### What the schema must grow

Three structural walls block the named targets:

1. **`FieldValue` is a closed scalar set.** Add `List`, `Map`, and `Float`
   variants (the smallest change that makes the value recursive, keeps the
   opaque-sheet `SheetSet` replication intact, and lets the Lua marshaller
   recurse). Inventories, stacked conditions, and per-skill proficiency then
   have a home.
2. **The Lua ABI is int-in/int-out with only the roller's own sheet.**
   Widen to a tagged return (so an action can yield a dice expression) plus
   a richer context table (`self`, `target`, `conditions`, `equipped`,
   `encounter`). Target-awareness and equipment-derived AC are the two hard
   walls, and both need richer input.
3. **No item/condition concept and no action cost/target metadata.** PF2e's
   three-action economy is the one place the substrate genuinely must bend,
   toward turns: an optional per-turn action budget (default None so 5e is
   unaffected) plus cost/target/range metadata on `ActionDef`, plus a
   per-token condition list.

Also required for a multi-system world: turn systems into loadable assets.
Today only one compiled-in `srd_5e()` exists despite `SheetData.system`
already tagging each sheet. Path: a loader trait + registry now, a PF2e
skeleton as a second built-in to force generalization under pressure, then
a data-driven system-pack format (manifest + `.lua` files, with a
structured license/attribution field carrying the OGL/CC/ORC notice
programmatically) when packs land. Movement and senses are hardcoded
(`MOVE_BUDGET=5`, `SIGHT_RADIUS=6`); these must become system-driven for
any ruleset with per-creature speed.

## 5. Generators + command grammar

Grounded in the dice module (`roll(expr, rng)`, NdS+M, caps 100/1000), the
templates module (burst/line/cone Chebyshev), the replicated `roll_log`
(cap 50), the DM-authority event log, and the existing `>` composer
(`whisper_draft`) that already renders a command prompt.

### Prior art worth copying

- **Tracery** (Kate Compton): a grammar is a symbol to array-of-strings
  map; `#symbol#` is a recursive random pick; modifiers chain after a dot
  (`.capitalize`, `.a`, `.ed`); `[key:#value#]` saves state for reuse.
  Tracery has no native weighting (the documented hack is duplicating
  entries). A ~200-line design worth reimplementing in Lua, not depending
  on. `improv` and `bracery` add the weighting and tag-filtering Tracery
  lacks.
- **Weighted tables** are die-range to result rows, i.e. weighting as
  contiguous integer spans. The universal shape is a list of
  `(weight-or-range, value)` where value is text, a nested-table reference,
  or a dice expression. Maps 1:1 onto a Lua array of `{w=, v=}`.
- **Command surfaces:** Roll20 rolls tables inline as `[[1t[table]]]` with
  per-item weights; Foundry Roll Tables give each row a weight + range and
  allow Document/Compendium/nested-table results; Avrae's `!alias` grammar
  uses positional `%1%`/`%*%` args plus an `argparse()` flag automaton
  (`-rr 2 -b 1d4`). Perchance is the leanest weighted grammar: indented
  lists, `[listName]` references, `^N` weights.
- **Generators:** donjon's name generator is a Markov chain over culture
  keyed sample-name sets; watabou exports structured JSON/Markdown;
  Perchance composes weighted lists.
- **Dice DSL** to grow into: keep/drop `kh/kl/dh/dl` (advantage), exploding
  `!`/`x`, reroll `r`/`rr`/`ro`, success counting `cs`/`cf` with
  comparators.

### Proposed Isometry grammar

Reuse the `>` composer. Rule: if the first token after `>` is a known verb,
route to the dispatcher; otherwise it stays a whisper (backward
compatible).

```
>gen | >generate   run a generator / roll a table   -> text (+ optional token/sheet)
>roll | >r         dice expression                  -> RollRecord (existing path)
>find | >search    fuzzy-search pack tables/entries -> list
>query | >q        oracle / yes-no / single lookup
>spawn             instantiate a token from a result
>tpl | >template   place burst/line/cone            -> pure Rust, no Lua
```

Argument shape (Avrae-flavored, Perchance-lean):
`>VERB [COUNT] TARGET [key=value ...] [with FREEFORM...]`, where COUNT is an
int or dice string, TARGET is a dotted pack id (`npc`, `loot.hoard`),
`key=value` covers `at=3,4` / `@selected` / `seed=1234`, and `with FREEFORM`
feeds filter tags to the grammar (improv-style), so `>gen 3 npc with farmer`
selects `#backstory_farmer#` branches.

Packs are Lua data. Ship a tiny engine prelude (`pick` weighted-choice,
`expand` Tracery rewriter, modifiers) once so authors write tables, not an
interpreter. Weight is `w` (default 1); nested tables are `{sub="id"}`; a
grammar is nested arrays with `#sym#`.

### Result flow reuses the existing event path

Generation is host-authoritative and single-sided: the host runs Lua once,
and only the result crosses the wire (the same model dice.rs already uses,
so cross-peer Lua determinism is a non-issue). Text results append to the
shared log (widen `roll_log` to `Vec<LogEntry>` = `Roll | Gen`, using the
existing `#[serde(default)]` for a clean migration, plus a
`GameEvent::Generated`). Token results emit
`SessionEvent::TokenPlaced` + `SheetSet` (both exist, both invertible, so a
generated NPC is a normal undoable placement). `>gen` previews with
`[insert] [reroll] [discard]` rather than committing; `>reroll` re-runs at
`seed+1` and is free until confirmed.

### The one load-bearing ABI change

`>gen`/`>q`/`>find`/`>spawn` need a string/tagged return, so add one entry
point beside `call_int`: `call_gen(func, args) -> Option<GenValue>` where
`GenValue = Text | List | Npc{name,text,sheet} | Roll{expr}`. `>roll` and
`>tpl` need no Lua change. The host RNG into piccolo is the key spike:
prefer an entropy tape (draw a `Vec<u32>` from the session `Rng` in Rust,
pass it as a Lua table + cursor) over a stateful callback, because it fits
the current owned-data-only boundary and the result-crosses-the-wire model.
Cap recursion depth and total picks, and fuel-budget each `>gen` so a
pathological grammar cannot hang the host.

## 6. serval GUI capabilities

Grounded in the verified capability audit.

### What renders today (confirmed)

Absolute positioning + z-index depth-sort (full CSS 2.1 stacking); clip-path
polygon/circle/ellipse; `image-rendering: pixelated`; flexbox; overflow
clipping; the full CSS transform-list via stylo; opacity, mix-blend-mode,
filter chains, backdrop-filter; box-shadow; gradients; border-radius; parley
text with decoration/ellipsis/selection/caret, ~91% COLR emoji, variable
fonts. Interaction primitives are richer than the app uses: `on_pointer`
(drag with capture, local coords + element size), `on_wheel`, a DOM-order
focus model, the full native form-control set, and
`overlay_at` + `anchor_point_clamped` (overflow-aware flip + clamp popups).

### Feasibility

| GUI idea | Verdict | Basis |
| --- | --- | --- |
| Radial/context menus, tooltips, submenus | Feasible now | `overlay_at` + `anchor_point_clamped` + clip-path wedges |
| Layered HUD (blur, shadow, opacity) | Feasible now | backdrop-filter, box-shadow, z-index shipped |
| Real drag (token move, drag-reorder turns, drag props) | Feasible now | `on_pointer` capture; replaces the deferred click-toggle reorder |
| Wheel to scroll the side panel | Feasible now (1-line host wiring) | Engine has `scroll_at`; the host window match has no MouseWheel arm |
| Minimap, reach/path/template highlights, palette swaps, health bars, range rings | Feasible now | divs + border-radius/gradients/clip-path; meerkat already runs a minimap surface |
| Particle/spell VFX, water shimmer, palette-cycle, live vello scenes | Feasible now via Path-B texture (app-side wiring) | xilem-serval exports `external_texture` backed by netrender `install_external_texture` + `DrawExternalTexture`, passing e2e (meerkat's orrery pattern). Highest-leverage VFX unblock |
| Smooth pan/zoom | Costly-but-possible now | Playable at battle scale but re-rasters the full viewport until the camera-offset composite lands; zoom hits the transform-origin limits |
| Sprite/cursor animation (idle bob, pulse) | Costly; needs a host tick loop | CSS transitions landed (RepaintOnly path) but the host is `ControlFlow::Wait` and pumps `tick_animations` only in networked sessions |
| Soft/feathered fog, vignettes via `mask-image` | Blocked (small engine add) | serval-layout parses `mask-image` but never lowers it in paint_emit; a Path-B texture can do soft fog today |
| Custom vector gauges (arc gauges, waveform timelines) | Blocked on chisel Path-A | The tile-cached `DrawPath` lane needs the unwired xilem-serval `leaf()` view |

### The defining constraint

Every state change rebuilds the whole xilem view tree and re-emits the whole
scene; only engine-side `:hover`/`:focus` restyles (RepaintOnly, no rebuild)
and `memoize` escape it. This is why continuous animation and per-pixel drag
are costly and the animation policy is deliberately sparse.

## A. Recommended plan sequence

Ordered by dependency. "Now" = app-side, no engine or external gate.
"Spike-gated" = needs a proof first. "Fork-gated" = waits on a decision in
section B.

1. **Viewport windowing** (now). The highest-leverage fix. Iterate only the
   visible tile range in `ground_tiles`/`prop_tiles`. Blocks region-scale
   maps and browser maps. Build before any board larger than ~40x40.
2. **Cheap GUI bundle** (now). Wheel to `scroll_at` (1 line), `on_pointer`
   drag (token move + drag-reorder turns), tooltips + radial/context menus,
   layered HUD. Retires two standing workarounds. Parallel-safe with 1.
3. **Lua plugin author guide** (now, doc). Write against the widened ABI, so
   plan it alongside 4.
4. **Schema + ABI generalization** (fork-gated). `FieldValue` List/Map/Float;
   tagged Lua return + context table; item/condition model; optional
   per-turn action budget; system registry + loader + a PF2e skeleton.
   Blocks packs (7), generators (5), conditions/items, and system-driven
   movement. Largest single lane.
5. **Command grammar + generators** (spike-gated, blocked on 4). Needs the
   string-returning ABI. Run the entropy-tape and result-UX spikes.
6. **World-graph traversal module** (now but product-gated by B). Reuses the
   weighted BFS + event log. Pairs with the REGION/transition-point work.
7. **Ruleset packs** (blocked on 4). PF2e first (it forces the widening),
   then 5.2.1, A5E, BRP, one OSR.
8. **isometry-web player client** (spike-gated: iroh-wasm transport). Render
   path and reusable stack are proven; transport and the rAF/input loop
   remain. Independent of 4-7.

Unblocked-and-buildable-now: 1, 2, 3, 6 (given REGION docs). Fork-gated: 4
(and thus 5, 7). Spike-gated: 5, 8, plus the perf-ceiling and browser-scale
probes.

## B. Open questions for Mark

1. **REGION representation.** Same substrate (a coarser `MapDocument`,
   reusing editor/fog/tokens/replication) or a distinct lightweight layer?
   Recommendation: same-substrate-first. Open sub-question: does REGION
   carry full per-tile fidelity or is it a lower-fidelity movement layer.
2. **Is a WORLD tier wanted near-term?** *Working recommendation (2026-07-07):
   defer the WORLD travel simulation, build the transition-point primitive
   now with the REGION/multi-map work.* The 80% people want from "world
   traversal" in a prepared-map VTT is moving the party between prepared
   locales, which is the transition-point + board-menu feature, cheap and
   needed regardless. The pointcrawl graph with travel ticks and
   getting-lost rolls is a heavier, separable feature that matters mainly
   for hexcrawl/wilderness-sandbox campaigns. Revisit the trigger: Isometry
   explicitly targeting wilderness-exploration play.
3. **Token identity across map swaps.** When the host switches the active
   map, do tokens/initiative/fog persist per-map or migrate with a
   travelling party? Gates the transition and world-graph design.
4. **Target-aware resolution.** Does the app adjudicate hit/miss/save
   (compare roll vs AC/DC and report the outcome) or stay a "roll and let
   the table decide" tool? Directly shapes the widened Lua ABI.
5. **Conditions vs geometry.** Should conditions that change movement or
   senses (speed, blinded, immobilized) be substrate-visible so movement and
   fog honor them, or stay pure plugin data? Decides whether
   `MOVE_BUDGET`/`SIGHT_RADIUS` become system-driven inputs.
6. **Systems as loadable data vs compiled-in, and when.** Commit to a
   data-driven system-pack format (third-party OSR without recompiling), and
   if so, does it land with campaign packs or after a second built-in proves
   the schema generalizes?
7. **isometry-web transport and trust posture.** iroh-wasm relay
   (recommended) vs matchbox vs DM-run WebSocket; and accept the
   friendly-table leak (full `GameSnapshot` in browser devtools) or add
   host-side fog/secret culling before sending to browser peers?
8. **Smooth pan/zoom vs snap-pan.** Commit to snap-pan permanently (matches
   the GBA reference, dodges the re-raster cost) or prioritize the netrender
   camera-offset composite as the one serval/netrender ask? Folds in
   copyleft handling for any CC-BY-SA pack co-mingled with proprietary
   assets.

## Provenance

Multi-agent landscape pass 2026-07-07 (workflow
`isometry-next-horizons-landscape`, run `wf_fb1ce42f-812`, plus a follow-up
generators/command-grammar research agent). Per-agent findings and
citations are in the workflow transcript. Licensing and serval-capability
strands were adversarially verified against primary sources and in-repo
`file:line`.
