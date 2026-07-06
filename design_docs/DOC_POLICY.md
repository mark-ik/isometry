# Documentation Policy

Adapted from the woodshed DOC_POLICY. Isometry has a single subsystem
(the app) plus a pure substrate crate, so the policy is intentionally
light.

## Core Principles

### 1. Control Doc Growth

Add to existing docs unless the material is substantial (>500 words),
covers a distinct topic, and is unrelated to any current document. Keep
total doc count low. Do not create files for one-time analyses.

### 2. Eliminate Redundancy

Audit before commits or after substantial changes. Newer documents are
generally more authoritative. If two docs disagree, reconcile them; do
not let drift accumulate.

### 3. No Legacy Friction

When a path changes, optimize for clean fit with the new path. Do not
preserve obsolete parallel systems or migration shims unless explicitly
needed for real-user data. Tests track current semantics only.

### 4. Location and Archival

- **Active docs**: live directly in `design_docs/`. Subdirectories may be
  added when a domain accumulates enough material to justify one. Until
  then, flat is fine.
- **Archive**: `design_docs/archive_docs/<YYYY-MM-DD>/` for retired plans
  and superseded notes. Move there rather than delete; delete only with
  rationale and confirmation.
- **Cross-references**: relative links.

### 5. README Requirements

`design_docs/DOC_README.md` is the canonical index for `design_docs/`. It
must contain:

- AI-assistant working principles for this project
- Index of all active docs with one-line descriptions
- Pointers to `DOC_POLICY.md` and `PROJECT_DESCRIPTION.md`

When docs are added, removed, or moved, `DOC_README.md` is updated in the
same session. If any other index disagrees with `DOC_README.md`,
`DOC_README.md` wins.

### 6. PROJECT_DESCRIPTION.md Ownership

`PROJECT_DESCRIPTION.md` is reserved for the maintainer. Do not edit
without explicit instruction. Treat it as authoritative; surface
contradictions for discussion.

The root `README.md` is derived from `PROJECT_DESCRIPTION.md` and current
authoritative docs. Speculative features without plans only appear in
`PROJECT_DESCRIPTION.md`.

### 7. Implementation Plans

Active plans are named `<YYYY-MM-DD>_<keyword>_plan.md` and carry:

- A dated **Status** line kept current (plan, in progress, landed,
  superseded by X).
- Phases with **done-conditions**, not time estimates.
- A **Findings** section for verified facts discovered during the work,
  dated, with code references.
- A **Progress** log, dated, appended as phases land.

Code samples in plans state whether they are illustrative or
compile-ready.

When a plan completes, extract any deferred or open points into a new
plan (or an existing one) before archiving it.
