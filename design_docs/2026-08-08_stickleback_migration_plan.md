# Stickleback Consumer Migration Plan (2026-08-08)

**Status: plan.** Founded from the 2026-08-08 wing audit. The
[shared-authority plan](2026-07-09_shared_authority_and_collaborative_building_plan.md)'s
**sequencing gate stands and governs**: no second Isometry runtime. Its
earlier tiers (host-owned private stores, peer-side Lua revalidation,
regenerated secrets, commit-reveal mechanics) are **superseded by this
plan** and stamped so at the source.

## 1. The debt

Live `campaign_sync.rs` (isometry-net) still assembles `LogSync` and
`SyncedSpace` directly. That is pre-rebase construction: Isometry doing
by hand what Stickleback's `JoinedSpace` now owns.

## 2. The boundary, stated once

Move campaign collaboration onto Stickleback `JoinedSpace`. **Isometry
retains**: its domain grammar (campaign operations), authorization, the
materializer, and the tactical sequencer. **Stickleback owns**: space
membership, log transport, sync. The seam is the same shape as every
other consumer rebase in the stack: domain sovereignty over a shared
carrier.

## 3. Gates

**K0 — JoinedSpace adoption.** `campaign_sync.rs` consumes `JoinedSpace`;
direct `LogSync`/`SyncedSpace` assembly deleted.
**Done when:** the existing multi-writer campaign receipts re-green on
the new path and the deleted assembly has no survivors (grep receipt).

**K1 — Authority receipts on the new carrier.** Authorization,
materializer, and conflict handling proven unchanged over Stickleback
transport.
**Done when:** a two-writer concurrent-edit receipt and a
refused-unauthorized-operation receipt both pass on `JoinedSpace`.

**K2 — Superseded-tier sweep.** The old tiers' machinery (private stores,
peer revalidation, secrets, commit-reveal) either deleted or explicitly
retained with a named reason.
**Done when:** the shared-authority plan's tier sections carry per-tier
dispositions and the code matches them.

## Stop rules

- The sequencing gate outranks this plan: any step that would mint a
  second runtime stops.
- Pack distribution over Stickleback (campaign-packs residue) waits for
  K0-K1; it does not drive them.

## Progress

- **2026-08-08:** founded from the audit; shared-authority plan stamped.
