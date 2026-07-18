//! Faction turns: the downtime tick where the world acts on itself.
//!
//! Between scenes, factions move. One tick draws a move per committed faction
//! from the world's own state and a host entropy tape, and each move is a
//! bundle of ordinary [`WorldEvent`]s -- always a `History` line (the
//! "meanwhile" the table reads before the next map) and, for the verbs that
//! reshape the setting, the change itself. Because a move commits through the
//! same `CampaignWorld::apply` path a DM edit does, a faction acting on the
//! world is never a special case downstream: it lands in the same log, hashes
//! into the same convergence, and replicates to every peer unchanged.
//!
//! The tick is pure and replayable -- the same world, tick, and seed always
//! yield the same batch, which the DM previews, edits, and commits. The verb
//! table here is the substrate's default; a content pack can supply richer
//! moves the way a system layers over the builtin rules, without any of the
//! commit machinery changing. Nothing here knows what a faction *is* beyond a
//! name, some tags, and some claims: the meaning is the pack's and the table's.

use serde::{Deserialize, Serialize};

use crate::{
    CampaignWorld, EntropyTape, HistoryEvent, StoryletProposal, StoryletRequirements,
    WorldCharacter, WorldEvent, WorldFaction, WorldPlace,
};

/// What a faction does on its downtime turn: the Stars-Without-Number verb set.
/// Each is a shape of move the tick can roll, and each writes the world its own
/// way -- courting brings in a person, fracturing splits off a rival, expanding
/// claims ground, scheming seeds a story, raiding only makes a rumor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactionVerb {
    Expand,
    Scheme,
    Raid,
    Court,
    Fracture,
}

impl FactionVerb {
    fn from_draw(draw: u64) -> Self {
        match draw % 5 {
            0 => Self::Expand,
            1 => Self::Scheme,
            2 => Self::Raid,
            3 => Self::Court,
            _ => Self::Fracture,
        }
    }

    /// The verb's tag, stamped on its history event so a pack or the narration
    /// lane can style "the Tide Court raided" differently from "it fractured".
    pub fn label(self) -> &'static str {
        match self {
            Self::Expand => "expand",
            Self::Scheme => "scheme",
            Self::Raid => "raid",
            Self::Court => "court",
            Self::Fracture => "fracture",
        }
    }
}

/// One faction's downtime action: the narrative record and the world change it
/// makes. The `change` is kept separate from `history` so the DM can strike the
/// change while keeping the story (a raid that stays a rumor) or the reverse,
/// and so a purely narrative beat is simply `change: None`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactionMove {
    pub faction: String,
    pub verb: FactionVerb,
    pub history: HistoryEvent,
    pub change: Option<WorldEvent>,
}

impl FactionMove {
    /// The replicated events this move commits, history first. The commit path
    /// applies each through `CampaignWorld::apply`, exactly as any world edit,
    /// so there is no faction-turn-specific apply anywhere.
    pub fn into_events(self) -> Vec<WorldEvent> {
        let mut events = vec![WorldEvent::History(self.history)];
        events.extend(self.change);
        events
    }
}

impl CampaignWorld {
    /// One downtime tick: a move per committed faction, at world time `tick`,
    /// drawn from a host entropy tape. Pure and replayable; the DM edits and
    /// commits the batch through [`crate::WorldEvent`]s. Factions are visited in
    /// id order (the map's), so the draw sequence -- and thus the batch -- is
    /// stable for a given world, tick, and seed.
    pub fn faction_turn(&self, tick: i64, tape: &mut EntropyTape) -> Vec<FactionMove> {
        self.factions
            .values()
            .map(|faction| {
                let verb = FactionVerb::from_draw(tape.draw());
                self.build_move(faction, verb, tick, tape)
            })
            .collect()
    }

    fn build_move(
        &self,
        faction: &WorldFaction,
        verb: FactionVerb,
        tick: i64,
        tape: &mut EntropyTape,
    ) -> FactionMove {
        let nonce = tape.draw() % 100_000;
        let (text, change) = match verb {
            FactionVerb::Court => {
                // A recruited character can be cast in a storylet role, so a
                // court move reaches back into the storylet graph: a story that
                // could not cast now can. The ally carries the faction's tags.
                let name = ally_name(nonce);
                let text = format!("{name} swore to the {}.", faction.name);
                let change = WorldEvent::Character(WorldCharacter {
                    id: format!("{}.ally.t{tick}.{nonce}", faction.id),
                    name,
                    tags: faction.tags.clone(),
                    faction: Some(faction.id.clone()),
                    place: None,
                });
                (text, Some(change))
            }
            FactionVerb::Fracture => {
                // A splinter carries its parent's tags plus a grievance tag, so
                // a `faction_tags` requirement nothing met before can now be
                // satisfied -- eligibility changes without touching the parent
                // (committed factions are immutable; a move adds, never mutates).
                let mut tags = faction.tags.clone();
                tags.push("splinter".to_owned());
                let text = format!("A faction broke from the {}.", faction.name);
                let change = WorldEvent::Faction(WorldFaction {
                    id: format!("{}.splinter.t{tick}.{nonce}", faction.id),
                    name: format!("Splinter of {}", faction.name),
                    tags,
                    claims: Vec::new(),
                });
                (text, Some(change))
            }
            FactionVerb::Expand => {
                let text = format!("The {} claimed new ground.", faction.name);
                let change = WorldEvent::Place(WorldPlace {
                    id: format!("{}.hold.t{tick}.{nonce}", faction.id),
                    name: format!("Hold of {}", faction.name),
                    tags: faction.tags.clone(),
                    map: None,
                });
                (text, Some(change))
            }
            FactionVerb::Scheme => {
                // A scheme seeds a storylet the table can later play. It requires
                // the faction's own first tag, so it is eligible the moment it
                // lands (a tagless faction schemes an always-open story).
                let key = format!("{}.scheme.t{tick}.{nonce}", faction.id);
                let text = format!("The {} set a scheme in motion.", faction.name);
                let change = WorldEvent::Storylet(StoryletProposal {
                    key,
                    entry: format!("A scheme of the {} comes due.", faction.name),
                    tags: vec!["faction-scheme".to_owned()],
                    requirements: StoryletRequirements {
                        faction_tags: faction.tags.first().cloned().into_iter().collect(),
                        hidden_facts: Vec::new(),
                        world_laws: Vec::new(),
                    },
                    roles: Vec::new(),
                    effects: Vec::new(),
                });
                (text, Some(change))
            }
            FactionVerb::Raid => {
                // The pure-narrative beat: a raid makes history but claims
                // nothing, so the DM sees a move whose `change` is None -- proof
                // the story and the mechanical change are separable.
                (format!("The {} raided a rival.", faction.name), None)
            }
        };
        let history = HistoryEvent {
            id: format!("{}.move.t{tick}.{nonce}", faction.id),
            time: tick,
            kind: "faction-turn".to_owned(),
            text,
            participants: vec![faction.id.clone()],
            place: None,
            tags: vec![verb.label().to_owned()],
        };
        FactionMove {
            faction: faction.id.clone(),
            verb,
            history,
            change,
        }
    }
}

/// A placeholder name pool. Real names are a pack's job (rung 7: names come from
/// content); this keeps the foundation legible without inventing a name system.
fn ally_name(nonce: u64) -> String {
    const NAMES: [&str; 8] = [
        "Bran", "Ysolde", "Cael", "Mirren", "Doran", "Sefa", "Vane", "Odile",
    ];
    NAMES[(nonce as usize) % NAMES.len()].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoryletError;

    fn faction(id: &str, name: &str, tags: &[&str]) -> WorldFaction {
        WorldFaction {
            id: id.to_owned(),
            name: name.to_owned(),
            tags: tags.iter().map(|t| t.to_owned().to_owned()).collect(),
            claims: Vec::new(),
        }
    }

    fn world_with(factions: Vec<WorldFaction>) -> CampaignWorld {
        let mut world = CampaignWorld::default();
        for f in factions {
            world.factions.insert(f.id.clone(), f);
        }
        world
    }

    #[test]
    fn one_move_per_faction_and_the_tick_is_replayable() {
        let world = world_with(vec![
            faction("tide", "Tide Court", &["river"]),
            faction("ash", "Ash Company", &["fire"]),
        ]);

        let mut a = EntropyTape::from_seed(7);
        let mut b = EntropyTape::from_seed(7);
        let first = world.faction_turn(3, &mut a);
        let second = world.faction_turn(3, &mut b);

        assert_eq!(first.len(), 2, "one move per committed faction");
        assert_eq!(first, second, "same world, tick, and seed => the same batch");
        // Every move records history at the tick, whatever else it does.
        assert!(first.iter().all(|m| m.history.time == 3));
        assert!(first.iter().all(|m| m.history.kind == "faction-turn"));
        // A different seed can produce a different batch (the tick is not fixed).
        let mut c = EntropyTape::from_seed(8);
        let other = world.faction_turn(3, &mut c);
        assert_ne!(first, other, "the batch is seed-driven, not constant");
    }

    #[test]
    fn a_faction_turn_reaches_the_storylet_graph() {
        // A storylet that needs a faction tagged `splinter` -- nothing carries
        // it, so nothing can play it yet.
        let world = world_with(vec![faction("tide", "Tide Court", &["river"])]);
        let storylet = StoryletProposal {
            key: "the-schism".to_owned(),
            entry: "The broken oath is called in.".to_owned(),
            tags: Vec::new(),
            requirements: StoryletRequirements {
                faction_tags: vec!["splinter".to_owned()],
                hidden_facts: Vec::new(),
                world_laws: Vec::new(),
            },
            roles: Vec::new(),
            effects: Vec::new(),
        };
        assert!(
            matches!(
                world.resolve_storylet(&storylet, []),
                Err(StoryletError::MissingFactionTag(_))
            ),
            "no faction is splintered yet"
        );

        // A fracture move splits off a `splinter`-tagged faction. Find one over
        // a fixed seed range (deterministic), apply its events, and the storylet
        // that could not resolve now can -- the tick changed eligibility.
        let mut applied = false;
        for seed in 0..64u64 {
            let mut tape = EntropyTape::from_seed(seed);
            for m in world.faction_turn(1, &mut tape) {
                if m.verb != FactionVerb::Fracture {
                    continue;
                }
                let mut after = world.clone();
                for event in m.into_events() {
                    after.apply(&event).expect("faction move events apply");
                }
                assert!(
                    after.resolve_storylet(&storylet, []).is_ok(),
                    "the splinter made the storylet eligible"
                );
                applied = true;
                break;
            }
            if applied {
                break;
            }
        }
        assert!(applied, "a fracture move is reachable within 64 seeds");
    }

    #[test]
    fn a_raid_is_a_rumor_with_no_world_change() {
        let world = world_with(vec![faction("ash", "Ash Company", &["fire"])]);
        for seed in 0..64u64 {
            let mut tape = EntropyTape::from_seed(seed);
            let moves = world.faction_turn(1, &mut tape);
            if let Some(raid) = moves.iter().find(|m| m.verb == FactionVerb::Raid) {
                assert!(raid.change.is_none(), "a raid claims nothing");
                // But it still makes history: into_events is never empty.
                assert_eq!(raid.clone().into_events().len(), 1, "just the rumor");
                return;
            }
        }
        panic!("a raid move is reachable within 64 seeds");
    }
}
