# CLAUDE.md — Isometry Repository Role

This file defines how Claude Code should behave in this repository. Read
it first when starting any session.

---

## Project Identity

**Isometry** is a pixel-art isometric virtual tabletop: a P2P map editor
and turn-based play substrate for D&D, Pathfinder, and other systems. The
DM prepares maps ahead of time and hosts a session; players join over p2p
and move through the maps in turns. The look and feel target is GBA-era
tactics games (Tactics Ogre: The Knight of Lodis, Final Fantasy Tactics
Advance): a locked isometric 2D lens first, 2:1 diamond tiles, quantized
height steps, low internal resolution integer-scaled with nearest-neighbor
sampling, and voxel-sourced appearance that keeps later 2.5D / 3D lenses open.

The substrate knows tiles, tokens, turns, facing, elevation, and area
templates. Game rules live in system plugins (schema plus scripts). The
substrate never knows what a hit point is.

Isometry is a standalone consumer of the Merely stack (Cambium,
genet-layout, netrender), the woodshed pattern: git deps on the mark-ik
remotes, patch mirror at the workspace root, machine-local path overrides
via a gitignored `.cargo/config.toml`.

See `design_docs/PROJECT_DESCRIPTION.md` for the product description and
`design_docs/DOC_README.md` for the doc index.

## Terminology

- **map**: a prepared board: tile layers, height field, props, spawn
  points. Authored in the editor, played in sessions.
- **tile**: one diamond cell of a map layer. Rendered as a DOM element.
- **token**: a playing piece on the map with position, facing, and an
  owner. May bind to a character sheet.
- **tileset**: a folder of sprites plus a manifest naming tile kinds.
  Appearance binds through CSS class vocabulary, so a campaign can reskin
  by swapping sheets.
- **voxel appearance**: token, prop, and tile art can be sourced from
  MagicaVoxel-style volumes and recipes, then baked to the locked isometric
  pixel lens. Voxels are asset/generation substrate, not map storage.
- **campaign**: maps, tilesets, sheets, and system choice bundled as a
  distributable pack.
- **system plugin**: a game system: character/item schemas plus scripted
  rules (Lua, via piccolo). 5e SRD and Pathfinder 2e are the first-party
  candidates.
- **session**: a hosted play instance: the DM's app is the authority,
  players replicate an ordered event log over p2p (iroh). "The DM" means
  whoever holds edit mode, not necessarily one person; see the
  shared-authority design doc.

Do not coin new names for these concepts mid-session.

## Document Structure

All authoritative design material lives in `design_docs/`. Read
`design_docs/DOC_README.md` first.

| Path | What's there |
| ---- | ----------- |
| `design_docs/DOC_README.md` | Index and AI working principles |
| `design_docs/DOC_POLICY.md` | Documentation governance |
| `design_docs/PROJECT_DESCRIPTION.md` | Product goals, features (maintainer-owned) |
| `design_docs/<date>_<keyword>_plan.md` | Active feature plans |
| `design_docs/archive_docs/<date>/` | Retired plans |

## Workspace Layout

```text
crates/
  isometry-core/    Pure-Rust substrate model: grids, iso math, map
                    documents, session events. No I/O, no UI, no net,
                    no genet deps.
  isometry-views/   Cambium view fns + CSS sheets (tilesets are
                    stylesheets). Host-agnostic.
  isometry-genet/  Native winit host: window, input, netrender present.
                    ISOMETRY_PROFILE=1 prints frame timers.
  isometry-net/     DM-authority replication over a pure protocol seam, with
                    iroh behind a feature.
  isometry-system/  System plugin lane: schemas plus piccolo Lua rules, with
                    the 5e SRD content pack.
  isometry-voxel/   Voxel appearance pipeline: .vox ingest, recipes, palette
                    swaps, and isometric sprite bakes.
```

Planned (phase-gated, see the bootstrap and horizon plans): `isometry-web`
(browser host), richer campaign packs, generator/worldbuilding tools, and
later live 2.5D / 3D voxel lenses.

Keep `isometry-core` pure: no `wgpu`, no `iroh`, no genet crates, no
file I/O. Event log semantics live in core; transport lives in
`isometry-net`.

## General Guidelines

- Rust: standard idioms. No `unsafe` without documented justification.
- 600-LOC ceiling per source file. Split before adding when approaching it.
- Plans go in `design_docs/` per the date-keyword-plan convention with
  done-conditions, not time estimates. Never `.claude/plans/`.
- Follow `DOC_POLICY.md` for documentation changes.
- Check the Merely ecosystem before writing a new module: genet,
  netrender, mere, woodshed, and the wgpu-* repos may already have the
  piece or the pattern.
- **Guard the feature-gated code**: run
  `cargo check --workspace --all-features --all-targets` after touching a
  sibling repo, and before committing anything in `isometry-net`. Every
  campaign feature is `default = []`, so a plain `cargo test` compiles
  neither them nor their tests: `--all-features` runs 182 tests where the
  default runs 173. They rot silently whenever mere moves, and nothing else
  will tell you. This is not hypothetical — 2026-07-17 found all four
  campaign features uncompilable (a removed `mooting` re-export, a
  duplicate `muniment`, and a de-async'd `MootStore::in_memory`), broken for
  days behind the default build.

## Important Don'ts

- Do not hard-code any single game system into the substrate. Rules
  belong in system plugins; the substrate stays geometry and turns.
- Do not add rollback netcode, CRDTs, or real-time sync machinery. The
  session model is DM-authority plus an ordered event log; that is a
  deliberate simplification, revisit only through a plan. (The revisit
  plan exists: `design_docs/2026-07-09_shared_authority_and_collaborative_building_plan.md`.
  Its conclusion: shared and DM-less authority still need no CRDTs; the
  guardrail stands.)
- Do not ship copyrighted game content. 5e SRD (CC-BY-4.0) and
  Pathfinder 2e (ORC) material only, with attribution.
- Do not treat camera freedom as a near-term rendering task. The locked
  isometric angle is the shipped 2D lens; later 2.5D / 3D modes are allowed
  because voxel source models dissolve the facing-art explosion, but they need
  their own plan and render lane.
- Do not add features beyond the active plan's current target without
  surfacing the scope change first.
