//! Public manifest data for content-pack generators.
//!
//! The manifest is pure, portable data. Filesystem loading belongs to the
//! system host; this crate defines the stable IDs and containment rules that a
//! pack can use equally from a local directory or a future P2P bundle.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::GenValue;

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
    /// The pack's **choreography**: what a beat looks like, and which beats a
    /// player may throw for themselves.
    ///
    /// This is why it belongs to the pack rather than the app. A beat name like
    /// `strike` or `cheer` is vocabulary the rules and the players speak; what it
    /// *looks* like is art direction, and what a table is allowed to express is a
    /// table's business. The app should not be the thing that decides you may
    /// cheer but not spit.
    #[serde(default)]
    pub choreography: Vec<BeatEntry>,
}

/// One named beat: its stylesheet, and whether it can be thrown as an emote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeatEntry {
    /// Beat name in the vocabulary (`strike`, `recoil`, `cheer`). The resolver
    /// names beats by this; the view lowers it to the CSS class `beat-<name>`.
    pub name: String,
    /// Menu label. Present makes the beat **emotable**: a player may throw it on
    /// their own token. Absent means it is a beat the *rules* produce (a strike,
    /// a fall), which no one gets to perform on demand.
    #[serde(default)]
    pub emote: Option<String>,
    /// Stylesheet fragment, relative to the pack root: the `@keyframes` and the
    /// `.beat-<name>` rule that plays them. Folders and stylesheets, as promised.
    pub style: String,
}

/// One Lua generator and its checked fixtures, relative to the pack root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorEntry {
    /// Local id within its pack, for example `forge_item`.
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub default_args: Option<GenValue>,
    #[serde(default)]
    pub lock_presets: Vec<GeneratorLockPreset>,
    pub script: String,
    #[serde(default)]
    pub fixtures: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorLockPreset {
    pub key: String,
    pub label: String,
    pub value: GenValue,
}

/// Host-neutral row used by generator selectors. Paths and Lua runtimes stay
/// in `isometry-system`; views receive only declared authoring data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratorChoice {
    pub id: String,
    pub name: String,
    pub default_args: GenValue,
    pub lock_presets: Vec<GeneratorLockPreset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentPackError {
    UnsupportedFormat(u32),
    MissingPackId,
    InvalidPackId(String),
    MissingPackName,
    MissingGeneratorId,
    DuplicateGenerator(String),
    InvalidLockPreset(String),
    UnsafePath(String),
    /// A beat with no name, or a name that would not survive being lowered to a
    /// CSS class (the view builds `beat-<name>` from it).
    InvalidBeatName(String),
    DuplicateBeat(String),
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
            let mut lock_keys = BTreeSet::new();
            for preset in &generator.lock_presets {
                if preset.key.trim().is_empty()
                    || preset.label.trim().is_empty()
                    || !lock_keys.insert(&preset.key)
                {
                    return Err(ContentPackError::InvalidLockPreset(format!(
                        "{}:{}",
                        generator.id, preset.key
                    )));
                }
            }
        }
        let mut beats = BTreeSet::new();
        for beat in &self.choreography {
            // The name becomes a CSS class (`beat-<name>`), so it must be safe to
            // paste into a stylesheet. A pack cannot smuggle a selector through it.
            if beat.name.trim().is_empty()
                || !beat
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(ContentPackError::InvalidBeatName(beat.name.clone()));
            }
            if !beats.insert(&beat.name) {
                return Err(ContentPackError::DuplicateBeat(beat.name.clone()));
            }
            if !is_pack_path(&beat.style) {
                return Err(ContentPackError::UnsafePath(beat.style.clone()));
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

    pub fn generator_choices(&self) -> Vec<GeneratorChoice> {
        self.generators
            .iter()
            .map(|entry| GeneratorChoice {
                id: self.generator_id(entry),
                name: if entry.name.trim().is_empty() {
                    entry.id.clone()
                } else {
                    entry.name.clone()
                },
                default_args: entry
                    .default_args
                    .clone()
                    .unwrap_or_else(|| GenValue::Text {
                        value: String::new(),
                    }),
                lock_presets: entry.lock_presets.clone(),
            })
            .collect()
    }
}

impl std::fmt::Display for ContentPackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedFormat(format) => {
                write!(f, "unsupported content-pack format: {format}")
            }
            Self::MissingPackId => write!(f, "content-pack id is required"),
            Self::InvalidPackId(id) => write!(f, "content-pack id cannot contain ':': {id}"),
            Self::MissingPackName => write!(f, "content-pack name is required"),
            Self::MissingGeneratorId => write!(
                f,
                "content-pack generator id is required and cannot contain ':'"
            ),
            Self::DuplicateGenerator(id) => write!(f, "duplicate content-pack generator: {id}"),
            Self::InvalidLockPreset(id) => {
                write!(f, "invalid or duplicate generator lock preset: {id}")
            }
            Self::InvalidBeatName(name) => write!(
                f,
                "beat name must be alphanumeric, '-' or '_' (it becomes a CSS class): {name:?}"
            ),
            Self::DuplicateBeat(name) => {
                write!(f, "duplicate beat within one content pack: {name}")
            }
            Self::UnsafePath(path) => write!(
                f,
                "content-pack asset path must stay below the pack root: {path}"
            ),
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

    fn beat(name: &str, style: &str) -> BeatEntry {
        BeatEntry {
            name: name.to_owned(),
            emote: None,
            style: style.to_owned(),
        }
    }

    #[test]
    fn a_beat_name_cannot_smuggle_a_selector() {
        // The name is pasted into a stylesheet as `.beat-<name>`, so a pack that
        // could put punctuation in it could escape its own rule and restyle the
        // whole app. Names are alphanumerics, '-' and '_', and nothing else.
        let mut m = manifest();
        m.choreography = vec![beat("cheer } .app { display: none; ", "beats/x.css")];
        assert!(matches!(
            m.validate(),
            Err(ContentPackError::InvalidBeatName(_))
        ));

        // And a stylesheet cannot reach outside the pack.
        m.choreography = vec![beat("cheer", "../../../etc/passwd")];
        assert!(matches!(m.validate(), Err(ContentPackError::UnsafePath(_))));

        // A plain one is fine.
        m.choreography = vec![beat("cheer", "beats/cheer.css"), beat("fall", "beats/f.css")];
        assert!(m.validate().is_ok());

        // Declared twice in one pack is an authoring mistake, not an override.
        m.choreography = vec![beat("cheer", "beats/a.css"), beat("cheer", "beats/b.css")];
        assert!(matches!(
            m.validate(),
            Err(ContentPackError::DuplicateBeat(_))
        ));
    }

    fn manifest() -> ContentPackManifest {
        ContentPackManifest {
            format: CONTENT_PACK_FORMAT,
            id: "demo".to_owned(),
            name: "Demo Pack".to_owned(),
            version: "0.1.0".to_owned(),
            choreography: Vec::new(),
            generators: vec![GeneratorEntry {
                id: "forge_item".to_owned(),
                name: "Forge item".to_owned(),
                default_args: Some(GenValue::Text {
                    value: "river".to_owned(),
                }),
                lock_presets: Vec::new(),
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
