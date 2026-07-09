//! The host-private campaign store: where the GM layer lives. Saved
//! with the campaign by the host; never part of `GameSnapshot` or any
//! `GameEvent`. Revealing a secret removes it here and returns the
//! public [`WorldFact`] for the host to commit to the shared log, so a
//! fact is always in exactly one of the two stores.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::fact::{SecretFact, WorldFact};

/// GM-only campaign state. `BTreeMap` so saves and any hashing are
/// deterministic.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CampaignStore {
    secrets: BTreeMap<String, SecretFact>,
}

impl CampaignStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace a secret. Returns the previous fact under that id,
    /// if any, so callers notice collisions.
    pub fn insert_secret(&mut self, fact: SecretFact) -> Option<SecretFact> {
        self.secrets.insert(fact.id.clone(), fact)
    }

    pub fn secret(&self, id: &str) -> Option<&SecretFact> {
        self.secrets.get(id)
    }

    /// All secrets, id-ordered (the DM's hidden-facts pane).
    pub fn secrets(&self) -> impl Iterator<Item = &SecretFact> {
        self.secrets.values()
    }

    pub fn len(&self) -> usize {
        self.secrets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }

    /// Reveal a secret to the table: removes it from the GM layer and
    /// returns its public face for the host to commit as an ordinary
    /// event. `None` if no secret has that id.
    pub fn reveal(&mut self, id: &str) -> Option<WorldFact> {
        self.secrets.remove(id).map(|s| s.to_world_fact())
    }

    /// Discard a secret without revealing it (the DM changed their
    /// mind; a generated fact was rejected after commit).
    pub fn remove(&mut self, id: &str) -> Option<SecretFact> {
        self.secrets.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact::RevealCondition;

    fn cursed_sword_secret() -> SecretFact {
        SecretFact {
            id: "sword-01.curse".to_owned(),
            text: "The river oath binds it: it will not bite river spirits.".to_owned(),
            tags: vec!["item:sword-01".to_owned(), "faction:eel-cult".to_owned()],
            reveal: RevealCondition::SlayTagged("undead".to_owned()),
        }
    }

    #[test]
    fn store_reveal_moves_fact_to_public_face() {
        let mut store = CampaignStore::new();
        store.insert_secret(cursed_sword_secret());
        assert_eq!(store.len(), 1);

        let fact = store.reveal("sword-01.curse").expect("secret exists");
        assert_eq!(fact.kind, "reveal");
        assert_eq!(fact.id, "sword-01.curse");
        assert!(fact.text.contains("river oath"));
        // Exactly one of the two stores holds it: now neither here...
        assert!(store.is_empty());
        // ...and revealing again finds nothing.
        assert_eq!(store.reveal("sword-01.curse"), None);
    }

    #[test]
    fn store_serde_round_trip() {
        let mut store = CampaignStore::new();
        store.insert_secret(cursed_sword_secret());
        let json = serde_json::to_string(&store).unwrap();
        let back: CampaignStore = serde_json::from_str(&json).unwrap();
        assert_eq!(back, store);
    }
}
