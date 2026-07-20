//! Isometry's Cambium view layer.
//!
//! View functions project [`isometry_core`] state into DOM-shaped views:
//! every visible tile, prop, and token is an element positioned by the
//! iso math, appearance bound through CSS class vocabulary so tilesets
//! arrive as stylesheets. Host-agnostic: the desktop host and the later
//! web host both drive [`board_root`].

mod board;
mod command;
mod compendium;
mod demo;
mod downtime;
mod generator;
mod governance;
mod overmap;
mod panel;
mod sheet;
mod state;
mod storylet;
mod theme;
mod widgets;

pub use board::{board_root, UiChild};
pub use demo::{demo_map, synth_map};
pub use overmap::{overmap_swatch, OvermapNodeKind, OVERMAP_CANVAS, OVERMAP_LEAF_KEY};
pub use state::{
    ActionRow, CompendiumTab, EditMode, FactionMoveRow, FogLevel, GenerationRequest,
    GovernanceBindingRow, GovernanceConflict, GovernanceResolutionRequest, InitiativeMode,
    InventoryRequest, ItemRow, MonsterRow, NetMode, SheetSchema, SpellRow, StoryletRow, UiState,
    PANEL_W,
};
pub use theme::board_css;
