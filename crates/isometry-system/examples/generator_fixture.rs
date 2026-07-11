//! Run one pack generator fixture from disk.
//!
//! Usage:
//! `cargo run -p isometry-system --example generator_fixture -- <script.lua> <fixture.json>`
//!
//! The fixture is a serialized `isometry_campaign::GeneratorFixture`. A
//! passing run proves both the typed proposal and its host entropy trace.

use std::path::PathBuf;

use isometry_campaign::GeneratorFixture;
use isometry_system::{GeneratorLimits, GeneratorRuntime};

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let script_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: generator_fixture <script.lua> <fixture.json>".to_owned())?;
    let fixture_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: generator_fixture <script.lua> <fixture.json>".to_owned())?;
    if args.next().is_some() {
        return Err("usage: generator_fixture <script.lua> <fixture.json>".to_owned());
    }

    let script = std::fs::read_to_string(&script_path)
        .map_err(|error| format!("read {}: {error}", script_path.display()))?;
    let fixture_json = std::fs::read_to_string(&fixture_path)
        .map_err(|error| format!("read {}: {error}", fixture_path.display()))?;
    let fixture: GeneratorFixture = serde_json::from_str(&fixture_json)
        .map_err(|error| format!("parse {}: {error}", fixture_path.display()))?;

    let mut runtime = GeneratorRuntime::load(&script, GeneratorLimits::default())?;
    runtime.run_fixture(&fixture)?;
    println!("fixture passed: {}", fixture.name);
    Ok(())
}
