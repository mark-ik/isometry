//! The consuming game's half of Law C, against real Mesocosm bytes.
//!
//! > Same seed format whether authored by play or by RNG, at every import slot
//! > in every game. Player history *displaces* procedural content; it never
//! > gates it.
//!
//! Mesocosm's own suite proves the two records are structurally identical when
//! it writes them. That is the easier half — a writer can hardly be surprised
//! by its own output. The claim that matters is about the *consumer*, and it
//! can only be tested here: given two files and no other information, this
//! campaign must have no way to sort them.
//!
//! Both fixtures come from `cargo run -p mesocosm-core --example
//! emit_chronicles` and are committed unchanged. `played.chronicle` is a
//! critter driven through the world until it had eaten several dozen
//! organisms; `rng.chronicle` is one nobody ever ran.

use isometry_campaign::{Arrival, ChronicleError, HistoryEvent};

const PLAYED: &[u8] = include_bytes!("fixtures/played.chronicle");
const RNG: &[u8] = include_bytes!("fixtures/rng.chronicle");

/// Everything this campaign can learn about an arriving creature, which is
/// exactly the surface a consumer could try to sort on.
fn observable(bytes: &[u8]) -> (u32, usize, usize, usize) {
    let arrival = Arrival::read(bytes).expect("a valid chronicle");
    (
        arrival.species(),
        arrival.chronicle().parts.len(),
        arrival.incorporated_parts(),
        arrival.chronicle().deeds.len(),
    )
}

#[test]
fn both_arrive_through_the_same_door() {
    // No branch, no second code path, no fallback. If importing a played
    // creature ever needed its own handling, this is where that would appear.
    for bytes in [PLAYED, RNG] {
        let arrival = Arrival::read(bytes).expect("both read identically");
        assert!(arrival.chronicle().parts.len() > 1);
        assert!(arrival.incorporated_parts() > 0, "both have eaten");
        assert_eq!(arrival.foreign().count(), 0, "neither has been anywhere yet");
    }
}

#[test]
fn nothing_this_campaign_can_observe_sorts_them() {
    // The heart of it. Every question the consumer can ask returns the same
    // *kind* of answer for both, and no answer means "this one was played".
    let (played_species, played_parts, played_eaten, played_deeds) = observable(PLAYED);
    let (rng_species, rng_parts, rng_eaten, rng_deeds) = observable(RNG);

    assert!(played_species > 0 && rng_species > 0, "both belong to a lineage");
    assert!(played_parts > 1 && rng_parts > 1, "both are composite");
    assert!(played_eaten > 0 && rng_eaten > 0, "both have provenance");
    assert_eq!(played_deeds, rng_deeds, "neither carries history yet");
}

#[test]
fn size_does_not_give_the_played_one_away() {
    // Nobody needs an is_player_made flag to break Law C: it is enough that
    // generated creatures are always small. The two fixtures are deliberately
    // comparable, so a part count carries no signal.
    let (_, played_parts, _, _) = observable(PLAYED);
    let (_, rng_parts, _, _) = observable(RNG);

    let ratio = played_parts.max(rng_parts) as f64 / played_parts.min(rng_parts) as f64;
    assert!(
        ratio < 2.0,
        "part counts are the same order ({played_parts} vs {rng_parts}); \
         a large gap would let a consumer guess origin without a marker"
    );
}

#[test]
fn both_take_a_roster_slot_of_the_same_shape() {
    // The import slot Law C talks about. An authored NPC, a played critter,
    // and a generated one are all a WorldCharacter with the same fields.
    let played = Arrival::read(PLAYED).unwrap().character("a", "Mire");
    let generated = Arrival::read(RNG).unwrap().character("b", "Thal");

    assert_eq!(played.faction, generated.faction, "neither arrives affiliated");
    assert_eq!(played.tags, generated.tags);
    assert_eq!(played.place, generated.place);
    assert_ne!(played.id, generated.id, "they are still two different characters");
}

#[test]
fn history_lands_on_both_identically() {
    // Displacement rather than gating: the generated creature is not a
    // second-class citizen of the campaign.
    for bytes in [PLAYED, RNG] {
        let mut arrival = Arrival::read(bytes).unwrap();
        let before = arrival.chronicle().parts.len();

        arrival.record(&HistoryEvent {
            id: "h1".into(),
            time: 40,
            kind: "held-the-ford".into(),
            text: "held the ford through the winter".into(),
            participants: vec!["the-vale".into()],
            place: Some("the-ford".into()),
            tags: vec!["siege".into()],
        });

        let back = Arrival::read(&arrival.to_bytes().unwrap()).unwrap();
        assert_eq!(back.history().len(), 1);
        assert_eq!(back.chronicle().parts.len(), before, "history changed no anatomy");
    }
}

#[test]
fn a_truncated_arrival_is_refused_rather_than_half_read() {
    let cut = &PLAYED[..PLAYED.len() / 2];
    assert!(
        matches!(
            Arrival::read(cut),
            Err(ChronicleError::Malformed | ChronicleError::Inconsistent)
        ),
        "half a record is not a creature"
    );
}

#[test]
fn a_version_bump_in_a_real_fixture_is_refused() {
    // Drift insurance. If Mesocosm changes the chronicle's shape without
    // bumping, the round-trip tests above fail; if it bumps, this one explains
    // why in one line.
    let mut bumped = PLAYED.to_vec();
    bumped[8..10].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        Arrival::read(&bumped),
        Err(ChronicleError::UnknownVersion { found: 1, expected: 0 })
    );
}
