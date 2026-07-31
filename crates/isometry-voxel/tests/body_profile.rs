//! The interchange receipt: real Mesocosm bytes become an Isometry sprite.
//!
//! The unit tests in `body.rs` build their own wire bytes, which proves the
//! reader is self-consistent and proves nothing about the writer. This file
//! reads `fixtures/critter.body`, produced by
//! `cargo run -p mesocosm-mesh --example emit_profile` in the mesocosm repo and
//! committed here unchanged.
//!
//! That distinction is the whole point of the seam. The games wing couples its
//! vessels **by data, not by types**, so no compiler checks that Mesocosm's
//! writer and this reader still agree — the cost the ruling booked openly. A
//! committed fixture is what buys the check back: if Mesocosm changes the
//! profile's shape without bumping its version, this test fails here rather
//! than a player's sprite quietly coming out wrong.
//!
//! When it does fail, the fix is a decision, not a re-copy. Either the writer
//! bumped the version (then this reader needs to learn the new one) or it
//! changed shape without bumping (then the writer is wrong).

use isometry_voxel::{BakeParams, BodyError, BodyProfile, bake_facing};

/// One critter, grown by incorporation: a founding trunk plus a limb and a
/// plate taken from two other species.
const CRITTER: &[u8] = include_bytes!("fixtures/critter.body");

#[test]
fn real_mesocosm_bytes_are_readable() {
    let body = BodyProfile::read(CRITTER).expect("the fixture is a valid body profile");

    assert_eq!(body.species, 7, "the lineage crossed intact");
    assert_eq!(body.size, [7, 9, 7], "the flattened grid crossed intact");
    assert_eq!(body.origin, [-2, -3, -3], "body space is signed and the origin says so");
    assert_eq!(body.cells.len(), 7 * 9 * 7);
    assert_eq!(body.attribution.len(), body.cells.len());
}

#[test]
fn the_critters_history_crossed_with_it() {
    // Wave 1.4's done-condition, and the part that is easy to lose: a
    // flattened grid records materials, so without the attribution half this
    // body would arrive as anonymous geometry.
    let body = BodyProfile::read(CRITTER).unwrap();

    assert_eq!(body.parts.len(), 3, "trunk, limb, plate");
    assert_eq!(body.incorporated_parts(), 2, "two of the three were eaten");

    let founding: Vec<_> = body.parts.iter().filter(|p| !p.is_incorporated()).collect();
    assert_eq!(founding.len(), 1, "one part was there at founding");

    let taken: Vec<_> = body.parts.iter().filter(|p| p.is_incorporated()).collect();
    assert_eq!(taken[0].from_species, Some(42));
    assert_eq!(taken[0].epoch, 3);
    assert_eq!(taken[1].from_species, Some(11));
    assert_eq!(taken[1].epoch, 7, "the plate was taken later than the limb");
}

#[test]
fn every_solid_cell_names_the_part_that_wrote_it() {
    let body = BodyProfile::read(CRITTER).unwrap();
    for (index, cell) in body.cells.iter().enumerate() {
        assert_eq!(
            *cell != 0,
            body.attribution[index] != 0,
            "cell {index} disagrees about being occupied"
        );
    }
    assert!(body.cells.iter().any(|c| *c != 0), "the fixture is not empty");
}

#[test]
fn a_real_body_bakes_to_a_sprite_from_every_facing() {
    // The done-condition's other half: body document -> Isometry sprite.
    let body = BodyProfile::read(CRITTER).unwrap();
    let voxels = body.voxels_by_part();
    let palette = body.origin_palette([90, 140, 60], [200, 120, 70]);

    for facing in 0..4 {
        let sheet = bake_facing(&voxels, &palette, facing, &BakeParams::default());
        assert!(sheet.w > 0 && sheet.h > 0, "facing {facing} produced a sprite");
        assert!(
            sheet.opaque_pixels() > 0,
            "facing {facing} produced visible pixels rather than a transparent sheet"
        );
    }
}

#[test]
fn the_sprite_shows_which_parts_were_taken() {
    // The legibility rule the wing settled: the world is colour-coded by role,
    // a creature by history. If a grown body baked to one flat colour, a
    // player could not read what it had eaten, and the seam would be carrying
    // provenance nobody could see.
    // Compared against the same body baked with one colour for both origins,
    // because the baker shades each cube face: no palette entry survives to a
    // pixel verbatim, so counting exact RGB matches would test the shader
    // rather than the claim. Distinct-colour count is what actually says
    // "a viewer can tell these apart".
    let body = BodyProfile::read(CRITTER).unwrap();
    let voxels = body.voxels_by_part();
    let own = [90, 140, 60];
    let taken = [200, 120, 70];

    let hues = |palette| {
        let sheet = bake_facing(&voxels, &palette, 0, &BakeParams::default());
        sheet
            .rgba
            .chunks_exact(4)
            .filter(|p| p[3] > 0)
            .map(|p| [p[0], p[1], p[2]])
            .collect::<std::collections::BTreeSet<_>>()
    };

    let uniform = hues(body.origin_palette(own, own));
    let by_origin = hues(body.origin_palette(own, taken));

    assert!(!uniform.is_empty(), "the body is visible at all");
    assert!(
        by_origin.len() > uniform.len(),
        "colouring by origin adds colours a uniform body does not have \
         ({} vs {}), so a viewer can see what this critter ate",
        by_origin.len(),
        uniform.len()
    );
}

#[test]
fn a_truncated_fixture_is_refused_rather_than_half_read() {
    // The refusal path, exercised against real bytes rather than synthetic
    // ones: a partial transfer must not produce a partial critter.
    let cut = &CRITTER[..CRITTER.len() / 2];
    assert!(
        matches!(BodyProfile::read(cut), Err(BodyError::Malformed | BodyError::Inconsistent)),
        "half a profile is refused"
    );
}

#[test]
fn a_version_bump_in_the_fixture_is_refused() {
    let mut bumped = CRITTER.to_vec();
    bumped[8..10].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        BodyProfile::read(&bumped),
        Err(BodyError::UnknownVersion { found: 1, expected: 0 })
    );
}
