//! Bakes the interchange fixture to a PNG strip, as a visual receipt that a
//! Mesocosm critter arrives in Isometry as a legible sprite.
//!
//! Run: cargo run -p isometry-voxel --example critter_sprite

use isometry_voxel::{BakeParams, BodyProfile, bake_strip};

fn main() {
    let bytes = include_bytes!("../tests/fixtures/critter.body");
    let body = BodyProfile::read(bytes).expect("the fixture reads");

    let params = BakeParams { half_w: 8, cube_h: 8, ..BakeParams::default() };
    let strip = bake_strip(
        &body.voxels_by_part(),
        &body.origin_palette([90, 140, 60], [200, 120, 70]),
        &params,
    );

    let out = std::env::args().nth(1).unwrap_or_else(|| "critter_sprite.png".into());
    std::fs::write(&out, strip.to_png()).expect("png is writable");
    println!(
        "wrote {out} ({}x{}) - species {}, {} parts, {} incorporated",
        strip.w,
        strip.h,
        body.species,
        body.parts.len(),
        body.incorporated_parts()
    );
}
