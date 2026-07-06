//! Isometry's pure substrate model.
//!
//! Everything here is geometry, documents, and events: tile grids, the
//! 2:1 diamond projection, map documents, and the session event log.
//! Game rules never live in this crate; they arrive as system plugins
//! (schema plus scripts) in a later phase. Keep this crate free of UI,
//! I/O, networking, and serval dependencies.

mod dice;
mod event;
mod grid;
mod iso;
mod map;
mod path;
mod turn;
mod visibility;

pub use dice::{roll, Rng, RollRecord};
pub use event::{EventError, SessionEvent, apply};
pub use grid::TileGrid;
pub use iso::{IsoGeometry, ScreenPoint, TileCoord, depth_key};
pub use map::{Facing, Layer, MapDocument, TileKindId, Token, TokenId};
pub use path::{path_to, reachable, MoveRules};
pub use turn::TurnList;
pub use visibility::{visible_from, visible_tiles, SightRules};
