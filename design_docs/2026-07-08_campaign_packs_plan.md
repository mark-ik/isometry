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
and `.vox` ingest works. P1 (the appearance lane) is complete, and the P2
compendium is complete: a sortable index, monster/spell/item pages,
spawn-to-board, Monsters/Spells/Items namespaces, and the full 5e-database
vendored (334 monsters, 319 spells, 237 items). This is the "campaign packs"
horizon the
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
   format. **Policy:** the three LODs are independent authored documents;
   cross-LOD consistency (the region shows mountains, the local map is swamp)
   is the GM's business, never derived. The world map is not a projection of
   local maps, since deriving one from the other reintroduces the volume
   explosion the ladder exists to avoid.
9. **Procgen amplifies a part library.** Part grammars assemble parts at
   attachment sockets; rules-coupled drops make appearance derive from stats
   (a "flaming longsword +1" assembles the matching parts and warm palette),
   the differentiator Roll20/Foundry lack since they treat item art as a
   static handle, so protect that lane; WFC or adjacency grammars build maps
   over the tile grid. **Sockets encode orientation, not just position** (a
   blade must know which way the grip faces): reserve a palette-index *range*
   whose index encodes direction, or pair a socket voxel with a facing voxel.
   Fix the convention on part-library day one; retrofitting sockets is
   miserable. The importer **validates reserved indices loudly** (MagicaVoxel
   remaps palette indices on some saves, and palette-swap artists clobber
   reserved slots) rather than mis-socketing in silence. Bake cache key =
   model hash + palette LUT + resolved transforms + projection params
   (`BakeParams`) + baker version, so an upgrade never serves a stale bake.
   Sync and determinism are decision 12.
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
12. **Generation is an op; DM-authority carries the result (2026-07-08
    pressure-test).** Generation is a `GameEvent` in the ordered event log
    (decision #2: iroh + DM-authority, not a CRDT or p2panda op): "apply
    generator G to this grid," with later hand-edits as ordinary subsequent
    ops. Peers replay the log, so a generated map the GM then tweaks is
    described correctly and for free. The DM is authority and **the op carries
    the generated result** (a tile grid is small), so peers never re-run
    generation and cross-peer determinism is not required. Seed-only replay
    (ship the seed, regenerate locally) is an opt-in *bandwidth* optimization,
    allowed only for generators that pass determinism discipline: integer or
    fixed-point noise (no ambient float), a portable seeded RNG, no `HashMap`
    iteration in the generation path (`BTreeMap` or indexed vecs; Rust's
    `RandomState` diverges silently), order-stable WFC retries, and
    content-hash generator identity (not a version string). A pack's Lua
    generator becomes a consensus function *only* under seed-only replay, so
    the DM-carries-result default keeps peer-shipped Lua off the consensus
    path; where seed-only is used the piccolo sandbox must be deterministic
    (no `os.time`, fuel limits enforced, a curated API). On any determinism
    doubt the fallback is to ship the grid: more bytes, never divergent worlds.

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

## Widget siblings (data_grid's family)

The compendium and creator create reusable widgets beside `data_grid`. Most
are xilem-serval view compositions (tier 1, CSS/DOM); a few are chisel leaves
(tier 4, Path-B). Build each when a real second consumer appears (extract on
second use), not speculatively.

- **overlay_panel** (view): a titled, closable float over the board; the
  compendium and the sheet are both instances, extract from `sheet_overlay`.
- **search_field / list_filter** (view): filter a grid or list by text and
  facets; the Monsters index wants it immediately.
- **tab_strip / segmented** (view): namespace nav (Monsters, Spells, Items,
  Conditions), shared with the roster tabs already in meerkat.
- **stat_list / field_list** (view): labeled key-value rows (STR 16 (+3), AC
  15); the monster page and the character sheet share it (the schema-driven
  field list noted earlier).
- **record_card / detail_panel** (view): a titled record with sections and
  action buttons; the monster page, an item card, a spell card.
- **chip_row / badge** (view): CR and type tags, the meerkat roster chips
  generalised.
- **virtual_list** (view over chisel arrangement): the single-column,
  non-tabular sibling of `data_grid` for long prose or action lists.
- **swatch_grid / palette_picker** (view over chisel `Swatch`): the creator's
  palette and preset picker (P3).
- **option_card** (view): a selectable preset tile with a thumbnail (P3).
- **appearance_preview** (chisel Path-B leaf): the live animated token
  preview, reused on the board and as a sheet/roster portrait (P3).

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
- 2026-07-08: P2 content foundation landed (commit b786760): `isometry-system`
  bestiary (Monster model + SRD starter, 3 tests). External pressure-test
  refined the procgen/LOD lanes: generation-as-op with DM-authority carrying
  the result (decision 12), LOD independence as explicit policy (decision 8),
  socket orientation + loud validation and a fuller bake-cache key (decision
  9). `data_grid` widget siblings catalogued (new section). Next: the Monsters
  index as a `data_grid` overlay (UiState + compendium overlay; conventions
  read, ready to build).
- 2026-07-08: **Monsters compendium index landed** (commit b9048d2), the first
  `data_grid` consumer: a fixed-width overlay (Name/CR/Type/HP/AC, sortable
  header, zebra) toggled by a Bestiary button, host-fed view-side rows so
  views stay system-agnostic. Headed receipt
  scry-shots/2026-07-08_isometry_compendium_datagrid.png. The overlay pattern
  now has two consumers (sheet + compendium), so `overlay_panel` is a real
  extraction candidate. P2 remaining: monster page, spawn-to-board, and the
  full 5e-database vendor (P2b).
- 2026-07-08: **monster page + spawn-to-board landed** (commit 87e0cb3).
  Clicking a name opens a record card + stat list + six-ability block +
  actions + Spawn; spawn drops the monster via the `TokenPlaced` path
  (undoable, joins the turn list), its sprite resolving to a per-monster
  palette-swap of the rig (orc/skeleton/wolf as distinct voxel tokens; real
  per-monster models are parts-pack work). First `record_card`/`stat_list`
  siblings realised. Receipts:
  scry-shots/2026-07-08_isometry_{monster_page,spawn_monsters}.png. **P2 core
  complete**; remaining is the full 5e-database vendor (P2b), then
  `search_field`/`tab_strip` as the list grows.
- 2026-07-08: `stat_list` extracted (commit 60a3d14; shared by monster page +
  sheet, first extract-on-second-use). **Spells + Items namespaces landed**
  (content 4e67f5b, view 49c8594): a `tab_strip` (first sibling beside
  `data_grid`) switches Monsters/Spells/Items; `data_grid` now serves three
  indexes with distinct columns; spell/item pages reuse entry_name +
  stat_list + prose. Receipts:
  scry-shots/2026-07-08_isometry_{spell_page,items_index}.png. Next: full
  5e-database vendor (P2b).
- 2026-07-08: **5e-database vendored (P2b); P2 complete** (commit 149c6bb).
  334 monsters, 319 spells, 237 items transformed from 5e-database into our
  trimmed format (data/*.json, ~695KB vs 3.3MB raw), loaded via serde
  include_str; the compendium browses the full SRD, virtualized, with wheel
  scroll. Receipt: scry-shots/2026-07-08_isometry_vendored_bestiary.png.
  Known gap: serval does not clip the grid body's overflow:hidden on
  absolutely-placed rows, so a scrolled window is not viewport-clipped (the
  DOM still virtualizes) - a serval-side follow-up. `search_field` is the
  next sibling now that lists are hundreds long.
- 2026-07-08: **search_field landed** (commit 2fe91c6), the third `data_grid`
  sibling. A filter box over the compendium (keys route via the host,
  whisper-style) filters all three indexes by name substring; resets on tab
  switch and close. The vendored 300+ lists are now navigable. Receipt:
  scry-shots/2026-07-08_isometry_compendium_search.png. Three catalogued
  siblings realized this session (stat_list, tab_strip, search_field);
  overlay_panel and record_card are the next extract-on-demand candidates.
- 2026-07-09: **overlay_panel + record_card extracted** (commit 2146762);
  five catalogued siblings now realized (overlay_panel backs the compendium +
  sheet; record_card backs the monster/spell/item pages). **Attempted the
  serval grid-clip fix and reverted it.** The change (clip lifted layers by
  their containing-block overflow, serval 0c248e5) passed isolated paint-list
  unit tests (clip + escape; 269 serval-layout green) but **blanked the
  isometry board**: every tile and token paints as an absolutely-placed lifted
  layer, and the re-applied clip whited-out the whole frame (a coordinate-space
  or clip-balance mismatch with the board's camera + pane the unit tests did
  not catch). Reverted (serval 6f812c4) to keep the tree rendering. The clip
  gap stays open; a rework needs board-level integration coverage (a headed
  capture in the loop), not just paint-list unit tests. Lesson: a core-paint
  change needs an app-level render check before it lands.
- 2026-07-09: retried with a headed check in the loop. Diagnostics found and
  fixed a real positional bug: the clip must use the node's own absolute origin
  (`child_origin = origin + l.location`), not the parent `origin` (the pane
  clip was landing at (0,0) instead of (228,0)). Verified the corrected clip is
  the true pane box ((228,0)..(1100,820)) and *contains* the board content (a
  tile at (648,140) sits inside), **yet the layer still rendered black**. So a
  correctly-positioned, containing clip blanks a lifted layer: the clip pushed
  in `paint_layer` is not in the coordinate space the lifted layer actually
  paints in. Reverted again; the board stresses this with ~2740 lifted layers
  (every tile + token). Next attempt must map netrender's `PushClip` semantics
  on the clean-stack layer path first (likely the clip rect is re-transformed
  by the active stack, so an absolute-coord clip double-applies), and gate on
  the headed board capture. This wants a dedicated serval+netrender pass, not
  more inline iteration.
