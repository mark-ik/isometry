//! Isometry's pure substrate model.
//!
//! Everything here is geometry, documents, and events: tile grids, the
//! 2:1 diamond projection, map documents, and the session event log.
//! Game rules never live in this crate; they arrive as system plugins
//! (schema plus scripts) in a later phase. Keep this crate free of UI,
//! I/O, networking, and genet dependencies.

mod beat;
mod dice;
mod event;
mod grid;
mod iso;
mod map;
mod narrate;
mod overmap;
mod path;
mod sheet;
mod template;
mod turn;
mod visibility;

pub use beat::Beat;
pub use dice::{roll, Rng, RollRecord};
pub use sheet::{FieldValue, SheetData, SheetDelta};
pub use template::{away, compass, distance, push_path, template_tiles, TemplateKind};
pub use event::{EventError, SessionEvent, apply};
pub use grid::TileGrid;
pub use iso::{IsoGeometry, ScreenPoint, TileCoord, depth_key};
pub use map::{Facing, Layer, MapDocument, TileKindId, Token, TokenId};
pub use narrate::{bearing, describe_from, describe_scene, describe_token, facing_word};
pub use overmap::{Overmap, OvermapEdge, OvermapNode};
pub use path::{path_to, reachable, MoveRules};
pub use turn::TurnList;
pub use visibility::{visible_from, visible_tiles, SightRules};
