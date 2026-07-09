# Campaign packs: content, compendium, and voxel appearance

**Date:** 2026-07-08
**Status:** plan, in progress. Direction decided 2026-07-08: SRD
compendium as a wiki (Fork A), 5e-database as the first dataset, token
appearance as a voxel-sourced recipe, and (maintainer, 2026-07-08) the
fixed camera downgraded to the 2D mode of a lens ladder rather than a
permanent invariant (decision 11). Landed: the bake proved soulful (CPU
spike, Findings) and `isometry-voxel` is founded (baker + recipe, 2D
mode, 6 tests), and voxel tokens now render on the live board with
palette-swap (receipt scry-shots/2026-07-08_isometry_voxel_tokens.png),
and `.vox` ingest works. P1 (the appearance lane) is complete; next is
the compendium slice (P2). This is the "campaign packs" horizon the
bootstrap plan deferred to its own plan
(2026-07-05_isometry_bootstrap_plan.md, closing paragraph).

**Thesis:** a campaign is a bundle of modules, and every module is data
that teaches by example. Rules content (the SRD), character appearance
(voxel rigs), and world parts (voxel tiles and props) are three pack
kinds over one spine: authored or vendored as data, shipped P2P, readable
as the tutorial for making your own. Appearance is sourced from voxels and
baked to pixel sprites at the one locked isometric angle, so the GBA
tactics aesthetic is a production pipeline rather than a hand-drawing
mountain.

## Relation to prior docs

- **Amends** bootstrap decision #5 (aesthetic contract) in two ways. Facing:
  the model moves from "two mirrored to four" toward "four mirrored to eight,
  target full iso-facing," motivated by flanking. Camera (maintainer,
  2026-07-08): the fixed camera is downgraded to the **2D mode** of a lens
  ladder, no longer a permanent invariant (decision 11). The rest of #5 (2:1
  tiles, quantized height, low-res nearest-neighbor pixel look, battle-scale)
  stands. A dated pointer is added at #5; reconcile its text when next edited.
- **Honors** bootstrap decision #4 (tileset-as-stylesheet): the voxel
  baker produces the sprite sheets a tileset references; appearance still
  binds through the CSS class vocabulary.
- **Honors** bootstrap decision #7 (licensing): SRD content is adopted
  under CC-BY-4.0 with attribution; no other-publisher content ships.
- **Concretizes** next_horizons lanes: rulesets/licensing (the compendium),
  generators/commands (procgen), map scales/traversal (the LOD ladder).

## Decisions on file (2026-07-08 session)

1. **Packs are modules, teach-by-example.** Three kinds: a **content pack**
   (rules data, e.g. the SRD bestiary), a **rig pack** (a character base:
   layer options, clips, palettes), a **parts pack** (voxel parts, sockets,
   palettes, generation grammars). All are data, all P2P-shippable, all
   readable as the worked example for authoring a new one.
2. **Compendium is a wiki, Fork A.** SRD content becomes structured data
   plus xilem_serval views, not browsed HTML documents. A namespace index
   per type is a `data_grid`; a page is a data-driven header plus stat
   grids plus actions plus prose. Wiki-links are navigation. Chosen over
   the HTML-document fork because it fits isometry's all-views host, feeds
   the grid real sortable data, and keeps content actionable (spawn a
   monster, roll a spell) rather than read-only.
3. **First dataset: 5e-database.** SRD-only, JSON files in-repo, code MIT,
   content labelled OGL 1.0a; the SRD 5.1 is dual-licensed, so we adopt it
   under CC-BY-4.0 with attribution per decision #7. Vendor the JSON and
   transform it into our own pack types at import time; do not depend on a
   live external API at runtime (a VTT wants content local and
   P2P-shippable). Open5e V2 is a later breadth option, but its
   third-party OGL content is forbidden by decision #7, so it may be used
   only filtered to SRD/CC-BY via its per-entry `document` field.
4. **Token appearance is a recipe, not an image.** `Token.sprite: String`
   grows into `Appearance { layers, palette, clips }`. The recipe is baked
   to a sprite sheet and bound through the existing CSS tileset vocabulary
   (decision #4 unchanged). Tiny on the wire: peers sync the recipe, not
   pixels.
5. **Appearance is sourced from voxels.** MagicaVoxel authoring, `dot_vox`
   ingest, baked to iso pixel sprites at the one locked angle with
   nearest-neighbor at low internal resolution (decision #5 aesthetic
   honored). Optional bake-then-touch-up in Aseprite (`asefile` reads
   layers/tags/slices). This is asset production feeding tilesets; it does
   not change how appearance binds.
6. **Facings: four mirrored to eight, target full iso-facing.** Refines
   decision #5's "two mirrored to four." Flanking makes finer facing a
   rules concern, so the sprite budget grows. Fixed camera unchanged.
7. **Emotes and animations are clip tags.** A clip (idle, walk, attack,
   hurt, down, emote-*) is a tag in the rig, fired by name through the
   piccolo Lua lane (`token:emote("taunt")`), the same path an SRD action
   already uses: action fires an event, the event picks the clip and writes
   the narration line. The rules drive the animation.
8. **Voxels are the asset and generation substrate, not world storage.**
   Each scale keeps its own data model; the map stays a `TileGrid` of
   interned tile kinds. World map, region, and local map are three LOD
   documents linked by "enter here." Voxels supply the art and the
   generation input at every scale; they are never the battlemap's storage
   format.
9. **Procgen amplifies a part library.** Part grammars with attachment
   sockets (reserved palette-index marker voxels, so sockets are authored
   inside MagicaVoxel with no side files); rules-coupled drops (a "flaming
   longsword +1" assembles the matching parts and palette, so appearance
   derives from stats); WFC or adjacency grammars over the tile grid for
   maps. Peers sync a seed plus generator version over the event log
   (decision #2 unchanged, deterministic), not the generated bytes. The
   baker caches by recipe hash. Generators may ship as Lua inside a pack.
10. **spritec is a donor, not a dependency.** The 3D-to-sprite tool
    (ProtoArt/spritec) is archived since 2020 and MPL-2.0; read it for
    technique only, per the graphshell posture. A baker is a fresh
    MIT/Apache crate on our own wgpu stack (graft/weld/scry/netrender give
    more depth than that prototype had).
11. **The camera is a lens mode, not a fixed law (maintainer, 2026-07-08).**
    Bootstrap decision #5's "rotation permanently out of scope" is amended.
    The locked isometric angle is the **2D mode**, one rung of a 2D / 2.5D /
    3D ladder: 2.5D is an adjustable-pitch live render of the voxel model, 3D
    a free camera. The voxel model is the single source of truth; the mode is
    a lens over it. The rest of #5 (2:1 tiles, quantized height, low-res
    nearest-neighbor look, battle-scale) stands; only camera fixity relaxes.
    Near-term path is unchanged, 2D baked sprites first. Synthesis: the cost
    #5 feared from rotation, multiplying facing art, is exactly what a voxel
    3D source dissolves, since live modes render any angle without per-facing
    art; the residual cost is runtime depth re-sort, not art.

## New crate: `isometry-voxel`

- **Why a new crate:** `isometry-core` stays pure (no wgpu, no I/O per
  CLAUDE.md), so a renderer and `.vox` loader cannot live there. Precondition
  before founding: grep the ecosystem (serval, netrender, wgpu-*, mere) for
  an existing voxel or sprite-bake piece and extend rather than duplicate;
  none is known. New crate is MIT/Apache, edition 2024 (founding convention).
- **Shape:** takes a voxel volume (procedural now, `dot_vox` next), renders
  orthographically at the locked iso angle x N facings, palette-snaps, emits
  a sprite sheet. CPU today (proves the look), wgpu later (speed plus the
  live-render option under the fixed camera).
- **Where the recipe lives:** `Appearance` is asset-layer data in this
  crate, not core. Core keeps `Token`'s minimal sprite handle; the view and
  host layers resolve a recipe to a baked sheet and a CSS class.

## Phases (done-conditions, in maintainer execution order)

Maintainer sequence 2026-07-08: doc, then baker (P1), then wiki (P2)
alongside.

### P1 Voxel appearance baker promotion

- Found `isometry-voxel` (after the ecosystem grep); move the spike's
  splatter in as tested library code. *Done:* crate builds, a unit test
  bakes a procedural figure to a deterministic sheet.
- `Appearance { layers, palette, clips }` recipe + the compositor (stack
  layers, palette LUT, pick frame by clip). *Done:* a recipe composites to
  a sheet.
- Wire one baked token onto the live board beside today's sprites (bake ->
  sprite sheet -> CSS class, decision #4). *Done:* a voxel-baked token
  renders on the board; headed receipt in scry-shots.
- `dot_vox` ingest of a real MagicaVoxel file. *Done:* a `.vox` bakes.

### P2 SRD content pack + compendium (Fork A)

- Vendor 5e-database SRD JSON; a pack-import transform into isometry
  content types, Monsters first, attribution recorded. *Done:* a Monsters
  pack loads from vendored data.
- Monsters namespace index as a sortable `data_grid` in a wiki pane in the
  isometry-serval host (not pelt). *Done:* index renders and sorts.
- Monster page view (header + stat grids + prose). *Done:* an index row
  opens its page.
- Spawn-onto-board action from a page, wired to the token model. *Done:* a
  monster becomes a token on the board.

### P3 Creator + clips (later)

- Creator pane: chisel `Swatch` palette, preset cards, a live animated
  preview leaf (Path-B). Clip state machine with Lua emote/action triggers
  (decision #7).

### P4 Procgen and scale ladder (vision)

- Part grammars, rules-coupled drops, WFC maps, world-map/region LOD
  documents linked by "enter here."

## Findings (verified)

- **2026-07-08 the voxel bake looks soulful.** A pure-`std` CPU
  orthographic iso splatter (z-buffer, three-tone face shading: top lifted,
  left 0.74, right 0.55) baked a procedural hero at four diagonal facings
  and composited it on a grass patch. It reads with real presence, and the
  material language is consistent between token and terrain because both go
  through the one renderer. The three-tone shading is where the soul lives
  (four multiplies per voxel). A CPU bake is representative of a wgpu
  render's look, so wgpu is a speed/scale concern, not a look risk.
  Procedural models drove it, so swapping to `dot_vox` is a data-source
  change with nothing in the renderer moving. This unblocks tokens, items,
  tiles, and world-map dioramas on one path. Throwaway spike (not in repo):
  `scratchpad/voxbake/voxbake.rs`; images
  `scratchpad/voxbake/{hero_facings,scene,scene_alt}.png`.

## Open forks (maintainer call)

1. **Fixed-camera lens. Resolved 2026-07-08 (decision 11):** the fixed camera
   is the 2D mode, camera freedom is in scope via the ladder. The live
   2.5D/3D renderer is future wgpu work. Open sub-question: how the pixel soul
   and clip animation carry into a live render. Render-to-low-res plus
   nearest-neighbor and palette snap keeps the look (the "3D rendered as pixel
   art" technique); sprite clips versus live posing may diverge, so the
   Appearance recipe drives both while the animation representation may fork.
2. **Final facing count.** Four versus eight (decision #6). Flanking argues
   eight; art and sort cost argue four-then-mirror. Confirm before art
   volume begins.
3. **Art source.** Author bespoke to our rig spec versus source a permissive
   pack. No layered isometric library exists; LPC is top-down and
   share-alike (viral). Bespoke is the likely path and where the distinctive
   look lives. Confirm before art volume begins.

## Progress

- 2026-07-08: plan created; voxel bake spike verified (Findings). Next per
  maintainer sequence: P1 (found `isometry-voxel`, ecosystem grep first),
  then P2 (wiki slice) alongside.
- 2026-07-08: `isometry-voxel` founded (baker + Appearance recipe, 2D mode; 5
  tests; commit 1e03825; ecosystem grep confirmed no existing piece).
  Maintainer amended the camera to a lens mode (decision 11); bootstrap #5
  pointer added. Next: board wiring (P1), then the compendium slice (P2).
- 2026-07-08: board wiring landed (commit c449bd2). `isometry-voxel` gained
  a dependency-free PNG/base64 encoder; `board_css` bakes the demo rig into
  `.token-knight` and a recoloured `.token-goblin`. Headed receipt shows
  voxel tokens on the live board with palette-swap (knight red, goblin
  green): scry-shots/2026-07-08_isometry_voxel_tokens.png. P1 remaining:
  `dot_vox` ingest of a real MagicaVoxel file. Then P2 (compendium).
- 2026-07-08: `dot_vox` ingest landed (commit 3162afa). `load_vox` parses
  `.vox` (Z-up to Y-up remap, `i -> palette[i-1]`), single-model scope;
  verified by test (dims + palette) and a visual orientation check. **P1
  (voxel appearance lane) complete.** Next: P2 compendium.
