//! Reading and mutating the world: overmap projection, party discovery,
//! storylet resolution, and the insert-once registries.
//!
//! Split out of `world.rs` on 2026-07-24; behavior unchanged.

use super::*;

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
                at: place.position.unwrap_or((0, 0)),
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

    /// A party's store of a named resource (0 when it has none).
    pub fn party_resource(&self, party: &str, key: &str) -> i64 {
        self.party_resources
            .get(party)
            .and_then(|store| store.get(key))
            .copied()
            .unwrap_or(0)
    }

    /// Add to a party's store of a named resource (a negative delta spends it).
    /// A store that reaches zero is dropped, so an untouched party keeps nothing.
    pub fn add_party_resource(&mut self, party: &str, key: &str, delta: i64) {
        let store = self.party_resources.entry(party.to_owned()).or_default();
        let amount = store.entry(key.to_owned()).or_insert(0);
        *amount += delta;
        if *amount <= 0 {
            store.remove(key);
        }
        if store.is_empty() {
            self.party_resources.remove(party);
        }
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

pub(crate) fn insert_same<T: Clone + PartialEq>(
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

