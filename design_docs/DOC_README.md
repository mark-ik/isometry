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

Rebuilt 2026-08-08 after the wing audit's archive pass: ten plans moved to
`archive_docs/2026-08-08/` with residues extracted to the receipts ledger.

| Doc | What it is |
| --- | ---------- |
| [DOC_POLICY.md](DOC_POLICY.md) | Documentation governance |
| [PROJECT_DESCRIPTION.md](PROJECT_DESCRIPTION.md) | Product goals and pillars (maintainer-owned) |
| [2026-08-08_protocol_hardening_plan.md](2026-08-08_protocol_hardening_plan.md) | **Active, first in the audit order. H0 landed 2026-08-08.** Versioned `Intent -> Resolved` for doorway transitions and overmap travel (replacing `Traveled { token }` peer derivation, a live resolve-once violation); protocol version, request identity, idempotency, and unsupported-version refusal on the envelope; the late-join replay receipt. Gates H0-H2, H1-H2 open. |
| [2026-08-08_stickleback_migration_plan.md](2026-08-08_stickleback_migration_plan.md) | **Active, second.** `campaign_sync.rs` off hand-assembled `LogSync`/`SyncedSpace` onto Stickleback `JoinedSpace`; Isometry keeps domain grammar, authorization, materializer, tactical sequencer; the no-second-runtime gate governs. Gates K0-K2. |
| [2026-08-08_extracted_receipts.md](2026-08-08_extracted_receipts.md) | The archive pass's extraction ledger: every residue from the ten archived plans (unmet headed/network receipts, N3, campaign-pack splits, worldbuilding residue, C7 receipts, the preserved diagonal ruling, the exploration headed receipt), each pointing where it lands. |
| [2026-08-02_overmap_presentation_plan.md](2026-08-02_overmap_presentation_plan.md) | **Active**, with two audit prerequisites reopened: a neutral region-paint seam (sprigging's `GraphCanvas` privately owns paint order; Mesocosm's minimap is the second consumer justifying the neutral layer) and hulls derived from final displayed positions incl. overrides, with uniform-position/unplaced/override/parallel-route/headed receipts. Also carries the recorded product direction: **source-time as a feature** (believed-then vs known-now vs retconned), the wing's claim carrier at campaign scale. |
| [2026-07-09_shared_authority_and_collaborative_building_plan.md](2026-07-09_shared_authority_and_collaborative_building_plan.md) | Re-scoped 2026-08-08: the **no-second-runtime sequencing gate stands**; the earlier tiers (host-owned stores, peer Lua revalidation, secrets, commit-reveal) are superseded by the Stickleback migration plan. Kept for the gate and the campaign grammars. |
| [2026-07-08_environmental_surfaces_plan.md](2026-07-08_environmental_surfaces_plan.md) | Design lane, **active only after its authority rewrite**: core stores surfaces and applies explicit deltas; Lua/system resolution chooses propagation once. |
| [2026-07-20_perf_and_cambification_plan.md](2026-07-20_perf_and_cambification_plan.md) | **Narrowed** to search/whisper text-field adoption and current file-size debt. |
| [2026-07-07_optional_intelligence_vision.md](2026-07-07_optional_intelligence_vision.md) | Vision record, **parked**; refresh authority and model assumptions only when activated. |

## Archive

`archive_docs/2026-08-08/` (audit pass; residues in the extracted-receipts
ledger): bootstrap (I0-I6 landed), next-horizons landscape, board
narration (N1/N2 landed), viewport/windowing, campaign packs (bulk
landed), worldbuilding (W0-W5 landed), adjudication (complete; law
preserved in the protocol plan), gameplay roadmap (C1-C7, C9a landed),
tile geometry seam (diagonal ruling preserved in the ledger), exploration
mode (E0-E6 landed).

None yet. Retired plans go to `archive_docs/<YYYY-MM-DD>/`.
