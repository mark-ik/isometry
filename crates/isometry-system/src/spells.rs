//! SRD spells (content pack). Hand-authored starter slice; the full set is
//! vendored from 5e-database in P2b. SRD 5.1, used under CC-BY-4.0 with
//! attribution (bootstrap decision #7). Fields mirror the 5e-database spell
//! schema, trimmed to what the compendium shows.

use serde::{Deserialize, Serialize};

/// A spell entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spell {
    pub key: String,
    pub name: String,
    /// 0 = cantrip.
    pub level: u8,
    pub school: String,
    pub casting_time: String,
    pub range: String,
    pub components: String,
    pub duration: String,
    pub desc: String,
}

impl Spell {
    /// Level as displayed ("Cantrip" or the number).
    pub fn level_label(&self) -> String {
        if self.level == 0 {
            "Cantrip".into()
        } else {
            self.level.to_string()
        }
    }
}

fn spell(
    key: &str,
    name: &str,
    level: u8,
    school: &str,
    casting_time: &str,
    range: &str,
    components: &str,
    duration: &str,
    desc: &str,
) -> Spell {
    Spell {
        key: key.into(),
        name: name.into(),
        level,
        school: school.into(),
        casting_time: casting_time.into(),
        range: range.into(),
        components: components.into(),
        duration: duration.into(),
        desc: desc.into(),
    }
}

/// The starter SRD spell list (hand-authored; SRD 5.1, CC-BY-4.0).
pub fn srd_spells() -> Vec<Spell> {
    vec![
        spell(
            "fire-bolt", "Fire Bolt", 0, "Evocation", "1 action", "120 feet", "V, S",
            "Instantaneous",
            "A mote of fire streaks to a target: ranged spell attack for 1d10 fire damage. Rises by a die at levels 5, 11, and 17.",
        ),
        spell(
            "magic-missile", "Magic Missile", 1, "Evocation", "1 action", "120 feet", "V, S",
            "Instantaneous",
            "Three darts of magical force, each hitting automatically for 1d4+1 force damage. One more dart per slot level above 1st.",
        ),
        spell(
            "cure-wounds", "Cure Wounds", 1, "Evocation", "1 action", "Touch", "V, S",
            "Instantaneous",
            "A creature you touch regains hit points equal to 1d8 + your spellcasting ability modifier. Another 1d8 per slot level above 1st.",
        ),
        spell(
            "shield", "Shield", 1, "Abjuration", "1 reaction", "Self", "V, S", "1 round",
            "An invisible barrier gives +5 AC until the start of your next turn, including against the triggering attack, and blocks magic missile.",
        ),
        spell(
            "mage-armor", "Mage Armor", 1, "Abjuration", "1 action", "Touch", "V, S", "8 hours",
            "An unarmored willing creature's base AC becomes 13 + its Dexterity modifier until the spell ends.",
        ),
        spell(
            "fireball", "Fireball", 3, "Evocation", "1 action", "150 feet", "V, S, M",
            "Instantaneous",
            "A 20-foot-radius burst of flame: each creature makes a Dexterity save, taking 8d6 fire damage (half on a success). +1d6 per slot level above 3rd.",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_spells_are_listed() {
        let s = srd_spells();
        assert!(s.len() >= 6);
        let fireball = s.iter().find(|s| s.key == "fireball").unwrap();
        assert_eq!(fireball.level, 3);
        assert_eq!(fireball.level_label(), "3");
        let bolt = s.iter().find(|s| s.key == "fire-bolt").unwrap();
        assert_eq!(bolt.level_label(), "Cantrip");
    }
}
