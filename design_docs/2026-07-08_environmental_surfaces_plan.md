# Environmental surfaces

**Date:** 2026-07-08
**Status:** design lane. Architecture plus a phased sketch; not scheduled.
Emerged from a 2026-07-08 conversation referencing Larian (Divinity: Original
Sin, Baldur's Gate 3) and Owlcat (Pathfinder: Kingmaker) environmental
systems.

**Thesis:** the ground can hold state (fire, water, grease, ice, poison, ...)
that spreads, interacts, and affects tokens. It fits isometry's
substrate/system split cleanly: the substrate owns that surfaces *exist*,
*spread*, and *render*; the system plugin owns what they *do* and how they
*combine*. How rich the interactions are is a **ruleset dial**, not a
substrate decision, so faithful-5e and Larian-surfaces are the same engine
with a different rules pack.

## The reference: how Larian adapted D&D

- **BG3** kept 5e's spine (d20, AC, saves, spell slots, classes) and grafted
  on **surfaces** from Divinity: Original Sin. The ground holds fire, water,
  grease, poison, ice, blood, and clouds, and they **interact**: water plus
  lightning shocks everyone standing in it, fire plus grease spreads, water
  plus cold makes slippery ice, fire plus water makes sight-blocking steam.
  Environmental **conditions** (Wet: vulnerable to cold/lightning, resistant
  to fire), height for advantage, and spells that create or ignite surfaces.
  The surface-combo layer is the departure from tabletop 5e, a Larian-ism.
- **Owlcat** (Kingmaker/WOTR) stayed closer to the book: terrain, traps,
  spell areas, few combos.
- **DOS** is the pure ancestor, no D&D at all.

## Architecture fit (nearly all of it already exists)

- **Surfaces are a tile layer.** The map is already layered `TileGrid`s of
  interned kinds; a surface layer (per tile: none / fire / water / grease /
  ice / poison / ...) is one more. The substrate knows a surface exists and
  renders; it never knows what fire does.
- **Spread is `flood_region`.** `isometry-core/grid.rs` already flood-fills;
  fire spreading to adjacent flammable tiles is that, adjacency-gated. Spread
  is deterministic, so it is decision-12 shaped (the DM runs it, the op
  carries the result, peers replay). Multiplayer-safe for free.
- **Templates place the areas.** `template.rs` Burst/Line/Cone already exist;
  Fireball paints a fire surface in its burst, Grease a grease surface, Web a
  web surface. The area system becomes the surface applicator.
- **Rendering rides the voxel pipeline.** Fire is animated flame voxels,
  water a blue overlay, ice pale, poison a green haze. One material language
  with terrain and tokens; elevation can matter (liquids pool downhill).
- **Rules and combos live in the Lua system plugin.** "Fire on grease becomes
  a bigger fire," "wet plus lightning shocks," "burning deals 1d4 at end of
  turn, DEX save to douse" are system rules, scripted in piccolo, narrated
  through the event pipeline. This is where D&D is *adapted*: 5e's own area
  spells (grease, web, wall of fire, cloudkill, stinking cloud) become surface
  generators, and the Larian interaction matrix is an extension layered on.

## The dial: substrate vs system

- **Substrate:** surfaces exist (a per-tile surface layer), spread
  (flood/adjacency), render (voxel overlays), and ride the event log. No
  rules.
- **System plugin (5e):** what each surface does (damage, saves, conditions),
  the interaction matrix (combos), and which spells generate which surfaces.
  Shipped as ruleset content, scriptable in Lua.
- **The ruleset dial:** faithful-5e (areas plus difficult terrain, few combos,
  Kingmaker-ish) versus surfaces-heavy (full elemental matrix, BG3/Larian).
  Same substrate, different rules pack. The configurability doctrine applied
  to environmental depth: don't pick Larian-versus-purist, ship the dial.

## Turn-based is simpler

Larian's surfaces are real-time simulations. isometry is turn-based: surfaces
tick on enter and at end of turn, spread resolves as a discrete op, and there
is no fluid sim. The tabletop cadence removes the hard part.

## Phase sketch (when scheduled)

- **S1 Surface layer + render.** A per-tile surface state in the substrate
  plus voxel-overlay rendering. *Done:* a fire tile renders on the board.
- **S2 Templates apply surfaces.** A spell or area paints a surface over
  templated tiles. *Done:* Grease paints a grease surface in a burst.
- **S3 Rules + conditions (system).** On-enter and end-of-turn effects
  (burning, wet, prone-on-ice) as 5e-plugin rules plus events/narrate. *Done:*
  standing in fire deals damage and logs it.
- **S4 Interactions + spread.** The combo matrix (fire+grease,
  water+lightning) plus adjacency spread as a deterministic op. *Done:* fire
  spreads through grease and the log and peers agree.

## Open questions

- **Stacking.** Can a tile hold fire and water at once, or do they resolve to
  steam immediately? Simplest is one surface per tile, with interactions
  transforming it.
- **Duration.** Surfaces decay after N turns versus persist until doused.
- **Elevation coupling.** Do liquids flow downhill? Optional, nice.
- **Data versus Lua.** A `(surface, trigger) -> result` table covers most of
  the matrix; reserve Lua for the exotic.

## Relation to prior docs

- Honors the bootstrap substrate/system split: surfaces exist in the
  substrate, rules live in the plugin.
- Rides `2026-07-08_campaign_packs_plan.md` decision 12: spread is a
  DM-authority op that carries its result, deterministic and replayed.
- Reuses `template.rs` (areas) and `grid.rs` (`flood_region`); renders through
  the voxel pipeline (`isometry-voxel`).
