//! SRD spells as content, loaded from the vendored pack.
//!
//! `../data/spells.json` is the SRD 5.1 spell list transformed from
//! 5e-database (P2b), used under CC-BY-4.0 with attribution (bootstrap
//! decision #7).

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

/// The SRD spell list, vendored from 5e-database (SRD 5.1, CC-BY-4.0).
pub fn srd_spells() -> Vec<Spell> {
    serde_json::from_str(include_str!("../data/spells.json")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spells_load_and_read() {
        let s = srd_spells();
        assert!(
            s.len() > 100,
            "vendored spell list should be the full SRD set"
        );
        let fireball = s.iter().find(|s| s.key == "fireball").unwrap();
        assert_eq!(fireball.level, 3);
        assert_eq!(fireball.level_label(), "3");
        let bolt = s.iter().find(|s| s.key == "fire-bolt").unwrap();
        assert_eq!(bolt.level_label(), "Cantrip");
    }
}
