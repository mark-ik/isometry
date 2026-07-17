use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::grid::TileGrid;
use crate::iso::TileCoord;
use crate::sheet::SheetData;

/// Index into the map's tileset vocabulary (`MapDocument::tile_kinds`).
/// Kind 0 is always "empty" by convention.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileKindId(pub u16);

/// Stable identity for a token within one map/session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TokenId(pub u32);

/// Grid-axis facing. `North` looks toward decreasing row (screen
/// upper-right under the fixed camera), and the rest follow clockwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facing {
    #[default]
    South,
    North,
    East,
    West,
}

/// The paintable tile layers, bottom to top. Tokens are not a layer;
/// they live in `MapDocument::tokens` and sort by depth key at render.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layer {
    Ground,
    Prop,
}

/// A playing piece: position, facing, owner, sprite binding. Character
/// sheet binding arrives with system plugins (I6) and hangs off `TokenId`
/// there, not here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub id: TokenId,
    pub at: TileCoord,
    pub facing: Facing,
    /// Sprite class within the campaign's token sheet vocabulary.
    pub sprite: String,
    /// Player name owning this token; `None` means DM-controlled.
    pub owner: Option<String>,
}

/// One authored board: tile layers over a height field, plus tokens.
///
/// This is the document the editor saves and the session replicates.
/// Appearance stays out of it: `tile_kinds` names classes, the tileset
/// stylesheet decides what they look like.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapDocument {
    pub name: String,
    /// Tile-kind class vocabulary; index 0 is "empty".
    pub tile_kinds: Vec<String>,
    pub ground: TileGrid<TileKindId>,
    pub props: TileGrid<TileKindId>,
    /// Height units per tile; renders via `IsoGeometry::elev_step`.
    pub elevation: TileGrid<u8>,
    pub tokens: Vec<Token>,
    /// Character sheets bound to tokens (system-agnostic data; a system
    /// plugin interprets them). `serde(default)` so older saves load.
    #[serde(default)]
    pub sheets: BTreeMap<TokenId, SheetData>,
    /// Tokens that are out of play. The substrate does not know *why* (hit
    /// points at zero, fled, banished): a system plugin decides that and says
    /// so. What the substrate does with it is generic and mechanical, the same
    /// way it treats elevation: skip the token's turn, refuse it as a target,
    /// and let the view show it as fallen. `serde(default)` so older saves load.
    #[serde(default)]
    pub defeated: BTreeSet<TokenId>,
    /// Named conditions on tokens (`prone`, `blinded`, ...). Opaque to the
    /// substrate: a system plugin decides what a name means and when it applies;
    /// the substrate stores it for display, for the rules' own reading, and for
    /// pack CSS (`cond-<name>` on the board). The *mechanical* projection of a
    /// condition arrives separately as [`Self::mobility`] numbers.
    #[serde(default)]
    pub conditions: BTreeMap<TokenId, BTreeSet<String>>,
    /// The system's current mechanical ruling per token: `(move budget, sight
    /// radius)` in tiles. Host-computed by the rules whenever conditions change,
    /// then replicated, because clients hold no rules engine and fog and reach
    /// preview are computed client-side. Absent means "use the sheet's base
    /// values": most tokens never enter this map.
    #[serde(default)]
    pub mobility: BTreeMap<TokenId, (u32, u32)>,
    /// Per-turn, per-token named integer counters, cleared when a token's turn
    /// begins. The substrate's one primitive for *every* per-turn resource: the
    /// system spends them, counts with them, and reads them, and the substrate
    /// only stores them and resets them at turn start. It knows nothing of
    /// "actions" or "attacks" -- a ruleset names the counters and decides what
    /// they cost and mean, so a three-action economy, a multiple-attack penalty,
    /// or a regenerating mana pool are all just configurations of this one map,
    /// none of them baked in. Absent counters read as zero.
    #[serde(default)]
    pub turn_counters: BTreeMap<TokenId, BTreeMap<String, i64>>,
}

impl MapDocument {
    /// An empty board of `width` x `height` at elevation 0.
    pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            name: name.into(),
            tile_kinds: vec!["empty".to_owned()],
            ground: TileGrid::new(width, height, TileKindId(0)),
            props: TileGrid::new(width, height, TileKindId(0)),
            elevation: TileGrid::new(width, height, 0),
            tokens: Vec::new(),
            sheets: BTreeMap::new(),
            defeated: BTreeSet::new(),
            conditions: BTreeMap::new(),
            mobility: BTreeMap::new(),
            turn_counters: BTreeMap::new(),
        }
    }

    /// Whether `id` currently has the named condition.
    pub fn has_condition(&self, id: TokenId, name: &str) -> bool {
        self.conditions
            .get(&id)
            .is_some_and(|set| set.contains(name))
    }

    /// Apply or clear one named condition. The substrate has no opinion about
    /// the name; the caller (the rules, via a replicated event) supplies the
    /// mechanical consequences separately through [`Self::set_mobility`].
    pub fn set_condition(&mut self, id: TokenId, name: &str, on: bool) {
        if on {
            self.conditions.entry(id).or_default().insert(name.to_owned());
        } else if let Some(set) = self.conditions.get_mut(&id) {
            set.remove(name);
            if set.is_empty() {
                self.conditions.remove(&id);
            }
        }
    }

    /// Record the system's mechanical ruling for `id`. `None` clears it back to
    /// the sheet's base values (the common case once every condition lifts).
    pub fn set_mobility(&mut self, id: TokenId, mobility: Option<(u32, u32)>) {
        match mobility {
            Some(m) => {
                self.mobility.insert(id, m);
            }
            None => {
                self.mobility.remove(&id);
            }
        }
    }

    /// A token's per-turn counter, zero when unset.
    pub fn turn_counter(&self, id: TokenId, key: &str) -> i64 {
        self.turn_counters
            .get(&id)
            .and_then(|c| c.get(key))
            .copied()
            .unwrap_or(0)
    }

    /// Add `delta` to a per-turn counter (creating it at zero). A counter that
    /// returns to zero is dropped, so an untouched turn stores nothing.
    pub fn bump_turn_counter(&mut self, id: TokenId, key: &str, delta: i64) {
        let entry = self.turn_counters.entry(id).or_default();
        let v = entry.entry(key.to_owned()).or_insert(0);
        *v += delta;
        if *v == 0 {
            entry.remove(key);
        }
        if entry.is_empty() {
            self.turn_counters.remove(&id);
        }
    }

    /// Wipe a token's per-turn counters. Called when its turn begins, so every
    /// per-turn resource refreshes at once without the substrate knowing what
    /// any of them mean.
    pub fn clear_turn_counters(&mut self, id: TokenId) {
        self.turn_counters.remove(&id);
    }

    /// The effective `(move budget, sight radius)` for `id`: the system's
    /// ruling when one is in force, else the sheet's base `speed`/`sight`
    /// fields, else the caller's defaults. The substrate itself has no numbers
    /// of its own here; `defaults` is the *view's* fallback for a sheetless
    /// token, not a rule.
    pub fn effective_mobility(&self, id: TokenId, defaults: (u32, u32)) -> (u32, u32) {
        if let Some(&m) = self.mobility.get(&id) {
            return m;
        }
        let sheet = self.sheet(id);
        let read = |key: &str, default: u32| {
            sheet
                .and_then(|s| s.int(key))
                .map(|v| v.max(0) as u32)
                .unwrap_or(default)
        };
        (read("speed", defaults.0), read("sight", defaults.1))
    }

    /// Whether `id` is out of play.
    pub fn is_defeated(&self, id: TokenId) -> bool {
        self.defeated.contains(&id)
    }

    /// Mark `id` in or out of play. Reversible, because a healed or revived
    /// token stands back up and the system is what says so.
    pub fn set_defeated(&mut self, id: TokenId, down: bool) {
        if down {
            self.defeated.insert(id);
        } else {
            self.defeated.remove(&id);
        }
    }

    pub fn sheet(&self, id: TokenId) -> Option<&SheetData> {
        self.sheets.get(&id)
    }

    pub fn set_sheet(&mut self, id: TokenId, sheet: SheetData) {
        self.sheets.insert(id, sheet);
    }

    /// Apply one resolved delta to the addressed token's sheet. Returns false
    /// when that token has no sheet, which is the only failure the substrate
    /// can see: it has no opinion about the field, the sign, or the magnitude.
    pub fn apply_delta(&mut self, delta: &crate::sheet::SheetDelta) -> bool {
        match self.sheets.get_mut(&delta.token) {
            Some(sheet) => {
                sheet.add_int(&delta.key, delta.add);
                true
            }
            None => false,
        }
    }

    /// Register a tile kind, returning its id; existing names are reused.
    pub fn intern_tile_kind(&mut self, name: &str) -> TileKindId {
        if let Some(i) = self.tile_kinds.iter().position(|k| k == name) {
            TileKindId(i as u16)
        } else {
            self.tile_kinds.push(name.to_owned());
            TileKindId((self.tile_kinds.len() - 1) as u16)
        }
    }

    pub fn layer_mut(&mut self, layer: Layer) -> &mut TileGrid<TileKindId> {
        match layer {
            Layer::Ground => &mut self.ground,
            Layer::Prop => &mut self.props,
        }
    }

    pub fn token(&self, id: TokenId) -> Option<&Token> {
        self.tokens.iter().find(|t| t.id == id)
    }

    pub fn token_mut(&mut self, id: TokenId) -> Option<&mut Token> {
        self.tokens.iter_mut().find(|t| t.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_counters_bump_read_and_clear() {
        let mut m = MapDocument::new("t", 4, 4);
        let a = TokenId(1);
        assert_eq!(m.turn_counter(a, "actions_spent"), 0, "unset reads zero");

        m.bump_turn_counter(a, "actions_spent", 1);
        m.bump_turn_counter(a, "actions_spent", 1);
        m.bump_turn_counter(a, "strikes", 3);
        assert_eq!(m.turn_counter(a, "actions_spent"), 2);
        assert_eq!(m.turn_counter(a, "strikes"), 3);

        // A counter returning to zero drops, so an untouched token stores nothing.
        m.bump_turn_counter(a, "actions_spent", -2);
        assert_eq!(m.turn_counter(a, "actions_spent"), 0);
        assert!(m.turn_counters[&a].get("actions_spent").is_none());

        // The turn ends: everything the token accrued clears at once.
        m.clear_turn_counters(a);
        assert_eq!(m.turn_counter(a, "strikes"), 0);
        assert!(m.turn_counters.get(&a).is_none());
    }

    #[test]
    fn intern_reuses_existing_kinds() {
        let mut m = MapDocument::new("t", 4, 4);
        let grass = m.intern_tile_kind("grass");
        let tree = m.intern_tile_kind("tree");
        assert_eq!(m.intern_tile_kind("grass"), grass);
        assert_ne!(grass, tree);
        assert_eq!(m.tile_kinds[0], "empty");
    }

    #[test]
    fn serde_round_trip() {
        let mut m = MapDocument::new("skirmish", 8, 8);
        let grass = m.intern_tile_kind("grass");
        m.ground.set(2, 3, grass);
        m.elevation.set(2, 3, 2);
        m.tokens.push(Token {
            id: TokenId(1),
            at: (2, 3),
            facing: Facing::East,
            sprite: "knight".to_owned(),
            owner: Some("mark".to_owned()),
        });
        let json = serde_json::to_string(&m).unwrap();
        let back: MapDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }
}
