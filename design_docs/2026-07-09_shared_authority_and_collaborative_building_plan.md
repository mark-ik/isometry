# Shared authority and collaborative building

**Date:** 2026-07-09
**Status:** revised implementation plan (2026-07-11). The original conclusion
that one ordered log should survive every tier was too broad. The first
multi-writer campaign-space slice is landed behind `isometry-net`'s
`campaign-p2p` feature; tactical play still uses the existing sequencer.
**Related:** [worldbuilding_generation_plan](2026-07-09_worldbuilding_generation_plan.md)
(decision 8 two-store split, W0 landed, the W2 generator ABI this doc leans
on), [campaign_packs_plan](2026-07-08_campaign_packs_plan.md) (decision 12
determinism discipline, which tier 3 promotes from optimization to
load-bearing), [optional_intelligence_vision](2026-07-07_optional_intelligence_vision.md)
(DM-authority as the trust boundary), and the personae suite vision
(capabilities as the one collaboration primitive; cross-repo,
`repos/personae/design_docs/2026-07-08_personae_across_the_suite.md`).

**Thesis:** a campaign is a signed multi-writer space; a combat exchange may
still need a temporary sequencer. Consistency is chosen per domain type rather
than imposed campaign-wide. p2panda supplies signed per-author logs and sync;
Isometry supplies type-specific materializers and policy. Moot names the group
and its governance, Murm carries private channels, Personae supplies identities
and grants, and Iroh carries p2panda's QUIC, gossip, and blobs.

## Position

The current host-authoritative model remains a valid single-player and
traditional-table projection. It is not the ownership model of a campaign.
Campaign authors write independently signed operations, keep concurrent work,
and derive views from the complete operation set. A pack-selected policy may
recognize a proposal as a campaign head, but recognition does not erase the
proposal, its endorsements, or a competing branch.

The tactical lane keeps total order where timing and contention require it:
initiative, movement into contested cells, resource spending, reactions, and
resolved randomness. The sequencer is a scoped lease or turn role, not the
campaign owner. Facts, drafts, maps, dialogue contributions, pack changes, and
most downtime authoring should not pass through that bottleneck.

What the tiers change:

| Domain | Write model | Resolution | Carrier |
| --- | --- | --- | --- |
| Campaign authoring | signed multi-writer operations | type-specific materializer + group policy | p2panda LogSync/gossip over Iroh |
| Live tactics | intents into a temporary sequencer | deterministic rules, ordered commits | current Isometry session lane, later shared transport |
| Group membership and policy | signed social records | Moot fold / configured governance | Moot over the same p2panda substrate |
| GM and secret channels | capability-scoped members | recipient set and reveal policy | Murm/private spaces; sealed blobs where needed |
| Single player | one writer and one local sequencer | the same folds with one author | local Muniment backend |

## Tier 1 compatibility path: the host role is transferable

DM-authority becomes a role, not a person. This keeps traditional hosted play
robust, but it is no longer the prerequisite for collaborative campaign
ownership. The session's identity is the tactical log plus campaign-space
heads, not the host's node id.

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

**The definitional collapse (revised 2026-07-11):** "the GM" is not the
campaign owner. It is a bundle of configured capabilities: global view,
secret custody, unrestricted authoring in selected channels, and perhaps a
tactical sequencing lease. Those capabilities may be split across people or
left unused. Two consequences:

- **Global view follows a capability.** Authorized holders receive encrypted
  secret records over a Murm/private space. Public campaign operations remain
  in the p2panda campaign space; live tactical commits remain in the ordered
  session lane. Pairwise whispers are simply smaller private spaces. This is
  access control over records, not a second owner for public campaign truth.
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

The shared substrate is largely available, but the domain work is real. Each
collaborative type needs an operation grammar, validation, a deterministic
materializer, conflict-preserving UI, policy hooks, retraction semantics, and
offline/convergence tests. The first generic shape now exists for campaign
proposals, endorsements, and recognition. Maps, facts, inventory, dialogue,
packs, secret grants, and tactical lease transfer remain separate migrations.

## Sequencing

1. Campaign proposal space: create/apply/branch envelopes, signed operations,
   endorsements, recognition, and convergence over Muniment-backed p2panda
   stores. **Landed locally 2026-07-11.**
2. Compose LogSync/gossip through shared `transport::SyncedSpace`; prove two
   Personae authors converge after live exchange and offline catch-up.
   **Landed locally 2026-07-11.**
3. Bind campaign membership and recognition policy to Moot; bind private
   campaign channels and secret delivery to Murm. **Policy evaluation, the
   group-scoped frozen-Moot-electorate bridge, signed campaign-to-Moot
   association, and signed policy selection/change records landed locally
   2026-07-11. The opt-in `campaign-moot` host composition now loads the live
   Moot store and derives contexts from its signed membership commitment. The
   signed adopt-or-branch resolution grammar, policy-gated materialization, and
   host-fed competing-binding surface landed locally 2026-07-12. Same-Moot
   founding conflicts use that Moot's admission context; later conflicts use
   the campaign's current binding. Unrelated founding Moots remain restricted
   because neither electorate can erase the other's claim. Murm's signed
   Join/Leave fold and deterministic membership commitment now back a private Isometry
   `{cabal, channel, members, revision}` secret-audience binding. Actual secret
   publication/receipt, payload sealing, cabal-key rotation after removal, and
   desktop actor wiring are deferred by the 2026-07-12 Murm peer-runtime plan:
   `murm-replication` must own accepted-operation processing, service lifecycle,
   retention, checkpoints, and native-drop import before Isometry rebases as a
   later domain consumer. Do not add a second Isometry actor or private-delivery
   path against the retiring host-composition seam.**
4. Migrate collaborative domains one at a time: drafts/facts, maps, packs,
   dialogue, inventory/equipment, then world simulation records. Each chooses
   its own merge/materialization rule and ships conflict UI with it.
5. Recast the existing host session as the tactical sequencer. Add signed lease
   transfer and deterministic peer validation without making the lease holder
   campaign owner.
6. Add commit-reveal randomness and sealed/auditable secrets for tables whose
   configured policy requires them.

The stable substrate invariants are now: geometry and turns in core, rules in
plugins, signed immutable operations, explicit domain materializers, and total
ordering only where the domain requires it.

## Next Game Slice

While the peer-runtime rebase is underway, the next useful Isometry work is a
complete targeted-action loop in the existing tactical sequencer. Today the
system can derive an action's dice expression and the UI can append its roll to
the shared log, but no action names a target or changes game state.

The slice is deliberately narrow:

1. A player submits an `ActionIntent { actor, target, action_key }`; the host
   checks turn ownership, target existence, range, and any system-defined
   prerequisites.
2. The injected rules system resolves that intent using host entropy into one
   `ActionResolved` event containing the public roll, hit/miss result, and typed
   sheet deltas. Peers apply the event, never rerun Lua or roll dice.
3. The first SRD action is adjacent melee attack against AC, changing separate
   `hp_current` and `hp_max` fields. Defeat and conditions can remain follow-ons;
   an HP change and explicit miss already make positioning, turns, equipment,
   sheets, and rolls one playable loop.
4. The sheet action enters target-pick mode; clicking a token submits the intent;
   the board and roll log show the resolved result. Local and remote hosts use
   the same resolver path.

**Done when:** two peers and solo play produce the same resolved action event
for a fixed host entropy tape; an out-of-range, wrong-turn, or missing-target
intent changes nothing; a successful attack changes only the target's current
HP; and the UI can select and resolve an attack without a GM editing sheet
fields by hand.

## Open questions

1. Host handoff transport: does iroh let peers re-dial a new node cleanly
   mid-session (new ticket distribution over the old connection), or does
   tier 1 v1 accept "everyone rejoins from the new host's ticket"?
2. Which Moot policy recognizes a proposal by default? The evaluator now
   supports any eligible member, fixed threshold, fractional threshold, and
   unanimity without hardcoding one. Explicit owner/grant policy waits on the
   Personae capability layer. Packs or a signed Moot policy operation must
   select the default.
3. Which operation families need true CRDT data types beyond set-valued folds?
   Map cells, ordered dialogue choices, inventory uniqueness, and counters must
   each name their conflict rule before migration.
4. Retraction vs the convergence hash: tombstones replay fine, but does
   compaction-after-tombstone need a resync checkpoint event so late
   joiners' hashes still converge? (Likely yes: compaction mints a new
   snapshot baseline, the same shape as a late join.)

## Progress

- 2026-07-09: Doc created from the shared-hosting / group-consensus
  discussion, extended with the collaborative-building modes. Tier 1
  named the only near-term candidate; tiers 2-3 gated on personae and W2
  respectively.
- 2026-07-11: Rejected campaign-wide single authority and single-log ordering.
  Added `CampaignProposalMode::{Create, Apply, Branch}` and a feature-gated
  campaign p2panda space over `mooting::MunimentStore`. Personae signs
  per-author operations; the deterministic view preserves concurrent proposals,
  endorsements, and recognized heads. Opposite arrival-order convergence and
  tamper/cross-campaign rejection are tested. Shared transport + `SyncedSpace`
  also pass a two-peer Personae test covering offline LogSync catch-up, live
  propagation, and equal final views. The live tactical sequencer is
  intentionally unchanged pending type-by-type migration.
- 2026-07-11: Added reusable recognition policy to Mooting and a
  `MootRoster` bridge that freezes the electorate at a signed revision.
  Isometry now distinguishes recognition claims from policy truth: outsider
  endorsements remain auditable but do not count, pre-threshold claims remain
  pending, and claims for stale electorate revisions do not produce applicable
  heads. This established the evaluator used by the association slice below.
- 2026-07-11: Added signed `GovernanceProposed` / `GovernanceClaimed` campaign
  operations. Initial association must pass the target Moot's admission
  context; later policy changes or Moot migrations must pass the campaign's
  current bound policy, so a destination cannot authorize its own takeover.
  Electorate fingerprints now include the Moot id, closing identical-roster
  cross-Moot replay. Invalid bindings are rejected and competing accepted
  bindings remain visible for explicit resolution rather than lowest-hash or
  last-writer selection.
- 2026-07-11: Added provider-neutral Moot authoring and deterministic
  `MootRoster::membership_revision`, committed only to winning signed join
  operations. Added Isometry's `campaign-moot` composition layer, which loads a
  live `MootStore` and derives campaign/admission/change contexts without
  caller-supplied revision bytes. Verified that Personae-authored membership
  counts, fauna does not invalidate recognition, and a later join makes an old
  claim stale under the new electorate.
