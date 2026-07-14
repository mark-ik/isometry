# Adjudication and representation

**Date:** 2026-07-14
**Status:** active plan (2026-07-14). **A0-A4 landed, plus defeat and force: a fight
can be won, and blows land physically.** A knight swings at a goblin, the app decides whether it lands, the goblin
loses hit points, drops at zero, stops taking turns, and stops being a legal target;
the winner cheers, and **a joined player can swing for themselves** while still being
unable to pronounce their own verdict. A5 (choreography as pack data) remains. This is the game lane: it answers the fork the project had been walking
around since 2026-07-07.

**Related:**
[next_horizons_landscape](2026-07-07_next_horizons_landscape.md) (this answers its
open question B.4 and takes its lane 4, the schema/ABI widening),
[shared_authority](2026-07-09_shared_authority_and_collaborative_building_plan.md)
(this supersedes that doc's "Next Game Slice" section: a game lane was living in a
governance doc),
[board_to_text_narration](2026-07-07_board_to_text_narration_plan.md) (narration is
the third renderer of the event this plan defines),
[environmental_surfaces](2026-07-08_environmental_surfaces_plan.md) (surfaces react
to the same resolved event),
[campaign_packs](2026-07-08_campaign_packs_plan.md) (choreography ships as pack
data).

## The decision

**The app adjudicates.** next-horizons B.4 asked whether Isometry compares a roll
against AC/DC and reports the outcome, or stays a "roll and let the table decide"
tool. It adjudicates.

The reason is that everything expensive already built only pays off under
adjudication. Isometry has line-of-sight and fog, so the machine knows who can see
whom. It has elevation, facing, reach, and Chebyshev templates, so the machine knows
who can reach whom. It has a bestiary with printed hit points and a 5e system plugin
that already derives `1d20+4` correctly. If the app declines to resolve the attack,
all of that is scenery and the isometric board is decoration on a dice roller. The
reference games (Knight of Lodis, FFTA) adjudicate; that is what makes position mean
something.

This also unblocks the largest lane in the project. next-horizons calls the schema
and Lua ABI widening "the largest single lane" and notes it blocks packs, generators,
conditions, and system-driven movement. That lane was fork-gated on B.4. Answering
B.4 releases it.

## The shape: resolve once, replicate the outcome, represent locally

Three layers, one event.

```text
ActionIntent { actor, target, action_key }        // a player asks
        |
        v  host (or turn-leader) resolves: turn ownership, range, prerequisites,
        |  then the system plugin's Lua + host entropy
        v
ActionResolved {                                   // the single replicated fact
    actor, target,
    roll:    RollRecord,        // the public dice, as today
    outcome: Hit | Miss | Save { .. },
    deltas:  Vec<SheetDelta>,   // typed: hp_current -= 5
    beats:   Vec<Beat>,         // named, timed, representational
}
        |
        +--> board  : plays the beats (CSS animation clock)
        +--> sheet  : applies the deltas
        +--> log    : narrates the outcome (board_to_text)
```

Peers **apply** `ActionResolved`. They never rerun Lua and never reroll dice. That
keeps the existing convergence-hash model intact and keeps the rules engine on one
machine, which is what the current sequencer already guarantees.

**Resolution is replicated. Representation is not.** The event names its beats;
each peer plays them locally against its own clock. A dropped frame on one laptop
cannot desync the log, because the beat is a consequence of the event rather than a
member of it. This is the same friendly-table split that already governs fog (host
sends full state, each viewer renders what its tokens see) and dice (roll locally,
share the result).

## Beats: one primitive for combat, environment, and emotes

A **beat** is a named, timed visual state on an actor: `strike`, `recoil`, `cast`,
`fall`, `cheer`, `shrug`. It is a CSS class the view sets, an `@keyframes` rule the
pack supplies, and nothing else. The engine's own animation clock interpolates it.

This is deliberately the *same* primitive Mark asked for as an emote system. An emote
is a beat with no `ActionResolved` behind it: a player throws `cheer` directly and it
replicates as its own small event. Combat beats and social beats differ only in what
authored them. Building the emote system separately from combat animation would be
building the same thing twice.

It is also the substrate for "two units squabble and the outcome is represented in
the squabble." The squabble is a *beat sequence* on the resolved event: attacker
`strike`, defender `recoil` or `parry`, chosen by `outcome`. The realer it gets from
there is a richer beat vocabulary and richer choreography, not a different
architecture. Nothing below has to change for that to grow.

**Beats are pack data, not engine data.** The keyframes live in the tileset
stylesheet, exactly as tile appearance already does (pillar 3: modding is folders and
stylesheets). A campaign that wants a different swing draws a different swing. This
is where the homebrewing belongs.

## Force: a stagger is a flourish, a shove is the truth

The two look alike on screen and are opposites underneath, so they are separate
fields on the resolution and separate Lua hooks on the action.

| | Stagger | Forced movement |
| --- | --- | --- |
| Example | any solid blow rocks you back | Shove, Thunderwave, repelling blast |
| Tile changes? | **no** | **yes** |
| Replicated? | no: it is a beat | yes: `ActionResolved.displaced` |
| Who decides where it lands | nobody: it returns | the **board** (`push_path`) |
| May a rule read it? | **never** | of course |
| Peers must agree? | no | exactly |

**Why a stagger must not move anybody.** It is tempting to have the shove be real
and let the victim walk back. That buys a scheduler, a pathfinder, a "what if there
is no path home" case, and a class of desync bugs, in exchange for nothing: a token
that always returns to its square never changed the square. Once the displacement is
transient it is representation, and representation is already local by design. Peers
may disagree about exactly where a sprite is mid-flinch and still hold byte-identical
game state, which is the same bargain Helldivers makes with its physics.

**Why it must not be derived from the animation.** The tempting version of "hit
testing" is to ask whether the attacker's animated box overlapped the victim's. That
is wrong twice. Isometric sprites overlap constantly by construction (depth-sorting
exists precisely because they do), so overlap carries no meaning; and collision
against animated boxes is a function of frame timing, which is local, so two peers
dropping different frames would compute different collisions and the log would
diverge. Nothing is lost by refusing it: the resolver already knows who swung, who
was hit, from which tile, and how hard. **Physical consequence is a consequence of
the resolution, not of the animation.**

**The two share one geometry and one keyframe family.** `away()` gives the unit step
between two tiles, `compass()` names it, and the stylesheet generates eight
directional pairs from the board's own projection so a shove travels exactly one
tile. A stagger runs `0 -> out -> 0` (it ends where it began). A shove runs the other
way: the board has *already* placed the token on its new tile, so the beat slides it
in from where it used to be. Same keyframes, reversed, and the only difference that
matters is that one of them changed the game.

## What this borrows instead of building

The explicit instruction is to take the best of the ecosystem and save the
homebrewing for the campaigns. Concretely, this plan builds no new subsystem:

| Need | Borrowed from | Not built |
| --- | --- | --- |
| Timed interpolation | genet's CSS animation clock (`tick_animations`) | a tween/easing library |
| Reduced motion | genet's `AnimationMode::Disabled` (jumps to final value) | a custom accessibility toggle |
| Frame gating | genet's `has_active_animations()` (clock-based, self-settling) | an "am I animating" flag in app state |
| Rules resolution | the existing piccolo/Lua system plugin | a rules DSL |
| Randomness | the existing `dice` module + host entropy | a second RNG path |
| Replication | the existing `GameEvent` log, snapshot, convergence hash | a new animation channel |
| Monster stats | the vendored 5e-database compendium (already carries HP and AC) | hand-authored stat blocks |
| Narration | the board_to_text lane | a combat-log formatter |

The one genuinely new thing is the `ActionResolved` event and its resolver. That is
the part that is actually Isometry's.

## Findings (verified 2026-07-14)

1. **Combat does not exist.** `ActionIntent`, `ActionResolved`, `hp_current`, and
   `hp_max` return zero hits across the workspace. `GameEvent`
   (`crates/isometry-net/src/protocol.rs:64-100`) carries map edits, turn ops,
   `Rolled`, `SheetSet`, `Fact`, inventory, item transfer, and generation records.
   **No event lets one token affect another.** The goblin's 7 hit points exist as a
   printed stat (`crates/isometry-system/src/bestiary.rs:30`) and `hp` exists as a
   sheet display field (`crates/isometry-system/src/lib.rs:1232`). Nothing reduces
   either. The table does the subtraction in their heads.

2. **Genet has a complete CSS animation runtime with zero production consumers.**
   `tick_animations` (`components/genet-layout/incremental.rs:1175`),
   `has_active_animations` (`:1290`), `set_animation_mode` for reduced motion, plus
   transition and animation event queues. The only callers in the entire ecosystem
   were genet's own tests (`components/genet-scripted/document.rs:2012-2125`). No
   app drove the clock. **Isometry is now its first production consumer.**

3. **The seam is app-side; the engine needed no changes.** Isometry's `redraw()`
   already owns the layout session (`self.layout: IncrementalLayout`, on which it
   calls `.apply()`), so `tick_animations` was directly reachable. A0 is 29 lines in
   `crates/isometry-genet/src/main.rs` and zero lines in genet.

4. **Beats are comfortably real-time.** Release, 1100x720, 30x30 synthetic board,
   1,892 windowed elements, with an endless beat on **every one of the 20 tokens
   simultaneously** (a deliberate worst case; a real exchange animates one or two):

   - **Steady animating frame: mean 10.1 ms, min 8.8, max 14.5** (n=722).
   - **724 frames in 10 s (~72 fps sustained).**
   - Cold first frame is ~58-99 ms (layout build plus pipeline warm), then settles.

   The cost is dominated by the unconditional `emit_paint_list` in `redraw()`, not by
   the number of animating elements, so one token beating costs about the same as
   twenty. That is the good direction: a real combat beat pays the same ~10 ms as
   this worst case.

5. **Idle is genuinely free.** Same board with no animation CSS: the app draws **1
   frame and stops**, and `has_active_animations()` reports false throughout. The
   clock does not pump a still board, so the loop returns to `ControlFlow::Wait` and
   an idle table costs nothing. Genet's documented contract holds exactly as written.

6. **Viewport windowing has landed** (`crates/isometry-views/src/board.rs:43-97`,
   "outside the pane: windowing skips the emit"). next-horizons lane 1 lists this as
   the unbuilt highest-leverage fix; that status is **stale**. Element count is now
   bounded by the pane, not the board, which is why the 30x30 stress board emits
   1,892 elements rather than the ~2,700 the old receipts predicted.

7. **The facing flip already owns the token's `transform` slot.** `.token-flip`
   (`crates/isometry-views/src/theme.rs:100`) is `transform: translateX(24px)
   scaleX(-1)`, and a CSS animation on `transform` outranks a normal declaration, so
   a west/south-facing token would lose its mirror mid-beat. **Beats therefore need
   their own element** (A1). This is the one structural change beats impose.

8. **Use `translate` for beats, not `rotate`/`scale`.** Genet conjugates transforms
   at the box origin rather than the spec's `50% 50%` default. `translate` is the one
   transform that is origin-independent, so it sidesteps the gap entirely. It also
   keeps the tick on `Applied::RepaintOnly` (no relayout).

9. **The workspace did not build at HEAD** (`d5e2e1e`, "Consume Genet"). Genet
   renamed its Stylo fork to `genet-stylo` on 2026-07-12, and `[patch]` cannot rename
   its target, so isometry's stale `stylo`/`stylo_atoms` patch entries produced a
   *second* package claiming `links = "servo_style_crate"` and resolution failed.
   Fixed by mirroring genet's own answer: redirect `stylo_taffy` at genet's vendored
   fork, whose manifest git-deps `genet-stylo` directly
   (`support/patches/stylo_taffy/Cargo.toml:26-27`). Repaired in this session's
   `Cargo.toml`.

10. **Harness trap worth recording.** The app's env flags use
    `std::env::var_os(..).is_some()`, which is **true for an empty value**. In
    PowerShell, both `$env:X = ''` and `[Environment]::SetEnvironmentVariable('X',
    $null)` leave an empty string rather than removing the variable, so a probe
    "turned off" that way is still **on**. This produced a convincing false finding
    (an apparent engine bug where `has_active_animations()` never returned false)
    that survived two rounds of measurement. Unset with `Remove-Item Env:X` or
    `[NullString]::Value`, and verify with a child process before trusting a run.

11. **A fresh layout session resets the animation clock to zero, and the spike
    masked it.** A beat's class change produces a *structural* mutation batch, so
    `redraw` rebuilds `IncrementalLayout` from scratch. A fresh session cascades with
    its animation clock still at zero, so stylo stamps the new `@keyframes` with
    `start_time = 0`. The host then ticked the clock to `self.clock.elapsed()` (two
    seconds of uptime), and a 420ms beat was already "over" before its first frame:
    `has_active_animations()` returned false and nothing moved. Fixed by rebasing
    `self.clock` whenever a fresh session is built, so host and engine share one
    timebase. **The A0 spike could not have caught this**, because its probe
    animation was `infinite` and an endless animation never ends no matter how far
    the clock jumps. A one-shot beat is the first thing that could expose it. The
    lesson generalizes: a synthetic probe that differs from the real case in one
    property (here, `infinite`) can validate the plumbing and still miss the bug.

12. **A spawned monster had no sheet.** `spawn_monster` placed a token with a sprite
    and nothing else, so the goblin's 7 hit points and AC 15 lived in the compendium
    and never reached a `SheetData`. Nothing could be done to it because there was
    nothing to do it *to*. The view now asks the host to bind the stat block
    (`spawn_sheet_request`), because what fields a creature has is the system's
    business rather than the board's.

## Phases

### A0. The animation clock (LANDED 2026-07-14)

Drive genet's CSS animation clock from the isometry host.

- `redraw()` advances the clock: `layout.tick_animations(&dom, now_s)`.
- `about_to_wait()` requests a frame while `layout.has_active_animations()`, then
  falls back to `ControlFlow::Wait` when the last beat ends.
- `App.clock: Instant` is the monotonic zero.

**Done when:** a CSS `@keyframes` rule on a token visibly animates in the app; the
loop sustains frames for the animation's duration and stops afterwards; an idle board
draws one frame. **All three verified** (finding 4 and 5).

### A1. The beat element (LANDED 2026-07-14)

Give the beat its own box so it stops fighting the facing flip (finding 7).

- `token_el` emits a beat wrapper carrying `left/top/z-index`; the sprite becomes an
  inner box carrying `.token`, the sprite class, layer classes, and `.token-flip`.
- The beat class goes on the wrapper. Flip and beat then compose instead of
  overriding.

**Done when:** a west-facing token plays a beat and keeps its mirror; a beat on an
east-facing token is visually identical to before; the emitted element count grows by
exactly one per visible token.

**Verified 2026-07-14:** the wrapper (`.beat`) carries board position and the beat;
the sprite inside carries `.token`, its sprite class, layer classes, and
`.token-flip`. The board renders unchanged through the new box (headed capture), and
clicks still reach the handler through it.

### A2. The targeted action loop (LANDED 2026-07-14)

The first action that changes the game.

- `ActionIntent { actor, target, action_key }`. The host validates turn ownership,
  target existence, range, and system prerequisites.
- The system plugin resolves it with host entropy into one `ActionResolved` carrying
  the public roll, the outcome, and typed sheet deltas. Peers apply; they do not
  rerun Lua or reroll.
- First action: adjacent melee attack against AC, changing `hp_current` against a
  separate `hp_max`. Defeat and conditions are follow-ons.
- The sheet action enters target-pick mode; clicking a token submits the intent; the
  board and roll log show the result.
- This forces the Lua ABI widening (a target context and a tagged return), which is
  next-horizons lane 4.

**Done when:** two peers and solo play produce the same `ActionResolved` for a fixed
host entropy tape; an out-of-range, wrong-turn, or missing-target intent changes
nothing; a hit changes only the target's `hp_current`; and an attack resolves without
a GM editing sheet fields by hand.

**Verified 2026-07-14.** Unit: a fixed tape yields an identical `Resolution`; a hit
subtracts only from the victim; a miss changes nothing; out-of-range, self-target,
and untargeted-action intents are refused *before any die is rolled* (proved by the
rng being undrawn afterwards). Replication: the hit lands identically on host and
client, a client proposing its own verdict is rejected, and a resolution addressing an
unsheeted token is refused whole rather than half-applied. In-app
(`ISOMETRY_COMBAT_SELFTEST=1`): a knight adjacent to a statted goblin swings four
times, missing on 11 and 9 and hitting for 7 and 12, taking the goblin from 7 hit
points to 0 and below. 134 workspace tests green.

### A2b. Defeat (LANDED 2026-07-14)

Closes open question 5. A fight that cannot be won is not a fight.

- The **system** decides, in Lua: `s_defeated(c)` returns whether `hp_current <= 0`.
  A `System` without the concept simply declares no `defeat_func`. Death saves, dying
  conditions, and revival are that script growing, not the substrate learning.
- The **substrate** obeys, generically. `MapDocument.defeated` is a set of tokens out
  of play; core does not know why. What it does with it is mechanical, exactly as it
  treats elevation: `TurnList::advance_skipping` passes the fallen by, the resolver
  refuses them as targets, and the view slumps and dims them.
- The resolver judges defeat against the sheet *after* the deltas land, and a
  defeated victim plays `fall` instead of `recoil`. The fall holds its final frame
  (`forwards`), and `.beat-down` keeps the pose once the beat class is cleared.

**Done when:** a killing blow marks the target defeated on every peer; the fallen
token is skipped by turn advance; swinging at a corpse is refused before any die is
rolled; and a downed token reads as downed on the board.

**Verified 2026-07-14.** Unit: a killing blow reports `defeated` and swaps the beat
to `fall`; a survivable hit does neither; a corpse is refused with `AlreadyDefeated`
and the rng is left undrawn. Replication: defeat reaches the client (or it would
still offer a corpse as a target), and turn advance skips the fallen on every peer,
computed from state each already holds rather than being told. In-app: the goblin
takes 7, drops to 0, falls, and the next two swings are refused.

### A2d. A player may act (LANDED 2026-07-14)

Closes open question 6. Combat was DM-only, because a client that proposed an
`ActionResolved` was refused (rightly: that is a verdict, and a peer cannot pronounce
its own). What was missing was the **ask**.

- `NetMessage::Action(ActionIntent { actor, target, action_key })`. Deliberately *not*
  a `GameEvent`: it carries no roll, no damage, no outcome. The client asks; the host
  answers.
- `HostSession` checks the only two things a rules-blind crate can (the actor exists,
  and it is yours) and **queues** the request. It cannot adjudicate: it holds no
  `System`. That is why this needed a channel rather than another event variant.
- The host *app*, which does hold the rules, drains the queue and resolves each
  request through the **same** `adjudicate` path its own swings take. One resolver,
  one entropy source, one set of rules, whoever asked.

**Done when:** a player's swing is resolved by the host and lands on every peer; a
player cannot act with a token it does not own; and an ask, by itself, changes
nothing.

**Verified 2026-07-14.** An ask does not enter the log (`seq` unchanged), is queued
once and drained once, and a client is refused when it swings someone else's sword.
The DM's own swings still resolve unchanged (in-app regression run).

### A3. Beats on resolution (LANDED 2026-07-14)

Wire A1 and A2 together: the resolved event drives the representation.

- `ActionResolved.beats` is populated by the resolver (attacker `strike`; defender
  `recoil` on Hit, `dodge` on Miss).
- The view plays them: set the class, let the clock run it, clear on completion via
  genet's animation-event queue.
- Honor `AnimationMode::Disabled` for reduced motion (free from the engine).

**Done when:** a hit and a miss are visually distinguishable on the board with the
roll log covered; beats replay identically from the log on a late joiner; a peer that
skips the beats still converges on the same hash.

**Verified 2026-07-14:** a hit plays `recoil` on the victim and a miss plays `dodge`,
so the two read differently with the log covered. Each of four swings animated (71
frames against the 1 a still board draws), which also proves the beat-clearing
lifecycle: the class is dropped once the engine's clock reports the last beat done,
so the *next* strike is a genuine restyle and animates rather than standing still.
The late-joiner replay is not yet exercised (`last_action` rides the snapshot, so a
joiner receives the most recent exchange rather than the whole history).

### A2c. Force: stagger and forced movement (LANDED 2026-07-14)

Blows land physically, without letting representation touch truth. See the "Force"
section above for the reasoning.

- `TargetSpec.stagger_func` (Lua `f(c, t, damage) -> 1|0`) rocks a victim off its
  feet. Cosmetic: `staggered-<dir>`, out and back, nothing moves.
- `TargetSpec.push_func` (Lua `f(c, t, damage) -> tiles`) *actually* relocates it.
  The rules say how far and which way; the **board** rules on where that lands, since
  a wall, a map edge, or another body stops a shove short and the system does not
  know the map (`isometry_core::push_path`).
- 5e ships both: any blow of 5+ staggers, and a new `shove` action does no damage and
  pushes one tile.

**Done when:** a plain attack never changes a tile; a shove does, on every peer,
landing on the identical square; a shove into a wall moves nobody; and the victim's
new position changes what can reach it.

**Verified 2026-07-14.** Unit: one resolution yields `staggered-e` with no `push`,
while a shove yields `push = ((1,0), 1)` and `shoved-e` with zero damage;
`push_path` refuses a wall, stops short on an obstacle, and walks two clear tiles.
Replication: the shoved goblin lands on the same tile on host and client. In-app:
the shove hits for 0, the goblin moves from (11,14) to (12,14), and **the knight's
next attack is refused as "out of reach (2 tiles, reach 1)"** -- forced movement
changed the game, which is exactly what a stagger may never do.

### A4. Emotes (LANDED 2026-07-14)

The same primitive, no `ActionResolved` behind it.

- A small `GameEvent::Emoted { token, beat }`, thrown from the token context menu or
  a `>` composer verb.
- Reuses A1's wrapper, A3's playback, and the pack's keyframes verbatim.

**Done when:** a player triggers an emote on their own token, every peer sees it, and
it costs no new rendering or replication machinery.

**Verified 2026-07-14.** `GameEvent::Emoted { token, beat }` reuses the beat wrapper,
the beat playback, and the pack's keyframes verbatim; the snapshot's beat channel was
generalized from "the last action" to a bare `last_beats` list, because an emote has
no resolution behind it and the board should not care which kind of event asked for a
flourish. Unlike an attack, **a client's own emote is accepted**: there is no verdict
to forge and no state to change, so the worst a liar can do is wave. Cheer, shrug, and
taunt ship as the starter vocabulary. **Ownership (added 2026-07-14):** a client may
emote only tokens it owns. Waving is harmless; puppeteering another player's
character or the DM's monsters is not, and the host now refuses it.
In-app: the winner cheers over the body.

### A5. Choreography as pack data

- Beat vocabulary and `@keyframes` move into the tileset/pack manifest so a campaign
  can restyle a swing.
- The resolver names beats by key; the pack decides what a key looks like.

**Done when:** a second pack changes the look of an attack without touching app code.

## Open questions

1. **Does the defender's beat need its own event, or ride the attacker's?** Riding is
   simpler and keeps one ordered fact per exchange. It constrains simultaneous
   reactions later. Riding, until a reaction system asks otherwise.
2. **Where does the resolver live when the sequencer rotates** (shared-authority tier
   2)? The turn-leader resolving with its own entropy is the natural fit, and the
   convergence hash already detects a liar. Not this plan's problem, but A2 should
   not assume "the host" is a person.
3. **Beat duration versus turn pacing.** A 420 ms beat is free at turn cadence. A
   long cast animation blocking the next intent is a design choice, not a technical
   one. Default: beats never block input.
4. **Do conditions become substrate-visible?** next-horizons B.5, still open, and A2
   does not force it. Deferred.
5. ~~**Nothing dies yet.**~~ **Answered 2026-07-14 (A2b).** Defeat is system-judged
   and substrate-obeyed, exactly as sketched: it is next-horizons B.5 arriving from
   the practical direction, and it suggests the same shape will work for the rest of
   the condition list (stunned, prone, blinded) when movement and senses need it.
6. **A client cannot yet attack.** The host adjudicates, and a client that proposes a
   resolution is rejected (as it must be, or a player picks their own damage). What is
   missing is the *ask*: a `NetMessage::ActionIntent` the host app can drain and
   resolve with its system plugin, since `isometry-net` is deliberately rules-blind and
   holds no `System`. Until then combat is DM-driven, which is the model the app
   already ships. This is the next piece of A2, not a design question.

## Progress

- **2026-07-14:** Doc created. B.4 answered (adjudicate). A0 landed: genet's CSS
  animation clock is driven from isometry's host loop (29 lines, no engine change).
  Spike measured a worst-case beat at **10.1 ms mean / ~72 fps** with 20 simultaneous
  animations, and confirmed **idle costs one frame**. Isometry is genet's first
  production consumer of the animation lane. Also repaired the `[patch.crates-io]`
  mirror, which had left the workspace unbuildable at HEAD after the Genet rename
  (finding 9). Spike scaffolding (a beat-probe stylesheet and two diagnostics, one of
  them inside genet) was reverted; the clock is all that remains.
- **2026-07-14 (later):** **A1, A2, and A3 landed. Isometry adjudicates.** The
  substrate learned `SheetDelta` and `Beat` without learning what either means; the
  Lua ABI widened to `f(c, t, roll)` so a script can see its target, and the hit rule
  became one line of Lua rather than a Rust branch; `GameEvent::ActionResolved` now
  carries the verdict, the deltas, and the beats, and a client that proposes its own
  verdict is refused. Verified in the app: a knight swings four times at an adjacent
  goblin, misses (11), hits for 7 (7 hp to 0), misses (9), hits for 12, with each
  exchange animating (71 frames against the 1 a still board draws). 134 workspace
  tests green. Also fixed a real animation bug the spike had masked (finding 11) and
  closed the spawn-without-a-sheet gap (finding 12).
