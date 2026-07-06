//! Isometry's pure substrate model.
//!
//! Everything here is geometry, documents, and events: tile grids, the
//! 2:1 diamond projection, map documents, and the session event log.
//! Game rules never live in this crate; they arrive as system plugins
//! (schema plus scripts) in a later phase. Keep this crate free of UI,
//! I/O, networking, and serval dependencies.

mod event;
mod grid;
mod iso;
mod map;

pub use event::{EventError, SessionEvent, apply};
pub use grid::TileGrid;
pub use iso::{IsoGeometry, ScreenPoint, TileCoord, depth_key};
pub use map::{Facing, Layer, MapDocument, TileKindId, Token, TokenId};
