//! Durable world data, storylet matching, and editable campaign drafts.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use isometry_core::{Overmap, OvermapEdge, OvermapNode};

use crate::{ItemProposal, LocalMapProposal, MapScale, SecretFact, WorldFact};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignWorld {
    #[serde(default)]
    pub factions: BTreeMap<String, WorldFaction>,
    #[serde(default)]
    pub places: BTreeMap<String, WorldPlace>,
    #[serde(default)]
    pub characters: BTreeMap<String, WorldCharacter>,
    #[serde(default)]
    pub routes: BTreeMap<String, WorldRoute>,
    #[serde(default)]
    pub laws: BTreeMap<String, WorldLaw>,
    #[serde(default)]
    pub history: Vec<HistoryEvent>,
    #[serde(default)]
    pub storylets: BTreeMap<String, StoryletProposal>,
    /// A faction's mutable numbers, keyed by faction id: banked downtime time,
    /// and the `want_<thing>` / `have_<thing>` pairs that drive radiant quests.
    /// A faction's "sheet" at a different scale, but integers only for now --
    /// enough for banking and demand, and `Eq` (unlike the float-carrying
    /// `SheetData` a token holds). Promoting it to a full sheet a system can read
    /// through its Lua is the "abilities are projections" refinement. Unlike the
    /// immutable identity in [`Self::factions`], this changes as a faction acts,
    /// so it overwrites rather than insert-onces.
    #[serde(default)]
    pub faction_sheets: BTreeMap<String, BTreeMap<String, i64>>,
    /// Who plays each faction: faction id -> the player name granted its
    /// channel. A faction is an owner name like any other (a token owned by a
    /// faction id belongs to that faction), and this grant lets a player command
    /// that faction's tokens as if their own -- the per-channel permission that
    /// makes a faction *playable* rather than only DM-run. Absent means the DM
    /// runs it. Session state, not authored content, but it lives beside
    /// `faction_sheets` because both are the mutable per-faction layer.
    #[serde(default)]
    pub faction_control: BTreeMap<String, String>,
    /// Where each traveling party sits on the overmap: party owner -> place id.
    /// A split party (C3) keeps separate positions; a single party has one entry.
    /// Session play-state, like [`Self::faction_control`], not authored content.
    #[serde(default)]
    pub party_node: BTreeMap<String, String>,
    /// Each party's travel pace, as a percent of normal time: 100 is normal, 50
    /// is fast (half the time), 200 is slow (double). Absent reads as 100. The
    /// number is all the substrate keeps; what a pace *trades* (fast loses
    /// passive Perception, slow lets you forage) is system business.
    #[serde(default)]
    pub party_pace: BTreeMap<String, i64>,
    /// The overmap a party has discovered: party owner -> the place ids it knows.
    /// The rest of the map is hidden -- unseeable, unroutable -- until revealed by
    /// travel, word of mouth, a guide, a skill check, or a map read. Fog at
    /// overmap scale, with explored memory: once known, a place stays known.
    #[serde(default)]
    pub party_known: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldFaction {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub claims: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldPlace {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub map: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldCharacter {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub faction: Option<String>,
    #[serde(default)]
    pub place: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldRoute {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Travel weight for the overmap: the abstract cost of taking this route.
    /// A road and a mountain pass between the same two places can differ, which
    /// is the whole point of a pointcrawl (the swamp shortcut versus the safe
    /// road). Zero (the unauthored default) reads as 1 when projected, so an
    /// unweighted route is still traversable at unit cost.
    #[serde(default)]
    pub weight: u32,
}

/// A named rule of the generated setting. `parameters` is pack vocabulary;
/// system plugins opt into keys they understand rather than the substrate
/// hardcoding what iron, fire, names, oaths, or magic mean.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldLaw {
    pub id: String,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub id: String,
    /// Pack-defined chronological tick. Equal ticks retain authored order.
    pub time: i64,
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub participants: Vec<String>,
    #[serde(default)]
    pub place: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoryletRequirements {
    /// Every tag must be carried by at least one committed faction.
    #[serde(default)]
    pub faction_tags: Vec<String>,
    /// IDs are checked against the host-private store, without exposing text.
    #[serde(default)]
    pub hidden_facts: Vec<String>,
    #[serde(default)]
    pub world_laws: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSlot {
    pub key: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StoryletEffect {
    Fact { fact: WorldFact },
    History { event: HistoryEvent },
    Item { item: ItemProposal },
    LocalMap { map: LocalMapProposal },
}

/// A quality-based narrative opportunity. Matching and casting are pure;
/// committing each effect remains an explicit host operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoryletProposal {
    pub key: String,
    pub entry: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub requirements: StoryletRequirements,
    #[serde(default)]
    pub roles: Vec<RoleSlot>,
    #[serde(default)]
    pub effects: Vec<StoryletEffect>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryletResolution {
    pub cast: BTreeMap<String, String>,
    pub effects: Vec<StoryletEffect>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoryletError {
    MissingFactionTag(String),
    MissingHiddenFact(String),
    MissingWorldLaw(String),
    UncastRole(String),
}

impl CampaignWorld {
    pub fn resolve_storylet<'a, I>(
        &self,
        storylet: &StoryletProposal,
        hidden_fact_ids: I,
    ) -> Result<StoryletResolution, StoryletError>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let hidden: BTreeSet<&str> = hidden_fact_ids.into_iter().collect();
        for tag in &storylet.requirements.faction_tags {
            if !self
                .factions
                .values()
                .any(|faction| faction.tags.iter().any(|candidate| candidate == tag))
            {
                return Err(StoryletError::MissingFactionTag(tag.clone()));
            }
        }
        for fact in &storylet.requirements.hidden_facts {
            if !hidden.contains(fact.as_str()) {
                return Err(StoryletError::MissingHiddenFact(fact.clone()));
            }
        }
        for law in &storylet.requirements.world_laws {
            if !self.laws.contains_key(law) {
                return Err(StoryletError::MissingWorldLaw(law.clone()));
            }
        }

        let mut used = BTreeSet::new();
        let mut cast = BTreeMap::new();
        for role in &storylet.roles {
            let candidate = self.characters.values().find(|character| {
                !used.contains(character.id.as_str())
                    && role.tags.iter().all(|tag| character.tags.contains(tag))
            });
            let Some(candidate) = candidate else {
                return Err(StoryletError::UncastRole(role.key.clone()));
            };
            used.insert(candidate.id.as_str());
            cast.insert(role.key.clone(), candidate.id.clone());
        }
        Ok(StoryletResolution {
            cast,
            effects: storylet.effects.clone(),
        })
    }

    pub fn apply(&mut self, event: &WorldEvent) -> Result<(), WorldError> {
        match event {
            WorldEvent::Faction(value) => insert_same(&mut self.factions, &value.id, value),
            WorldEvent::Place(value) => insert_same(&mut self.places, &value.id, value),
            WorldEvent::Character(value) => insert_same(&mut self.characters, &value.id, value),
            WorldEvent::Route(value) => {
                if !self.places.contains_key(&value.from) || !self.places.contains_key(&value.to) {
                    return Err(WorldError::UnknownRouteEndpoint(value.id.clone()));
                }
                insert_same(&mut self.routes, &value.id, value)
            }
            WorldEvent::Law(value) => insert_same(&mut self.laws, &value.id, value),
            WorldEvent::History(value) => {
                if let Some(existing) = self.history.iter().find(|event| event.id == value.id) {
                    return if existing == value {
                        Ok(())
                    } else {
                        Err(WorldError::ConflictingId(value.id.clone()))
                    };
                }
                self.history.push(value.clone());
                self.history.sort_by_key(|event| event.time);
                Ok(())
            }
            WorldEvent::Storylet(value) => insert_same(&mut self.storylets, &value.key, value),
            WorldEvent::FactionSheet { faction, sheet } => {
                // A faction's resources change as it acts, so this overwrites --
                // it is the one mutable world entity, and the reason it is a
                // separate variant rather than another insert_same registry.
                self.faction_sheets.insert(faction.clone(), sheet.clone());
                Ok(())
            }
            WorldEvent::FactionControlSet { faction, player } => {
                match player {
                    Some(name) => self.faction_control.insert(faction.clone(), name.clone()),
                    None => self.faction_control.remove(faction),
                };
                Ok(())
            }
            WorldEvent::PartyMoved { party, node } => {
                // The substrate records where the party is; whether the step was
                // legal (an edge exists, the pace afforded it) is the travel
                // resolver's business (E2), and the host offers only reachable
                // nodes. E0 is "no rules attached".
                self.party_node.insert(party.clone(), node.clone());
                // Arriving discovers where you are and what is one step away.
                self.discover_around(party, node);
                Ok(())
            }
            WorldEvent::PartyPaceSet { party, pace } => {
                self.party_pace.insert(party.clone(), *pace);
                Ok(())
            }
            WorldEvent::NodeRevealed { party, node } => {
                // A place learned some other way: a rumour, a guide, a map read.
                self.reveal(party, node);
                Ok(())
            }
        }
    }

    /// A faction's mutable numbers, if it has any bound yet.
    pub fn faction_sheet(&self, faction: &str) -> Option<&BTreeMap<String, i64>> {
        self.faction_sheets.get(faction)
    }

    /// The player who plays `faction`, if its channel has been granted to one.
    pub fn faction_controller(&self, faction: &str) -> Option<&str> {
        self.faction_control.get(faction).map(String::as_str)
    }

    /// Project the world's geography into a travelable overmap: a node per place,
    /// an edge per route. The pointcrawl the party explores is not a second
    /// authored graph; it is this *view* of the places and routes the campaign
    /// already has, so the geography stays single-sourced. Node positions are not
    /// set here (a rendering layout sets them later); pathfinding needs only the
    /// routes' weights, and an unweighted route costs 1.
    pub fn overmap(&self) -> Overmap {
        let mut overmap = Overmap::new(String::new());
        overmap.nodes = self
            .places
            .values()
            .map(|place| OvermapNode {
                id: place.id.clone(),
                name: place.name.clone(),
                at: (0, 0),
                site: place.map.clone(),
            })
            .collect();
        overmap.edges = self
            .routes
            .values()
            .map(|route| OvermapEdge {
                from: route.from.clone(),
                to: route.to.clone(),
                weight: route.weight.max(1),
                directed: false,
            })
            .collect();
        overmap
    }

    /// Which overmap node a party (keyed by its owner) currently sits on.
    pub fn party_at(&self, party: &str) -> Option<&str> {
        self.party_node.get(party).map(String::as_str)
    }

    /// A party's travel pace as a percent of normal (100 when unset).
    pub fn pace(&self, party: &str) -> i64 {
        self.party_pace.get(party).copied().unwrap_or(100)
    }

    /// The travel time, in ticks, for `party` to reach `to` from `from` at its
    /// current pace: the shortest route's total weight scaled by the pace percent
    /// (100 normal, 50 fast/half, 200 slow/double), at least 1. `None` when `to`
    /// is unreachable. The same edge costs different ticks at different paces,
    /// which is the point; what a pace trades for the time is the system's, not
    /// this function's.
    pub fn travel_cost(&self, party: &str, from: &str, to: &str) -> Option<u64> {
        let (_, weight) = self.overmap().route(from, to)?;
        let pct = self.pace(party).max(1) as u64;
        Some(((weight as u64 * pct) / 100).max(1))
    }

    /// Whether `party` has discovered `node`.
    pub fn knows(&self, party: &str, node: &str) -> bool {
        self.party_known
            .get(party)
            .is_some_and(|known| known.contains(node))
    }

    /// Reveal a place to a party. Idempotent; once known, always known. The
    /// substrate does not care *how* it was found -- travel, a rumour, a guide, a
    /// map read -- only that it now is.
    pub fn reveal(&mut self, party: &str, node: &str) {
        self.party_known
            .entry(party.to_owned())
            .or_default()
            .insert(node.to_owned());
    }

    /// Reveal a place and everywhere one route from it to a party: arriving
    /// somewhere, you learn it and see where you could go next. This is how
    /// travel discovers the map, a step at a time, without a guide or a check.
    pub fn discover_around(&mut self, party: &str, node: &str) {
        self.reveal(party, node);
        let neighbours: Vec<String> = self
            .overmap()
            .neighbours(node)
            .into_iter()
            .map(|(id, _)| id.to_owned())
            .collect();
        for neighbour in neighbours {
            self.reveal(party, &neighbour);
        }
    }

    /// The overmap as `party` knows it: only the places it has discovered and the
    /// routes between two known places. What it has not found it cannot see or
    /// plot a course to, so pathfinding on this view refuses to route through the
    /// dark. A party that knows nothing gets an empty map.
    pub fn overmap_for(&self, party: &str) -> Overmap {
        let full = self.overmap();
        let Some(known) = self.party_known.get(party) else {
            return Overmap::new(full.name);
        };
        let mut out = Overmap::new(full.name);
        out.nodes = full
            .nodes
            .into_iter()
            .filter(|node| known.contains(&node.id))
            .collect();
        out.edges = full
            .edges
            .into_iter()
            .filter(|edge| known.contains(&edge.from) && known.contains(&edge.to))
            .collect();
        out
    }
}

fn insert_same<T: Clone + PartialEq>(
    values: &mut BTreeMap<String, T>,
    id: &str,
    value: &T,
) -> Result<(), WorldError> {
    if id.trim().is_empty() {
        return Err(WorldError::MissingId);
    }
    if let Some(existing) = values.get(id) {
        return if existing == value {
            Ok(())
        } else {
            Err(WorldError::ConflictingId(id.to_owned()))
        };
    }
    values.insert(id.to_owned(), value.clone());
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldEvent {
    Faction(WorldFaction),
    Place(WorldPlace),
    Character(WorldCharacter),
    Route(WorldRoute),
    Law(WorldLaw),
    History(HistoryEvent),
    Storylet(StoryletProposal),
    /// Bind or update a faction's mutable numbers. Overwrites, because a
    /// faction's resources and banked time change as it acts.
    FactionSheet {
        faction: String,
        sheet: BTreeMap<String, i64>,
    },
    /// Grant (or, with `None`, revoke) a player's control of a faction's channel.
    /// The DM's ruling; every peer applies it, and a controlling player may then
    /// command the faction's tokens.
    FactionControlSet {
        faction: String,
        player: Option<String>,
    },
    /// Move a party (keyed by its owner) to an overmap node (a place id). The
    /// substrate records the position; adjacency and travel cost are the
    /// resolver's and the host's, not this event's.
    PartyMoved {
        party: String,
        node: String,
    },
    /// Set a party's travel pace, as a percent of normal time (100/50/200).
    PartyPaceSet {
        party: String,
        pace: i64,
    },
    /// Reveal an overmap place to a party (a rumour, a guide's directions, a map
    /// read the reader passed). The DM commits it; travel discovers on its own.
    NodeRevealed {
        party: String,
        node: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldError {
    MissingId,
    ConflictingId(String),
    UnknownRouteEndpoint(String),
    MissingStartingMap(String),
    DuplicateMap(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftMap {
    pub scale: MapScale,
    pub map: LocalMapProposal,
}

/// One host-private, inspectable proposal. Its public pieces lower into world
/// and map events; `secrets` lower only into the private campaign store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignDraft {
    pub id: String,
    pub name: String,
    pub world: CampaignWorld,
    #[serde(default)]
    pub maps: Vec<DraftMap>,
    #[serde(default)]
    pub secrets: Vec<SecretFact>,
    #[serde(default)]
    pub rewards: Vec<ItemProposal>,
    pub starting_map: String,
    pub final_storylet: String,
}

impl CampaignDraft {
    pub fn validate(&self) -> Result<(), WorldError> {
        if self.id.trim().is_empty() {
            return Err(WorldError::MissingId);
        }
        if !self.maps.iter().any(|map| map.map.id == self.starting_map) {
            return Err(WorldError::MissingStartingMap(self.starting_map.clone()));
        }
        if !self.world.storylets.contains_key(&self.final_storylet) {
            return Err(WorldError::ConflictingId(self.final_storylet.clone()));
        }
        let mut rebuilt = CampaignWorld::default();
        for event in self.public_world_events() {
            rebuilt.apply(&event)?;
        }
        let mut map_ids = BTreeSet::new();
        for map in &self.maps {
            if !map_ids.insert(map.map.id.as_str()) {
                return Err(WorldError::DuplicateMap(map.map.id.clone()));
            }
            map.map
                .lower(map.scale)
                .map_err(|_| WorldError::ConflictingId(map.map.id.clone()))?;
        }
        Ok(())
    }

    pub fn public_world_events(&self) -> Vec<WorldEvent> {
        self.world
            .factions
            .values()
            .cloned()
            .map(WorldEvent::Faction)
            .chain(self.world.places.values().cloned().map(WorldEvent::Place))
            .chain(
                self.world
                    .characters
                    .values()
                    .cloned()
                    .map(WorldEvent::Character),
            )
            .chain(self.world.routes.values().cloned().map(WorldEvent::Route))
            .chain(self.world.laws.values().cloned().map(WorldEvent::Law))
            .chain(self.world.history.iter().cloned().map(WorldEvent::History))
            .chain(
                self.world
                    .storylets
                    .values()
                    .cloned()
                    .map(WorldEvent::Storylet),
            )
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storylet_requires_private_fact_and_casts_existing_character() {
        let mut world = CampaignWorld::default();
        world.factions.insert(
            "tide".into(),
            WorldFaction {
                id: "tide".into(),
                name: "Tide Court".into(),
                tags: vec!["river".into()],
                claims: vec![],
            },
        );
        world.characters.insert(
            "mara".into(),
            WorldCharacter {
                id: "mara".into(),
                name: "Mara".into(),
                tags: vec!["warden".into()],
                faction: Some("tide".into()),
                place: None,
            },
        );
        world.laws.insert(
            "iron-remembers".into(),
            WorldLaw {
                id: "iron-remembers".into(),
                name: "Iron remembers".into(),
                text: "Iron keeps the name of its maker.".into(),
                tags: vec!["magic".into()],
                parameters: BTreeMap::new(),
            },
        );
        let storylet = StoryletProposal {
            key: "sunken-vow".into(),
            entry: "The old oath surfaces.".into(),
            tags: vec![],
            requirements: StoryletRequirements {
                faction_tags: vec!["river".into()],
                hidden_facts: vec!["vow.secret".into()],
                world_laws: vec!["iron-remembers".into()],
            },
            roles: vec![RoleSlot {
                key: "warden".into(),
                tags: vec!["warden".into()],
            }],
            effects: vec![],
        };
        assert!(matches!(
            world.resolve_storylet(&storylet, []),
            Err(StoryletError::MissingHiddenFact(_))
        ));
        let resolved = world.resolve_storylet(&storylet, ["vow.secret"]).unwrap();
        assert_eq!(resolved.cast["warden"], "mara");
    }

    #[test]
    fn campaign_draft_rejects_duplicate_map_ids_before_commit() {
        let mut world = CampaignWorld::default();
        world.storylets.insert(
            "finale".into(),
            StoryletProposal {
                key: "finale".into(),
                entry: "Finale".into(),
                tags: vec![],
                requirements: Default::default(),
                roles: vec![],
                effects: vec![],
            },
        );
        let map = LocalMapProposal {
            id: "same".into(),
            name: "Same".into(),
            width: 2,
            height: 2,
            default_ground: "grass".into(),
            cells: vec![],
            spawn_zones: vec![],
            transitions: vec![],
            encounter_anchors: vec![],
        };
        let draft = CampaignDraft {
            id: "draft".into(),
            name: "Draft".into(),
            world,
            maps: vec![
                DraftMap {
                    scale: MapScale::Region,
                    map: map.clone(),
                },
                DraftMap {
                    scale: MapScale::Local,
                    map,
                },
            ],
            secrets: vec![],
            rewards: vec![],
            starting_map: "same".into(),
            final_storylet: "finale".into(),
        };
        assert_eq!(
            draft.validate(),
            Err(WorldError::DuplicateMap("same".into()))
        );
    }

    fn place(id: &str, name: &str) -> WorldPlace {
        WorldPlace {
            id: id.into(),
            name: name.into(),
            tags: vec![],
            map: None,
        }
    }

    fn route(id: &str, from: &str, to: &str, weight: u32) -> WorldRoute {
        WorldRoute {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            tags: vec![],
            weight,
        }
    }

    #[test]
    fn the_overmap_projects_from_places_and_routes() {
        let mut world = CampaignWorld::default();
        for (id, name) in [("village", "Village"), ("forest", "Forest"), ("ruins", "Ruins")] {
            world.places.insert(id.into(), place(id, name));
        }
        // The forest opens into a tactical map; the node carries it as its site.
        world.places.get_mut("forest").unwrap().map = Some("forest-map".into());
        world.routes.insert("r1".into(), route("r1", "village", "forest", 2));
        world.routes.insert("r2".into(), route("r2", "forest", "ruins", 3));
        // An unweighted route (weight 0) still costs 1 once projected.
        world.routes.insert("r3".into(), route("r3", "village", "ruins", 0));

        let overmap = world.overmap();
        assert_eq!(overmap.nodes.len(), 3, "a node per place");
        assert_eq!(
            overmap.node("forest").and_then(|n| n.site.as_deref()),
            Some("forest-map"),
            "a place's tactical map becomes the node's site"
        );
        // The direct village->ruins route projects to cost 1 (weight 0 -> 1),
        // cheaper than through the forest (5). Pathfinding runs on the projection.
        let (path, cost) = overmap.route("village", "ruins").expect("the ruins are reachable");
        assert_eq!(path, vec!["village", "ruins"]);
        assert_eq!(cost, 1, "an unweighted route projects to unit cost");
    }

    #[test]
    fn a_party_sits_on_an_overmap_node_and_travels() {
        let mut world = CampaignWorld::default();
        world.places.insert("village".into(), place("village", "Village"));
        world.places.insert("forest".into(), place("forest", "Forest"));
        world.routes.insert("r1".into(), route("r1", "village", "forest", 2));

        assert_eq!(world.party_at("A"), None, "the party starts off the map");
        world
            .apply(&WorldEvent::PartyMoved {
                party: "A".into(),
                node: "village".into(),
            })
            .unwrap();
        assert_eq!(world.party_at("A"), Some("village"));
        // The projected overmap says the forest is reachable, so travel there.
        assert!(world.overmap().route("village", "forest").is_some());
        world
            .apply(&WorldEvent::PartyMoved {
                party: "A".into(),
                node: "forest".into(),
            })
            .unwrap();
        assert_eq!(world.party_at("A"), Some("forest"), "the party travelled the edge");
    }

    #[test]
    fn pace_scales_the_travel_cost() {
        let mut world = CampaignWorld::default();
        world.places.insert("village".into(), place("village", "Village"));
        world.places.insert("forest".into(), place("forest", "Forest"));
        world.routes.insert("r1".into(), route("r1", "village", "forest", 4));

        // Default pace is normal (100%): the cost is the route's weight.
        assert_eq!(world.pace("A"), 100);
        assert_eq!(world.travel_cost("A", "village", "forest"), Some(4));

        // Fast (50%) halves the time; slow (200%) doubles it. Same edge, same
        // party, different ticks.
        world
            .apply(&WorldEvent::PartyPaceSet { party: "A".into(), pace: 50 })
            .unwrap();
        assert_eq!(world.travel_cost("A", "village", "forest"), Some(2), "fast is half the time");
        world
            .apply(&WorldEvent::PartyPaceSet { party: "A".into(), pace: 200 })
            .unwrap();
        assert_eq!(world.travel_cost("A", "village", "forest"), Some(8), "slow is double");

        // A cost never rounds to zero, and an unreachable destination has none.
        assert_eq!(world.travel_cost("A", "village", "atlantis"), None);
    }

    #[test]
    fn a_party_discovers_the_overmap_as_it_travels() {
        let mut world = CampaignWorld::default();
        for id in ["village", "forest", "ruins", "island"] {
            world.places.insert(id.into(), place(id, id));
        }
        world.routes.insert("r1".into(), route("r1", "village", "forest", 2));
        world.routes.insert("r2".into(), route("r2", "forest", "ruins", 2));
        // The island has no route to it.

        // A party that knows nothing sees an empty overmap.
        assert!(world.overmap_for("A").nodes.is_empty(), "the unfound map is dark");
        assert!(!world.knows("A", "village"));

        // Arriving at the village discovers it and its neighbour (the forest),
        // but not what is two steps on (the ruins).
        world
            .apply(&WorldEvent::PartyMoved { party: "A".into(), node: "village".into() })
            .unwrap();
        assert!(world.knows("A", "village"));
        assert!(world.knows("A", "forest"), "and one step on");
        assert!(!world.knows("A", "ruins"), "but not two steps on");
        // The known overmap shows only what has been found, and refuses to route
        // through the dark.
        let known = world.overmap_for("A");
        assert_eq!(known.nodes.len(), 2);
        assert!(known.route("village", "ruins").is_none(), "cannot plot a course into the unknown");

        // Travel on to the forest, and the ruins come into view.
        world
            .apply(&WorldEvent::PartyMoved { party: "A".into(), node: "forest".into() })
            .unwrap();
        assert!(world.knows("A", "ruins"), "arriving at the forest reveals the ruins");

        // A rumour reveals the island directly, though no road leads there.
        world
            .apply(&WorldEvent::NodeRevealed { party: "A".into(), node: "island".into() })
            .unwrap();
        assert!(world.knows("A", "island"), "word of mouth reaches the unreachable");
    }
}
