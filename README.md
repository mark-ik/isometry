# Isometry

A pixel-art isometric virtual tabletop. One player preps maps and hosts;
the group joins peer-to-peer and plays in turns. GBA-tactics look (2:1
diamond tiles, sculpted elevation, integer-scaled pixels) with
voxel-sourced appearance baked to the locked isometric 2D lens first, and
kept open for later 2.5D / 3D lenses. The core is system-agnostic: game
rules arrive as schema-plus-script plugins, and the substrate only knows
tiles, tokens, turns, facing, and elevation.

Built in Rust on the Merely stack (xilem_serval, serval-layout,
netrender). Status: bootstrap; see
[design_docs/2026-07-05_isometry_bootstrap_plan.md](design_docs/2026-07-05_isometry_bootstrap_plan.md).

## Workspace

- `crates/isometry-core`: pure substrate model (grids, iso math, map
  documents, session events). No UI, no I/O, no network.
- `crates/isometry-voxel`: voxel appearance pipeline (`.vox` ingest,
  recipes, palette swaps, and isometric sprite bakes).

Docs live in [design_docs/](design_docs/), indexed by
[design_docs/DOC_README.md](design_docs/DOC_README.md).
