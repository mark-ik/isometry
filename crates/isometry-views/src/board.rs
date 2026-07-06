//! The board screen: side panel plus the iso board pane.
//!
//! Every ground tile, elevation-column filler, prop, and token is one
//! absolutely-positioned element inside the `.board` container; the
//! container's inline `left`/`top` carries the camera, so a pan is a
//! single attribute change on one element. Depth comes from
//! [`isometry_core::depth_key`] as a plain z-index.

use isometry_core::{depth_key, MapDocument, TileCoord, TileKindId, Token};
use xilem_serval::{clickable, el, AnyView, ServalCtx, ServalElement};

use crate::panel::side_panel;
use crate::state::UiState;

pub type UiChild = Box<dyn AnyView<UiState, (), ServalCtx, ServalElement>>;

/// One diamond at tile `at`, drawn at `elevation`, with `class` deciding
/// its paint. Clicking selects the tile.
fn tile_el(ui: &UiState, at: TileCoord, elevation: i32, class: String) -> UiChild {
    let geo = &ui.geo;
    let (cx, cy) = geo.tile_to_screen(at, elevation);
    let (x, y) = (cx - geo.tile_w / 2.0, cy - geo.tile_h / 2.0);
    let z = depth_key(at, elevation);
    Box::new(clickable(
        el("div", ()).attr("class", class).attr(
            "style",
            format!("left: {x}px; top: {y}px; z-index: {z};"),
        ),
        move |ui: &mut UiState, _| {
            ui.click_tile(at);
        },
    ))
}

fn kind_name(map: &MapDocument, kind: TileKindId) -> &str {
    map.tile_kinds
        .get(kind.0 as usize)
        .map(String::as_str)
        .unwrap_or("empty")
}

/// The ground layer: a column of filler diamonds up to the tile's
/// elevation, then the top diamond carrying the kind class.
fn ground_tiles(ui: &UiState) -> Vec<UiChild> {
    let map = &ui.map;
    let mut out: Vec<UiChild> = Vec::new();
    for (col, row, kind) in map.ground.iter() {
        if kind.0 == 0 {
            continue;
        }
        let at: TileCoord = (col as i32, row as i32);
        let elev = *map.elevation.get(col, row).unwrap_or(&0) as i32;
        for step in 0..elev {
            out.push(tile_el(ui, at, step, "tile tile-under".to_owned()));
        }
        let mut class = format!("tile tile-{}", kind_name(map, *kind));
        if (col + row) % 2 == 1 {
            class.push_str(" alt");
        }
        if ui.selected == Some(at) {
            class.push_str(" tile-selected");
        }
        out.push(tile_el(ui, at, elev, class));
    }
    out
}

/// Props stand on their tile: anchored bottom-center on the diamond,
/// one depth step above the ground they occupy.
fn prop_tiles(ui: &UiState) -> Vec<UiChild> {
    let map = &ui.map;
    let geo = &ui.geo;
    let mut out: Vec<UiChild> = Vec::new();
    for (col, row, kind) in map.props.iter() {
        if kind.0 == 0 {
            continue;
        }
        let at: TileCoord = (col as i32, row as i32);
        let elev = *map.elevation.get(col, row).unwrap_or(&0) as i32;
        let (cx, cy) = geo.tile_to_screen(at, elev);
        let z = depth_key(at, elev) + 1;
        let class = format!("prop prop-{}", kind_name(map, *kind));
        // 20x24 body, base at the diamond center.
        let (x, y) = (cx - 10.0, cy - 24.0);
        out.push(Box::new(el("div", ()).attr("class", class).attr(
            "style",
            format!("left: {x}px; top: {y}px; z-index: {z};"),
        )));
    }
    out
}

fn token_el(ui: &UiState, token: &Token) -> UiChild {
    let geo = &ui.geo;
    let elev = *ui
        .map
        .elevation
        .get(token.at.0.max(0) as u32, token.at.1.max(0) as u32)
        .unwrap_or(&0) as i32;
    let (cx, cy) = geo.tile_to_screen(token.at, elev);
    let z = depth_key(token.at, elev) + 2;
    // 8x12 sprite at 3x (24x36), feet at the diamond center.
    let (x, y) = (cx - 12.0, cy - 32.0);
    Box::new(
        el("div", ())
            .attr("class", format!("token token-{}", token.sprite))
            .attr(
                "style",
                format!("left: {x}px; top: {y}px; z-index: {z};"),
            ),
    )
}

/// The screen root the runner diffs.
pub fn board_root(ui: &UiState) -> UiChild {
    let mut layers: Vec<UiChild> = ground_tiles(ui);
    layers.extend(prop_tiles(ui));
    layers.extend(ui.map.tokens.iter().map(|t| token_el(ui, t)));
    let (camx, camy) = ui.camera;
    Box::new(
        el(
            "div",
            (
                side_panel(ui),
                el(
                    "div",
                    el("div", layers).attr("class", "board").attr(
                        "style",
                        format!("left: {camx}px; top: {camy}px;"),
                    ),
                )
                .attr("class", "pane"),
            ),
        )
        .attr("class", "app"),
    )
}
