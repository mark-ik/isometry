//! Bakes the interchange fixture to a PNG strip, as a visual receipt that a
//! Mesocosm critter arrives in Isometry as a legible sprite.
//!
//! Run the committed fixture:
//! `cargo run -p isometry-voxel --example critter_sprite -- [output.png]`
//!
//! Bake an arbitrary compatible crossing:
//! `cargo run -p isometry-voxel --example critter_sprite -- --body critter.body --out output.png`

use isometry_voxel::{BODY_SCHEMA, BakeParams, BodyProfile, bake_strip};
use serde::Serialize;

#[derive(Serialize)]
struct BakeReceipt {
    gate: &'static str,
    schema: &'static str,
    profile_digest: String,
    species: u32,
    parts: usize,
    incorporated: usize,
    sheet_size: [i32; 2],
    opaque_pixels: usize,
}

fn main() {
    let mut body_path = None;
    let mut out = None;
    let mut receipt = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--body" => body_path = Some(args.next().expect("--body needs a path")),
            "--out" => out = Some(args.next().expect("--out needs a path")),
            "--receipt" => receipt = Some(args.next().expect("--receipt needs a path")),
            _ if out.is_none() => out = Some(arg),
            _ => panic!("unexpected argument {arg}"),
        }
    }

    let supplied;
    let bytes = match body_path {
        Some(path) => {
            supplied = std::fs::read(path).expect("body profile is readable");
            supplied.as_slice()
        }
        None => include_bytes!("../tests/fixtures/critter.body"),
    };
    let body = BodyProfile::read(bytes).expect("the body profile reads");

    let params = BakeParams {
        half_w: 8,
        cube_h: 8,
        ..BakeParams::default()
    };
    let strip = bake_strip(
        &body.voxels_by_part(),
        &body.origin_palette([90, 140, 60], [200, 120, 70]),
        &params,
    );

    let out = out.unwrap_or_else(|| "critter_sprite.png".into());
    std::fs::write(&out, strip.to_png()).expect("png is writable");
    if let Some(path) = receipt {
        let receipt = BakeReceipt {
            gate: "V2",
            schema: BODY_SCHEMA,
            profile_digest: format!("fnv1a64:{:016x}", fnv1a64(bytes)),
            species: body.species,
            parts: body.parts.len(),
            incorporated: body.incorporated_parts(),
            sheet_size: [strip.w, strip.h],
            opaque_pixels: strip.opaque_pixels(),
        };
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&receipt).expect("receipt encodes"),
        )
        .expect("receipt is writable");
    }
    println!(
        "wrote {out} ({}x{}) - species {}, {} parts, {} incorporated",
        strip.w,
        strip.h,
        body.species,
        body.parts.len(),
        body.incorporated_parts()
    );
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
