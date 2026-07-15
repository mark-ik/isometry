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
5. **C5: Multi-character parties + recruitment** — ownership is already
   per-token; add a configurable cap (default 4) and a `convince` action whose
   consequence is allegiance (`OwnerSet`), adjudicated like any other action.
   Done when a player runs four tokens with correct fog and a convinced goblin
   fights for them on every peer.
6. **C6: Dialogue** — surface storylet choices in-app; the conversation-economy
   lane stays post-keystone with the intelligence vision.
7. **C7: Factions as participants** — worldbuilding rung 7; a faction player is
   an owner name over a faction-turn channel. Waits on the moot/murm rebase.
8. **C8: World map (pointcrawl)** — stays deferred per next-horizons' trigger;
   transition points cover the prepared-locale 80%.
9. **C9: Campaign pack options** — strongest first: a PF2e skeleton to force the
   action-spec shape to generalize; then a real pixel tileset as pack CSS; pack
   distribution after the murm peer-runtime lands.

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
