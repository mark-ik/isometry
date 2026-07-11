//! SRD equipment as content, loaded from the vendored pack.
//!
//! `../data/items.json` is the SRD 5.1 equipment list transformed from
//! 5e-database (P2b), used under CC-BY-4.0 with attribution (bootstrap
//! decision #7).

use serde::{Deserialize, Serialize};

/// An equipment entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub key: String,
    pub name: String,
    /// "Weapon", "Armor", "Adventuring Gear", ...
    pub category: String,
    pub cost: String,
    pub weight: String,
    /// The mechanical line (damage for weapons, AC for armor), if any.
    pub detail: String,
    pub desc: String,
}

/// The SRD equipment list, vendored from 5e-database (SRD 5.1, CC-BY-4.0).
pub fn srd_items() -> Vec<Item> {
    serde_json::from_str(include_str!("../data/items.json")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_load_and_read() {
        let items = srd_items();
        assert!(
            items.len() > 50,
            "vendored equipment should be the full SRD set"
        );
        let sword = items.iter().find(|i| i.key == "longsword").unwrap();
        assert_eq!(sword.category, "Weapon");
        assert!(sword.detail.contains("1d8"));
    }
}
