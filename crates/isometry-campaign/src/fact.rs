//! Facts and their visibility: the shared vocabulary of the two-store
//! split. A [`SecretFact`] lives host-side until revealed; a
//! [`WorldFact`] is the public envelope that crosses the wire and
//! accumulates in the table-visible campaign journal.

use serde::{Deserialize, Serialize};

/// Which audience can see a piece of campaign state. v1 is two-layer:
/// per-player reveal waits for a whisper-style channel outside consensus
/// state (worldbuilding plan, decision 8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    /// Everyone at the table.
    Public,
    /// The DM only. Never serialized into shared state.
    Gm,
}

/// When a hidden fact may come to light. The substrate stores and
/// displays a condition; it never evaluates one. A system plugin or the
/// DM decides when a condition is met, and the DM can always reveal
/// manually regardless of the authored condition (same posture as world
/// laws: data here, interpretation above).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevealCondition {
    /// No authored trigger; the DM reveals when it feels right.
    Manual,
    /// An identify-style examination (spell, lore check, appraisal).
    Identify,
    /// Attunement or extended use.
    Attune,
    /// Brought to or used in a place carrying this tag.
    UseInPlace(String),
    /// A creature carrying this tag is slain with/near it.
    SlayTagged(String),
    /// A disposition/trust score reaches this threshold.
    TrustThreshold(i64),
    /// Its true name is spoken.
    SpeakName,
}

/// A hidden truth, host-side only. Ids are pack- or generator-assigned
/// strings, unique within a campaign.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretFact {
    pub id: String,
    /// The GM-facing text; becomes the public text on reveal.
    pub text: String,
    /// Motif/requirement tags (`faction:eel-cult`, `law:iron`).
    pub tags: Vec<String>,
    pub reveal: RevealCondition,
}

/// A public campaign fact: the envelope committed to the shared log and
/// accumulated in the journal. Reveals, generated-object public faces,
/// narration, and faction-turn results all use this one shape,
/// distinguished by `kind` (`reveal`, `narration`, `history`, ...).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldFact {
    /// Id of the object or secret this fact concerns (empty if free).
    pub id: String,
    /// What kind of entry this is; a view/plugin vocabulary, not ours.
    pub kind: String,
    pub text: String,
    pub tags: Vec<String>,
}

impl SecretFact {
    /// The public face of this secret once revealed.
    pub fn to_world_fact(&self) -> WorldFact {
        WorldFact {
            id: self.id.clone(),
            kind: "reveal".to_owned(),
            text: self.text.clone(),
            tags: self.tags.clone(),
        }
    }
}
