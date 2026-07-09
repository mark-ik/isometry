//! SRD equipment (content pack). Hand-authored starter slice; the full set is
//! vendored from 5e-database in P2b. SRD 5.1, used under CC-BY-4.0 with
//! attribution (bootstrap decision #7).

use serde::{Deserialize, Serialize};

/// An equipment entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub key: String,
    pub name: String,
    /// "Weapon", "Armor", "Gear".
    pub category: String,
    pub cost: String,
    pub weight: String,
    /// The mechanical line (damage for weapons, AC for armor), if any.
    pub detail: String,
    pub desc: String,
}

fn item(key: &str, name: &str, category: &str, cost: &str, weight: &str, detail: &str, desc: &str) -> Item {
    Item {
        key: key.into(),
        name: name.into(),
        category: category.into(),
        cost: cost.into(),
        weight: weight.into(),
        detail: detail.into(),
        desc: desc.into(),
    }
}

/// The starter SRD equipment list (hand-authored; SRD 5.1, CC-BY-4.0).
pub fn srd_items() -> Vec<Item> {
    vec![
        item(
            "dagger", "Dagger", "Weapon", "2 gp", "1 lb", "1d4 piercing",
            "A simple melee weapon with finesse, light, and thrown (range 20/60).",
        ),
        item(
            "longsword", "Longsword", "Weapon", "15 gp", "3 lb", "1d8 slashing",
            "A martial melee weapon with the versatile property (1d10 when wielded in two hands).",
        ),
        item(
            "longbow", "Longbow", "Weapon", "50 gp", "2 lb", "1d8 piercing",
            "A martial ranged weapon: ammunition, heavy, two-handed, range 150/600.",
        ),
        item(
            "shield", "Shield", "Armor", "10 gp", "6 lb", "+2 AC",
            "A shield strapped to the arm raises Armor Class by 2 while wielded.",
        ),
        item(
            "chain-mail", "Chain Mail", "Armor", "75 gp", "55 lb", "AC 16",
            "Heavy armor (base AC 16). Requires Strength 13 and imposes disadvantage on Stealth checks.",
        ),
        item(
            "potion-of-healing", "Potion of Healing", "Gear", "50 gp", "0.5 lb", "Regain 2d4+2 HP",
            "Drinking or administering this potion as an action restores 2d4+2 hit points.",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_items_are_listed() {
        let items = srd_items();
        assert!(items.len() >= 6);
        let sword = items.iter().find(|i| i.key == "longsword").unwrap();
        assert_eq!(sword.category, "Weapon");
        assert_eq!(sword.detail, "1d8 slashing");
    }
}
