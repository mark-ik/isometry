# Gameplay roadmap

**Date:** 2026-07-14
**Status:** active plan. Order set by Mark 2026-07-14; **C1 (conditions), C2 (transition points), and C3 (split-party time) landed 2026-07-15**; C4 (generators + command grammar) is next.
**Related:** [adjudication_and_representation_plan](2026-07-14_adjudication_and_representation_plan.md)
(complete; this continues its lane), [next_horizons_landscape](2026-07-07_next_horizons_landscape.md)
(C1 answers its open question B.5; C2 takes its lane 2 recommendation),
[worldbuilding_generation_plan](2026-07-09_worldbuilding_generation_plan.md)
(C7 is its rung 7).

## The order

Substrate depth before gameplay breadth. Each earlier item makes a later one
cheaper: conditions make recruitment's "temporarily" expressible, regions make
split-party meaningful, and the resolver's consequence list (deltas, defeat,
displacement, conditions, allegiance) grows one type at a time.

1. **C1: Conditions** (next-horizons B.5) — **landed 2026-07-15**, below.
2. **C2: Regional locations + transition points** — **landed 2026-07-15**,
   below. (REGION-scale maps were already just `MapDocument`s; what was missing
   was the door.)
3. **C3: Split-party time** — **landed 2026-07-15**, below.
4. **C4: Generators + command grammar** — **landed 2026-07-15**, below.
5. **C5: Multi-character parties + recruitment** — **landed 2026-07-15**, below.
6. **C6: Dialogue** — **landed 2026-07-15**, below.
7. **C7: Factions as participants** — worldbuilding rung 7; a faction player is
   an owner name over a faction-turn channel. Waits on the moot/murm rebase.
8. **C8: World map (pointcrawl)** — stays deferred per next-horizons' trigger;
   transition points cover the prepared-locale 80%.
9. **C9: Campaign pack options** — the PF2e skeleton **landed 2026-07-17**,
   below. Remaining: a real pixel tileset as pack CSS; pack distribution after
   the murm peer-runtime lands.

## C1: Conditions (LANDED 2026-07-15)

**The decision (answers B.5): conditions are substrate-visible, and movement and
senses become system-driven.** The pattern is defeat's, generalized: the system
judges (Lua names the condition and computes its mechanical projection), the
substrate obeys (stores names it does not understand, honors numbers it does).

- **Core** stores `conditions: BTreeMap<TokenId, BTreeSet<String>>` (opaque
  names, for display, Lua, and pack CSS) and `mobility: BTreeMap<TokenId,
  (speed, sight)>` (the system's current ruling, host-computed, replicated).
  The substrate never knows what `prone` means; it knows this token currently
  moves 2 and sees 6.
- **Sheets** gain base `speed` and `sight` fields (5e defaults 5 and 6, matching
  the retired constants). The mobility map is the *effective* value; base stays
  editable and untouched.
- **System** projection: the Lua character table gains condition booleans
  (`c.prone`), and `s_speed(c)` / `s_sight(c)` return the effective numbers.
  Client peers hold no Lua, so the projection replicates as data with the
  condition change, exactly as a resolution replicates its outcome.
- **Actions** can inflict a condition: `TargetSpec.condition_on_hit` (5e gains
  `trip`, which does no damage and applies `prone`). The DM (and solo play)
  toggles via the token menu; a replicated `ConditionSet` carries the
  recomputed mobility.
- **Views**: reach preview uses the token's effective speed, fog uses each
  token's own effective sight (per-origin radius, replacing the single viewer
  radius), and the token wrapper gains a `cond-<name>` class so packs can
  style conditions the way they style beats.

**Done when:** the `MOVE_BUDGET` and `SIGHT_RADIUS` constants are gone (demoted
to defaults); tripping a token halves its reach preview on every peer; a blinded
token's own fog goes dark without dimming its allies'; standing up restores
both; and a corpse still cannot be tripped.

**Verified 2026-07-15.** Unit: a trip applies `prone` and the projection travels
with it (base speed 5 halves to 2, sight untouched); tripping the already-prone
applies nothing; blinded is dark-not-slow and immobilized is slow-not-dark, both
judged in Lua with no Rust branch; no conditions means no override at all.
Replication: the condition and its numbers land on the client (which computes fog
and reach locally, so it must hold them), standing up restores base, and a client
proposing a `ConditionSet` is refused (a condition is a rules ruling). In-app: the
trip hits for 0, the goblin's effective mobility drops from (6, 6) to (3, 6) (its
30 ft base halved by the script), the prone pose renders, and the constants are
demoted to sheetless-token defaults.

## C2: Transition points (LANDED 2026-07-15)

**Doors are walked, travel is ruled.** `MapTransition` already existed as pack
and generator data (`at`, `target_map`, `target_entry`); nothing rendered or
played it. Now:

- The board renders every transition on the active map as a door tile
  (`tile-door`), and a Play-mode move that lands on one walks through it.
- `GameEvent::Traveled { token }` names only the traveler. Everything else is
  resolved deterministically from replicated state when the event applies: which
  door (the tile the token stands on), the destination (the target's named entry,
  else its first spawn zone), a free landing tile (the same outward scan spawning
  uses), and a fresh id on collision (minted above every token on every map, so
  the globally-keyed inventories stay sound and are rekeyed with it).
- The traveler carries everything it is: sheet, conditions, mobility numbers,
  defeat flag, inventory. Travel is not a cure; prone crosses with you.
- **The board follows the last player out.** When no player-owned token remains
  on the active map, the target activates exactly as a manual `MapActivated`
  (fresh board, fresh turn order). The DM's furniture stays home.
- The host sweeps for tokens standing on doors after every applied move, so a
  client walks through a door by walking; a client `Traveled` intent is refused.
  Arriving on the far door does not bounce you back: the sweep fires on arrival
  onto a door, so you must step off the doorway and back on to return. Solo play
  routes through the *same* `apply_game` logic via a scratch snapshot, so there
  is exactly one travel implementation.

**Done when:** a party walks a door from one prepared map to another on every
peer. **Verified 2026-07-15.** Replication: the knight crosses carrying sheet,
prone, and its halved numbers; the field's stored copy no longer holds it; the
goblin furniture stays; an id collision mints a fresh id and the sword follows
it; off-door travel is refused; peers converge. In-app: the knight clicks onto
the purple door tile, the status reads "the party moves on: hut", and the board
is the hut with the knight standing at the named entry.

## C3: Split-party time (LANDED 2026-07-15)

**A full round is a tick; the DM declares the rest; the door reconciles.**

- `TurnList` now counts **rounds**: the cursor wrapping past the top of the
  order, including wraps that skip the fallen. A fresh order (new initiative) is
  a fresh encounter, so the count restarts.
- Each stored map keeps an absolute **clock** in ticks (`GameSnapshot::clocks`).
  A completed round on the active map ticks its clock automatically; the DM's
  pass-time verb (`GameEvent::TimeAdvanced`, host-committed, panel buttons "+1
  time" / "+10 time") adds the downtime no turn order measures. A bare board
  with no stored map keeps no clock: time is a campaign feature.
- **Simultaneity is presentation** (the Helldivers rule again): while parties
  are split, locations' clocks drift freely and nothing needs to agree. The
  moment anyone crosses a door, **the destination's clock catches up to the
  traveler's** (`max`), because nobody arrives before they left. That single
  rule is the whole of reconciliation; the world clock is simply the latest
  location.
- The panel shows `round N · time T (map)`. Location-level initiative (one local
  round per world tick, for the rare cross-location trigger) remains available
  as a table discipline and needs no new machinery.

**Done when:** two parties on two maps accumulate different local time and the
clock reconciles when they rejoin. **Verified 2026-07-15.** Unit: rounds count
wraps, count wraps-past-the-fallen, and reset with a fresh order. Replication:
three fought rounds plus four declared ticks put the field at 7 while the quiet
hut stays at 0; the knight crosses and the hut catches up to 7 on every peer; a
client declaring time is refused. In-app: the DM passes 4 ticks in the field,
the knight walks the door, and the clocks read field 4, hut 4. The in-app run
also caught a real bug the sim could not: solo travel's scratch snapshot was
built without the clocks, so reconciliation ran against empty time and wiped
the ledger on copy-back.

## C4: Generators + command grammar (LANDED 2026-07-15)

**A `>` command line, and the NPC-lowering gap closed.** The map-first pass found
that the flagship (`>gen npc`) mostly needed a front door: the generator overlay's
generate/reroll/lock/commit surface already existed, and the real missing piece was
that committing a generated NPC did *nothing* (a `_ => {}` no-op) and no npc
generator existed.

- **The command line** is a dedicated, testable mode entered by `>` (the way `w`
  opens a whisper), captured with the host-keystroke pattern. The parser is a pure
  `command.rs` module (unit-tested); dispatch on `UiState` routes each verb to
  machinery that already exists.
- **Verbs**: `>spawn <query>` (bestiary-resolved statted creature, host-gated),
  `>gen <kind>` (selects a matching generator and opens the existing overlay on a
  fresh preview), `>find <query>` (unified read-only substring search over monsters,
  items, spells, shown in the panel), `>roll <expr>` (shared log), `>time <n>` (the
  C3 clock verb), `>help`. Short aliases (`s`/`g`/`r`/`t`).
- **The NPC bridge**: `npc.lua` picks a bestiary archetype by entropy plus a name;
  the commit arm looks the key up in the bestiary and lowers it through
  `monster_sheet` under the generated name, so a generated "Vane" is a real,
  fightable kobold that joins initiative. A key with no bestiary match falls back to
  a default sheet. Placement uses snapshot-side free-tile and globally-unique-id
  helpers (the travel id discipline), and the events flow through the existing
  Remote/Local commit dispatch, so a generated NPC replicates.
- **Determinism**: `ISOMETRY_GEN_SEED` fixes the generator tape so previews and
  rerolls are reproducible; otherwise the wall clock seeds it as before.

**Done when:** `>gen npc` previews, rerolls, and commits a statted spawn.
**Verified 2026-07-15.** Unit: the parser (verbs, aliases, tolerated `>`/case/pad,
non-numeric time is a mistake not zero); the npc generator yields a bestiary-backed
creature that stats up, is deterministic per seed, and varies on reroll. In-app
(`ISOMETRY_CMD_SELFTEST` + `ISOMETRY_GEN_SEED=5`): `>find sword` returns a unified
list (Flying Sword monster, Greatsword/Longsword/Shortsword items, Arcane Sword
spell); `>spawn gobl` places a statted goblin; `>gen npc` previews kobold "Vane",
commits, and a 5-HP NPC named Vane stands on the board in initiative (tokens 4->6).

An adversarial review pass (parallel reviewers, each finding verified by a skeptic)
caught four real bugs the happy-path tests missed, all now fixed with regressions:
`>spawn` in a hosted session mutated the local map directly (token never
replicated, wiped by the next mirror, orphan sheet) — now routed through the
authority like every other mutator; `next_token_id` maxed only the active map,
risking a collision with a resident of another stored map — now campaign-global;
`free_snapshot_tile`/`free_spawn_tile` could return an off-board tile on a map
narrower than the scan stride, failing the commit forever — now bounds-clamped;
and `>roll` hard-coded "DM", misattributing a joined player's roll — now the actual
roller. The first two were pre-existing bugs in the spawn path that `>spawn`
exposed, so the fix improves the compendium spawn button too.

## C5: Multi-character parties + recruitment (LANDED 2026-07-15)

**Allegiance is the next consequence, and the resolver's layering carried it
unchanged.** Multi-character control was already free: fog computes per-owner over
*all* a viewer's tokens, so owning four is owning four. C5 added the cap and
recruitment.

- **`convince`** is a targeted action shaped exactly like an attack: a Charisma
  pitch (`1d20 + cha_mod + prof`) against the target's resolve DC (a new `will`
  field; monsters get `8 + WIS mod`). Its consequence is allegiance, not damage.
- **The split held.** The resolver only *reports* the win (`Resolution.recruited`
  = the target won over); it never touches owners, because owners live on the
  token and the cap is table policy — neither is the sheet's. The **host** rules
  the owner change and the cap, exactly as it rules where a shove lands: the new
  owner is the *actor's* owner, a player's party is capped (the DM, owner `None`,
  is uncapped), and a full party makes the pitch land-but-not-hold.
- **`party_cap`** (default 4) rides on the snapshot, so every peer enforces the
  same limit. The owner change replicates as `ActionResolved.owner_changes`, and
  every peer recomputes fog from it (a new ally feeds your sight; an ex-ally
  stops).
- **Temporary allegiance** (charm that lifts) is a follow-on: it wants condition
  *durations*, which the on/off condition model does not have yet. V1 ships
  permanent-until-changed; the DM can re-own to revert.

**Done when:** a player runs four tokens with correct fog and a convinced goblin
fights for them on every peer. **Verified 2026-07-15.** Unit: convince reports a
recruit when the pitch beats the resolve DC and none when it misses; a plain
attack never reports a recruit. Replication: the owner change lands on host and
client, and does no damage. In-app (`ISOMETRY_CONVINCE_SELFTEST`): a bard (party
of 2, cap 3) convinces a goblin — it joins A (party of 3, at the cap) — then a
second pitch lands its roll but is refused, "sways Goblin (21) but your party is
full". Fog recomputes for the new owner.

An adversarial review pass caught four real bugs the happy-path tests missed,
all fixed with regressions: the cap counted only the *active* map, so a split
party across stored maps (C3) could exceed it — now campaign-global; in a session
the recruit is deferred to `net_outbox`, so a batch of same-owner intents all read
a stale count and blew past the cap — the Remote branch now folds the owner change
into the local map immediately (idempotent on mirror-back); `a_convince_hit`
errored on a pre-C5 sheet with no `will` field — now `t.will or 12`; and
`apply_snapshot` never mirrored `clocks` (a C3 omission) or `party_cap` into the
view. The review also surfaced a latent sharp edge fixed here: a zero-damage
action (convince, shove, trip) was pushing a `field -= 0` delta, which on a sheet
lacking that field *created* it at 0 — an invented `hp_current` of 0 then read as
defeated. Zero deltas are no longer pushed.

**Adjacent gap noted (not fixed):** a client's `TokenMoved` is not
ownership-gated on the host — a joined player could move any token, not only its
own. Not required by the done-when (which is about controlling *your* tokens),
but a real authority follow-up.

## C6: Dialogue — the storylet surface (LANDED 2026-07-15)

**A surface over an existing engine.** The W5 storylet machinery was complete —
`CampaignWorld::resolve_storylet` matches requirements (faction tags, host-private
secret facts, world laws) and casts roles, and `HostSession::commit_storylet`
applies the effects (facts, history, items, maps) as replicated events — but
nothing in the app *showed* or *played* one. C6 is that surface, and nothing more:
the conversation-economy / DM-in-the-loop-inference lane stays post-keystone with
the optional-intelligence vision.

- **The overlay** (a DM-only menu, mirroring the generator overlay) lists each
  storylet with its entry line, whether it is playable now, and — when locked —
  *why* ("needs a faction tagged 'cult'", "no character fits the role 'envoy'").
  Cycle, read the cast, Play the ready one.
- **Host-computed rows.** Matching reads host-private secrets, so the host
  resolves every storylet against the current world + `secret_ids()` and hands the
  view `StoryletRow`s; a joined client never receives them. Recomputed while the
  surface is open, so a storylet lights up the moment its requirements are met (a
  convinced faction, a revealed secret) — the storylet graph paying itself off.
- **Playing commits.** `play_storylet` arms a request the host drains through the
  same solo/session split as a campaign commit: solo runs a temp `HostSession`
  and mirrors the result back; a session routes `NetBridge::commit_storylet` so
  the effects replicate. All DM-gated (`can_edit_inventory`).

**Done when:** a storylet's choices are visible in-app and playing one commits its
effects. **Verified 2026-07-15.** Unit: the surface refuses a client, refuses to
play a locked storylet, and arms a request for a ready one. In-app
(`ISOMETRY_STORYLET_SELFTEST`): a ready storylet and a locked one are surfaced with
the right status ("needs a faction tagged 'cult'"), and playing the ready one
commits its fact into the journal ("gate-met": true).

An adversarial review caught three real issues, all fixed with regressions: in a
session the DM's `self.campaign` was a stale boot-time copy, so storylet
availability (which reads host-private secrets) resolved against out-of-date
secrets — now synced from the authority in the same `pump_net` block that syncs
journal/history; a played storylet re-lights (repeatable while its requirements
hold), and its Item effect minted a fixed `storylet.{key}.{index}` id that
collided and hard-failed the whole commit on a second play — the id is now
disambiguated per grant, matching the replay-safe Fact/History effects; and a
session storylet outcome reused the campaign-commit channel, mislabelling itself
"committed campaign draft" — the shared outcome text is now neutral.

## C9a: The PF2e skeleton (LANDED 2026-07-17)

**A second ruleset, to find the 5e-isms before they calcified.** Not a port: a
skeleton whose job is to put weight on the action spec five phases of 5e work
hand-rolled. It found two, both now fixed, and both of which **5e wanted too**.

**Fixed: the verdict was a boolean.** `hit_func` returned `1|0`, so hit-or-miss
was the only expressible outcome. PF2e's Strike has four rungs (beat the DC by
10 = critical success; miss by 10 = critical failure). `hit_func` now returns a
**degree** (`2/1/0/-1`) and `hit` is simply `degree >= 1`. This cost **no ABI
change** — the Lua boundary already returned an integer, so the boolean was
just a degree we were throwing away. 5e's `a_attack_hit` returns `1|0`
unchanged and never sees the difference; the binary system is the two middle
rungs of the same ladder.

**Fixed: damage could not scale.** A PF2e critical doubles dice *and* modifiers,
which is not expressible as an addend. `TargetSpec.damage_mult_func` returns a
**percent** (integers only, like the rest of the ABI): 200 doubles, 100 is the
default. The tell that this was a real gap and not a PF2e quirk: **5e's own
save-for-half (a fireball) could not be expressed either**, and now can, as 50.

**Not fixed, and now named** (each wants its own plan, none blocks the other):

- ~~**No action economy.**~~ and ~~**No multiple-attack penalty.**~~ **Both
  fixed 2026-07-17 at the system level, by one primitive.** The two turned out
  to be the *same* need -- per-turn, per-token integer state that resets at turn
  start -- so `MapDocument.turn_counters` (a named-integer ledger the substrate
  stores blind and clears when a token's turn begins) buys both, and neither
  "three actions" nor "-5 per attack" is baked anywhere. An action declares an
  `afford_func` (Lua guard reading the counters) and a `turn_effect` (counter
  deltas); the counters inject into the character table as `c.turn_<key>`. PF2e
  configures it entirely in Lua and on the sheet: `actions_per_turn = 3` (a
  sheet field, so a quickened creature carries 4), a Strike costs one action and
  counts toward MAP, `p_strike` folds `-5 * min(turn_strikes, 2)` into the bonus,
  and `p_afford_strike` refuses a fourth Strike. It is **opt-in**: 5e declares
  neither hook and spends no counter, exactly like the degree ladder. This is
  precisely the "one place the substrate must bend, toward turns" next-horizons
  predicted -- and it bent generically, not toward PF2e. **Verified at the
  system layer** (a quickened fighter's four Strikes read 0/-5/-10/-10 MAP; a
  plain fighter's fourth Strike is `CannotAfford`; 5e spends nothing).
  **The app-integration landed 2026-07-17**, once the iroh 1.0 / p2panda 0.7
  upstreaming unwedged the workspace lock (mere had moved to upstream both, so
  isometry followed; see the iroh/p2panda commit). The turn loop now: the host
  injects a token's counters as `turn_<key>` sheet fields before resolving (same
  channel as conditions); the resolver's `turn_effect` rides `ActionResolved`
  and every peer folds the same integer deltas via `bump_turn_counter`, applied
  verbatim like any sheet delta (the authority never reruns the afford rule --
  that gate lives where the Lua ran); and a `TurnAdvance` clears the *incoming*
  token's counters, so its economy refills the moment its turn begins. **Free
  play stays inert**: with no initiative there is no turn to reset against, so
  the host neither injects nor records counters (else a PF2e token would be
  capped at three strikes forever) -- that policy lives in the host, which knows
  free-play-vs-initiative, not in the blind substrate. Verified end to end:
  `per_turn_counters_replicate_and_reset_when_the_turn_comes_round` (net, both
  peers converge on the same ledger and the same reset) and
  `ending_a_turn_refreshes_the_incoming_token_per_turn_counters` (views, solo).
- ~~**Conditions have no values.**~~ **Fixed 2026-07-17.** Conditions went from
  a set of names to a `BTreeMap<name, i64>` -- a magnitude per condition, with
  plain on/off stored as 1 and zero meaning absent (the exact shape
  `turn_counters` already had). The number injects into the character table as
  an integer, so a script reads `c.frightened` as 2, not as a `frightened-2`
  name it would have to parse -- the lie the old encoding required is gone. The
  magnitude rides `ActionResolved`/`ConditionSet` and every peer stores the same
  integer, blind to what it means. It is opt-in: a `condition_value_func` on the
  target spec rides the degree ladder for a graded condition, and its absence
  means plain magnitude 1, so every existing on/off action is unchanged.
  Applying on a hit can only *worsen* a condition (a weaker fear never undoes a
  stronger one); the DM's manual `ConditionSet` still sets or clears any value.
  Proven by the PF2e vehicle that needed it -- **Demoralize**, the second
  action, ties three of these C9 generalizations together at once: it targets
  Will (a save, not AC), reads the degree ladder, and turns the degree into a
  magnitude (`frightened 2` on a critical success, `1` on a success). The loop
  closes because a *different* rule reads it back: a frightened striker swings at
  `-N`, folded into `p_strike` from `c.frightened`. So a rule writes a magnitude
  and another rule spends it, and neither number is baked in Rust. Verified:
  `pf2e_demoralize_frightens_by_degree` and `a_frightened_striker_swings_at_a_penalty`
  (system), `conditions_carry_a_magnitude` (core), and
  `a_graded_condition_replicates_at_its_magnitude` (net, both peers hold the same
  number). The board also emits a `cond-frightened-2` class so a pack can show
  magnitude without the view knowing what it means.
- ~~**No raw die.**~~ **Fixed 2026-07-17.** `call_int_ctx2` passes the natural
  die beside the total (`f(c, t, roll, die)`), and Lua discards arguments a
  function does not declare, so every existing script was unaffected. Both
  systems immediately wanted it, which is the tell that it was a real gap: PF2e
  shifts the degree a rung on a natural 20/1, and **5e finally crits**, closing
  a TODO its own script had been carrying ("crits and fumbles need the raw die,
  which the ABI does not pass yet"). A single-die base yields the natural roll;
  a multi-die base has no one natural roll, so it passes nil.

**The ladder is opt-in, which is the real result.** 5e uses three rungs (crit on
a natural 20, hit, miss) and never a critical failure, because a natural 1 in 5e
simply misses. PF2e uses all four. Neither system pays for the other's
complexity, and both ride one resolver.

**Done when:** a second ruleset exercises the action spec and the 5e-isms are
either fixed or named. **Verified 2026-07-17.** PF2e reports all four degrees
(a guaranteed crit, a guaranteed fumble, and both middle rungs reachable); a
critical doubles the whole effect (proved by re-running the *same seed* with
the AC set to exactly the roll, so only the degree differs, and the log says
`x200%` rather than silently reporting a bigger number); and 5e is provably
unchanged — a hit is degree 1, a miss is degree 0, never a fumble, and no
multiplier appears in its log. 185 workspace tests green under `--all-features`.

## Progress

- 2026-07-14: Doc created with Mark's ordering. C1 design settled: the
  defeat pattern generalized, projection travels with the change.
- 2026-07-15: C1 landed and verified. The resolver's consequence list is now
  deltas, defeat, displacement, and conditions-with-numbers; movement and senses
  are system-driven end to end. 153 workspace tests green.
- 2026-07-15: C2 landed and verified. Doors render, travel is ruled once and
  applied identically everywhere, and the board follows the last player out.
  156 workspace tests green. C3 (split-party time) is next and now has its
  substrate: parties genuinely on different maps.
- 2026-07-15: C3 landed and verified. Rounds are substrate truth, clocks are
  per-location, the DM declares downtime, and the door is where timelines
  meet. 158 workspace tests green. C4 (generators + command grammar) is next.
- 2026-07-15: C4 landed and verified. A `>` command line fronts spawn/gen/find/
  roll/time; the NPC-lowering gap is closed (npc.lua + bestiary bridge), so
  `>gen npc` ends in a statted, fightable creature. Mapped with a parallel
  understanding pass and checked with an adversarial review pass. 165 workspace
  tests green. C5 (multi-character parties + recruitment) is next.
- 2026-07-15: C5 landed and verified. `convince` recruits a creature to your
  side; the resolver reports the win, the host rules the owner change and the
  cap; allegiance replicates and refreshes fog. Adversarially reviewed. 170
  workspace tests green. C6 (dialogue) is next.
- 2026-07-15: C6 landed and verified. The storylet surface shows narrative
  opportunities with availability + why-locked and plays a ready one, committing
  its effects; host-computed (reads secrets), DM-only. Adversarially reviewed.
  173 workspace tests green. C7 (factions as participants) is next, and waits on
  the moot/murm rebase.
