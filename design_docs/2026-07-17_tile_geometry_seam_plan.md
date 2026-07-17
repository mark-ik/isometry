# The tile geometry seam

**Date:** 2026-07-17
**Status:** plan (not scheduled). The diagonal-movement decision below **landed
2026-07-17**; the seam itself awaits a product reason.
**Related:** [next_horizons_landscape](2026-07-07_next_horizons_landscape.md)
(its lane 2 already found this shape for the WORLD tier: "generalizing
'neighbor' from grid-adjacency to graph-edges is the whole change"),
[gameplay_roadmap](2026-07-14_gameplay_roadmap_plan.md) (C8, the deferred
pointcrawl, is the same generalization).

## The question

Can a map be tiled in something other than 2:1 diamonds? Hexes, triangles, or a
non-grid graph?

**Yes, and the substrate is further along than it looks** — because the rules
never asked about squares. But it is a substrate change, not a pack change, and
it is a *product* question before a technical one.

## What is already geometry-agnostic

This is the surprising half, and it is the reason the seam is tractable at all.

- **The rules layer.** The resolver asks `distance(a, b)` and never "how many
  squares". 5e and PF2e do not know what a tile is. Reach, templates, and the
  whole action spec are already expressed over an abstract metric.
- **Storage.** `TileGrid<T>` is a rectangular array indexed by `(col, row)`.
  Hex axial (or offset) coordinates live in a rectangular array perfectly well;
  this is the standard representation, not a workaround.
- **The drawn shape.** A tile is a CSS `clip-path` polygon. A hexagon is a
  different polygon string and nothing else. Per pillar 3 (tileset as
  stylesheet) this belongs in pack CSS anyway, where the beats already moved.
- **Fog, LOS, and reach.** All defined over "tiles, neighbours, and a distance",
  not over squareness.

## What is pinned to the square-diamond (verified 2026-07-17)

| Pin | Where |
| --- | --- |
| the 2:1 diamond projection and its inverse | `IsoGeometry::tile_to_screen` / `screen_to_tile` |
| the neighbour set | `reachable`'s step list (`path.rs`) |
| Chebyshev distance | `distance()` (`template.rs`) |
| eight compass points | `away()` / `compass()`, and the eight generated force-beat keyframes (`force_css`) |
| burst / line / cone | `template_tiles()`, over Chebyshev |
| depth sorting | `depth_key()` |

## The seam

One trait (or enum) carrying six operations:

```text
project(tile, elevation) -> screen      unproject(screen) -> tile
neighbours(tile) -> [tile]              distance(a, b) -> u32
directions() -> [(name, step)]          depth(tile, elevation) -> i32
```

Everything above the seam (rules, fog, reach, templates, the resolver, packs)
is already written against exactly these. `IsoGeometry` is a value passed
around rather than a set of globals, so a `HexGeometry` slots in beside it.

This is the same generalization C8 (pointcrawl) needs: a waypoint graph is a
geometry whose `neighbours` are edges and whose `distance` is a weight.
**Hex, triangle, and pointcrawl are one piece of work, not three.**

## Difficulty, honestly

- **Hex: tractable, and a real VTT need.** A regular tiling with a uniform
  distance metric, six neighbours, standard axial coordinates, and it composes
  with per-tile elevation (Wesnoth). The force beats regenerate from the
  projection, so six directions fall out of the same code that makes eight.
- **Triangle: exotic.** Two orientations per cell, so the neighbour set depends
  on the cell's parity; templates and facing get strange. Doable, but research.
- **Top-down square: nearly free.** A different projection, same everything else.
- **Pointcrawl: already planned** as C8, deferred behind transition points.

## The product question comes first

The aesthetic anchor is GBA tactics (Tactics Ogre, FFTA): a locked isometric
lens over 2:1 diamonds. `CLAUDE.md` names that the shipped 2D lens. A hex board
is **a different product lens, not a refinement of this one** — so the seam
should wait for a reason ("this campaign needs hexes") rather than being built
because it is possible. The cost of waiting is low: the seam's shape is now
known and written down, and nothing being built above it makes it harder.

## Decided and landed: movement is eight-way (2026-07-17)

Checking the pins surfaced a genuine inconsistency, now fixed.

`reachable` walked **four** neighbours, while `distance()` is **Chebyshev**
(diagonals cost one) and `away()` names **eight** compass points. So a diagonal
tile was "one away": close enough to *melee*, and a legal square to be *shoved
into*, but impossible to *walk to*. Three parts of one substrate disagreed
about what adjacency means.

Two coherent resolutions existed — make movement eight-way, or make distance
Manhattan (which is what FFTA does, and would have been the more faithful
anchor). **Mark chose diagonal movement**, so `reachable` now steps all eight
neighbours and one diagonal costs one, exactly as Chebyshev says. Reach preview,
melee range, and shove direction now agree.

**Verified:** `budget_caps_the_reach` now asserts the Chebyshev shape (a
two-step budget reaches `(2,1)` via a diagonal-plus-straight and `(2,2)` via two
diagonals, while Chebyshev 3 stays out of reach); 185 workspace tests green; the
in-app combat loop is unchanged.

## Progress

- 2026-07-17: Doc created from an audit of the geometry pins. Eight-way movement
  decided and landed. The seam itself is specified but unscheduled: it needs a
  product reason, and it should be built once for hex *and* pointcrawl together.
