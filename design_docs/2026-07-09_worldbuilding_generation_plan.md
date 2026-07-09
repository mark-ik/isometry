# Worldbuilding Generation Plan

**Date:** 2026-07-09
**Status:** plan. Created from the hidden-modifiers / generated-worlds
discussion. This plan extends the campaign-pack lane with generator and
worldbuilding machinery. It does not replace the existing substrate/system
split: Isometry's substrate stores geometry, turns, and inspectable generated
state; system plugins decide what game rules mean; campaign packs supply taste.

**Thesis:** take random generation far by making it authored and inspectable.
A pack provides tables, parts, constraints, secrets, world laws, storylets, and
generators. The DM host runs generation, previews it, and commits the result as
ordinary campaign state. Players receive the result, not the random process.
This scales from hidden item modifiers to local maps, region/world maps, and
Wildermyth-shaped narrative arcs without putting nondeterminism into replay.

## Relation to prior docs

- Builds on [campaign_packs_plan](2026-07-08_campaign_packs_plan.md) decision
  12: generation is host-authoritative, and the result crosses the wire. Seed
  replay is an optional bandwidth optimization only for deterministic
  generators.
- Builds on [optional_intelligence_vision](2026-07-07_optional_intelligence_vision.md):
  deterministic tables and storylets are the floor; models can help author or
  rephrase, but runtime play cannot depend on them.
- Uses [board_to_text_narration_plan](2026-07-07_board_to_text_narration_plan.md)
  as the factual projection for generated scenes and future recaps.
- Complements [environmental_surfaces_plan](2026-07-08_environmental_surfaces_plan.md):
  custom world laws can decide what fire, water, iron, names, moons, oaths, or
  blood do, while the substrate only stores surface/state facts.

## Prior art (researched 2026-07-09)

Four bodies of work map onto this plan's rungs almost one to one. Cite them
for shape; adapt, don't copy.

- **Caves of Qud history generation (Grinblat, GDC 2018).** Validates
  decision 4 exactly: no full simulation. Generate historical events first,
  then *rationalize* them after the fact with replacement grammars over a
  curated text corpus. Secrets are history facts the player hasn't found yet;
  they can be discovered in any order and a journal sorts them
  chronologically. That is this plan's reveal model applied to worldbuilding:
  a generated history event marked `gm` is a secret; revealing it appends to
  a campaign journal. Villages are then generated *from* the history
  (name, government, culture, relationships), which is what "projections"
  should mean in decision 4. Sources: [GDC talk](https://gdcvault.com/play/1024990/Procedurally-Generating-History-in-Caves),
  [paper](https://www.pcgworkshop.com/archive/grinblat2017subverting.pdf).
- **Cyclic dungeon generation (Dormans, Unexplored).** For rung 3, structure
  should come from *cycles*, not from WFC alone: generate a loop of two paths
  between entrance and goal, then apply patterns to the loop: lock-and-key,
  hidden shortcut, guarded treasure, valve. Nested cycles read as designed,
  not scattered, and lock/key/secret patterns are exactly D&D battle-map
  intent. WFC/adjacency rules then fill texture inside the structural
  skeleton; prop scatter last. A tabletop adaptation already exists
  ([Sersa Victory](https://sersavictory.itch.io/cyclic-dungeon-generation)),
  useful as a pack-authoring vocabulary. Sources:
  [overview](https://www.gamedeveloper.com/design/unexplored-s-secret-cyclic-dungeon-generation-),
  [ctrl500 article](https://ctrl500.com/game-design/handcrafted-feel-dungeon-generation-unexplored-explores-cyclic-dungeon-generation/).
- **Storylet theory (Short, Kreminski).** The storylet definition in decision
  6 is the standard one (content + prerequisites + effects). The selection
  problem has named solutions: *quality-based* (requirements over world
  facts decide eligibility), *salience-based* (score eligible storylets by
  how specifically they match the current situation, surface the best), and
  *waypoint* (pathfind through narrative space toward a destination beat).
  Isometry's DM-in-the-loop version: QBN eligibility filters, salience
  *sorts a suggestion hand for the DM*, and the DM is the drama manager.
  Campaign skeletons (rung 6) are the waypoint layer. Sources:
  [Beyond Branching](https://emshort.blog/2016/04/12/beyond-branching-quality-based-and-salience-based-narrative-structures/),
  [storylets survey](https://emshort.blog/2019/01/06/kreminski-on-storylets/).
- **Wildermyth event design.** Two techniques transfer. *Role casting*: an
  event declares roles with trait/relationship/stat predicates and casts
  them from the existing character pool, like a casting call; reusing
  existing NPCs instead of minting new ones is what makes a world feel
  persistent. *String of pearls*: a campaign locks required beginning/end
  modules and fills the middle from the eligible pool, which is the rung-6
  shape. Sources: [event design philosophy](https://wildermyth.com/wiki/Event_design_philosophy),
  [story inputs/outputs](https://wildermyth.com/wiki/Story_Inputs_and_Outputs).

## Decisions

1. **Generated content is state, not prose.** Prose is a view. The durable
   product is typed objects: items, places, factions, laws, secrets, maps,
   storylets, and events. This keeps results searchable, editable, revealable,
   and usable by rules/plugins.
2. **Every generated object has visibility layers.** At minimum:
   - `public`: what players can see now.
   - `gm`: hidden truth, tags, modifiers, locked facts.
   - `revealed_to`: per-player or table-wide revealed facts.
   A cursed sword, a disguised NPC, a false law, and a secret faction all use
   the same shape. Where each layer physically lives (host store vs shared
   log) is decision 8; `revealed_to` is deferred past v1 there.
3. **Hidden modifiers are first-class.** Items, NPCs, places, and spells can
   carry hidden modifiers with reveal conditions. The reveal condition is data:
   identify, attune, use in a place, kill a foe, earn trust, speak a name,
   inspect under moonlight, and so on.
4. **Worldbuilding is a history log plus projections.** Dwarf-Fortress-level
   ambition should enter as a small event log first: founding, migration, war,
   disaster, pact, schism, exile, lost artifact. The present world is projected
   from those events. Do not simulate every citizen or economy before the event
   spine is useful.
5. **World laws are pack data.** Custom magic and customs are "laws" that
   generators and rules can consult: iron disrupts fae magic, true names bind
   spirits, the dead cannot cross running water, fire strengthens in drought,
   oathbreakers carry marks. A law can affect item modifiers, surfaces,
   dialog, encounters, and map generation.
6. **Storylets are the narrative unit.** A storylet is a typed card with
   requirements, roles, beats, choices, and effects. It fills the space between
   static adventure text and open-ended LLM improvisation.
7. **The DM commits, peers replay.** A generated map, item, region, world fact,
   or storylet result is committed as data in the host's ordered log or saved
   campaign pack. Peers do not have to rerun Lua, WFC, or model output to
   converge.
8. **Secrets never enter the replicated log (2026-07-09).** Two facts force
   this. Convergence is an FNV-1a rolling hash over the postcard bytes of
   every `(seq, GameEvent)`, so all peers must hold byte-identical logs;
   per-recipient filtering of events would break the convergence check
   itself. And anything in `GameSnapshot` or the log sits in every client's
   memory, readable by a curious player regardless of UI. So the architecture
   is two stores: the **host-private campaign store** holds the `gm` layer
   (hidden modifiers, secret facts, unrevealed history, storylet internals),
   and the **shared log** carries only public projections. A reveal is an
   ordinary `GameEvent` that *publishes* a fact into the log; commit of a
   generated object publishes its public face. `revealed_to` (per-player
   secrets) cannot ride the hashed log either; v1 scopes reveal to
   table-wide, and per-player reveal later gets a DM-to-player whisper
   channel that is explicitly outside consensus state, like a direct
   message, not like state. `visibility.rs` already anticipated this
   split: "filtered on the wire versus filtered at render is a
   session-policy choice above this module." This resolves open question 3:
   the answer is wire-culled by construction, not render-hidden.
9. **Cast before you create.** Generators that need an NPC, faction, or place
   fill role slots from the existing campaign pool first (Wildermyth's
   casting call), minting new entities only when no candidate matches the
   role's predicates. Reuse is what makes generated content accumulate into
   a world instead of a stream of strangers.

## Pack shape

Illustrative, not a fixed on-disk format yet:

```text
pack.toml
systems/
  5e-srd.toml
assets/
  vox/
  palettes/
  tilesets/
parts/
  weapons.toml
  armor.toml
  ruins.toml
content/
  monsters.json
  items.json
  spells.json
  factions.toml
world/
  laws.toml
  cultures.toml
  history_tables.toml
storylets/
  missing_heir.toml
  haunted_road.toml
  oathbound_sword.toml
generators/
  names.lua
  loot.lua
  local_maps.lua
  regions.lua
  campaign.lua
maps/
docs/
  attribution.md
fixtures/
  loot_smoke.toml
  local_map_smoke.toml
```

The authoring rule: a pack should be readable as the tutorial for itself. A
small pack with five items, one local-map generator, two factions, one world
law, and three storylets is more valuable than an opaque giant generator.

## Data model sketches

Illustrative, not compile-ready.

```rust
pub enum Visibility {
    Public,
    Gm,
    RevealedTo(Vec<PlayerId>),
}

pub struct SecretFact {
    pub id: String,
    pub text: String,
    pub tags: Vec<String>,
    pub reveal: RevealCondition,
}

pub struct HiddenModifier {
    pub id: String,
    pub public_hint: Option<String>,
    pub gm_effect: EffectSpec,
    pub reveal: RevealCondition,
}

pub struct ItemInstance {
    pub id: String,
    pub template: String,
    pub public_name: String,
    pub public_tags: Vec<String>,
    pub hidden: Vec<HiddenModifier>,
    pub appearance: Option<AppearanceRef>,
}

pub struct WorldLaw {
    pub id: String,
    pub public_name: String,
    pub public_text: String,
    pub hidden_clause: Option<SecretFact>,
    pub hooks: Vec<LawHook>,
}

pub struct Storylet {
    pub id: String,
    pub requires: Vec<Requirement>,
    pub roles: Vec<RoleSlot>,
    pub beats: Vec<Beat>,
    pub effects: Vec<EffectSpec>,
}
```

The shared principle is that generators create these objects and the host
commits them. A model may suggest them, but it does not become the authority.

## Randomization ladder

### Rung 1: Items and hidden modifiers

Best first slice. Base item plus material, quality, modifier, origin, quirk,
secret, and appearance recipe.

Examples:

- `public`: "Old river-steel longsword"
- `gm`: oathbound, hates undead, forged_by=eel_cult
- `reveal`: identify hints at cold metal; kill undead unlocks the river oath;
  bringing it to an eel-cult shrine reveals its true name.
- `effects`: +1 attack; secret drawback against river spirits until cleansed.

**Done when:** a generated item has public text, hidden GM-only modifiers, a
reveal condition, and an equipment-visible appearance change; the DM can reveal
one hidden fact and the table view updates without changing the item identity.

### Rung 2: NPCs, monsters, and factions

Generate from archetype and role, not from nothing. A pack supplies cultist,
knight, witch, goblin, merchant, priest, dragon; the generator fills name,
faction, goal, gear, hidden tie, rumor, palette, and optional dialog hooks.

**Done when:** generating an NPC produces a token/sheet plus at least one hidden
faction or secret fact, and that fact can feed a storylet requirement.

### Rung 3: Local maps

Generate `MapDocument` outputs from tile grammars, room/road grammars, WFC or
adjacency rules, elevation passes, prop scatter, spawn zones, transitions, and
encounter anchors. Voxels supply tile/prop parts and appearance, but the map is
still tile grids and elevation.

Structure before texture: the skeleton should come from cyclic generation
(loops with lock-and-key / hidden-shortcut / guarded-treasure patterns, see
prior art), because cycles carry tactical intent and read as designed. WFC
and adjacency rules fill terrain texture inside that skeleton; prop scatter
runs last. Lock-and-key composes directly with hidden modifiers: the key can
be an item secret, the lock a revealable map fact.

**Done when:** a local-map generator creates a playable battle map, the DM can
hand-edit it, and the committed map travels in a session snapshot/log without
peers rerunning the generator.

### Rung 4: Region maps

A region is a coarser `MapDocument`, not a projection of local maps. It has
roads, districts, lairs, sites, encounter zones, and transition points into
local maps.

**Done when:** a region generator creates a coarse map with linked entrances to
two local maps, and a host event swaps the table into one linked local map.

### Rung 5: World graph

The world tier is a pointcrawl graph: places as nodes, routes as edges, tags as
terrain/weather/faction control, and travel costs as system-interpreted data.
This can later ride chart/mere graph primitives, but Isometry should first keep
it small and pure.

**Done when:** a generated world graph has sites, routes, faction claims, and
links to region/local documents; the party can move along an edge and trigger a
storylet or encounter.

### Rung 6: Campaign skeletons

Campaign generation is storylets plus clocks and roles. A skeleton defines acts,
beats, factions, secrets, escalation clocks, required scene roles, optional
sidebars, and reward paths. It should feel Wildermyth-shaped: structured enough
to cohere, flexible enough to fill with generated places and people.

**Done when:** a campaign generator creates a small arc with a starting region,
two factions, three secrets, one law, two local maps, and a final confrontation
storylet, all inspectable and editable before play.

### Rung 7: Faction turns (the world tick)

The missing rung between a generated world and a Wildermyth-shaped living one:
between sessions, factions act. Each faction carries goals and assets; a
downtime step generates one move per faction (expand, scheme, raid, court,
fracture) as new **history events**, constrained by world laws and current
claims. The DM previews the batch in the same reroll/lock/commit table, edits,
and commits; committed moves become rumors, storylet fuel, and map changes.
This is the Stars-Without-Number faction-turn shape and it reuses the whole
stack: history log (decision 4), generators (W2), storylet requirements (W4).
Nothing here is real-time; the tick runs when the DM asks for it.

**Done when:** running one downtime tick against a committed world produces
2-4 faction events the DM can edit and commit, at least one of which changes
a storylet's eligibility or a region-map fact.

**Expanded 2026-07-09: factions as participants, not just a tick.**

- **Time-coupled turns ("meanwhile...").** The tick is proportional to
  in-world time the table spent in the local map. Needs a world clock the
  substrate lacks: v1 is the host stamping elapsed world time on scene
  exit; factions **bank** that time and spend it on their turn. Banking
  keeps turns batched (nothing ticks mid-scene), proportional, and inside
  the no-real-time doctrine. Committed moves present as a "meanwhile"
  interstitial via the board-to-text narration lane before the next map.
- **A faction sheet is just a sheet.** `SheetData` is system-agnostic, so a
  faction is a sheet at a different scale: resources, people, assets,
  goals. Guild-scale is the expected norm (fighters/mages/thieves/bards/
  artisan guilds, cults, churches, companies), not nations.
- **Abilities are projections.** What a faction can do on its turn derives
  from resources + people + its history events (decision 4 applied to
  mechanics): lose the harbor in a war event, lose fleet actions until it
  is retaken, and the log says why.
- **Radiant quests are faction demand.** The creative-mode prompt engine
  pointed at faction resource deficits: the mages guild lacks lodestone, so
  a delivery/heist/negotiation storylet proposal spawns with the guild cast
  as patron (decision 9 casting). Faction demand is quest supply, which
  makes "why would my character fetch this" diegetic, and gives members a
  standing reason to bring things home to their faction.
- **Factions can be played.** Blades in the Dark's crew sheet is the proof:
  a shared faction-PC advancing beside the characters, clocks ticking in
  downtime. A player (or the table) playing a faction instead of or beside
  a PC is a per-channel permission in the shared-authority doc's terms.
  Prior art: BitD crew + faction clocks, Reign's company rules, Pendragon's
  winter phase (time-coupled world advance), Skyrim's radiant quests (the
  generative loop this borrows its name from), SWN factions (the base tick).

**Also done when (participant upgrade):** a session that spends stamped
world time in a local map yields a proportional faction turn whose committed
moves render as a "meanwhile" interstitial, and at least one faction deficit
generates a storylet proposal with that faction cast as patron.

## World laws and custom magic

World laws are where custom magic becomes more than a spell list. They are
pack-authored rules that generators and system plugins can query.

Examples:

- **Iron law:** iron breaks glamour; iron weapons reveal fae disguises.
- **Name law:** true names bind spirits; a hidden name is a revealable secret.
- **Water law:** dead kings cannot cross running water.
- **Moon law:** divine spells fail under a false moon.
- **Oath law:** oathbreakers carry visible marks after a failed hidden check.
- **Memory law:** dragons hoard memories, so loot is a recollection, not gold.

The substrate should not interpret those laws directly. It stores the law and
the facts it creates. System plugins decide when a law affects rules; generators
use it as a constraint and motif source.

**Done when:** a generated item, NPC, map feature, and storylet all reference
the same world law, and changing/removing that law before commit changes the
generated outputs coherently.

## Generator runtime

Two modes:

- **Commit-result mode (default):** host runs generator, stores the output data.
  Peers replay output only. This is robust and should be the default for maps,
  campaigns, model-assisted output, and any Lua generator with uncertain
  determinism.
- **Seed-replay mode (optional):** host commits generator id + seed and peers
  rerun it. Allowed only for vetted deterministic generators: fixed integer RNG,
  order-stable data structures, content-hash generator identity, fuel limits,
  and no ambient host state.

The first implementation should use commit-result mode.

## Authoring surface

The generator UI should be a preview table, not a command that mutates instantly.

Controls:

- `Generate`
- `Reroll`
- `Lock` fields/roles/rooms/results the DM likes
- `Reveal/Hide` visibility layers
- `Inspect source` for the pack table/storylet that produced a result
- `Commit`
- `Discard`

This is the safety valve for randomness: a generator proposes, the DM edits,
then commits.

## Phases

### W0: Data visibility and generated log channel

**Landed 2026-07-09.** `isometry-campaign` founded (open question 1's
recommendation taken): `SecretFact`/`RevealCondition`/`WorldFact`/
`Visibility` + the host-private `CampaignStore` (reveal moves a fact out of
the GM layer and returns its public face, so a fact is always in exactly one
store). `FieldValue` widened with `Float`/`List`/`Map` (appended variants;
postcard tags by index). `GameEvent::Fact(WorldFact)` + an uncapped
`GameSnapshot::journal` (entries are campaign state, unlike the roll log's
capped noise); the host rejects client `Fact` intents. Guard:
`secrets_stay_host_side_until_revealed` (isometry-net) proves the
done-condition including the no-secret-bytes check on peer snapshots.
Lua marshalling and the sheet view skip/summarize nested fields until
W1/W2 give them real consumers.

Add the smallest state vocabulary that can hold hidden facts and generated
results, shaped by decision 8 (secrets never enter the replicated log).

- Extend value storage beyond flat scalars (`FieldValue::List`, `Map`, and
  likely `Float`), or add a parallel generated-object store if that proves
  cleaner.
- Add the **host-private campaign store**: where `gm`-layer objects, secret
  facts, and unrevealed history live. Saved with the campaign, never
  serialized into `GameSnapshot` or `GameEvent`.
- Add a replicated public channel (`GameEvent` side): committed public faces
  of generated objects plus `FactRevealed`-shaped events, so revealed facts
  travel like rolls and sheet changes and accumulate into a table-visible
  campaign journal (the Qud journal shape).
- Visibility in v1 is two-layer (public, GM-only) with table-wide reveal;
  per-player `revealed_to` waits for a whisper channel outside consensus
  state.

**Done when:** a generated fact with GM-only text can be stored and saved
host-side, revealed to the table as an ordinary logged event, and replicated
without clients rerunning a generator; and a peer's `GameSnapshot` bytes
provably contain no unrevealed `gm` data.

### W1: Hidden item modifiers

Implement item templates/instances and hidden modifiers as the first concrete
consumer.

- Base items from the SRD item data.
- Modifier tables: material, quality, enchantment, curse, origin, quirk.
- Reveal conditions.
- Equipment hooks scoped enough to affect sheet display and a token appearance
  layer.

**Done when:** a generated sword can be equipped, changes a visible stat or
action expression, changes the token appearance, carries a hidden curse, and
reveals the curse through a DM action.

### W2: Generator pack ABI

Define the pack-side generator API before broadening to maps/campaigns.

- `call_gen(args) -> GenValue`, where `GenValue` can be text, object, list,
  item, NPC, map patch, world fact, or storylet proposal.
- Fuel and recursion caps.
- Host-provided entropy tape.
- Fixture runner for pack authors.
- Lock semantics: `Lock` pins chosen *values*, which regeneration receives as
  constraints (`args.locked`). Locks are not entropy-tape replay; tape replay
  breaks the moment structure changes, constraints survive any reroll.
- Casting API (decision 9): a generator asks the host to fill a role from the
  campaign pool (`cast_role(predicates) -> Option<EntityRef>`) and only mints
  a new entity on `None`.

**Done when:** a pack-owned Lua generator can produce deterministic fixture
output under a test seed, and the host preview UI can reroll/lock/commit.

### W3: Local map generator

Use the generator ABI to create `MapDocument` outputs.

- Tile grammar.
- Elevation grammar.
- Prop scatter.
- Spawn zones and transition points.
- Encounter anchors.

**Done when:** a generated local map is playable, hand-editable, saveable, and
committed as result data.

### W4: World facts, laws, and storylets

Add the campaign-state layer: factions, places, laws, history events, secrets,
and storylets.

**Done when:** a storylet can require a faction tag and a hidden fact, generate
an encounter/map/item proposal, and commit its effects to campaign state.

### W5: Region/world/campaign generator

Compose the prior pieces into an editable campaign draft.

**Done when:** one pack can generate a small campaign: world graph, one region,
two local maps, faction conflict, hidden secrets, custom law, item reward, and a
final encounter, all inspectable before commit.

## Findings

- 2026-07-09: `isometry-core::FieldValue` is currently `Int | Text | Bool`
  only (`crates/isometry-core/src/sheet.rs`), so inventories, nested
  modifiers, condition lists, and storylet state need either a widened value
  model or a separate generated-object store.
- 2026-07-09: replicated shared state is `GameSnapshot { map, turns,
  roll_log }`, and `GameEvent` currently covers map events, turn events,
  rolls, and sheet replacement (`crates/isometry-net/src/protocol.rs`). There
  is no generated-result, narration, secret, item, or world-fact channel yet.
- 2026-07-09: `MapDocument` is intentionally tile layers, elevation, tokens,
  and token-bound sheets (`crates/isometry-core/src/map.rs`). This supports the
  plan's rule that voxels are appearance/generation substrate, not local-map
  storage.
- 2026-07-09 (review pass): session convergence is an FNV-1a rolling hash
  over the postcard bytes of each `(seq, GameEvent)`
  (`crates/isometry-net/src/protocol.rs`), so peers must hold byte-identical
  logs; this is what forces decision 8's two-store split. Per-recipient
  event filtering is architecturally unavailable on the hashed log.
- 2026-07-09 (review pass): three existing modules are assets to this plan
  and were not previously cited: `isometry-core/src/visibility.rs` (LOS
  geometry, whose header already defers the wire-vs-render filtering policy
  this plan now decides), `isometry-system/src/items.rs` + `data/items.json`
  (the SRD 5.1 equipment list W1 templates from, CC-BY-4.0), and
  `isometry-core/src/narrate.rs` (the deterministic board-to-text projection
  generated scenes and recaps consume).

## Open questions

1. Should generated objects live in `isometry-core` beside `MapDocument`, or in
   a new pure `isometry-campaign` crate consumed by net/views/host?
   *Recommendation (2026-07-09):* new `isometry-campaign` crate. Campaign
   objects are not geometry, core's purity rule is the repo's most
   load-bearing invariant, and the host-private store (decision 8) needs a
   home that net can depend on for public projections without core growing
   an items/factions vocabulary.
2. Is the first replicated channel `GameEvent::Narration`, a broader
   `GameEvent::WorldFact`, or a generic campaign-state patch?
   *Leaning (2026-07-09):* `WorldFact`-shaped with a `kind` tag, because the
   journal, reveals, and faction-turn results all want the same envelope;
   plain narration is then a kind, not a sibling channel.
3. ~~Are hidden facts visible to browser clients at all, or must the host cull
   GM-only data before snapshot for stricter trust than the current friendly
   table model?~~ Resolved by decision 8: wire-culled by construction; the
   friendly-table model keeps table-wide reveal as the v1 scope.
4. How much of item/equipment belongs in the substrate versus the system plugin?
   Bias: substrate stores instances/slots/tags; system plugin interprets
   effects.
5. Do world laws affect tactical rules directly, or only through system-plugin
   hooks? Bias: plugin hooks, not substrate branching.
6. Reveal conditions: a closed data enum (identify, attune, use-in-place,
   slay-tagged, trust-threshold, speak-name) with a Lua-hook escape hatch, or
   Lua predicates from the start? Bias: data enum first; the substrate stores
   and displays conditions without interpreting them (same posture as laws),
   the DM can always reveal manually, and hooks arrive with W2's ABI.
7. Does the world-graph rung eventually ride chartulary's container-graph
   substrate instead of a bespoke pointcrawl struct? Plan already says start
   small and pure; revisit after W5 when the shape is known.

## Progress

- 2026-07-09: Plan created from the hidden-modifiers / worldbuilding
  generation discussion. Scope set as a ladder: hidden item modifiers first,
  then generator ABI, local maps, storylets/world laws, then campaign-scale
  generation.
- 2026-07-09: Review + research pass. Added prior art (Qud history
  generation, cyclic dungeon generation, storylet selection theory,
  Wildermyth role casting); decision 8 (secrets never enter the replicated
  log; two-store architecture forced by the convergence hash, resolving open
  question 3); decision 9 (cast before you create); rung 7 (faction turns as
  the world tick); cyclic structure-before-texture guidance in rung 3; lock
  and casting semantics in W2; W0 reshaped around the host-private store
  with a no-secret-bytes done-condition; recommendations recorded on open
  questions 1 and 2; findings on `visibility.rs`, `items.rs`, `narrate.rs`,
  and the protocol hash.
- 2026-07-09: W0 landed (see the phase section): `isometry-campaign`
  crate, widened `FieldValue`, `GameEvent::Fact` + journal, host-only
  fact commit, and the no-secret-bytes replication guard. Next: W1
  (hidden item modifiers as the first concrete consumer).
- 2026-07-09: Rung 7 expanded from a DM-triggered tick to factions as
  participants: time-coupled "meanwhile" turns banked from stamped scene
  time, faction sheets as ordinary `SheetData`, abilities as history
  projections, radiant quests from faction deficits, and factions as
  playable entities (BitD crew shape; permissions per the shared-authority
  doc).
