//! Writes the return fixture: a Mesocosm critter that came here, joined a
//! faction, did some things, and is going home.
//!
//! This is the inbound half of the loop closing. Mesocosm's
//! `tests/homecoming.rs` reads what this writes, keeps the history it cannot
//! interpret, acts on the one verb it can, and founds the next generation.
//!
//! Regenerate with:
//!
//! ```text
//! cargo run -p isometry-campaign --example emit_return
//! ```
//!
//! then copy `fixtures/returned.chronicle` into mesocosm's
//! `crates/mesocosm-core/fixtures/`.

use std::{fs, path::PathBuf};

use isometry_campaign::{Arrival, HistoryEvent};

fn main() {
    let bytes = include_bytes!("../tests/fixtures/played.chronicle");
    let mut arrival = Arrival::read(bytes).expect("the fixture is a valid chronicle");

    // A name makes a critter a borg; a faction makes a borg a character. Both
    // are appended facts, which is why neither needed a new artifact kind.
    let mut character = arrival.character("mire-01", "Mire of the Ford");
    character.faction = Some("the-vale".into());
    character.place = Some("the-ford".into());

    for event in [
        history("h1", "joined-a-faction", 40, "took the Vale's colours"),
        history("h2", "held-the-ford", 52, "held the ford through the winter"),
        history("h4", "was-sung-about", 77, "the Vale still sings about it"),
    ] {
        arrival.record(&event);
    }

    // And one fact in the SHARED vocabulary. Narrating a loss and claiming one
    // in another game's anatomy are different acts; only this call makes the
    // second, and only this call Mesocosm will act on.
    arrival.record_loss(1, 61);

    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    fs::create_dir_all(&out).expect("the fixture directory is writable");
    let file = out.join("returned.chronicle");
    let written = arrival.to_bytes().expect("a chronicle is always encodable");
    fs::write(&file, &written).expect("the fixture is writable");

    println!(
        "wrote {} ({} bytes) - {} as {}, faction {:?}, {} deeds",
        file.display(),
        written.len(),
        character.id,
        character.name,
        character.faction,
        arrival.chronicle().deeds.len(),
    );
}

fn history(id: &str, kind: &str, time: i64, text: &str) -> HistoryEvent {
    HistoryEvent {
        id: id.into(),
        time,
        kind: kind.into(),
        text: text.into(),
        participants: vec!["the-vale".into()],
        place: Some("the-ford".into()),
        tags: vec!["war".into()],
    }
}
