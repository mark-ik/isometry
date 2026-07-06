//! The side panel: map facts, edit modes, the tile-kind palette, and
//! undo/redo/save/load. Every control is a clickable div; the palette
//! swatches take their color from the same `tile-<kind>` classes the
//! board uses, so the palette can never drift from the tileset.

use isometry_core::TileKindId;
use xilem_serval::{clickable, el, text};

use crate::board::UiChild;
use crate::state::{EditMode, UiState};

fn mode_button(mode: EditMode, active: bool) -> UiChild {
    let class = if active { "btn btn-active" } else { "btn" };
    Box::new(clickable(
        el("div", text(mode.label())).attr("class", class),
        move |ui: &mut UiState, _| {
            ui.mode = mode;
            ui.status = format!("mode: {}", mode.label());
        },
    ))
}

fn swatch(ui: &UiState, kind: TileKindId, name: &str) -> UiChild {
    let mut class = format!("swatch tile-{name}");
    if ui.brush == kind {
        class.push_str(" swatch-active");
    }
    let label = name.to_owned();
    Box::new(clickable(
        el("div", ()).attr("class", class),
        move |ui: &mut UiState, _| {
            ui.brush = kind;
            ui.status = format!("brush: {label}");
        },
    ))
}

fn action_button(label: &'static str, enabled: bool, act: fn(&mut UiState)) -> UiChild {
    let class = if enabled { "btn" } else { "btn btn-dim" };
    Box::new(clickable(
        el("div", text(label)).attr("class", class),
        move |ui: &mut UiState, _| act(ui),
    ))
}

pub fn side_panel(ui: &UiState) -> UiChild {
    let modes: Vec<UiChild> = EditMode::ALL
        .iter()
        .map(|&m| mode_button(m, m == ui.mode))
        .collect();
    let swatches: Vec<UiChild> = ui
        .map
        .tile_kinds
        .iter()
        .enumerate()
        .map(|(i, name)| swatch(ui, TileKindId(i as u16), name))
        .collect();
    let selected = match ui.selected {
        Some((c, r)) => {
            let elev = *ui
                .map
                .elevation
                .get(c.max(0) as u32, r.max(0) as u32)
                .unwrap_or(&0);
            format!("selected: ({c}, {r}) h{elev}")
        }
        None => "selected: none".to_owned(),
    };
    Box::new(el(
        "div",
        (
            el("div", text("Isometry")).attr("class", "side-title"),
            el("div", text(ui.map.name.clone())).attr("class", "side-line"),
            el(
                "div",
                text(format!(
                    "{}x{} board, {} tokens",
                    ui.map.ground.width(),
                    ui.map.ground.height(),
                    ui.map.tokens.len()
                )),
            )
            .attr("class", "side-line"),
            el("div", text(selected)).attr("class", "side-line side-strong"),
            el("div", text("Mode")).attr("class", "side-heading"),
            el("div", modes).attr("class", "btn-row"),
            el("div", text("Brush")).attr("class", "side-heading"),
            el("div", swatches).attr("class", "swatch-row"),
            el("div", text("Map")).attr("class", "side-heading"),
            el(
                "div",
                (
                    action_button("Undo", ui.can_undo(), |ui| ui.undo()),
                    action_button("Redo", ui.can_redo(), |ui| ui.redo()),
                    action_button("Save", true, |ui| ui.save_requested = true),
                    action_button("Load", true, |ui| ui.load_requested = true),
                ),
            )
            .attr("class", "btn-row"),
            el("div", text(ui.status.clone())).attr("class", "side-status"),
            el("div", text("arrows: pan / ctrl+z, ctrl+y")).attr("class", "side-hint"),
        ),
    )
    .attr("class", "side"))
}
