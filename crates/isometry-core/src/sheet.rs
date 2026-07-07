//! Character-sheet **data**: system-agnostic field values bound to
//! tokens. The substrate stores a sheet; it never interprets it. What a
//! field means (an ability score, hit points), how derived stats compute,
//! and what an action rolls all live in a system plugin above the
//! substrate. This keeps the core free of any one game's rules, the same
//! split the geometry and turns already follow.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One sheet field value. A small closed set the substrate can store and
/// replicate; a system plugin decides what each field means.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FieldValue {
    Int(i64),
    Text(String),
    Bool(bool),
}

impl FieldValue {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            FieldValue::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            FieldValue::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// A character sheet's data: which system defines it, plus the field
/// values keyed by field name (ordered, so serde and any hashing are
/// deterministic).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetData {
    pub system: String,
    pub fields: BTreeMap<String, FieldValue>,
}

impl SheetData {
    pub fn new(system: impl Into<String>) -> Self {
        Self {
            system: system.into(),
            fields: BTreeMap::new(),
        }
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.fields.get(key).and_then(FieldValue::as_int)
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        self.fields.get(key).and_then(FieldValue::as_text)
    }

    pub fn set_int(&mut self, key: impl Into<String>, n: i64) {
        self.fields.insert(key.into(), FieldValue::Int(n));
    }

    pub fn set_text(&mut self, key: impl Into<String>, s: impl Into<String>) {
        self.fields.insert(key.into(), FieldValue::Text(s.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_access_and_serde_round_trip() {
        let mut s = SheetData::new("5e-srd");
        s.set_int("str", 16);
        s.set_text("name", "Aldric");
        assert_eq!(s.int("str"), Some(16));
        assert_eq!(s.text("name"), Some("Aldric"));
        assert_eq!(s.int("name"), None);
        let json = serde_json::to_string(&s).unwrap();
        let back: SheetData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}
