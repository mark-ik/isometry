//! Durable world data, storylet matching, and editable campaign drafts.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

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
        }
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
}
