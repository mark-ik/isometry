//! The SRD bestiary: monsters as content, loaded from the vendored pack.
//!
//! The data in `../data/monsters.json` is the SRD 5.1 bestiary transformed
//! from 5e-database into this trimmed shape (P2b). Used under CC-BY-4.0 with
//! attribution (bootstrap decision #7). The `sprite` key ties a monster to a
//! voxel appearance, so spawning drops a baked token onto the board.

use serde::{Deserialize, Serialize};

/// One attack or trait line on a monster.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonsterAction {
    pub name: String,
    pub desc: String,
    /// Attack bonus to hit, if this is an attack.
    pub to_hit: Option<i32>,
    /// Damage expression, e.g. "1d6+2 slashing".
    pub damage: Option<String>,
}

/// A statted monster: a compendium entry and a spawnable token.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Monster {
    pub key: String,
    pub name: String,
    pub size: String,
    pub kind: String,
    pub alignment: String,
    pub armor_class: i32,
    pub hit_points: i32,
    pub hit_dice: String,
    pub speed_ft: i32,
    /// STR, DEX, CON, INT, WIS, CHA.
    pub abilities: [i32; 6],
    pub challenge_rating: f32,
    pub xp: i32,
    pub actions: Vec<MonsterAction>,
    /// Token appearance key (a voxel rig / tileset class).
    pub sprite: String,
}

impl Monster {
    /// D&D ability modifier for ability `i` (0 = STR ... 5 = CHA).
    pub fn ability_mod(&self, i: usize) -> i32 {
        (self.abilities[i] - 10).div_euclid(2)
    }

    /// Challenge rating as displayed ("1/8", "1/4", "1/2", or a whole number).
    pub fn cr_label(&self) -> String {
        let cr = self.challenge_rating;
        if cr == 0.125 {
            "1/8".into()
        } else if cr == 0.25 {
            "1/4".into()
        } else if cr == 0.5 {
            "1/2".into()
        } else {
            format!("{}", cr as i32)
        }
    }
}

/// The SRD bestiary, vendored from 5e-database (SRD 5.1, CC-BY-4.0).
pub fn srd_bestiary() -> Vec<Monster> {
    serde_json::from_str(include_str!("../data/monsters.json")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bestiary_loads_and_is_statted() {
        let b = srd_bestiary();
        assert!(b.len() > 50, "vendored bestiary should be the full SRD set");
        let goblin = b.iter().find(|m| m.key == "goblin").unwrap();
        assert_eq!(goblin.armor_class, 15);
        assert_eq!(goblin.hit_points, 7);
        assert_eq!(goblin.ability_mod(1), 2); // DEX 14 -> +2
        assert_eq!(goblin.cr_label(), "1/4");
        assert!(!goblin.actions.is_empty());
    }

    #[test]
    fn cr_labels_read_as_fractions() {
        let orc = srd_bestiary().into_iter().find(|m| m.key == "orc").unwrap();
        assert_eq!(orc.cr_label(), "1/2");
    }
}
