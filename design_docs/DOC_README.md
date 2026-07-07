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
| [2026-07-07_next_horizons_landscape.md](2026-07-07_next_horizons_landscape.md) | Post-bootstrap landscape: six lanes (Lua docs, map scales/traversal, isometry-web, rulesets/licensing, generators/commands, serval GUI), recommended sequence, open forks |
| [2026-07-07_viewport_windowing_and_chrome_plan.md](2026-07-07_viewport_windowing_and_chrome_plan.md) | Active plan: viewport windowing (raise the ~30-40 board ceiling) + cheap GUI bundle (wheel scroll, drag, overlays) |
| [2026-07-07_optional_intelligence_vision.md](2026-07-07_optional_intelligence_vision.md) | Vision (post-keystone): optional DM-loaded models/agents; the DM-in-the-loop dialog system, conversation economy, inference architecture, and the dynamic-content opportunity catalog |

## Archive

None yet. Retired plans go to `archive_docs/<YYYY-MM-DD>/`.
