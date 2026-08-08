# Overmap Presentation Plan: Hulls, Backdrop, and a Map That Reads as Terrain

**Date:** 2026-08-02
**Status:** Active, with **two prerequisites reopened by the 2026-08-08
audit** ahead of the "already landed" list:
(1) **a neutral region-paint seam**: sprigging's `GraphCanvas` privately
owns paint order and geometry, so "extend the existing leaf" is not
implementable as written; a product-free region/composite layer is now
justified by the Mesocosm minimap as second consumer;
(2) **hulls derive from final displayed node positions**, including local
placement overrides, never from authored coordinates alone. Required
receipts: uniform-position, unplaced-node, override, parallel-route, and
a headed screenshot.
**Product direction recorded (wing session, 2026-08-08): source-time as a
feature** — the overmap viewed from historical standpoints: what was
believed then, what the table knows now, what was retconned, which map
version a character possessed. This is the wing's *claim* carrier at
campaign scale; it rides this plan's machinery once the prerequisites
land.
**Companion:** mere `design_docs/mere_docs/implementation_strategy/2026-07-21_projection_proofs_plan.md`
(the arrangement register), mesocosm's minimap (`mesocosm-views`, the first
Hulls consumer and the working example to follow).

---

## 1. What this is

The overmap swatch is contract-native (P4 landed: `overmap_score` →
`scenomise::solve` → scene → `GraphCanvasSwatch`) but its presentation is
ball-and-stick: dots, straight edges, labels, no sense that a site *sits in*
a region of the world. Mark's brief: shape hulls around nodes to give a sense
of node-associated territory, put them against a backdrop, and be cleverer
about data representations generally (mosaic subgraphs for inventories are
the standing example).

This plan upgrades the overmap's presentation without touching campaign
truth, discovery (E6), or the score/scene contract's product-freedom.

## 2. Already landed (do not rebuild)

- **`sceno::Arrangement::Hulls`** (mere, 2026-08-02): bounded nearest-site
  partition. Every coordinate-placed item is a site; each cell is the bounds
  clipped by perpendicular bisectors; the solver emits one `sceno::Region`
  per site with a `Footprint::Polygon` contour. Cells tile the bounds.
- **`sceno::Region`** was already the scene face of the hulls lane; the
  solver now fills it.
- **`numen::FieldExtent::Polygon`** (mere, 2026-08-02): containment + signed
  boundary distance, persisted through graph-kernel. Available when a region
  needs scripted/inferred meaning; not required for presentation.
- **`mesocosm-views`** (mesocosm, 2026-08-02): the first Hulls consumer.
  `minimap.rs` is the adapter shape (disclose → solve → resolve meaning
  vessel-side); `leaf.rs` is a sprigging `Leaf` that paints region polygons,
  site dots, and a position marker from a solved scene. Read both before
  writing isometry's version.

## 3. The work

### 3.1 Hulls behind the overmap

Add a Hulls realization to the overmap swatch: territory cells around the
discovered sites, painted under the existing nodes and edges.

- **Adapter**: a second score (or an extended `overmap_score`) with
  `Arrangement::Hulls`. Sites are the discovered overmap nodes' authored
  `at` positions; bounds fit the discovered extent with a margin. Only
  discovered nodes are sites: the unfound map is not drawn, and a cell for
  an unfound site would leak its existence.
- **Meaning is per-vessel** (ruled 2026-08-02): isometry tints by
  campaign facts it already owns. First cut: faction control where a
  faction claim exists, biome/terrain kind otherwise, neutral where neither.
  The contract carries geometry; `WorldPlace`/faction resolution stays in
  `isometry-campaign`.
- **Paint**: follow mesocosm's `MinimapLeaf` (translucent fill, full-strength
  boundary, cells under nodes). Either extend the existing overmap leaf or
  paint regions in the same leaf before nodes. Do not add a second leaf key
  unless layering forces it.
- **Isometry caveat**: overmap cells are *derived* territory, not simulation
  truth (unlike mesocosm, where the partition rule is the world's own).
  Present them softer: lower fill alpha, or `Region.confidence` mapped to
  opacity. Do not let derived territory read as authored borders.

### 3.2 Backdrop

DOM layer under the leaf (ruled 2026-08-02: dynamically generated, not
static; DOM unless effects force painting into the leaf).

- The overmap swatch sits in a DOM element; give it a background layer
  generated from campaign data. The natural isometry backdrop: a baked
  low-res isometric render of each site's tactical map (the voxel bake
  pipeline already produces sprites), composited as region-anchored images,
  or a single generated terrain wash from biome facts.
- Start with the cheapest honest version: a generated (not shipped-asset)
  terrain wash tinted from the same per-region facts as 3.1, as CSS. Baked
  map thumbnails are a follow-up once region anchoring is proven.
- Tilesets-are-stylesheets applies: the backdrop binds through CSS class
  vocabulary so campaigns can reskin it.

### 3.3 Region interactivity

Cells are hit targets: click a cell to select its site (same action as
clicking the node), hover to show the region's facts (faction, biome,
travel weight of routes touching it). Follow the graph-canvas pattern:
paint stays in the leaf, hit targets are native DOM positioned from the
same solved scene, so a11y and keyboard come free. The polygon hit test
can use the region contour; a bounding-box approximation is acceptable for
the first cut if the precise test fights the DOM.

### 3.4 What NOT to do

- Do not compute layout in isometry-views. The solver owns placement; the
  adapter disclosed facts. Hand-rolled geometry is the debt P4 just deleted.
- Do not put faction or biome vocabulary into sceno/scenomise. The audit
  rule stands: neither portable crate mentions any product.
- Do not draw undiscovered territory, even unlabeled. Discovery is a
  campaign rule, not a presentation choice.
- Do not block on Mosaic/Atlas. They are reserved arrangements with their
  own future consumers (inventories; geographic). This plan is Hulls only.

## 4. Done conditions

- The overmap swatch shows territory cells around discovered sites, tinted
  by vessel-owned meaning, under the existing nodes and edges.
- A generated backdrop renders under the cells as a DOM layer, restylable
  via CSS class vocabulary.
- Cells are clickable and answer hover with region facts.
- `cargo test --workspace --all-features` green; the sceno audit tests
  (type names stay `sceno::`) untouched and passing.
- A headed scenario screenshot in `Code/testing/isometry/` showing the
  upgraded overmap, per the screenshots harness convention.

## 5. Open questions (ask Mark, do not decide unilaterally)

1. Faction tint vs biome tint when both exist: layered, blended, or
   faction-wins?
2. Should undiscovered edges of a discovered cell be clipped hard (fog
   boundary) or faded?
3. Baked map thumbnails as region backdrops: worth it now, or after the
   wash proves the layering?
