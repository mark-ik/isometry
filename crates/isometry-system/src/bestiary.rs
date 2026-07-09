//! The SRD bestiary: pre-statted monsters as content.
//!
//! A content pack is data (design_docs/2026-07-08_campaign_packs_plan.md,
//! decision 1). This module carries a hand-authored starter slice; the full
//! set is vendored from 5e-database later (P2b), transformed into this shape.
//! Fields mirror the 5e-database monster schema (name, size, type, AC, HP,
//! hit dice, speed, the six abilities, CR, XP, actions), trimmed to what the
//! compendium shows and the board spawns.
//!
//! Content is SRD 5.1, used under CC-BY-4.0 with attribution (bootstrap
//! decision #7). The `sprite` key ties a monster to its voxel appearance, so
//! spawning a monster drops a baked token onto the board.

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

fn action(name: &str, to_hit: i32, damage: &str, desc: &str) -> MonsterAction {
    MonsterAction {
        name: name.into(),
        desc: desc.into(),
        to_hit: Some(to_hit),
        damage: Some(damage.into()),
    }
}

/// The starter SRD bestiary (hand-authored; SRD 5.1, CC-BY-4.0). The full
/// compendium is vendored from 5e-database in P2b.
pub fn srd_bestiary() -> Vec<Monster> {
    vec![
        Monster {
            key: "goblin".into(),
            name: "Goblin".into(),
            size: "Small".into(),
            kind: "humanoid (goblinoid)".into(),
            alignment: "neutral evil".into(),
            armor_class: 15,
            hit_points: 7,
            hit_dice: "2d6".into(),
            speed_ft: 30,
            abilities: [8, 14, 10, 10, 8, 8],
            challenge_rating: 0.25,
            xp: 50,
            actions: vec![
                action("Scimitar", 4, "1d6+2 slashing", "Melee weapon attack, reach 5 ft."),
                action("Shortbow", 4, "1d6+2 piercing", "Ranged weapon attack, range 80/320 ft."),
            ],
            sprite: "goblin".into(),
        },
        Monster {
            key: "skeleton".into(),
            name: "Skeleton".into(),
            size: "Medium".into(),
            kind: "undead".into(),
            alignment: "lawful evil".into(),
            armor_class: 13,
            hit_points: 13,
            hit_dice: "2d8+4".into(),
            speed_ft: 30,
            abilities: [10, 14, 15, 6, 8, 5],
            challenge_rating: 0.25,
            xp: 50,
            actions: vec![action("Shortsword", 4, "1d6+2 piercing", "Melee weapon attack, reach 5 ft.")],
            sprite: "skeleton".into(),
        },
        Monster {
            key: "wolf".into(),
            name: "Wolf".into(),
            size: "Medium".into(),
            kind: "beast".into(),
            alignment: "unaligned".into(),
            armor_class: 13,
            hit_points: 11,
            hit_dice: "2d8+2".into(),
            speed_ft: 40,
            abilities: [12, 15, 12, 3, 12, 6],
            challenge_rating: 0.25,
            xp: 50,
            actions: vec![action("Bite", 4, "2d4+2 piercing", "Melee weapon attack; DC 11 STR or prone.")],
            sprite: "wolf".into(),
        },
        Monster {
            key: "orc".into(),
            name: "Orc".into(),
            size: "Medium".into(),
            kind: "humanoid (orc)".into(),
            alignment: "chaotic evil".into(),
            armor_class: 13,
            hit_points: 15,
            hit_dice: "2d8+6".into(),
            speed_ft: 30,
            abilities: [16, 12, 16, 7, 11, 10],
            challenge_rating: 0.5,
            xp: 100,
            actions: vec![action("Greataxe", 5, "1d12+3 slashing", "Melee weapon attack, reach 5 ft.")],
            sprite: "orc".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_bestiary_is_statted() {
        let b = srd_bestiary();
        assert_eq!(b.len(), 4);
        let goblin = b.iter().find(|m| m.key == "goblin").unwrap();
        assert_eq!(goblin.armor_class, 15);
        assert_eq!(goblin.hit_points, 7);
        assert_eq!(goblin.ability_mod(1), 2); // DEX 14 -> +2
        assert_eq!(goblin.cr_label(), "1/4");
        assert_eq!(goblin.actions.len(), 2);
    }

    #[test]
    fn cr_labels_read_as_fractions() {
        let orc = srd_bestiary().into_iter().find(|m| m.key == "orc").unwrap();
        assert_eq!(orc.cr_label(), "1/2");
    }

    #[test]
    fn monster_round_trips_json() {
        let m = &srd_bestiary()[0];
        let json = serde_json::to_string(m).unwrap();
        let back: Monster = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, m);
    }
}
