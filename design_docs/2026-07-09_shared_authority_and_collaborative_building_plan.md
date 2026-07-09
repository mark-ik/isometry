# Shared authority and collaborative building

**Date:** 2026-07-09
**Status:** design record with one near-term candidate (tier 1). Tiers 2 and 3
are gated and deliberately not scheduled. This doc is the CLAUDE.md guardrail
("no rollback, no CRDTs; revisit only through a plan") being revisited through
a plan; its conclusion is that the guardrail survives every tier.
**Related:** [worldbuilding_generation_plan](2026-07-09_worldbuilding_generation_plan.md)
(decision 8 two-store split, W0 landed, the W2 generator ABI this doc leans
on), [campaign_packs_plan](2026-07-08_campaign_packs_plan.md) (decision 12
determinism discipline, which tier 3 promotes from optimization to
load-bearing), [optional_intelligence_vision](2026-07-07_optional_intelligence_vision.md)
(DM-authority as the trust boundary), and the personae suite vision
(capabilities as the one collaboration primitive; cross-repo,
`repos/personae/design_docs/2026-07-08_personae_across_the_suite.md`).

**Thesis:** sharing hosting duties, and even removing the DM entirely, does
not require CRDTs or rollback. The single ordered event log survives every
version of this. What varies is three separable things: who **orders** the
log, who **validates** intents, and who is allowed to **know secrets**. Those
three questions form a ladder, and turn-based play answers the ordering
question almost for free.

## Position

The current model (bootstrap decision: DM-authority, ordered log, FNV
convergence hash, no optimistic mutation) stays the spine. CRDTs solve
concurrent divergent edits that must merge; a tabletop session is a totally
ordered conversation with a referee, and every tier below keeps total order.
The referee just stops being a fixed person.

What the tiers change:

| | Orders the log | Validates | Holds secrets |
| --- | --- | --- | --- |
| Today | the DM's app | the DM's app | the DM (CampaignStore) |
| Tier 1 | the host-role holder | same | moves with the role |
| Tier 2 | the active turn's app | every peer (deterministic Lua) | the GM-grant holder |
| Tier 3 | the active turn's app | every peer | nobody (committed, sealed, audited) |

## Tier 1: the host role is transferable (near-term candidate)

DM-authority becomes a role, not a person. The session's identity is the log
plus the campaign, not the host's node id.

- **Handoff protocol:** a late-join in reverse. The outgoing host freezes
  intents (rejects with "host migrating"), transfers `GameSnapshot` + seq +
  log hash + the private `CampaignStore` to the incoming host, peers
  reconnect to the new ticket, play resumes. The convergence hash carries
  across, so a mid-session handoff is provably lossless.
- **Secret custody moves with the role.** Whoever holds the host role holds
  the GM layer. Two co-DMs taking shifts is this; so is "the DM's laptop
  died and the co-DM resumes from last save."
- **What it buys:** removes the single point of failure, enables co-DM
  shifts and rotating-DM campaigns (troupe-style play, one player DMs each
  arc), and forces the session-identity work that every later tier needs.
- **Personae fit (later):** the host role is naturally a signed transferable
  grant once personae lands in isometry; until then a host token in the
  campaign save is enough.

**Done when:** a session survives a live host handoff with both peers'
convergence hashes intact, the new host can reveal a secret the old host
authored, and a rejected mid-handoff intent is retried successfully against
the new host.

## Tier 2: mechanical authority (gated on demand + personae)

The DM stops being needed for the *rules* layer.

- **Validation is already distributable.** The system plugin is
  deterministic Lua over piccolo with no ambient state, so every peer can
  validate every intent identically. Nobody needs to be trusted for
  mechanics.
- **Ordering: the turn structure is a leader schedule.** The active player's
  app sequences events during its own turn; `TurnAdvance` is the handoff.
  Round-robin leadership the game already performs socially, no Raft, no
  per-event voting. Out-of-turn events (reactions, table talk, DM-less
  rulings) go to the current leader for ordering, same as intents go to the
  host today.
- **Honesty model: detectable and attributable, not prevented.** The
  convergence hash already detects divergence; personae-signed events make
  it attributable. At a table of four friends this is the right cost point,
  and it extends the log's existing philosophy rather than replacing it.

**Done when:** a full combat resolves with sequencing rotating per turn, all
peers validating locally, and an injected invalid event being rejected by
every peer independently (not just by a privileged host).

## Tier 3: the app runs the campaign (gated on W2)

No human GM. Three things the DM provided need mechanical replacements.

- **Dice: commit-reveal randomness.** Each peer commits a hash of a nonce,
  then reveals; the XOR seeds the roll batch. Two message rounds at
  turn-based cadence is cheap, and no single app can rig a roll. (Today's
  host-rolled `RollRecord` stays for DM-led play; this is a second dice
  mode, not a replacement.)
- **Rulings and choices: table actions.** A proposed ruling or storylet
  choice commits after a majority vote, or the storylet's cast role-owner
  decides for their own scene. Just another event kind in the intent/commit
  shape.
- **Secrets: commit now, audit later.** Decision 8 assumed a human allowed
  to know everything; without one, generation must run identically on every
  peer, which makes **seed-replay (campaign-packs decision 12) load-bearing
  instead of a bandwidth optimization**, with all of W2's determinism
  discipline as the entry fee. Every peer's app computes the hidden layer
  but does not show it. Reading your own process memory cannot be cheaply
  prevented, so fairness comes from auditability: generated secrets are
  hash-committed into the log at generation time, every reveal is checked
  against its commitment (no retcons), and at campaign end the seed unseals
  so anyone can verify the hidden layer was played straight. Trust, but
  verify at the epilogue. The upgrade path for stronger tables is
  threshold-splitting the seal key so a reveal needs a quorum; do not scope
  it before someone asks.

**Done when:** a four-player, no-DM one-shot completes: seed sealed at
start, commit-reveal dice throughout, one voted ruling, one storylet whose
reveal matches its commitment, and a post-game audit that reproduces the
hidden layer from the unsealed seed.

## Collaborative world, campaign, and content building

Shared authority is not only about running sessions; the same machinery
lets the table *author* together. Two modes, one mechanism.

**The creative/survival frame (2026-07-09).** The compact way to say all of
the below: today's model is already one player in creative mode (the DM
authors freely: editor, secrets, commits) while everyone else plays
survival (priced, validated moves only). Collaborative building generalizes
who holds creative permissions, over which channels, and when:

- The mode is **per-channel and per-player**, not global: survival on the
  battle map while the world journal is creative; one player creative over
  their own home village. "Mode" is a permission preset over event channels
  (map edits, facts, drafts, storylet proposals), configured by pack/table
  policy, not hardcoded.
- **Survival authoring** is creation with a price and a gate (the
  plugin-priced declaration below); **creative authoring** is the same
  event with price and gate turned off. One vocabulary, one dial.
- **Mode flips are logged events**, thrown by the DM (tier 1) or a vote
  (tier 3), so "a creative window during downtime, survival in the dungeon"
  replays correctly. Downtime-as-creative-window is the expected rhythm.

### Prep-time: co-authoring the campaign pack

- **The preview table goes multiplayer.** The worldbuilding plan's
  Generate / Reroll / Lock / Commit surface stops being DM-only: any peer
  can propose (generate or hand-write), lock fields they feel strongly
  about, and the commit gate depends on tier (DM commits in tier 1, quorum
  commits in tier 3). A proposal is data long before it is true; commit is
  what makes it true. This is the existing generator governance applied to
  people.
- **A draft channel beside the play log.** World-draft facts, places,
  factions, and storylet sketches accumulate in a draft log with the same
  `WorldFact` envelope (`kind: "draft"`), then bake into pack data on
  acceptance. Drafts are cheap and disposable; the pack is the artifact.
- **Attribution is provenance, not decoration.** Draft and committed facts
  carry an author field (a name now, a personae signature later), so a
  finished pack can credit its table and a contested fact has a history.

### Session-time: players add truths

Prior art here is rich and maps cleanly:

- **Microscope-style history rounds:** each player in turn adds a history
  event / place / era. Mechanically identical to a storylet proposal round
  with rotating proposer, riding tier 2's turn-as-leader-schedule.
- **Fate-style declarations and FitD flashbacks/devil's bargains:** a
  player spends a system-plugin resource to declare a fact ("I know a guy
  here"). The declaration is a `WorldFact` intent whose *cost* the system
  plugin validates mechanically and whose *acceptance* is the table's
  commit gate. The substrate never decides if it is good fiction; it
  decides if it was paid for and agreed to.
- **Player-authored secrets:** a player can author a fact hidden from the
  *other* players (a secret backstory tie). It enters the GM layer (or its
  tier-3 committed-sealed equivalent) authored-by-player, revealable by
  the usual conditions. Hidden information stops being DM-exclusive.

### Safety and retraction

An append-only log needs a first-class retraction story before strangers
co-author anything. A `Retract(fact_id)` event tombstones a fact and views
stop showing it; actual byte removal happens at snapshot compaction (the
save file materializes state, not history), so an X-card moment does not
live forever in the campaign file. Tombstone-then-compact keeps replay
coherent and makes deletion real.

### What this costs

Almost nothing new at the substrate level: the `WorldFact` envelope, the
two-store split, the preview-table UI, and the intent/commit shape all
exist or are already planned in W0-W4. The genuinely new pieces are the
draft channel, the author field, votes as an event kind, and tombstones,
each a small vocabulary addition rather than a system.

## Sequencing

1. Tier 1 host handoff, when session robustness starts to matter (first
   candidate after the worldbuilding ladder's early rungs).
2. Collaborative prep (draft channel + multiplayer preview) with W4's
   storylet/world-fact layer, DM-gated commit first.
3. Tier 2 with personae-in-isometry, pulled by demand for DM-less mechanics.
4. Tier 3 with the W2 ABI's determinism discipline proven, pulled by demand
   for DM-less campaigns.

Nothing above changes the substrate invariants: geometry and turns in core,
rules in plugins, one ordered log, convergence by hash.

## Open questions

1. Host handoff transport: does iroh let peers re-dial a new node cleanly
   mid-session (new ticket distribution over the old connection), or does
   tier 1 v1 accept "everyone rejoins from the new host's ticket"?
2. Does the draft channel live in the session log (simple, replays with the
   campaign) or beside it (keeps play replay lean)? Bias: beside, same
   pattern as the CampaignStore.
3. Votes: simple majority of connected peers, or role-weighted (the
   storylet's cast owner outvotes)? Bias: keep it data the pack/system can
   configure; the substrate just counts.
4. Retraction vs the convergence hash: tombstones replay fine, but does
   compaction-after-tombstone need a resync checkpoint event so late
   joiners' hashes still converge? (Likely yes: compaction mints a new
   snapshot baseline, the same shape as a late join.)

## Progress

- 2026-07-09: Doc created from the shared-hosting / group-consensus
  discussion, extended with the collaborative-building modes. Tier 1
  named the only near-term candidate; tiers 2-3 gated on personae and W2
  respectively.
