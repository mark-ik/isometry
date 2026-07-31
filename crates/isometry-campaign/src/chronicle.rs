//! Creatures arriving from Mesocosm, and the history this campaign adds to
//! them.
//!
//! A sibling of [`isometry_voxel::body`], and the other half of the same seam.
//! A body profile carries what a creature *looks like*; a chronicle carries
//! what it *is and did*. Appearance goes to the voxel lane, the record comes
//! here, and neither needs the other.
//!
//! # The roster slot is the one we already had
//!
//! An arriving creature becomes a [`WorldCharacter`] — the same struct an
//! authored NPC uses, with the same fields, stored the same way. That is not a
//! convenience; it is the point. The games wing's third law says player history
//! *displaces* procedural content and never gates it, and the proof it demands
//! is that the consuming game cannot tell a played creature from a generated
//! one. A separate `ImportedCharacter` type would fail that test by existing.
//!
//! Note where the arrival lands: `WorldCharacter` carries a `faction`. A
//! creature that joins one becomes a character in the wing's exact sense — a
//! faction-associated borg — so this seam is where that promotion happens.
//!
//! # Nothing here links Mesocosm
//!
//! The wing couples its vessels by data, not by types, so the wire structs
//! below are a deliberate local mirror. Postcard is positional: **field order
//! is what actually binds**, names are documentation. Keep them in step with
//! `mesocosm-core`'s `chronicle` module, and rely on the fixture test rather
//! than on care.
//!
//! # Appending, never editing
//!
//! [`Arrival::record`] is the only way this crate changes a chronicle. Deeds
//! written by other games are carried out exactly as they came in, including
//! payloads nothing here can parse. That is the keystone rule — additive facts,
//! opaque preservation — and the reason a creature can cross three games
//! without anybody's history being quietly dropped.

use serde::{Deserialize, Serialize};

use crate::world::{HistoryEvent, WorldCharacter};

/// The schema this reads and writes.
pub const CHRONICLE_SCHEMA: &str = "mesocosm.chronicle/v0";

/// How this campaign names itself when it appends a fact.
pub const VESSEL: &str = "isometry";

/// The one verb in the wing's shared vocabulary that this campaign writes.
/// Shared means the payload is a contract, not a convenience: see
/// [`Arrival::record_loss`].
pub const LOST_PART: &str = "lost-part";

const MAGIC: [u8; 8] = *b"MESOCHRN";
const VERSION: u16 = 0;
const HEADER_LEN: usize = 10;

/// Why a chronicle could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChronicleError {
    TooShort { got: usize },
    /// The magic names a different schema. A body profile read as a chronicle
    /// lands here rather than as a confusing decode failure.
    WrongSchema,
    /// This is a chronicle, from a writer we do not agree with.
    UnknownVersion { found: u16, expected: u16 },
    Malformed,
    /// Decoded, but a creature with no parts was never a body.
    Inconsistent,
}

/// Where one part came from. `from_species` is `None` at founding.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct PartOrigin {
    pub from_species: Option<u32>,
    pub from_part: Option<u32>,
    pub epoch: u64,
}

impl PartOrigin {
    pub fn is_incorporated(&self) -> bool {
        self.from_species.is_some()
    }
}

/// One thing that happened, in the recording game's own words.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Deed {
    pub vessel: String,
    pub verb: String,
    pub at: u64,
    /// The recording game's payload. Opaque to everyone else, preserved by
    /// everyone else.
    pub detail: Vec<u8>,
}

/// A creature as a record. Field order must match the writer's.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct Chronicle {
    pub species: u32,
    pub parts: Vec<PartOrigin>,
    pub deeds: Vec<Deed>,
}

/// What this campaign puts in a deed's `detail` when it records history.
///
/// Written as JSON so Isometry can recover a whole [`HistoryEvent`] on the way
/// back, while every other game sees an opaque blob it is obliged to keep.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
struct HistoryDetail {
    id: String,
    text: String,
    #[serde(default)]
    participants: Vec<String>,
    #[serde(default)]
    place: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

/// A creature that has arrived, and the record it arrived with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrival {
    chronicle: Chronicle,
}

impl Arrival {
    /// Reads a chronicle, refusing anything it cannot vouch for.
    ///
    /// Magic and version are checked before the payload is touched, so a
    /// newer writer is diagnosed rather than mis-decoded into a creature that
    /// looks plausible and is wrong.
    pub fn read(bytes: &[u8]) -> Result<Self, ChronicleError> {
        if bytes.len() < HEADER_LEN {
            return Err(ChronicleError::TooShort { got: bytes.len() });
        }
        if bytes[..8] != MAGIC {
            return Err(ChronicleError::WrongSchema);
        }
        let found = u16::from_le_bytes([bytes[8], bytes[9]]);
        if found != VERSION {
            return Err(ChronicleError::UnknownVersion { found, expected: VERSION });
        }

        let chronicle: Chronicle =
            postcard::from_bytes(&bytes[HEADER_LEN..]).map_err(|_| ChronicleError::Malformed)?;
        if chronicle.parts.is_empty() {
            return Err(ChronicleError::Inconsistent);
        }
        Ok(Self { chronicle })
    }

    pub fn chronicle(&self) -> &Chronicle {
        &self.chronicle
    }

    /// The lineage this creature belongs to.
    pub fn species(&self) -> u32 {
        self.chronicle.species
    }

    /// How many of its parts it took off other organisms.
    pub fn incorporated_parts(&self) -> usize {
        self.chronicle.parts.iter().filter(|part| part.is_incorporated()).count()
    }

    /// The roster slot. An ordinary [`WorldCharacter`], indistinguishable from
    /// an authored one except by what the record says.
    ///
    /// Naming is the caller's: a critter becomes a borg by being named, and
    /// this campaign is where that name gets attached.
    pub fn character(&self, id: impl Into<String>, name: impl Into<String>) -> WorldCharacter {
        WorldCharacter {
            id: id.into(),
            name: name.into(),
            tags: Vec::new(),
            faction: None,
            place: None,
        }
    }

    /// Appends a fact in this campaign's vocabulary.
    ///
    /// The only mutation. Nothing here rewrites or removes a foreign deed,
    /// which is what lets a creature cross three games with its history whole.
    pub fn record(&mut self, event: &HistoryEvent) {
        let detail = HistoryDetail {
            id: event.id.clone(),
            text: event.text.clone(),
            participants: event.participants.clone(),
            place: event.place.clone(),
            tags: event.tags.clone(),
        };
        self.chronicle.deeds.push(Deed {
            vessel: VESSEL.to_string(),
            verb: event.kind.clone(),
            at: event.time.max(0) as u64,
            detail: serde_json::to_vec(&detail).unwrap_or_default(),
        });
    }

    /// Records that the creature lost a part, in the **shared** vocabulary.
    ///
    /// Distinct from [`record`](Self::record) on purpose, and the distinction
    /// is the subtle half of the protocol. A game's own verbs are opaque to
    /// everyone else, so their payload can be whatever suits — this campaign
    /// writes JSON. But a verb *two games both act on* is a contract, and a
    /// contract needs an agreed payload: Mesocosm reads a little-endian `u32`
    /// part index here and will refuse anything else rather than guess.
    ///
    /// Writing a [`HistoryEvent`] whose `kind` happens to be `"lost-part"`
    /// through `record` does **not** do this, and should not: that is this
    /// campaign narrating a loss, which Mesocosm correctly declines to act on.
    /// Losing an arm in the fiction and losing a part in another game's anatomy
    /// are different claims, and only this call makes the second one.
    ///
    /// The shared vocabulary is deliberately tiny. Every verb added to it is a
    /// coupling between vessels that has to be kept in step forever.
    pub fn record_loss(&mut self, part: u32, at: u64) {
        self.chronicle.deeds.push(Deed {
            vessel: VESSEL.to_string(),
            verb: LOST_PART.to_string(),
            at,
            detail: part.to_le_bytes().to_vec(),
        });
    }

    /// The history this campaign wrote, recovered from its own deeds.
    ///
    /// Only ours: another game's deeds are carried, not translated. Guessing
    /// at a foreign vocabulary is how fact loss starts.
    pub fn history(&self) -> Vec<HistoryEvent> {
        self.chronicle
            .deeds
            .iter()
            .filter(|deed| deed.vessel == VESSEL)
            .filter_map(|deed| {
                let detail: HistoryDetail = serde_json::from_slice(&deed.detail).ok()?;
                Some(HistoryEvent {
                    id: detail.id,
                    time: deed.at as i64,
                    kind: deed.verb.clone(),
                    text: detail.text,
                    participants: detail.participants,
                    place: detail.place,
                    tags: detail.tags,
                })
            })
            .collect()
    }

    /// Deeds from other games, which this one keeps and does not interpret.
    pub fn foreign(&self) -> impl Iterator<Item = &Deed> {
        self.chronicle.deeds.iter().filter(|deed| deed.vessel != VESSEL)
    }

    /// Writes the chronicle back for the next game to read.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ChronicleError> {
        let payload =
            postcard::to_allocvec(&self.chronicle).map_err(|_| ChronicleError::Malformed)?;
        let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&payload);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(chronicle: &Chronicle) -> Vec<u8> {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&postcard::to_allocvec(chronicle).unwrap());
        bytes
    }

    fn creature() -> Vec<u8> {
        framed(&Chronicle {
            species: 7,
            parts: vec![
                PartOrigin::default(),
                PartOrigin { from_species: Some(42), from_part: Some(1), epoch: 3 },
            ],
            deeds: vec![Deed {
                vessel: "mesocosm".into(),
                verb: "outlived-its-kind".into(),
                at: 9,
                detail: vec![7, 7, 7],
            }],
        })
    }

    fn event(id: &str, kind: &str, time: i64) -> HistoryEvent {
        HistoryEvent {
            id: id.into(),
            time,
            kind: kind.into(),
            text: "held the ford through the winter".into(),
            participants: vec!["the-vale".into()],
            place: Some("the-ford".into()),
            tags: vec!["siege".into()],
        }
    }

    #[test]
    fn a_creature_arrives_into_an_ordinary_roster_slot() {
        let arrival = Arrival::read(&creature()).unwrap();
        let character = arrival.character("mire-01", "Mire");

        assert_eq!(character.id, "mire-01");
        assert_eq!(character.name, "Mire");
        assert_eq!(character.faction, None, "arriving does not join anything");
        assert_eq!(arrival.incorporated_parts(), 1);
    }

    #[test]
    fn this_campaign_appends_history_in_its_own_words() {
        let mut arrival = Arrival::read(&creature()).unwrap();
        arrival.record(&event("h1", "held-the-ford", 40));

        let back = Arrival::read(&arrival.to_bytes().unwrap()).unwrap();
        let history = back.history();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0], event("h1", "held-the-ford", 40), "our own event round-trips whole");
    }

    #[test]
    fn another_games_deeds_are_carried_untouched() {
        // Opaque preservation. Mesocosm's deed means nothing here and must
        // come back byte for byte, payload included.
        let mut arrival = Arrival::read(&creature()).unwrap();
        arrival.record(&event("h1", "held-the-ford", 40));

        let back = Arrival::read(&arrival.to_bytes().unwrap()).unwrap();
        let foreign: Vec<_> = back.foreign().collect();

        assert_eq!(foreign.len(), 1);
        assert_eq!(foreign[0].vessel, "mesocosm");
        assert_eq!(foreign[0].verb, "outlived-its-kind");
        assert_eq!(foreign[0].detail, vec![7, 7, 7]);
        assert_eq!(back.history().len(), 1, "and ours is not confused with theirs");
    }

    #[test]
    fn recording_never_removes_anything() {
        let mut arrival = Arrival::read(&creature()).unwrap();
        let before = arrival.chronicle().deeds.len();
        for n in 0..5 {
            arrival.record(&event(&format!("h{n}"), "wintered", 40 + n));
        }
        assert_eq!(arrival.chronicle().deeds.len(), before + 5);
        assert_eq!(arrival.foreign().count(), 1, "the foreign deed survived all of it");
    }

    #[test]
    fn a_body_profile_is_not_a_chronicle() {
        let mut profile = b"MESOBODY".to_vec();
        profile.extend_from_slice(&0u16.to_le_bytes());
        profile.extend_from_slice(&[0; 8]);
        assert_eq!(Arrival::read(&profile), Err(ChronicleError::WrongSchema));
    }

    #[test]
    fn a_newer_writer_is_diagnosed() {
        let mut bytes = creature();
        bytes[8..10].copy_from_slice(&4u16.to_le_bytes());
        assert_eq!(
            Arrival::read(&bytes),
            Err(ChronicleError::UnknownVersion { found: 4, expected: 0 })
        );
    }

    #[test]
    fn a_partless_record_is_refused() {
        let bytes = framed(&Chronicle { species: 1, parts: vec![], deeds: vec![] });
        assert_eq!(Arrival::read(&bytes), Err(ChronicleError::Inconsistent));
        assert_eq!(Arrival::read(b"MESO"), Err(ChronicleError::TooShort { got: 4 }));
    }
}
