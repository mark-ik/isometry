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
