//! Public manifest data for content-pack generators.
//!
//! The manifest is pure, portable data. Filesystem loading belongs to the
//! system host; this crate defines the stable IDs and containment rules that a
//! pack can use equally from a local directory or a future P2P bundle.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The current JSON format for an Isometry content-pack manifest.
pub const CONTENT_PACK_FORMAT: u32 = 1;

/// A content pack's declared generators. Other pack content will join this
/// envelope without changing generator identity or record semantics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentPackManifest {
    pub format: u32,
    /// Stable namespace, for example `river-clans`.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub generators: Vec<GeneratorEntry>,
}

/// One Lua generator and its checked fixtures, relative to the pack root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorEntry {
    /// Local id within its pack, for example `forge_item`.
    pub id: String,
    pub script: String,
    #[serde(default)]
    pub fixtures: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentPackError {
    UnsupportedFormat(u32),
    MissingPackId,
    InvalidPackId(String),
    MissingPackName,
    MissingGeneratorId,
    DuplicateGenerator(String),
    UnsafePath(String),
}

impl ContentPackManifest {
    /// Validate identity and asset references before a host opens any files.
    pub fn validate(&self) -> Result<(), ContentPackError> {
        if self.format != CONTENT_PACK_FORMAT {
            return Err(ContentPackError::UnsupportedFormat(self.format));
        }
        if self.id.trim().is_empty() {
            return Err(ContentPackError::MissingPackId);
        }
        if self.id.contains(':') {
            return Err(ContentPackError::InvalidPackId(self.id.clone()));
        }
        if self.name.trim().is_empty() {
            return Err(ContentPackError::MissingPackName);
        }
        let mut generator_ids = BTreeSet::new();
        for generator in &self.generators {
            if generator.id.trim().is_empty() || generator.id.contains(':') {
                return Err(ContentPackError::MissingGeneratorId);
            }
            if !generator_ids.insert(&generator.id) {
                return Err(ContentPackError::DuplicateGenerator(generator.id.clone()));
            }
            if !is_pack_path(&generator.script) {
                return Err(ContentPackError::UnsafePath(generator.script.clone()));
            }
            for fixture in &generator.fixtures {
                if !is_pack_path(fixture) {
                    return Err(ContentPackError::UnsafePath(fixture.clone()));
                }
            }
        }
        Ok(())
    }

    /// The stable fully-qualified id stored in generator requests and records.
    pub fn generator_id(&self, entry: &GeneratorEntry) -> String {
        format!("{}:{}", self.id, entry.id)
    }

    /// Find a declared generator by its fully-qualified id.
    pub fn generator(&self, id: &str) -> Option<&GeneratorEntry> {
        self.generators
            .iter()
            .find(|entry| self.generator_id(entry) == id)
    }
}

impl std::fmt::Display for ContentPackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedFormat(format) => write!(f, "unsupported content-pack format: {format}"),
            Self::MissingPackId => write!(f, "content-pack id is required"),
            Self::InvalidPackId(id) => write!(f, "content-pack id cannot contain ':': {id}"),
            Self::MissingPackName => write!(f, "content-pack name is required"),
            Self::MissingGeneratorId => write!(f, "content-pack generator id is required and cannot contain ':'"),
            Self::DuplicateGenerator(id) => write!(f, "duplicate content-pack generator: {id}"),
            Self::UnsafePath(path) => write!(f, "content-pack asset path must stay below the pack root: {path}"),
        }
    }
}

impl std::error::Error for ContentPackError {}

/// Pack paths are always slash-separated relative paths. This avoids a host
/// accepting different traversal spellings on Windows, Unix, or P2P bundles.
fn is_pack_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> ContentPackManifest {
        ContentPackManifest {
            format: CONTENT_PACK_FORMAT,
            id: "demo".to_owned(),
            name: "Demo Pack".to_owned(),
            version: "0.1.0".to_owned(),
            generators: vec![GeneratorEntry {
                id: "forge_item".to_owned(),
                script: "generators/forge_item.lua".to_owned(),
                fixtures: vec!["fixtures/forge_item.json".to_owned()],
            }],
        }
    }

    #[test]
    fn manifest_qualifies_generator_ids() {
        let manifest = manifest();
        manifest.validate().unwrap();
        assert_eq!(
            manifest.generator_id(&manifest.generators[0]),
            "demo:forge_item"
        );
        assert!(manifest.generator("demo:forge_item").is_some());
    }

    #[test]
    fn manifest_rejects_parent_paths() {
        let mut manifest = manifest();
        manifest.generators[0].script = "../outside.lua".to_owned();
        assert_eq!(
            manifest.validate(),
            Err(ContentPackError::UnsafePath("../outside.lua".to_owned()))
        );
    }

    #[test]
    fn manifest_reserves_colons_for_fully_qualified_generator_ids() {
        let mut manifest = manifest();
        manifest.id = "demo:bad".to_owned();
        assert_eq!(
            manifest.validate(),
            Err(ContentPackError::InvalidPackId("demo:bad".to_owned()))
        );
    }
}
