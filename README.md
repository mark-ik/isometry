# Isometry

A pixel-art isometric virtual tabletop. One player preps maps and hosts;
the group joins peer-to-peer and plays in turns. GBA-tactics look (fixed
camera, 2:1 diamond tiles, sculpted elevation, integer-scaled pixels),
system-agnostic core: game rules arrive as schema-plus-script plugins,
the substrate only knows tiles, tokens, turns, facing, and elevation.

Built in Rust on the Strophos stack (xilem_serval, serval-layout,
netrender). Status: bootstrap; see
[design_docs/2026-07-05_isometry_bootstrap_plan.md](design_docs/2026-07-05_isometry_bootstrap_plan.md).

## Workspace

- `crates/isometry-core`: pure substrate model (grids, iso math, map
  documents, session events). No UI, no I/O, no network.

Docs live in [design_docs/](design_docs/), indexed by
[design_docs/DOC_README.md](design_docs/DOC_README.md).
