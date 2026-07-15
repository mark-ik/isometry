//! The board screen: side panel plus the iso board pane.
//!
//! Every ground tile, elevation-column filler, prop, and token is one
//! absolutely-positioned element inside the `.board` container; the
//! container's inline `left`/`top` carries the camera, so a pan is a
//! single attribute change on one element. Depth comes from
//! [`isometry_core::depth_key`] as a plain z-index.

use std::collections::HashSet;

use isometry_core::{depth_key, path_to, MapDocument, TileCoord, TileKindId, Token};
use cambium::{AnyView, GenetCtx, GenetElement, clickable, el, text};

use crate::panel::side_panel;
use crate::state::{EditMode, FogLevel, UiState};

pub type UiChild = Box<dyn AnyView<UiState, (), GenetCtx, GenetElement>>;

/// One diamond at tile `at`, drawn at `elevation`, with `class` deciding
/// its paint. Clicking selects the tile.
fn tile_el(ui: &UiState, at: TileCoord, elevation: i32, class: String) -> UiChild {
    let geo = &ui.geo;
    let (cx, cy) = geo.tile_to_screen(at, elevation);
    let (x, y) = (cx - geo.tile_w / 2.0, cy - geo.tile_h / 2.0);
    let z = depth_key(at, elevation);
    Box::new(clickable(
        el("div", ())
            .attr("class", class)
            .attr("style", format!("left: {x}px; top: {y}px; z-index: {z};")),
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

/// Viewport culling margins (logical px). A tile is kept when its diamond,
/// elevation column, or standing sprite can still touch the pane, so the
/// margins are generous and asymmetric: a tile above the pane only pokes
/// down by half a diamond, while a tile below the pane can poke up by a
/// full elevation column plus a sprite. Over-emitting a thin ring is cheap;
/// clipping a visible tile is a bug.
const CULL_MARGIN_X: f32 = 32.0;
const CULL_MARGIN_ABOVE: f32 = 24.0;
const CULL_MARGIN_BELOW: f32 = 176.0;

/// Whether tile `at` can touch the board pane under the current camera, so
/// the whole-grid emitters only build elements the viewport can show. Until
/// the host reports a viewport (`(0, 0)`), this returns `true`, so an unset
/// viewport degrades to the pre-windowing "emit everything" behavior.
/// Battle-to-region scale is the design center; a pathologically tall tile
/// (elevation past ~18) below the pane edge is the one case the fixed bottom
/// margin does not cover, and that is outside the aesthetic.
fn in_view(ui: &UiState, at: TileCoord) -> bool {
    let (vw, vh) = ui.viewport;
    if vw <= 0.0 || vh <= 0.0 {
        return true;
    }
    let (bx, by) = ui.geo.tile_to_screen(at, 0);
    let px = bx + ui.camera.0;
    let py = by + ui.camera.1;
    px >= -CULL_MARGIN_X
        && px <= vw + CULL_MARGIN_X
        && py >= -CULL_MARGIN_ABOVE
        && py <= vh + CULL_MARGIN_BELOW
}

/// The ground layer: a column of filler diamonds up to the tile's
/// elevation, then the top diamond carrying the kind class. In Play
/// mode the selected token's reach tints blue and the hovered path
/// tints lighter still.
fn ground_tiles(ui: &UiState) -> Vec<UiChild> {
    let map = &ui.map;
    let playing = ui.mode == EditMode::Play;
    let path: HashSet<TileCoord> = if playing {
        ui.hover_tile
            .filter(|t| ui.reach.contains_key(t))
            .map(|t| path_to(&ui.reach, t).into_iter().collect())
            .unwrap_or_default()
    } else {
        HashSet::new()
    };
    let template: HashSet<TileCoord> = ui.template_preview();
    let doors = ui.door_tiles();
    let mut out: Vec<UiChild> = Vec::new();
    for (col, row, kind) in map.ground.iter() {
        if kind.0 == 0 {
            continue;
        }
        let at: TileCoord = (col as i32, row as i32);
        if !in_view(ui, at) {
            continue; // outside the pane: windowing skips the emit
        }
        let fog = ui.fog_level(at);
        if fog == FogLevel::Hidden {
            continue; // unexplored: the dark pane shows through
        }
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
        if playing {
            if path.contains(&at) {
                class.push_str(" tile-path");
            } else if ui.reach.contains_key(&at) {
                class.push_str(" tile-reach");
            }
        }
        if template.contains(&at) {
            class.push_str(" tile-template");
        }
        // A transition point: walk onto it and you are on the other map.
        if doors.contains(&at) {
            class.push_str(" tile-door");
        }
        out.push(tile_el(ui, at, elev, class));
        if fog == FogLevel::Dim {
            out.push(shroud_el(ui, at, elev)); // remembered terrain, dimmed
        }
    }
    out
}

/// A dim overlay diamond over an explored-but-unseen tile.
fn shroud_el(ui: &UiState, at: TileCoord, elev: i32) -> UiChild {
    let geo = &ui.geo;
    let (cx, cy) = geo.tile_to_screen(at, elev);
    let (x, y) = (cx - geo.tile_w / 2.0, cy - geo.tile_h / 2.0);
    let z = depth_key(at, elev) + 2;
    Box::new(
        el("div", ())
            .attr("class", "fog-shroud")
            .attr("style", format!("left: {x}px; top: {y}px; z-index: {z};")),
    )
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
        if !in_view(ui, at) {
            continue;
        }
        if ui.fog_level(at) == FogLevel::Hidden {
            continue;
        }
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
    let mut class = format!("token token-{}", token.sprite);
    // Equipment appearance remains pack CSS: public layer keys become stable
    // token classes, while the future voxel compositor can replace the same
    // projection with a baked recipe without changing campaign data.
    if let Some(inventory) = ui.inventories.get(&token.id) {
        for item_id in inventory.equipped.values() {
            if let Some(item) = inventory.items.get(item_id) {
                for layer in item.appearance_layers() {
                    class.push_str(" token-layer-");
                    class.push_str(&layer_class(layer));
                }
            }
        }
    }
    // One drawn side, mirrored for the other two facings (the GBA
    // economy): E/N flip, S/W stay.
    if matches!(
        token.facing,
        isometry_core::Facing::East | isometry_core::Facing::North
    ) {
        class.push_str(" token-flip");
    }
    let id = token.id;

    // A1: the beat rides on a wrapper, not on the sprite. `.token-flip` already
    // owns the sprite's `transform` for facing, and a CSS animation on
    // `transform` outranks a normal declaration, so a beat on the same box would
    // strip a west/south-facing token of its mirror mid-swing. Two boxes: the
    // wrapper is placed and beats, the sprite inside is drawn and flipped. They
    // compose instead of fighting.
    let mut wrapper = "beat".to_owned();
    if let Some(beat) = ui.beats.get(&id) {
        wrapper.push_str(" beat-");
        wrapper.push_str(beat);
    }
    let down = ui.map.is_defeated(id);
    if down {
        wrapper.push_str(" beat-down");
    }
    // Conditions render as classes, like beats and equipment layers, so a pack
    // can style `cond-prone` the way it styles a swing.
    if let Some(conditions) = ui.map.conditions.get(&id) {
        for name in conditions {
            wrapper.push_str(" cond-");
            wrapper.push_str(name);
        }
    }
    // A corpse is not a target, so it does not offer itself as one.
    if ui.picking_target() && !down {
        wrapper.push_str(" beat-targetable");
    }
    let sprite: Vec<UiChild> = vec![Box::new(el("div", ()).attr("class", class))];
    Box::new(clickable(
        el("div", sprite)
            .attr("class", wrapper)
            .attr("style", format!("left: {x}px; top: {y}px; z-index: {z};")),
        move |ui: &mut UiState, _| {
            // In target-pick mode a click on a token names the victim rather
            // than selecting it.
            if ui.picking_target() {
                ui.pick_action_target(id);
            } else {
                ui.click_token(id);
            }
        },
    ))
}

/// Make a pack layer key safe for the CSS-class vocabulary. Different raw
/// punctuation collapses intentionally: appearance keys are authored ids, and
/// pack validation can reject collisions once packs gain a formal manifest.
fn layer_class(key: &str) -> String {
    key.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod layer_class_tests {
    use super::layer_class;

    #[test]
    fn equipment_layer_keys_map_to_stable_css_classes() {
        assert_eq!(layer_class("effect:Flame"), "effect-flame");
        assert_eq!(layer_class("weapon/longsword +1"), "weapon-longsword--1");
    }
}

/// A ground marker diamond under a token (turn-active gold, selection
/// green), one depth step above the tile it stands on.
fn marker_el(ui: &UiState, token_id: isometry_core::TokenId, class: &str) -> Option<UiChild> {
    let token = ui.map.token(token_id)?;
    let elev = *ui
        .map
        .elevation
        .get(token.at.0.max(0) as u32, token.at.1.max(0) as u32)
        .unwrap_or(&0) as i32;
    let (cx, cy) = ui.geo.tile_to_screen(token.at, elev);
    let z = depth_key(token.at, elev) + 1;
    let (x, y) = (cx - 14.0, cy - 7.0);
    Some(Box::new(
        el("div", ())
            .attr("class", class.to_owned())
            .attr("style", format!("left: {x}px; top: {y}px; z-index: {z};")),
    ))
}

/// One menu row per active condition: clicking asks the host to clear it.
fn condition_items(ui: &UiState, id: isometry_core::TokenId) -> Vec<UiChild> {
    ui.map
        .conditions
        .get(&id)
        .map(|set| {
            set.iter()
                .map(|name| {
                    let name = name.clone();
                    let label = format!("Clear: {name}");
                    Box::new(clickable(
                        el("div", text(label)).attr("class", "menu-item"),
                        move |ui: &mut UiState, _| {
                            ui.clear_condition_request = Some((id, name.clone()));
                            ui.close_context_menu();
                        },
                    )) as UiChild
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One menu row per emote the *packs* offer.
///
/// The app no longer owns this vocabulary. A pack declares which beats are
/// emotable (a `name` plus an `emote` label in its manifest) and draws them, so
/// a campaign can add a rude gesture or remove one without the app knowing.
fn emote_items(ui: &UiState, id: isometry_core::TokenId) -> Vec<UiChild> {
    ui.emotes
        .iter()
        .map(|(beat, label)| {
            let beat = beat.clone();
            Box::new(clickable(
                el("div", text(label.clone())).attr("class", "menu-item menu-emote"),
                move |ui: &mut UiState, _| ui.emote(id, &beat),
            )) as UiChild
        })
        .collect()
}

/// The right-click context menu, or `None` when closed. An absolutely-
/// positioned card at the click position (pane-local px) with token actions.
fn context_menu_overlay(ui: &UiState) -> Option<UiChild> {
    let (id, (mx, my)) = ui.context_menu?;
    let token = ui.map.token(id)?;
    let title = format!("{} {}", token.sprite, id.0);
    Some(Box::new(
        el(
            "div",
            (
                el("div", text(title)).attr("class", "menu-title"),
                clickable(
                    el("div", text("Sheet")).attr("class", "menu-item"),
                    |ui: &mut UiState, _| {
                        ui.open_or_bind_sheet();
                        ui.close_context_menu();
                    },
                ),
                clickable(
                    el("div", text("End turn")).attr("class", "menu-item"),
                    |ui: &mut UiState, _| {
                        ui.end_turn();
                        ui.close_context_menu();
                    },
                ),
                // Emotes: the same beat primitive combat uses, with no
                // resolution behind it. A player may throw one for themselves.
                emote_items(ui, id),
                // One "shake off <condition>" row per active condition. The
                // click only *asks*; the host recomputes what the token can do.
                condition_items(ui, id),
                clickable(
                    el("div", text("Remove")).attr("class", "menu-item"),
                    move |ui: &mut UiState, _| ui.remove_token(id),
                ),
                clickable(
                    el("div", text("Close")).attr("class", "menu-item"),
                    |ui: &mut UiState, _| ui.close_context_menu(),
                ),
            ),
        )
        .attr("class", "context-menu")
        .attr("style", format!("left: {mx}px; top: {my}px;")),
    ))
}

/// The screen root the runner diffs.
pub fn board_root(ui: &UiState) -> UiChild {
    let mut layers: Vec<UiChild> = ground_tiles(ui);
    layers.extend(prop_tiles(ui));
    // Markers and tokens follow fog: a marker only shows on a token the
    // viewer can currently see.
    let marker_shown = |id: isometry_core::TokenId| {
        ui.map
            .token(id)
            .map(|t| ui.token_visible(t))
            .unwrap_or(false)
    };
    if let Some(id) = ui.selected_token {
        if marker_shown(id) {
            layers.extend(marker_el(ui, id, "marker marker-select"));
        }
    }
    if let Some(active) = ui.turns.active() {
        if marker_shown(active) {
            layers.extend(marker_el(ui, active, "marker marker-turn"));
        }
    }
    layers.extend(
        ui.map
            .tokens
            .iter()
            .filter(|t| ui.token_visible(t))
            .map(|t| token_el(ui, t)),
    );
    // Windowing metric: with `ISOMETRY_PROFILE` on, report how many
    // elements the viewport emits. It should stay bounded by the pane, not
    // grow with the board (see the windowing plan).
    if std::env::var_os("ISOMETRY_PROFILE").is_some() {
        eprintln!("[isometry] board elements emitted: {}", layers.len());
    }
    let (camx, camy) = ui.camera;
    let mut pane_children: Vec<UiChild> = vec![Box::new(
        el("div", layers)
            .attr("class", "board")
            .attr("style", format!("left: {camx}px; top: {camy}px;")),
    )];
    if let Some(overlay) = crate::sheet::sheet_overlay(ui) {
        pane_children.push(overlay);
    }
    if let Some(overlay) = crate::compendium::compendium_overlay(ui) {
        pane_children.push(overlay);
    }
    if let Some(overlay) = crate::generator::generator_overlay(ui) {
        pane_children.push(overlay);
    }
    if let Some(overlay) = crate::storylet::storylet_overlay(ui) {
        pane_children.push(overlay);
    }
    if let Some(overlay) = crate::governance::governance_overlay(ui) {
        pane_children.push(overlay);
    }
    if let Some(menu) = context_menu_overlay(ui) {
        pane_children.push(menu);
    }
    Box::new(
        el(
            "div",
            (
                side_panel(ui),
                el("div", pane_children).attr("class", "pane"),
            ),
        )
        .attr("class", "app"),
    )
}
