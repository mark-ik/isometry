# Exploration mode

**Date:** 2026-07-18
**Status:** ACTIVE. **Trigger pulled 2026-07-18** (Mark: "overmap exploration is
part of the game"). C8 is un-deferred; building from E0. The exploration board is
the **overmap** (Mark's term): the pointcrawl graph above the tactical maps.
**Related:**
[2026-07-07_next_horizons_landscape.md](2026-07-07_next_horizons_landscape.md)
(decision 2 deferred the WORLD tier; this revisits its trigger),
[2026-07-14_gameplay_roadmap_plan.md](2026-07-14_gameplay_roadmap_plan.md)
(C8's slot),
[2026-07-17_tile_geometry_seam_plan.md](2026-07-17_tile_geometry_seam_plan.md)
(the pointcrawl *graph* is that plan's graph geometry; not re-scoped here),
[2026-07-09_worldbuilding_generation_plan.md](2026-07-09_worldbuilding_generation_plan.md)
(rung 7 world tick, now built as C7).

## The reframe: exploration is a mode, not a map

C8 was filed as "world map (pointcrawl)", which makes it sound like a travel
minigame. It is more than that, and seeing why changes the decision.

Both rulesets structure play as a triad:

- **PF2e** is explicit: three *modes* of play, encounter / exploration /
  downtime, each with its own rules.
- **5e** frames it as three *pillars*: combat / exploration / social.

Isometry has built two of the three. Encounter and combat is the action system
(`resolve_action`, the degree ladder, the action economy). Downtime and social
arrived with C6 storylets and C7 faction turns. Exploration is the leg with
almost nothing under it.

So the pointcrawl graph is the *board* for exploration mode, the way a
`MapDocument` is the board for encounter mode. The rules this doc is about
(PF2e's exploration activities and hexploration, 5e's travel pace, getting-lost,
and exhaustion) are the *mode itself*. This doc scopes that mode. It does not
re-scope the graph, which the tile-geometry seam plan already frames as one
generalization shared with hex and triangle.

## The trigger, pulled (2026-07-18)

Next-horizons decision 2 deferred the WORLD tier with a specific trigger:
"Isometry explicitly targeting wilderness-exploration play." **Mark pulled it on
2026-07-18**: overmap exploration is part of the game. So the product question is
answered, and this is now an active build lane rather than a conditional one.
What had changed since the trigger was written (2026-07-07), which is why the
cost was already low when the call came:

- C3 gave every map an absolute tick clock and a pass-time verb (`TimeAdvanced`).
- C7 built the world/downtime tick and banked time (`BANK_PER_MOVE`), which is
  exactly the "small party-level tick counter" next-horizons named as world
  tempo.
- This session landed conditions-with-values, which is the exhaustion primitive.

So two of the three modes and several of exploration mode's primitives now exist.
The trigger is unchanged; the cost behind it has dropped. This doc's job is to
show how far, so the trigger becomes a clean product call rather than a leap into
unknown work.

## The rules, by shape

What the two systems put in exploration mode, grouped by the substrate shape each
needs. These are the mechanic shapes, not the exact numbers; the numbers are
system-plugin content (see Licensing).

| Layer | PF2e | 5e | Shape it needs |
| --- | --- | --- | --- |
| Exploration activity | Scout, Search, Avoid Notice, Defend, Follow the Expert, Hustle | navigator / mapper / forager / lookout, marching order | a per-token named **stance** the system interprets |
| Tempo / pace | travel speed x terrain; hexploration activities per day | fast / normal / slow, each a tradeoff | a party-level **pace setting** plus the **world tick** |
| Navigation | survey / navigation checks | getting lost: navigator check vs terrain DC | a **travel resolution** (system judges, substrate applies) |
| Attrition | fatigued, drained; forced march | **exhaustion 1-6**; forced march; foraging | **graded conditions** and **integer resources** |
| Encounter check | per activity, per hex | random encounter by terrain and time | a per-tick roll that may inject a battle-map node |

The through-line: every row is a stance, a resource, a tick, a graded condition,
or a resolution. Those are primitives Isometry either has or is one small module
from.

## Findings: the primitives already exist (2026-07-18, verified)

Mapped against the current codebase:

- **Exhaustion is graded conditions, already shipped.** 5e exhaustion 1-6 and
  PF2e fatigued/drained are `frightened 2`'s siblings: a named condition carrying
  a magnitude. `MapDocument::condition_value` / `set_condition`
  (`isometry-core/src/map.rs:134,146`) store and read exactly this. Zero new
  substrate.
- **Tempo is the world tick, already shipped.** Per-map clocks
  (`isometry-net/src/protocol.rs:63`) advance on rounds and on the DM's
  `TimeAdvanced` (`protocol.rs:262`). C7's banked-time budget
  (`isometry-campaign/src/faction.rs:28,124`) is the same tick driving downtime
  tempo. An edge traversal advances the clock; pace and terrain scale by how
  much.
- **Foraging is want/have resources, pattern shipped.** The radiant-quest deficit
  reads `want_<thing>` over `have_<thing>` on a faction sheet
  (`faction.rs:131,141`). A party's food and water is the same shape at party
  scale.
- **Encounter triggers exist.** `EncounterAnchor`
  (`isometry-campaign/src/map.rs:49`) already tags a spot with encounter intent,
  and transition points (C2) already move the party onto a prepared map. A random
  encounter is a per-tick roll that resolves to injecting one.
- **The travel resolver has a template.** `resolve_action`
  (`isometry-system/src/lib.rs:1580`) is the pattern: the system judges and
  returns a `Resolution` of deltas/beats/conditions, peers apply and never rerun
  Lua. A travel check is the same pattern at edge scale.

## The two genuinely new pieces

Everything above is composition. Two pieces are actually new:

1. **The pointcrawl graph** (the board). A pure `isometry-core` module: nodes are
   sites, edges carry an abstract weight (distance, time, difficulty).
   Pathfinding reuses `reachable` / `path_to`
   (`isometry-core/src/path.rs:30,98`), a weighted BFS whose only change is
   generalizing "neighbour" from grid-adjacency to graph-edges. **Not re-scoped
   here**: the tile-geometry seam plan frames hex, triangle, and pointcrawl as
   one seam of six ops (project / unproject / neighbours / distance / directions
   / depth). Exploration mode consumes that seam; it does not own it.

2. **The travel resolver** (the rules). An edge-scale mirror of `resolve_action`:
   given a party, an edge, a pace, and each member's exploration stance, the
   system rules the outcome (arrive; arrive but lose time; get lost onto a wrong
   edge; trigger an encounter; accrue exhaustion; forage a resource), and the
   substrate applies it (advance the clock, move the party to a node, set a
   graded condition, adjust a resource, inject an encounter). Illustrative only,
   not compile-ready:

   ```rust
   // ILLUSTRATIVE.
   struct TravelIntent { party: PartyId, edge: EdgeId, pace: Pace }
   struct TravelResolution {
       arrived: NodeId,                          // may be the wrong node (lost)
       ticks: u64,                               // clock advance: pace x weight
       conditions: Vec<(TokenId, String, i64)>,  // exhaustion, per member
       resources: Vec<(String, i64)>,            // foraged food/water, party
       encounter: Option<NodeId>,                // a battle-map node to enter
       beats: Vec<Beat>,                         // representation, as ever
   }
   ```

   `TravelResolution` reuses the exact consequence vocabulary the action resolver
   already replicates (conditions, beats), so the net layer carries it with the
   existing machinery.

## Where "mode" lives

The open design question. PF2e's three modes are a first-class concept: encounter
mode has an initiative order, exploration mode does not, downtime mode is
untimed. Isometry has never named "mode" in the substrate. Two options:

- **A. Mode is emergent.** There is no `Mode` enum. "Encounter" is "a `TurnList`
  with an active token exists"; "exploration" is "the active board is a
  pointcrawl graph without a `TurnList`"; "downtime" is "the faction/world tick".
  The substrate stays mode-blind, and the app and system infer the mode from
  what state is present. This matches the doctrine: the substrate never knows
  what a hit point is, and need not know what a mode is.
- **B. Mode is explicit.** A small party-level `mode` field the system reads, so
  an exploration activity can be gated to exploration mode and a pace applies
  only while traveling.

**Recommendation: A, emergent, until a rule genuinely needs B.** The substrate
already distinguishes "in initiative" from "free play" (a `TurnList` with an
active token versus an empty one), and C7's played-faction and free-play work
leaned on exactly that seam. Exploration mode is "on a graph board, advancing a
party along edges". If a specific rule turns out to need an explicit flag to be
expressible, add it then, the way the afford gate added turn counters only once
the economy needed them.

## The substrate/system split for travel

Unchanged doctrine, stated for this lane so it does not drift:

- **Substrate owns**: the graph (nodes, weighted edges), the party position on
  it, the clock/tick, the per-token stance slot (an opaque name), integer
  resources, graded conditions, and applying a `TravelResolution`. It never knows
  what Scout does or when you get lost.
- **System owns** (Lua): what each exploration activity does, what a pace costs in
  ticks, the navigation check and its DC, foraging yields, forced-march and
  exhaustion rules, and the encounter table. The signature is
  `f(party, edge, pace, stances) -> TravelResolution`, the travel analogue of the
  hit rule being one line of script.

## Licensing

- **PF2e is fully ORC-open**, hexploration and exploration activities included, so
  PF2e content ships. As with the action-spec skeleton, PF2e is the better first
  vehicle: it exercises the substrate hooks with real, shippable rules.
- **5e SRD (CC-BY) covers the basics** (exhaustion, travel pace) but the richer
  exploration layer (foraging DCs, getting-lost specifics) is thinner in the SRD
  than in the non-open PHB/DMG. So 5e gets the substrate hooks and a lighter
  activity set; verify exact SRD coverage before shipping any 5e exploration
  content. The substrate primitives are content-neutral, so this constrains
  content, not architecture.

## Phases (done-conditions, not estimates)

Gated behind the wilderness trigger. Listed so the shape is visible; not
scheduled.

- **E0: The graph board. LANDED 2026-07-18** (substrate + session; rendering
  next). The `Overmap` primitive (a fresh weighted Dijkstra, not the grid BFS),
  projected from `CampaignWorld`'s existing places + routes, with the party's
  position as replicated session state and a `PartyMoved` event. *Done when* a
  party sits on a node and paths along weighted edges, no rules attached: met.
  Remaining for a playable E0 is only the overmap *rendering* (drawing the graph,
  clicking a node to travel), which is app UI, not substrate.
- **E1: Pace and tick.** A party-level pace setting; traversing an edge advances
  the map clock by a system-computed amount. *Done when* the same edge costs
  different ticks at different paces, replicated, and the split-party clocks (C3)
  stay coherent.
- **E2: The travel resolver.** `resolve_travel` mirroring `resolve_action`; the
  system rules arrive / lost / time; peers apply. *Done when* a navigation
  failure lands the party on a different node or adds ticks, decided once and
  replicated, and a client cannot pronounce its own travel verdict.
- **E3: Exploration activities.** A per-token stance slot the system reads (Scout,
  Search, Avoid Notice). *Done when* choosing Scout versus Search changes a travel
  outcome (initiative on the next encounter, or find-versus-speed), entirely in
  Lua.
- **E4: Attrition.** Forced march accrues exhaustion (graded condition, reused);
  foraging adjusts a party resource. *Done when* a long march sets exhaustion N
  and a forage roll changes food, both system-ruled, both replicated.
- **E5: Encounter checks.** A per-tick roll that may inject a battle-map node via
  the existing `EncounterAnchor` / transition machinery. *Done when* traveling can
  drop the party into a prepared or generated encounter, and returning resumes the
  graph.

Order matters: E0-E1 are the board and tempo (mostly reuse), E2 is the one new
resolver, E3-E5 are Lua-heavy and lean on primitives already shipped.

## Open forks

- **Party as a unit.** Exploration acts on a *party*, not a token. Is a party a
  first-class substrate object, or the set of tokens sharing an owner and board?
  Leaning on the latter (owners already group tokens), but forced march and
  shared resources may want a party handle.
- **Graph and battle-map coupling.** A node holds a prepared `MapDocument` (via
  `EncounterAnchor` / transition), so entering an encounter is C2's travel.
  Confirm the graph node reuses the transition primitive rather than a parallel
  one.
- **How much hexploration.** The PF2e hex subsystem (activities per day,
  Reconnoiter, Map) is richer than a plain pointcrawl. Ship the pointcrawl first;
  hex activities are an E3 stance expansion, not a separate board.

## Progress

- **2026-07-18:** doc written, scoping only. Reframes C8 as exploration mode, the
  missing third leg of both systems' play triad. Finds most primitives already
  shipped (graded conditions = exhaustion, world tick = tempo, want/have =
  foraging, `EncounterAnchor` = encounter triggers, `resolve_action` = the
  resolver template); isolates the two new pieces (the pointcrawl graph, cited to
  the tile-geometry seam plan, and a travel resolver). Recommends emergent mode
  (option A).
- **2026-07-18:** **trigger pulled.** Mark: overmap exploration is part of the
  game. Status ACTIVE; the board is named the *overmap*. Building from E0.
- **2026-07-18:** **E0 substrate core landed.** `Overmap` in `isometry-core`
  (`overmap.rs`): a pointcrawl graph (`OvermapNode` sites, weighted `OvermapEdge`
  routes, directed or not) with `neighbours`, `reachable_within(from, budget)`,
  and `route(from, to)`. Pure geometry, 4 tests green. Two findings:
  1. **Weighted Dijkstra, not the grid's uniform BFS.** The doc said E0 reuses
     `reachable`; it reuses the *shape* (reachable-within-budget + a path) but not
     the code, because overmap routes carry unequal weights and uniform BFS
     cannot cost them. The overmap owns a small bounded Dijkstra instead.
  2. **The graph likely projects from `CampaignWorld`, not a new authored field.**
     `CampaignWorld` already models geography as `places` + `routes` (W-plan).
     The next E0 increment (the party sitting on a node, moving along edges) is
     probably an `Overmap` *projected* from those (nodes = places, edges =
     routes + a weight), with the party's node as session state, rather than a
     second authored graph. Open: `WorldRoute` has no weight yet (derive from
     place positions, or add one). This keeps the overmap primitive pure and the
     authored geography single-sourced.
  Remaining E0: the projection + the party's position on the overmap + moving it,
  which is the session/app wiring the done-condition names.
- **2026-07-18:** **E0 complete** (substrate + session). `WorldRoute` gained a
  `weight`; `CampaignWorld::overmap()` projects places + routes into an `Overmap`
  (the geography stays single-sourced, no second authored graph); the party's
  position is `CampaignWorld.party_node` (owner -> node, beside `faction_control`)
  set by a replicated `WorldEvent::PartyMoved`. A place's tactical `map` becomes
  the node's `site`, so entering a site reuses C2's transition. Verified:
  `the_overmap_projects_from_places_and_routes`,
  `a_party_sits_on_an_overmap_node_and_travels` (campaign), and
  `a_party_travels_the_overmap_and_every_peer_agrees` (net). E0's done-condition
  is met; only the overmap *rendering* (app UI) remains before it is playable on
  screen. Next: E1 (pace and tick) or the overmap render, then E2 (the travel
  resolver).
