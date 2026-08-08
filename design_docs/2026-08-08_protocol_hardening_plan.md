# Protocol Hardening Plan (2026-08-08)

**Status: plan.** Founded from the 2026-08-08 wing audit's two protocol
findings, extracting the corrective work from the
[adjudication plan](archive_docs/2026-08-08/2026-07-14_adjudication_and_representation_plan.md)
and the
[gameplay roadmap](archive_docs/2026-08-08/2026-07-14_gameplay_roadmap_plan.md)
before their archival. The adjudication plan's law is untouched and
governs here: **the system rules once; every peer applies, never
re-derives.**

## 1. The violation

`Traveled { token }` (roadmap C-series) makes every peer derive the
destination, landing position, replacement identity, inventory remaps,
clock reconciliation, and map activation from a token. The live
application path repeats that adjudication per peer. That is resolve-once
broken in the two highest-traffic places: doorway transitions and overmap
travel.

## 2. The correction

Replace both transition and overmap travel with versioned
`Intent -> Resolved` payloads that **name every resulting consequence
explicitly**: destination map, landing, identity replacement, inventory
remaps, clock reconciliation, and activation. Peers apply the payload;
nothing is re-derived. This is the same envelope shape `ActionResolved`
already uses for tactical actions, extended to the two travel paths.

## 3. Protocol envelope hardening

`ActionIntent` / `ActionResolved` (isometry-net `protocol.rs`) gain, as
one versioned change:

- an explicit **protocol version**;
- **request identity** (who asked, which request);
- an **idempotency rule** (applying the same `Resolved` twice is a no-op,
  by identity, not by luck);
- **unsupported-version refusal** (a peer that cannot speak the version
  refuses legibly instead of misapplying).

## 4. Gates

**H0 — Envelope fields.** Version, request identity, idempotency,
refusal, on the existing tactical path.
**Done when:** a duplicate `Resolved` is a proven no-op; an
unsupported-version peer refuses with a receipt; existing replays
re-green under the versioned envelope.

**H1 — Travel as Resolved.** Doorway transitions carry the full explicit
consequence payload.
**Done when:** a doorway crossing produces one `Resolved` naming every
consequence; peers apply without derivation; **late-join replay is
proven** (the adjudication plan's missing receipt): a peer joining after
the transition reconstructs identical state from the log alone.

**H2 — Overmap travel as Resolved.** Same treatment for overmap
journeys.
**Done when:** an overmap journey is one explicit `Resolved`; split-party
clocks reconcile from the payload, not from peer derivation; a headed
two-peer receipt exists.

## 5. Stop rules

- No peer derives a consequence, ever. If a consequence is expensive to
  name explicitly, that is the design telling you it was never one
  consequence.
- Version negotiation refuses; it never degrades silently.
- Extracted receipts from the archived plans
  ([ledger](2026-08-08_extracted_receipts.md)) that touch travel land
  here, not in a revived roadmap.

## Progress

- **2026-08-08:** founded from the audit; adjudication and roadmap plans
  archived with their residues extracted.
