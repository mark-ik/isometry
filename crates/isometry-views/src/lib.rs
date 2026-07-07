//! Isometry's xilem_serval view layer.
//!
//! View functions project [`isometry_core`] state into DOM-shaped views:
//! every visible tile, prop, and token is an element positioned by the
//! iso math, appearance bound through CSS class vocabulary so tilesets
//! arrive as stylesheets. Host-agnostic: the desktop host and the later
//! web host both drive [`board_root`].

mod board;
mod demo;
mod panel;
mod sheet;
mod state;
mod theme;

pub use board::{board_root, UiChild};
pub use demo::{demo_map, synth_map};
pub use state::{
    EditMode, FogLevel, InitiativeMode, NetMode, SheetSchema, UiState, PANEL_W,
};
pub use theme::board_css;
