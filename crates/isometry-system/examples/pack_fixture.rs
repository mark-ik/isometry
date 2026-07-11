//! Run one fixture declared by an Isometry content pack.
//!
//! Usage:
//! `cargo run -p isometry-system --example pack_fixture -- <pack-dir> <generator-id> <fixture-path>`

use std::path::PathBuf;

use isometry_system::{GeneratorLimits, GeneratorPack};

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let pack_dir = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: pack_fixture <pack-dir> <generator-id> <fixture-path>".to_owned())?;
    let generator = args
        .next()
        .ok_or_else(|| "usage: pack_fixture <pack-dir> <generator-id> <fixture-path>".to_owned())?
        .to_string_lossy()
        .into_owned();
    let fixture = args
        .next()
        .ok_or_else(|| "usage: pack_fixture <pack-dir> <generator-id> <fixture-path>".to_owned())?
        .to_string_lossy()
        .into_owned();
    if args.next().is_some() {
        return Err("usage: pack_fixture <pack-dir> <generator-id> <fixture-path>".to_owned());
    }

    let pack = GeneratorPack::load(pack_dir)?;
    pack.run_fixture(&generator, &fixture, GeneratorLimits::default())?;
    println!("fixture passed: {generator} {fixture}");
    Ok(())
}
