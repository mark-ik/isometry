//! Typed generator proposals and deterministic host entropy.
//!
//! A generator returns a proposal, never a session mutation. The host validates
//! and chooses which proposal to commit as ordinary public events. This keeps
//! pack execution replayable and prevents a content script from writing into a
//! campaign behind the table's back.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CampaignDraft, LocalMapProposal, StoryletProposal, WorldFact};

/// One typed value crossing the pack-generator ABI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GenValue {
    Text { value: String },
    Object { fields: BTreeMap<String, GenValue> },
    List { values: Vec<GenValue> },
    Item { item: ItemProposal },
    Npc { npc: NpcProposal },
    MapPatch { patch: MapPatchProposal },
    WorldFact { fact: WorldFact },
    Storylet { storylet: StoryletProposal },
    LocalMap { map: LocalMapProposal },
    Campaign { campaign: CampaignDraft },
}

impl GenValue {
    /// Reject nesting that would make a generated proposal expensive or
    /// surprising to inspect. The runtime applies this after decoding Lua's
    /// JSON result; pack tools should apply it before presenting previews too.
    pub fn validate_depth(&self, max_depth: usize) -> Result<(), GenValueError> {
        self.validate_at_depth(0, max_depth)
    }

    fn validate_at_depth(&self, depth: usize, max_depth: usize) -> Result<(), GenValueError> {
        if depth > max_depth {
            return Err(GenValueError::TooDeep { max_depth });
        }
        match self {
            Self::Object { fields } => fields
                .values()
                .try_for_each(|value| value.validate_at_depth(depth + 1, max_depth)),
            Self::List { values } => values
                .iter()
                .try_for_each(|value| value.validate_at_depth(depth + 1, max_depth)),
            Self::Text { .. }
            | Self::Item { .. }
            | Self::Npc { .. }
            | Self::MapPatch { .. }
            | Self::WorldFact { .. }
            | Self::Storylet { .. }
            | Self::LocalMap { .. }
            | Self::Campaign { .. } => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenValueError {
    TooDeep { max_depth: usize },
}

impl std::fmt::Display for GenValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooDeep { max_depth } => {
                write!(f, "generated value exceeds maximum depth of {max_depth}")
            }
        }
    }
}

impl std::error::Error for GenValueError {}

/// A generated item before the host assigns its campaign instance id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemProposal {
    pub template: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A generated NPC identity and pack-owned descriptive tags. Its eventual
/// token, sheet, inventory, and dialogue state are separate host decisions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpcProposal {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// An abstract map patch. `operations` deliberately stays pack vocabulary in
/// W2; the voxel/map substrate will validate and lower it before a commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapPatchProposal {
    pub target: String,
    #[serde(default)]
    pub operations: Vec<GenValue>,
}

/// The stable request passed to a pack's `call_gen(request_json, entropy,
/// request)` Lua function. `request` is the host-marshaled table form of this
/// value. Locked values are visible constraints, not hidden GM state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorRequest {
    pub generator: String,
    pub args: GenValue,
    #[serde(default)]
    pub locks: BTreeMap<String, GenValue>,
}

/// A pack-author fixture. The host runs the request from `seed` and compares
/// both the typed proposal and the exact entropy draws, catching accidental
/// changes to generator structure as well as output data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorFixture {
    pub name: String,
    pub seed: u64,
    pub request: GeneratorRequest,
    pub expected: GenValue,
    #[serde(default)]
    pub expected_draws: Vec<u64>,
}

/// A host-accepted generation result in commit-result mode. The record keeps
/// the visible request, one host entropy draw, and the decoded proposal so a
/// peer or restored campaign never needs to rerun pack code to know what was
/// accepted. Lowering a proposal into an inventory, map, or story state is a
/// separate, type-specific host operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationRecord {
    /// Campaign-unique host-assigned id. It gives commit-result replay an
    /// idempotency key independent of the pack's own proposal vocabulary.
    pub id: String,
    pub request: GeneratorRequest,
    /// The exact host entropy draw recorded by the runtime for this result.
    pub entropy: u64,
    pub proposal: GenValue,
}

/// Validation failures for a public generation record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenerationRecordError {
    MissingId,
    MissingGenerator,
    InvalidValue(GenValueError),
}

impl GenerationRecord {
    /// Check the public envelope at the replication boundary. The runtime has
    /// already imposed its host-selected output cap; this repeats the depth
    /// check so a malformed local event cannot create an uninspectable record.
    pub fn validate(&self, max_depth: usize) -> Result<(), GenerationRecordError> {
        if self.id.trim().is_empty() {
            return Err(GenerationRecordError::MissingId);
        }
        if self.request.generator.trim().is_empty() {
            return Err(GenerationRecordError::MissingGenerator);
        }
        self.request
            .args
            .validate_depth(max_depth)
            .map_err(GenerationRecordError::InvalidValue)?;
        self.request
            .locks
            .values()
            .try_for_each(|value| value.validate_depth(max_depth))
            .map_err(GenerationRecordError::InvalidValue)?;
        self.proposal
            .validate_depth(max_depth)
            .map_err(GenerationRecordError::InvalidValue)
    }
}

impl std::fmt::Display for GenerationRecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingId => write!(f, "generation record id is required"),
            Self::MissingGenerator => write!(f, "generation record generator is required"),
            Self::InvalidValue(error) => write!(f, "generation record has invalid value: {error}"),
        }
    }
}

impl std::error::Error for GenerationRecordError {}

/// A deliberate cast request, separate from generic NPC generation. A pack
/// can use the role and locks to propose a character without receiving an
/// authority to create or place one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastRoleRequest {
    pub role: String,
    #[serde(default)]
    pub constraints: BTreeMap<String, GenValue>,
    #[serde(default)]
    pub locks: BTreeMap<String, GenValue>,
}

/// Deterministic host-owned entropy for one generation sequence.
///
/// The draw log is persisted with the generator record by the host. Scripts
/// receive a single draw per call and cannot source entropy themselves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntropyTape {
    pub seed: u64,
    state: u64,
    #[serde(default)]
    pub draws: Vec<u64>,
}

impl EntropyTape {
    pub fn from_seed(seed: u64) -> Self {
        Self {
            seed,
            state: seed,
            draws: Vec::new(),
        }
    }

    /// Record and return the next SplitMix64 draw. This is specified locally
    /// rather than using a general RNG so generation replay remains stable.
    pub fn draw(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        self.draws.push(value);
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entropy_tape_is_repeatable_and_records_draws() {
        let mut first = EntropyTape::from_seed(42);
        let mut second = EntropyTape::from_seed(42);
        assert_eq!(first.draw(), second.draw());
        assert_eq!(first.draw(), second.draw());
        assert_eq!(first.draws, second.draws);
    }

    #[test]
    fn locks_round_trip_as_visible_generator_inputs() {
        let request = GeneratorRequest {
            generator: "castle".to_owned(),
            args: GenValue::Text {
                value: "coast".to_owned(),
            },
            locks: BTreeMap::from([(
                "culture".to_owned(),
                GenValue::Text {
                    value: "river-clans".to_owned(),
                },
            )]),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("river-clans"));
        assert_eq!(
            serde_json::from_str::<GeneratorRequest>(&json).unwrap(),
            request
        );
    }

    #[test]
    fn depth_validation_rejects_nested_collections_over_limit() {
        let value = GenValue::List {
            values: vec![GenValue::List {
                values: vec![GenValue::Text {
                    value: "too deep".to_owned(),
                }],
            }],
        };
        assert!(matches!(
            value.validate_depth(1),
            Err(GenValueError::TooDeep { max_depth: 1 })
        ));
    }

    #[test]
    fn generation_record_requires_a_public_identity_and_bounded_values() {
        let record = GenerationRecord {
            id: "generated.river-blade.1".to_owned(),
            request: GeneratorRequest {
                generator: "demo:forge-item".to_owned(),
                args: GenValue::Text {
                    value: "river".to_owned(),
                },
                locks: BTreeMap::new(),
            },
            entropy: 7,
            proposal: GenValue::Item {
                item: ItemProposal {
                    template: "demo:river-blade".to_owned(),
                    name: "River Blade".to_owned(),
                    tags: Vec::new(),
                },
            },
        };
        assert!(record.validate(4).is_ok());

        let mut missing_id = record.clone();
        missing_id.id.clear();
        assert_eq!(
            missing_id.validate(4),
            Err(GenerationRecordError::MissingId)
        );
    }
}
