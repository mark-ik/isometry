//! Typed campaign item state. Templates belong to a rules/content pack;
//! instances, ownership, equipment, and generated modifiers belong to a
//! campaign. The public inventory is replicated, while [`HiddenItemModifier`]
//! stays in the GM store until a reveal commits it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::RevealCondition;

/// Globally stable campaign identity for one item instance. It is a string so
/// pack generators can mint readable deterministic ids (`reward-03.sword`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ItemId(pub String);

impl ItemId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// A small common equipment vocabulary. A system may ignore any slot, but
/// packs do not need to invent incompatible strings for ordinary gear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EquipmentSlot {
    MainHand,
    OffHand,
    Head,
    Body,
    Feet,
    Accessory,
}

/// Classifies a generated modifier for naming, authoring tools, and later
/// rules plugins. The substrate stores values but never interprets them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemModifierKind {
    Material,
    Quality,
    Enchantment,
    Curse,
    Origin,
    Quirk,
}

/// One public modifier already known to the table. `stats` is a system-owned
/// key/value vocabulary (`attack_bonus`, `ac`, custom pack keys); rules
/// plugins decide how it changes sheets and actions. `appearance_layer` is a
/// rig/tileset layer key for the voxel appearance pipeline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemModifier {
    pub id: String,
    pub kind: ItemModifierKind,
    pub name: String,
    #[serde(default)]
    pub stats: BTreeMap<String, i64>,
    #[serde(default)]
    pub appearance_layer: Option<String>,
}

/// A public item instance. `template` names pack content (for example
/// `srd5e:longsword`); the instance captures the rolled/generated result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemInstance {
    pub id: ItemId,
    pub template: String,
    pub name: String,
    #[serde(default = "one")]
    pub quantity: u32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub modifiers: Vec<ItemModifier>,
    /// Public base layers contributed when this item is equipped.
    #[serde(default)]
    pub appearance_layers: Vec<String>,
}

fn one() -> u32 {
    1
}

impl ItemInstance {
    /// Add a revealed modifier. Identical replay is idempotent; a different
    /// modifier under the same id is a deterministic conflict.
    pub fn attach_modifier(&mut self, modifier: ItemModifier) -> Result<(), InventoryError> {
        if let Some(existing) = self.modifiers.iter().find(|m| m.id == modifier.id) {
            return if existing == &modifier {
                Ok(())
            } else {
                Err(InventoryError::ConflictingModifier(modifier.id))
            };
        }
        self.modifiers.push(modifier);
        Ok(())
    }

    /// The overmap places this item discloses when read as a map. A carried
    /// chart tags the nodes it depicts as `reveals:<place>`; reading it (behind
    /// a literacy check the system judges) hands those places to the party. This
    /// is how a bought or looted map delivers somewhere far off the party has
    /// never travelled near. Ordinary gear reveals nothing.
    pub fn revealed_places(&self) -> impl Iterator<Item = &str> {
        self.tags
            .iter()
            .filter_map(|tag| tag.strip_prefix("reveals:"))
            .filter(|place| !place.is_empty())
    }

    /// Appearance layers selected by equipment, in authored order. The view
    /// later resolves these keys through an `isometry-voxel::Appearance` rig.
    pub fn appearance_layers(&self) -> impl Iterator<Item = &str> {
        self.appearance_layers.iter().map(String::as_str).chain(
            self.modifiers
                .iter()
                .filter_map(|modifier| modifier.appearance_layer.as_deref()),
        )
    }
}

/// A character's public carried and equipped items. `BTreeMap` gives stable
/// saves and deterministic wire encoding.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    pub items: BTreeMap<ItemId, ItemInstance>,
    #[serde(default)]
    pub equipped: BTreeMap<EquipmentSlot, ItemId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryError {
    DuplicateItem(ItemId),
    UnknownItem(ItemId),
    ConflictingModifier(String),
}

impl Inventory {
    pub fn insert(&mut self, item: ItemInstance) -> Result<(), InventoryError> {
        if self.items.contains_key(&item.id) {
            return Err(InventoryError::DuplicateItem(item.id));
        }
        self.items.insert(item.id.clone(), item);
        Ok(())
    }

    pub fn equip(&mut self, slot: EquipmentSlot, item: ItemId) -> Result<(), InventoryError> {
        if !self.items.contains_key(&item) {
            return Err(InventoryError::UnknownItem(item));
        }
        self.equipped.insert(slot, item);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), InventoryError> {
        for item in self.equipped.values() {
            if !self.items.contains_key(item) {
                return Err(InventoryError::UnknownItem(item.clone()));
            }
        }
        Ok(())
    }

    pub fn item_mut(&mut self, id: &ItemId) -> Option<&mut ItemInstance> {
        self.items.get_mut(id)
    }

    /// Every overmap place the carried maps in this pack disclose, in item-id
    /// order. Duplicates across charts are left in; the caller dedupes against
    /// what the party already knows.
    pub fn revealed_places(&self) -> impl Iterator<Item = &str> {
        self.items.values().flat_map(ItemInstance::revealed_places)
    }

    /// Remove a whole instance for transfer. Any equipped references to it are
    /// cleared in the same operation, so it cannot remain worn by two tokens.
    pub fn take(&mut self, id: &ItemId) -> Result<ItemInstance, InventoryError> {
        let item = self
            .items
            .remove(id)
            .ok_or_else(|| InventoryError::UnknownItem(id.clone()))?;
        self.equipped.retain(|_, equipped| equipped != id);
        Ok(item)
    }
}

/// A GM-only modifier generated with an item. It is deliberately a distinct
/// record from `ItemModifier`: serializing it in the private store is safe,
/// but it must not enter `Inventory` until the host reveals it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HiddenItemModifier {
    pub id: String,
    pub item: ItemId,
    pub modifier: ItemModifier,
    pub reveal: RevealCondition,
}

/// The public event payload created when a hidden modifier is revealed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemModifierReveal {
    pub id: String,
    pub item: ItemId,
    pub modifier: ItemModifier,
}

impl HiddenItemModifier {
    pub fn public_face(&self) -> ItemModifierReveal {
        ItemModifierReveal {
            id: self.id.clone(),
            item: self.item.clone(),
            modifier: self.modifier.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sword() -> ItemInstance {
        ItemInstance {
            id: ItemId::new("reward-03.sword"),
            template: "srd5e:longsword".to_owned(),
            name: "Fine Longsword".to_owned(),
            quantity: 1,
            tags: vec!["weapon".to_owned()],
            modifiers: Vec::new(),
            appearance_layers: vec!["weapon:longsword".to_owned()],
        }
    }

    fn chart(id: &str, places: &[&str]) -> ItemInstance {
        ItemInstance {
            id: ItemId::new(id),
            template: "map".to_owned(),
            name: "Old Chart".to_owned(),
            quantity: 1,
            tags: std::iter::once("map".to_owned())
                .chain(places.iter().map(|p| format!("reveals:{p}")))
                .collect(),
            modifiers: Vec::new(),
            appearance_layers: Vec::new(),
        }
    }

    #[test]
    fn a_chart_discloses_its_tagged_places_and_gear_discloses_none() {
        let map = chart("chart.citadel", &["citadel", "shrine"]);
        assert_eq!(
            map.revealed_places().collect::<Vec<_>>(),
            vec!["citadel", "shrine"]
        );
        // An ordinary weapon, and an empty `reveals:` tag, disclose nothing.
        assert!(sword().revealed_places().next().is_none());
        let mut junk = sword();
        junk.tags.push("reveals:".to_owned());
        assert!(junk.revealed_places().next().is_none());
    }

    #[test]
    fn a_pack_aggregates_every_carried_chart() {
        let mut pack = Inventory::default();
        pack.insert(sword()).unwrap();
        pack.insert(chart("chart.a", &["keep"])).unwrap();
        pack.insert(chart("chart.b", &["citadel"])).unwrap();
        // Item-id order: chart.a before chart.b before reward-03.sword.
        assert_eq!(
            pack.revealed_places().collect::<Vec<_>>(),
            vec!["keep", "citadel"]
        );
    }

    #[test]
    fn equipped_item_reveals_modifier_layers_idempotently() {
        let mut inventory = Inventory::default();
        inventory.insert(sword()).unwrap();
        inventory
            .equip(EquipmentSlot::MainHand, ItemId::new("reward-03.sword"))
            .unwrap();
        let modifier = ItemModifier {
            id: "reward-03.sword.flaming".to_owned(),
            kind: ItemModifierKind::Enchantment,
            name: "Flaming".to_owned(),
            stats: BTreeMap::from([("attack_bonus".to_owned(), 1)]),
            appearance_layer: Some("effect:flame".to_owned()),
        };
        let item = inventory.item_mut(&ItemId::new("reward-03.sword")).unwrap();
        item.attach_modifier(modifier.clone()).unwrap();
        item.attach_modifier(modifier).unwrap();
        assert_eq!(
            item.appearance_layers().collect::<Vec<_>>(),
            vec!["weapon:longsword", "effect:flame"]
        );
        inventory.validate().unwrap();
    }

    #[test]
    fn taking_an_equipped_item_clears_its_slot() {
        let mut inventory = Inventory::default();
        inventory.insert(sword()).unwrap();
        let id = ItemId::new("reward-03.sword");
        inventory
            .equip(EquipmentSlot::MainHand, id.clone())
            .unwrap();
        let moved = inventory.take(&id).unwrap();
        assert_eq!(moved.id, id);
        assert!(inventory.items.is_empty());
        assert!(inventory.equipped.is_empty());
    }
}
