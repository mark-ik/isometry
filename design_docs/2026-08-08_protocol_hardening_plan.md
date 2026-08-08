# Protocol Hardening Plan (2026-08-08)

**Status: in progress (2026-08-08).** H0 and H1 landed; H2 open. Founded
from the 2026-08-08 wing audit's two protocol
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

**H0 — Envelope fields. LANDED 2026-08-08.** Version, request identity,
idempotency, refusal, on the existing tactical path.
**Done when:** a duplicate `Resolved` is a proven no-op; an
unsupported-version peer refuses with a receipt; existing replays
re-green under the versioned envelope.

**H1 — Travel as Resolved. LANDED 2026-08-08.** Doorway transitions carry
the full explicit consequence payload.
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

## Findings

- **2026-08-08.** **The version belongs in the handshake, not on every
  message.** Postcard is not self-describing, so a peer reading another
  build's frame usually fails to decode it and the message never arrives
  at all. A per-message version field would therefore be unreachable
  exactly when it is most needed, and would cost a field on every
  message to guard a case the framing already fails. What it *can* guard
  is the other half: a body that decodes and means something else. That
  needs the version to arrive before any state does, which is what the
  handshake gives: the client's `Hello` and the host's `Snapshot` are
  each the first thing that side sends. Both are also already
  asymmetric-first messages, so no new round trip was added.
  (`crates/isometry-net/src/protocol.rs`, `PROTOCOL_VERSION`.)

- **2026-08-08.** **`apply_game` was the only honest home for the
  idempotency ledger.** Three separate call sites apply events: the host
  (`try_commit`), the client (`drain_pending`), and the source-time
  replay (`GameSourceHistory::snapshot_at`, which re-runs a Codicil
  prefix over an origin snapshot). A guard held in the sessions would
  have missed the third and would have to be remembered at each new one.
  Putting the seen-set in `GameSnapshot` makes the guard part of the
  event's own application and replicates it, which a late joiner needs:
  it seeds from a snapshot holding a resolution's *effects* but not its
  history, and would otherwise take a replayed verdict a second time.
  Cost: one field on 15 struct literals across the workspace.

- **2026-08-08.** **A per-peer watermark was rejected in favour of a
  seen-set.** A "highest nonce applied" watermark is O(peers) instead of
  O(actions), but a client reconnecting restarts its nonce counter while
  keeping its `PeerId` (which the transport derives from the node id), so
  the watermark would silently swallow every real action after a
  reconnect. Silently dropping verdicts is worse than a set that grows by
  16 bytes an action, so the set is uncapped, on the same reasoning the
  journal already carries.

- **2026-08-08.** **Request identity had to be the authority's word.** The
  client cannot name its own `PeerId` (the host derives it from the
  connection), and a client that could would be able to number its
  request as another player's and have *their* next verdict discarded as
  a duplicate. So the split is: the asker owns the nonce (it needs to
  match an answer to its question), the host owns the peer half and
  restamps it on receipt. A client reusing its own nonce only harms
  itself, which is acceptable and documented.

- **2026-08-08.** **A version-refused peer could still have written to the
  log.** `intent_refusal` accepts `GameEvent::Rolled` from anybody by
  design (the friendly-table trust model), so refusing only the `Hello`
  would have left a mismatched peer able to push dice into the shared
  log. The refusal is therefore recorded per peer and checked *before*
  `on_message` matches at all. A related pre-existing hole is untouched
  and out of H0's scope: a peer that never sends `Hello` is not gated
  either, it merely owns no token, so `Rolled` still reaches the log from
  a silent peer.

- **2026-08-08 (H1).** **The activation rule had to be read one moment
  earlier, and that is what made it nameable.** The derivation asked "is
  any player-owned token still on this map" *after* removing the
  traveler. A resolver runs before anything moves, so the same question
  has to be asked as "is anyone but the traveler still here". Identical
  answer, different vantage, and the different vantage is the whole
  trick: a consequence can only be named in advance if it can be decided
  in advance. Every field of the payload turned out to be decidable
  pre-departure; the one that needed re-framing was this one.

- **2026-08-08 (H1).** **The carry is not a consequence, so it is not on
  the wire.** The traveler's sheet, conditions, mobility, and defeat move
  through the door with it. It is tempting to name them too, in the name
  of explicitness, but they are not decisions: they are the token still
  being itself on the other side, and restating them would put a copy of
  replicated state into every crossing. What the payload names is every
  place the old code made a *choice* (which door, which map, which tile,
  which id, which inventories, what the clock became, whether the board
  followed). The identity-preserving move stays mechanical. The stop rule
  cuts the other way here: a consequence expensive to name is a sign it
  was never one consequence, and the carry was never a consequence at
  all.

- **2026-08-08 (H1).** **The clock is named as a value, not as a rule.**
  `destination_clock` is the number the destination's clock becomes, not
  "pull it up to the traveler's". A peer that applied `max(dest, source)`
  would be re-deriving, however cheaply, and cheap derivations are how
  the expensive ones get justified later. The C3 rule is preserved
  exactly, on the resolver, where it is computed once.

- **2026-08-08 (H1).** **Late-join replay would have passed before this
  change, and that is not the same as being proven.** The old derivation
  was deterministic over replicated state, so a from-the-log replay
  converged; nobody had ever asserted it. What is new is that it now
  converges for a *reason* rather than by the coincidence of every peer
  running identical derivation code over identical authored data. The
  receipt that shows the difference is
  `applying_a_crossing_reads_no_door_table`: re-author the doors out of
  the world and replay the same log. The derivation would have refused
  (nobody stands on a door any more, and there is no target to look up);
  the payload lands identically, because it names its own two ends.

- **2026-08-08 (H1).** **The host's door sweep could not call
  `snapshot_of`.** The sweep lives inside a `&mut` borrow of the app's
  runner, and `snapshot_of` takes `&self`, so the whole-`self` method is
  out of reach exactly where the resolver needs its input. Split into a
  free `snapshot_with_journal(journal, ui)` that the method now delegates
  to, rather than growing a fourth `GameSnapshot` literal.
  (`crates/isometry-genet/src/dispatch.rs`.)

- **2026-08-08.** **`isometry-graphshell` does not compile at `main`.**
  `AdvertisedAction` gained an `input_form` field upstream and
  `crates/isometry-graphshell/src/lib.rs:434` was not updated. Verified
  pre-existing by stashing this work and rebuilding. Unrelated to the
  protocol; the H0 verification excluded that crate.

## Progress

- **2026-08-08:** founded from the audit; adjudication and roadmap plans
  archived with their residues extracted.

- **2026-08-08.** **H0 landed.** The envelope now carries a
  `PROTOCOL_VERSION` (1) and a `RequestId { peer, nonce }`.

  **What the envelope carries.** `NetMessage::Hello` gained `version`
  and `NetMessage::Snapshot` gained `version`, so each side declares its
  dialect in the first message it sends. `NetMessage::VersionRefused
  { offered, supported }` is the new typed refusal, appended last so the
  earlier postcard variant indices are unmoved. `ActionIntent` and
  `ActionResolved` both gained a `request: RequestId`, and the
  resolution echoes the intent's. `GameSnapshot` gained
  `applied_actions: BTreeSet<RequestId>`, the replicated ledger of
  verdicts already taken.

  **Where the version gate lives, and why.** In the two session state
  machines, not the transport and not a per-message field. The host
  checks in `session/messages.rs::on_message`: a bad `Hello` is refused
  before the peer is recorded as a player, and the refusal is remembered
  and re-checked ahead of every later message from that peer, so nothing
  it sends reaches game state. The client checks in
  `session/client.rs::on_message`: a `Snapshot` in an unknown version is
  refused with no state, seq, or hash adopted, and the session then
  reads nothing further. Refusal is terminal on both sides; there is no
  downgrade path, because silently degrading is the misapplication the
  version exists to prevent. Placing it in the state machines rather
  than in `iroh_link` is what makes it testable without two machines.
  See Findings for why not per-message.

  **Where idempotency lives.** In `apply_game`'s `ActionResolved` arm
  (`session/apply.rs`): the request id is checked against
  `state.applied_actions` first and returns `Ok(())` untouched on a
  repeat, and is recorded only after the resolution actually applied (a
  resolution refused for an unsheeted target stays askable). Because the
  ledger is in the snapshot rather than in a session, every application
  site is covered at once, including the source-time replay.

  **Receipts.** `crates/isometry-net/tests/protocol_envelope.rs`:
  `a_duplicate_resolution_is_a_no_op` (whole-snapshot equality after the
  second apply, plus named assertions that hit points, the roll log, and
  `beat_seq` are each unmoved, and that host and client log hashes still
  agree); `two_identical_strikes_are_two_verdicts` (the no-op is by
  identity, not by content); `an_unsupported_version_is_refused_with_a_
  receipt` (both directions, each naming offered and supported, with the
  host's log and the client's state proven untouched);
  `the_host_attributes_an_ask_to_the_peer_it_arrived_on` (request
  identity, including that a forged peer half is corrected);
  `the_supported_version_is_admitted`.

  **Re-greened.** All 43 `tests/replication.rs` cases pass under the
  versioned envelope, including the iroh transport's 11 (the real QUIC
  handshake now carries the version). The test's `attack_hit` fixture
  had to mint a fresh request per call: two tests commit two resolutions
  built from it, and under the old fixture they would have shared an id
  and the second would have been correctly swallowed. That is the
  idempotency rule biting in the fixtures, not a regression.
  `isometry-views` (55) and `isometry-genet` (5) also pass, and
  `cargo check --workspace --all-features --all-targets` is clean apart
  from `isometry-graphshell` (see Findings). `cargo clippy -p
  isometry-net --all-targets` produces exactly the warning counts it
  produced before this change.

  **Breaking wire change, as the plan expects.** `ActionResolved` and
  `ActionIntent` changed shape, `Hello` and `Snapshot` gained a field,
  and postcard tags by position, so captured logs and saved checkpoints
  from before this commit will not decode. No migration shim was
  written, per the doc policy's no-legacy-friction rule. The `serde`
  defaults on `GameSnapshot::applied_actions` exist for the JSON save
  path, not as a compatibility bridge.

  **Not touched.** No peer-side derivation was added anywhere; travel
  (`Traveled`, `TravelResolved`) is untouched and remains H1/H2's work.
  `UiState` does not mirror `applied_actions`, so a snapshot rebuilt from
  the view (`snapshot_of`) starts with an empty ledger; that is a host
  bootstrap and a prevalidation clone, neither of which replays a log.

- **2026-08-08.** **H1 landed.** `GameEvent::Traveled { token }` is gone.
  A doorway crossing is now `GameEvent::TransitionResolved`, carrying a
  `TransitionResolved` payload that names every consequence.

  **What the payload names.** `request: RequestId` (the H0 ledger's
  handle), `token` (the traveler, by its id on the near side), `from_map`
  and `to_map` (both ends of the door, so an auditor reading the log
  needs no map registry), `landing: TileCoord` (the arrival tile the
  authority chose: named entry, else first spawn zone, then the outward
  free-tile scan), `arrival: TokenId` (the identity it lands under, equal
  to `token` unless the destination already held that id),
  `inventory_remaps: Vec<(TokenId, TokenId)>` (non-empty exactly when a
  replacement stranded one), `destination_clock: u64` (what the
  destination's clock becomes, the C3 rule computed once), and
  `activated: Option<String>` (the board following the last player out,
  as a named consequence rather than a peer's inference from who is
  left). Six derivations became eight fields.

  **Where the old derivations died.** All of them lived in one function,
  `apply.rs::travel`, which ran inside `apply_game` on every peer: the
  door lookup against `maps[active].transitions`, the destination anchor
  and free-tile scan against `target.spawn_zones` and its occupancy, the
  id mint scanning every token on every map, the clock `max`, and the
  activation test. That function is deleted. Its body now lives, split in
  two, in the new `session/travel.rs`: `resolve_transition` (the
  authority's, returning the payload) and `apply_transition` (everyone's,
  which looks up no door, searches no tile, mints no id, compares no
  clock, and infers no activation). The three emit sites were converted
  to resolve-then-commit: the host's door sweep
  (`isometry-genet/src/dispatch.rs`, which now rules each crossing and
  broadcasts the verdict), solo/hot-seat play
  (`isometry-views/src/state/play.rs::travel`, which is itself the
  authority), and the sim tests' `cross` helper. `intent_refusal` refuses
  a client-sent `TransitionResolved` as the verdict it now is.

  **Receipts.** `crates/isometry-net/tests/transition_resolution.rs`:
  `a_doorway_crossing_names_every_consequence` (whole-payload equality
  against the values the old derivation produced for the same fixture,
  then the applied board);
  `an_identity_collision_is_named_with_the_inventory_that_follows_it`;
  **`a_late_joiner_reconstructs_the_crossing_from_the_log_alone`**, the
  headline receipt the adjudication plan never landed, which proves it
  two independent ways: a peer connecting *after* the crossing holds the
  host's state, log hash, and seq, and separately the origin plus the
  ordered log alone (via `GameSourceHistory`, rehosted through
  `HostSession::with_history`) rebuilds the same state, the same log
  hash, and the same seq with no snapshot to copy from;
  `applying_a_crossing_reads_no_door_table` (the same log replayed over a
  world whose transitions were re-authored away, which the derivation
  could not have survived); and `a_repeated_crossing_is_a_no_op` (the H0
  ledger over the H1 payload, plus that a renumbered crossing is judged
  rather than swallowed). `travel_off_a_door_is_refused_and_clients_
  cannot_rule_it` moved its refusal to where the ruling is: an off-door
  crossing now fails at `resolve_transition` and mints no event at all.

  **Re-greened.** `cargo test -p isometry-net --all-features`: 11 lib
  (including the iroh transport's), 5 `protocol_envelope.rs`, 43
  `replication.rs`, 5 new. `isometry-core` (56), `isometry-views` (55),
  `isometry-genet` (5). `cargo check --workspace --all-features
  --all-targets` clean apart from `isometry-graphshell` (see Findings).
  `cargo clippy -p isometry-net --all-targets` produces the identical
  warning set to HEAD, verified by stashing this work and re-running.
  Headed runtime receipt: `ISOMETRY_TRAVEL_SELFTEST=1` walks the knight
  onto the field's door at (12, 14); the board follows it to `hut`, it
  lands on the entry door at (2, 2), the field no longer holds it, and
  the clocks reconcile `{"field": 4, "hut": 4}`.

  **Breaking wire change, as the plan expects.** `PROTOCOL_VERSION` is 2
  and `iroh_link::ALPN` moved to `isometry/session/v2` with it, so an
  older peer cannot dial and one that somehow did would be refused by
  H0's handshake gate. The new variant sits in `Traveled`'s old slot
  deliberately: postcard tags by index, so replacing in place leaves
  every later variant's tag where it was and the break is this variant's
  body alone. No migration shim, per the no-legacy-friction rule.

  **Not touched.** H2 is untouched: overmap travel is still
  `GameEvent::TravelResolved`, still an inline-struct variant, and its
  clock/encounter/exhaustion consequences still apply as they did. The
  no-`Hello` hole from H0's Findings stays open (a peer that never sends
  `Hello` is not version-gated; it owns no token, so it can still only
  push `Rolled`). No headed two-peer receipt was taken for the doorway
  path: that is H2's stated done-condition, and H1's sim receipts cover
  the same resolve-then-commit shape the app's sweep uses.
