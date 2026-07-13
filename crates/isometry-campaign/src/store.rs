//! The host-private campaign store: where the GM layer lives. Saved
//! with the campaign by the host; never part of `GameSnapshot` or any
//! `GameEvent`. A reveal moves a secret into a durable pending state before
//! the host commits its public [`WorldFact`]. That makes an interrupted reveal
//! recoverable rather than losing a secret between the two stores.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::fact::{SecretFact, WorldFact};
use crate::item::{HiddenItemModifier, ItemModifierReveal};

/// GM-only campaign state. `BTreeMap` so saves and any hashing are
/// deterministic.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CampaignStore {
    secrets: BTreeMap<String, SecretFact>,
    /// Reveals prepared for a public commit but not yet finalized. This is
    /// serialized so a restored host can reconcile it against the journal.
    #[serde(default)]
    pending_reveals: BTreeMap<String, SecretFact>,
    /// Item modifiers whose effects/names are GM-only until a reveal commits
    /// their public projection to the replicated inventory.
    #[serde(default)]
    hidden_item_modifiers: BTreeMap<String, HiddenItemModifier>,
    #[serde(default)]
    pending_item_modifier_reveals: BTreeMap<String, HiddenItemModifier>,
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

    pub fn pending_reveal(&self, id: &str) -> Option<&SecretFact> {
        self.pending_reveals.get(id)
    }

    /// All secrets, id-ordered (the DM's hidden-facts pane).
    pub fn secrets(&self) -> impl Iterator<Item = &SecretFact> {
        self.secrets.values()
    }

    pub fn secret_ids(&self) -> impl Iterator<Item = &str> {
        self.secrets.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.secrets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
            && self.pending_reveals.is_empty()
            && self.hidden_item_modifiers.is_empty()
            && self.pending_item_modifier_reveals.is_empty()
    }

    /// Move a secret into the recoverable pending state and return its public
    /// face. `None` if the secret is missing or already pending.
    pub fn begin_reveal(&mut self, id: &str) -> Option<WorldFact> {
        let secret = self.secrets.remove(id)?;
        let public = secret.to_world_fact();
        self.pending_reveals.insert(id.to_owned(), secret);
        Some(public)
    }

    /// Finalize a reveal after its public fact has committed to the journal.
    pub fn finish_reveal(&mut self, id: &str) -> Option<SecretFact> {
        self.pending_reveals.remove(id)
    }

    /// Restore a pending secret after its public commit was rejected.
    pub fn abort_reveal(&mut self, id: &str) -> Option<SecretFact> {
        let secret = self.pending_reveals.remove(id)?;
        self.secrets.insert(id.to_owned(), secret.clone());
        Some(secret)
    }

    /// Public faces that still need reconciliation after the host is restored.
    pub fn pending_world_facts(&self) -> impl Iterator<Item = WorldFact> + '_ {
        self.pending_reveals.values().map(SecretFact::to_world_fact)
    }

    pub fn insert_hidden_item_modifier(
        &mut self,
        modifier: HiddenItemModifier,
    ) -> Option<HiddenItemModifier> {
        self.hidden_item_modifiers
            .insert(modifier.id.clone(), modifier)
    }

    pub fn hidden_item_modifier(&self, id: &str) -> Option<&HiddenItemModifier> {
        self.hidden_item_modifiers.get(id)
    }

    pub fn pending_item_modifier_reveal(&self, id: &str) -> Option<&HiddenItemModifier> {
        self.pending_item_modifier_reveals.get(id)
    }

    /// Begin the same two-phase reveal protocol used for facts. The item
    /// modifier remains private until its public inventory event commits.
    pub fn begin_item_modifier_reveal(&mut self, id: &str) -> Option<ItemModifierReveal> {
        let modifier = self.hidden_item_modifiers.remove(id)?;
        let public = modifier.public_face();
        self.pending_item_modifier_reveals
            .insert(id.to_owned(), modifier);
        Some(public)
    }

    pub fn finish_item_modifier_reveal(&mut self, id: &str) -> Option<HiddenItemModifier> {
        self.pending_item_modifier_reveals.remove(id)
    }

    pub fn abort_item_modifier_reveal(&mut self, id: &str) -> Option<HiddenItemModifier> {
        let modifier = self.pending_item_modifier_reveals.remove(id)?;
        self.hidden_item_modifiers
            .insert(id.to_owned(), modifier.clone());
        Some(modifier)
    }

    pub fn pending_item_modifier_reveals(&self) -> impl Iterator<Item = ItemModifierReveal> + '_ {
        self.pending_item_modifier_reveals
            .values()
            .map(HiddenItemModifier::public_face)
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
    fn reveal_stays_pending_until_the_public_commit_finishes() {
        let mut store = CampaignStore::new();
        store.insert_secret(cursed_sword_secret());
        assert_eq!(store.len(), 1);

        let fact = store.begin_reveal("sword-01.curse").expect("secret exists");
        assert_eq!(fact.kind, "reveal");
        assert_eq!(fact.id, "sword-01.curse");
        assert!(fact.text.contains("river oath"));
        assert!(store.secret("sword-01.curse").is_none());
        assert!(store.pending_reveal("sword-01.curse").is_some());
        assert_eq!(store.pending_world_facts().collect::<Vec<_>>(), vec![fact]);

        store.finish_reveal("sword-01.curse");
        assert!(store.is_empty());
    }

    #[test]
    fn aborted_reveal_restores_the_secret() {
        let mut store = CampaignStore::new();
        store.insert_secret(cursed_sword_secret());
        store.begin_reveal("sword-01.curse").expect("secret exists");

        let restored = store
            .abort_reveal("sword-01.curse")
            .expect("pending secret");
        assert_eq!(restored.id, "sword-01.curse");
        assert!(store.secret("sword-01.curse").is_some());
        assert!(store.pending_reveal("sword-01.curse").is_none());
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
