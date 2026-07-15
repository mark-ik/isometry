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
| [2026-07-14_adjudication_and_representation_plan.md](2026-07-14_adjudication_and_representation_plan.md) | Active plan, the game lane: **the app adjudicates** (answers next-horizons B.4 and releases its fork-gated lane 4). Resolve once, replicate the outcome, represent locally: `ActionIntent` → `ActionResolved { roll, outcome, deltas, beats }`, applied by peers who never rerun Lua. **Beats** are one primitive for combat, environment, and emotes, carried by genet's CSS animation clock (Isometry is its first production consumer). **A0-A4 landed 2026-07-14, plus defeat: a fight can be won.** A knight swings at a goblin, the app decides whether it lands, the goblin loses hit points, drops at zero, stops taking turns and stops being a legal target, and the winner cheers. Lua ABI widened to `f(c, t, roll)` so the hit rule ("beats AC") and the defeat rule (`hp_current <= 0`) are each one line of script rather than a Rust branch; the substrate obeys a generic out-of-play set without knowing why. A client cannot pronounce its own verdict, but may emote its own tokens (nothing to forge). **Force (2026-07-14)** splits a *stagger* (a blow rocks you back and you recover: representation, never replicated, may never feed a rule) from *forced movement* (Shove, Thunderwave: the tile really changes, so the board rules on the landing square and every peer applies it). Both share one geometry and one directional keyframe family, run in opposite directions. **A player may act (2026-07-14):** a client sends an *ask* (`NetMessage::Action`), never a verdict; the rules-blind session checks only ownership and queues it, and the host app resolves it through the same path its own swings take. **Choreography is pack data (A5, 2026-07-14):** the beat vocabulary moved out of the app into a `core` pack's `beats/*.css`; a campaign draws its own swing and picks its own emotes by declaring beat names, and the app declares no beat keyframes of its own. **The plan is complete.** |
| [2026-07-14_gameplay_roadmap_plan.md](2026-07-14_gameplay_roadmap_plan.md) | Active plan: the gameplay order Mark set 2026-07-14 (conditions → regions/transitions → split-party time → generators → parties/recruitment → dialogue → factions → world map → pack options). **C1 conditions landed 2026-07-15** (answers next-horizons B.5): conditions are substrate-visible opaque names; the system's Lua computes their mechanical projection (speed, sight) and the numbers replicate; `MOVE_BUDGET`/`SIGHT_RADIUS` demoted to defaults; `trip` is the first condition-inflicting action; fog is per-token sight. Next: C2 regions + transition points. |

## Archive

None yet. Retired plans go to `archive_docs/<YYYY-MM-DD>/`.
