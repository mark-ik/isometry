# Gameplay roadmap

**Date:** 2026-07-14
**Status:** active plan. Order set by Mark 2026-07-14; **C1 (conditions) and C2 (transition points) landed 2026-07-15**; C3 (split-party time) is next.
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
3. **C3: Split-party time** — a per-location tick ledger summed into the world
   clock; simultaneity is presentation (the Helldivers rule) unless a rule reads
   it, in which case initiative is over *locations*, one local round per world
   tick. Done when two parties on two maps accumulate different local time and
   the world clock reconciles when they rejoin.
4. **C4: Generators + command grammar** — `>gen`, `>spawn`, `>find` on the
   existing composer, over the landed W2 runtime. Done when `>gen npc` previews,
   rerolls, and commits a statted spawn.
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
