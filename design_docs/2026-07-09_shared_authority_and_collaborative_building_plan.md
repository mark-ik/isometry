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

**Implementation boundary (2026-07-09):** `HostSession` now owns its private
`CampaignStore`, and the desktop host persists the public snapshot plus GM
layer and Codicil history through a versioned Muniment checkpoint. A fresh
`--host --campaign <name>` restores and reconciles that checkpoint. Tier 1
still needs to transfer it live, then add a Personae-backed signed host grant.
Personae's current identity/signature primitives fit that grant; its
capability-grant layer is not yet a dependency Isometry should pretend exists.
The desktop bridge is now an Armillary actor: Tokio/iroh ownership stays off
the winit kernel, while typed snapshot, campaign, and history updates return
to the kernel for UI and persistence. That is the local ownership boundary the
live-transfer state machine will build on.

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

**The three modes (2026-07-09, revised from a two-mode draft).** Edit mode,
creative mode, survival mode. Today's model is one player in **edit mode**
(the DM authors freely: editor, secrets, commits) while everyone else plays
**survival** (priced, validated moves only). **Creative mode is not "edit
for everyone"**: it keeps the turn-based structure and makes a *game* of
building the world together, with its own rulebook.

**The definitional collapse (2026-07-09):** "the GM" is not a protocol
role; **the GM is whoever is in edit mode, and that need not be one
person.** Two consequences:

- **Global view follows edit mode.** Only edit-mode holders see the GM
  layer. With multiple simultaneous holders (co-DMs), the private
  `CampaignStore` replicates among them over a directed channel, the
  whisper shape, never the hashed public log. Decision 8 generalizes to
  three replication rings: the public log (everyone, hash-converged), the
  GM ring (edit-mode holders, directed sync), and whispers (pairs).
  Players talk freely, as always; table talk was never consensus state.
- **Edit mode and sequencing are separable.** Holding edit mode grants
  authoring and the global view, not necessarily log ordering: tier 2's
  rotating turn-leader can sequence while two edit-mode holders author.
  Tier 1's "host handoff" decomposes into two transfers that usually but
  not necessarily travel together: the sequencer role and edit-mode
  membership. The symmetry that makes
this cheap: survival mode validates moves against game rules; creative mode
validates contributions against **world-shape rules**. Both are pack/plugin
data; both run through the same intent/commit pipeline.

- Modes are **per-channel and per-player**, not global: survival on the
  battle map while the journal is in a creative round; one trusted player
  granted edit over their own home village. A mode is a permission-plus-
  rulebook preset over event channels, configured by pack/table policy.
  The same preset shape covers **playing a faction** instead of or beside
  a PC (worldbuilding rung 7's participant upgrade): a faction player
  holds survival-mode permissions over the faction-turn channel.
- **Mode flips are logged events**, thrown by the DM (tier 1) or a vote
  (tier 3), so "a creative round during downtime, survival in the dungeon"
  replays correctly. Downtime-as-creative-window is the expected rhythm.

### Creative mode as a graph-tending game

The failure mode of "everybody edit freely" is mush: isolated characters,
orphaned plotlines, items nothing cares about. The best collaborative
worldbuilding games solve this with turn structure plus **attachment
rules**, and Isometry's node/edge world state can enforce them mechanically.

Prior art (adapt, don't copy):

- **Fiasco (setup phase):** the purest no-isolated-nodes mechanic shipped:
  every authored element (relationship, need, object, location) *is* an
  edge between two players; unattached things cannot exist.
- **Microscope:** nesting-as-connectivity (add only inside/adjacent to
  existing history), a per-round **focus** bounding contributions to a
  neighborhood, and the **palette** (agreed yes/no content lists: decorum
  as data).
- **Dawn of Worlds:** a costed action menu per turn (points economy for
  create-race / raise-mountain / found-city): creation priced *inside*
  creative play.
- **The Quiet Year:** prompt-driven turns; **contempt tokens** ritualize
  disagreement instead of blocking it.
- **Dresden Files city creation / Ex Novo / street magic:** attachment
  quotas (every location gets a face, every face a want).

The mechanic, composed for Isometry:

1. **Attachment rule (floor):** a contribution lands with at least one
   edge to existing content; cast-before-create (worldbuilding decision 9)
   already biases reuse. New nodes arrive via relationships.
2. **Shape constraints per kind (rules):** pack-authored min-connectivity
   schemas: a *significant* NPC needs a place, a stake, and a hook; an
   item needs an origin and a holder; a secret needs a reachable reveal
   condition. Checkable by the substrate exactly like movement range.
   Significance is the threshold; background extras need nothing.
3. **Gaps become the prompt deck (fun):** the app surfaces under-connected
   nodes, dormant storylet requirements, and dangling secrets as turn
   prompts ("the eel cult and the salt tax have never met: what does one
   think of the other?"). Satisfying a dormant requirement lights an arc
   up visibly: the game is making the world load-bearing, not just bigger.
4. **Rounds, focus, palette, veto (decorum):** turn order from the
   existing turn system; a declared focus bounds each round's
   neighborhood; the palette constrains players and generators alike (the
   same constraint data world laws already are); tombstones are the
   X-card floor beneath a contempt-style gesture.

Connectivity is not aesthetic. An unconnected fact is unreachable by
storylet requirements, invisible to the prompt engine, and dead weight in
play; the attachment rules are what make co-authored content *reachable*
by the machinery that pays it off later.

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
stop showing it; snapshot compaction can remove it from the active campaign
checkpoint. It cannot erase copies already received by peers or present in
backups, so the product promise is active-state removal plus a clear local
data-retention policy. Tombstone-then-compact keeps replay coherent without
claiming impossible retroactive deletion.

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
