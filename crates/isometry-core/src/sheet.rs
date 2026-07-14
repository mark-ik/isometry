//! Character-sheet **data**: system-agnostic field values bound to
//! tokens. The substrate stores a sheet; it never interprets it. What a
//! field means (an ability score, hit points), how derived stats compute,
//! and what an action rolls all live in a system plugin above the
//! substrate. This keeps the core free of any one game's rules, the same
//! split the geometry and turns already follow.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One sheet field value. A small closed set the substrate can store and
/// replicate; a system plugin decides what each field means. `List` and
/// `Map` nest, so inventories, modifier stacks, and condition lists fit
/// without a schema change (worldbuilding plan W0). New variants append
/// at the end: postcard encodes the variant index, so inserting one
/// would silently re-tag every later variant on the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FieldValue {
    Int(i64),
    Text(String),
    Bool(bool),
    Float(f64),
    List(Vec<FieldValue>),
    Map(BTreeMap<String, FieldValue>),
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

    pub fn as_float(&self) -> Option<f64> {
        match self {
            FieldValue::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[FieldValue]> {
        match self {
            FieldValue::List(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&BTreeMap<String, FieldValue>> {
        match self {
            FieldValue::Map(m) => Some(m),
            _ => None,
        }
    }
}

/// One typed change to one integer field of one token's sheet: the only
/// way a resolved action reaches game state.
///
/// The substrate applies it without knowing what the field means, exactly
/// as it stores a sheet without interpreting it. A system plugin decides
/// that `hp_current` is hit points and that a sword subtracts from it; the
/// core only knows how to add a signed number to a named integer. No clamp
/// is applied here: whether a value may go negative (death saves, debt,
/// overheal) is a rule, so the system owns it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetDelta {
    pub token: crate::map::TokenId,
    pub key: String,
    pub add: i64,
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

    /// Add `add` to an integer field, creating it at zero when absent. The
    /// substrate's whole understanding of a resolved action.
    pub fn add_int(&mut self, key: &str, add: i64) {
        let cur = self.int(key).unwrap_or(0);
        self.set_int(key, cur + add);
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

    #[test]
    fn nested_values_round_trip() {
        let mut inventory = BTreeMap::new();
        inventory.insert("weight".to_owned(), FieldValue::Float(3.5));
        inventory.insert(
            "items".to_owned(),
            FieldValue::List(vec![
                FieldValue::Text("longsword".to_owned()),
                FieldValue::Text("rope".to_owned()),
            ]),
        );
        let mut s = SheetData::new("5e-srd");
        s.fields
            .insert("inventory".to_owned(), FieldValue::Map(inventory));

        let v = s.fields.get("inventory").unwrap();
        let map = v.as_map().unwrap();
        assert_eq!(map.get("weight").unwrap().as_float(), Some(3.5));
        assert_eq!(map.get("items").unwrap().as_list().unwrap().len(), 2);
        assert_eq!(v.as_int(), None);

        let json = serde_json::to_string(&s).unwrap();
        let back: SheetData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}
