# design_docs Index

Canonical index for `design_docs/`. Per DOC_POLICY §5, this file wins over
any other index and is updated in the same session as any doc change.

## Working principles for AI assistants

- Read `../CLAUDE.md` first for repo role, terminology, and don'ts.
- Verify claims against the codebase, not doc-to-doc consistency.
- Plans carry done-conditions, not time estimates.
- `PROJECT_DESCRIPTION.md` is maintainer-owned; surface contradictions,
  do not edit unasked.
- The substrate/system split is load-bearing: geometry and turns in the
  substrate, rules in system plugins. Keep it that way in every doc.

## Active docs

| Doc | What it is |
| --- | ---------- |
| [DOC_POLICY.md](DOC_POLICY.md) | Documentation governance |
| [PROJECT_DESCRIPTION.md](PROJECT_DESCRIPTION.md) | Product goals and pillars (maintainer-owned) |
| [2026-07-05_isometry_bootstrap_plan.md](2026-07-05_isometry_bootstrap_plan.md) | Founding plan: architecture decisions, engine probes, phases I0-I6 (all landed) |
| [2026-07-07_next_horizons_landscape.md](2026-07-07_next_horizons_landscape.md) | Post-bootstrap landscape: six lanes (Lua docs, map scales/traversal, isometry-web, rulesets/licensing, generators/commands, genet GUI), recommended sequence, open forks |
| [2026-07-07_viewport_windowing_and_chrome_plan.md](2026-07-07_viewport_windowing_and_chrome_plan.md) | Active plan: viewport windowing (raise the ~30-40 board ceiling) + cheap GUI bundle (wheel scroll, drag, overlays) |
| [2026-07-07_optional_intelligence_vision.md](2026-07-07_optional_intelligence_vision.md) | Vision (post-keystone): optional DM-loaded models/agents; the DM-in-the-loop dialog system, conversation economy, inference architecture, and the dynamic-content opportunity catalog |
| [2026-07-07_board_to_text_narration_plan.md](2026-07-07_board_to_text_narration_plan.md) | Active plan: deterministic board-to-text serializer (`narrate` module); N1 scene + N2 viewer-relative/fog-aware landed, N3 model fluency deferred |
| [2026-07-08_campaign_packs_plan.md](2026-07-08_campaign_packs_plan.md) | Active plan: campaign packs as modules. SRD compendium wiki (Fork A, 5e-database), voxel-sourced token appearance (`isometry-voxel` baker, recipe not image), emotes/procgen, LOD scale ladder. Voxel bake proved soulful (CPU spike). Open forks: fixed-camera lens, facing count, art source |
| [2026-07-08_environmental_surfaces_plan.md](2026-07-08_environmental_surfaces_plan.md) | Design lane (not scheduled): environmental surfaces (fire/water/grease/ice/poison) as a substrate tile layer that spreads (flood_region) and renders (voxels); rules + interaction combos in the Lua system plugin. Ruleset dial: faithful-5e ↔ Larian-surfaces. Rides decision-12 determinism. Reference: BG3/Larian, Owlcat |
| [2026-07-09_worldbuilding_generation_plan.md](2026-07-09_worldbuilding_generation_plan.md) | W0-W5 implementation ladder landed: private/public/Codicil checkpoints; generated inventory/equipment/hidden modifiers; bounded typed Lua generators; pack discovery and host preview; playable local-map lowering and multi-map replication; typed factions/places/characters/routes/laws/history/storylets; private-fact requirements and existing-character role casting; and an inspectable River Oath campaign draft with region, two local maps, conflict, secret, law, reward, and finale. Signed/P2P pack distribution and later simulation rungs remain expansion work. |
| [2026-07-09_shared_authority_and_collaborative_building_plan.md](2026-07-09_shared_authority_and_collaborative_building_plan.md) | Revised implementation plan: campaigns are signed multi-writer p2panda spaces with type-specific materializers; live tactics alone retain a temporary sequencer. Moot owns group policy, Murm private channels, Personae identity/grants, shared transport Iroh/gossip/LogSync. Proposals, convergence, signed governance and adopt-or-branch resolution, live conflict UI, and Murm-backed frozen secret audiences are landed; desktop actor wiring, sealed secret delivery, and domain migration remain. Its "Next Game Slice" section moved to the adjudication plan below, where it belongs. |
| [2026-07-14_adjudication_and_representation_plan.md](2026-07-14_adjudication_and_representation_plan.md) | Active plan, the game lane: **the app adjudicates** (answers next-horizons B.4 and releases its fork-gated lane 4). Resolve once, replicate the outcome, represent locally: `ActionIntent` → `ActionResolved { roll, outcome, deltas, beats }`, applied by peers who never rerun Lua. **Beats** are one primitive for combat, environment, and emotes, carried by genet's CSS animation clock (Isometry is its first production consumer). **A0-A3 landed 2026-07-14: a knight swings at a goblin, the app decides whether it lands, and the goblin loses hit points.** Lua ABI widened to `f(c, t, roll)` so the hit rule ("beats AC") is one line of script, not a Rust branch; a client cannot pronounce its own verdict. A4 emotes and A5 pack choreography remain; defeat and client-initiated attacks are the named next pieces. |

## Archive

None yet. Retired plans go to `archive_docs/<YYYY-MM-DD>/`.
