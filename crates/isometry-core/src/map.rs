use std::collections::BTreeMap;

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
        }
    }

    pub fn sheet(&self, id: TokenId) -> Option<&SheetData> {
        self.sheets.get(&id)
    }

    pub fn set_sheet(&mut self, id: TokenId, sheet: SheetData) {
        self.sheets.insert(id, sheet);
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
