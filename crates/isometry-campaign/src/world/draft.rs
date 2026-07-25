//! World events, their errors, and the editable campaign draft.
//!
//! A draft is what a DM is still authoring; committing turns it into the
//! replicated events every peer folds.
//!
//! Split out of `world.rs` on 2026-07-24; behavior unchanged.

use super::*;

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

