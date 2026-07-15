//! The side panel: map facts, edit modes, the tile-kind palette, and
//! undo/redo/save/load. Every control is a clickable div; the palette
//! swatches take their color from the same `tile-<kind>` classes the
//! board uses, so the palette can never drift from the tileset.

use isometry_core::{TemplateKind, TileKindId, TokenId};
use cambium::{clickable, el, text};

use crate::board::UiChild;
use crate::state::{EditMode, UiState};

fn next_template_kind(k: TemplateKind) -> TemplateKind {
    let i = TemplateKind::ALL.iter().position(|&x| x == k).unwrap_or(0);
    TemplateKind::ALL[(i + 1) % TemplateKind::ALL.len()]
}

/// Messages: the whisper composer (start typing, cycle target) and the
/// message log.
fn messages_section(ui: &UiState) -> UiChild {
    let target = ui
        .whisper_target
        .clone()
        .unwrap_or_else(|| "table".to_owned());
    let draft = if ui.composing {
        format!("> {}_", ui.whisper_draft)
    } else {
        "(w to whisper)".to_owned()
    };
    Box::new(el(
        "div",
        (
            el(
                "div",
                (
                    clickable(
                        el("div", text("Whisper")).attr("class", "btn"),
                        |ui: &mut UiState, _| ui.start_compose(),
                    ),
                    clickable(
                        el("div", text(format!("to: {target}"))).attr("class", "btn"),
                        |ui: &mut UiState, _| ui.cycle_whisper_target(),
                    ),
                ),
            )
            .attr("class", "btn-row"),
            el("div", text(draft)).attr("class", "roll-line"),
            el(
                "div",
                ui.messages
                    .iter()
                    .rev()
                    .take(5)
                    .map(|m| {
                        Box::new(el("div", text(m.clone())).attr("class", "roll-line")) as UiChild
                    })
                    .collect::<Vec<UiChild>>(),
            )
            .attr("class", "roll-log"),
        ),
    )
    .attr("class", "messages"))
}

/// Measure controls: template shape toggle, size stepper, and the
/// distance readout (from the clicked anchor to the hovered tile).
fn measure_controls(ui: &UiState) -> UiChild {
    let dist = match ui.measured_distance() {
        Some(d) => format!("size {} · dist {d}", ui.template_size),
        None => format!("size {} · dist -", ui.template_size),
    };
    Box::new(el(
        "div",
        (
            el(
                "div",
                (
                    clickable(
                        el("div", text(format!("tpl: {}", ui.template_kind.label())))
                            .attr("class", "btn"),
                        |ui: &mut UiState, _| {
                            ui.template_kind = next_template_kind(ui.template_kind);
                        },
                    ),
                    clickable(
                        el("div", text("-")).attr("class", "btn btn-mini"),
                        |ui: &mut UiState, _| {
                            ui.template_size = ui.template_size.saturating_sub(1).max(1);
                        },
                    ),
                    clickable(
                        el("div", text("+")).attr("class", "btn btn-mini"),
                        |ui: &mut UiState, _| {
                            ui.template_size = (ui.template_size + 1).min(12);
                        },
                    ),
                ),
            )
            .attr("class", "btn-row"),
            el("div", text(dist)).attr("class", "side-line"),
        ),
    )
    .attr("class", "measure"))
}

const TOKEN_SPRITES: [&str; 2] = ["knight", "goblin"];

fn sprite_swatch(ui: &UiState, sprite: &'static str) -> UiChild {
    let mut class = format!("swatch sprite-swatch token-{sprite}");
    if ui.token_sprite == sprite {
        class.push_str(" swatch-active");
    }
    Box::new(clickable(
        el("div", ()).attr("class", class),
        move |ui: &mut UiState, _| {
            ui.token_sprite = sprite.to_owned();
            ui.status = format!("token brush: {sprite}");
        },
    ))
}

/// One turn-list row: click selects the token, the trailing toggle
/// moves it in or out of the turn order (out = free movement).
fn turn_row(ui: &UiState, id: TokenId) -> UiChild {
    let token = ui.map.token(id).expect("row for a live token");
    let listed = ui.turns.contains(id);
    let active = ui.turns.active() == Some(id);
    let mut class = "turn-row".to_owned();
    if active {
        class.push_str(" turn-row-active");
    }
    if ui.selected_token == Some(id) {
        class.push_str(" turn-row-selected");
    }
    let owner = token.owner.as_deref().unwrap_or("dm");
    let label = format!(
        "{}{} {} ({owner})",
        if active { "> " } else { "" },
        token.sprite,
        id.0
    );
    Box::new(el(
        "div",
        (
            clickable(
                el("div", text(label)).attr("class", "turn-label"),
                move |ui: &mut UiState, _| ui.select_token(id),
            ),
            clickable(
                el("div", text(if listed { "out" } else { "in" }))
                    .attr("class", "btn btn-mini"),
                move |ui: &mut UiState, _| ui.toggle_turn(id),
            ),
        ),
    )
    .attr("class", class))
}

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

const DICE: [(&str, &str); 7] = [
    ("d20", "1d20"),
    ("d12", "1d12"),
    ("d10", "1d10"),
    ("d8", "1d8"),
    ("d6", "1d6"),
    ("d4", "1d4"),
    ("2d6", "2d6"),
];

fn dice_button(label: &'static str, expr: &'static str) -> UiChild {
    Box::new(clickable(
        el("div", text(label)).attr("class", "btn btn-mini"),
        move |ui: &mut UiState, _| ui.roll_dice(expr),
    ))
}

/// Initiative controls: a mode toggle (individual / side) and a roll.
fn init_controls(ui: &UiState) -> UiChild {
    Box::new(
        el(
            "div",
            (
                clickable(
                    el("div", text(format!("init: {}", ui.initiative_mode.label())))
                        .attr("class", "btn"),
                    |ui: &mut UiState, _| {
                        ui.initiative_mode = ui.initiative_mode.toggled();
                        ui.status = format!("initiative: {}", ui.initiative_mode.label());
                    },
                ),
                clickable(
                    el("div", text("Roll init")).attr("class", "btn"),
                    |ui: &mut UiState, _| ui.roll_initiative(),
                ),
            ),
        )
        .attr("class", "btn-row"),
    )
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
    let children: Vec<UiChild> = vec![
        Box::new(el("div", text("Isometry")).attr("class", "side-title")),
        Box::new(el("div", text(ui.map.name.clone())).attr("class", "side-line")),
        Box::new(
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
        ),
        Box::new(el("div", text(selected)).attr("class", "side-line side-strong")),
        Box::new(el("div", text("Mode")).attr("class", "side-heading")),
        Box::new(el("div", modes).attr("class", "btn-row")),
        Box::new(el("div", text("Brush")).attr("class", "side-heading")),
        Box::new(el("div", swatches).attr("class", "swatch-row")),
        Box::new(el("div", text("Map")).attr("class", "side-heading")),
        Box::new(
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
        ),
        Box::new(el("div", text("Tokens")).attr("class", "side-heading")),
        Box::new(
            el(
                "div",
                TOKEN_SPRITES
                    .iter()
                    .map(|s| sprite_swatch(ui, s))
                    .collect::<Vec<UiChild>>(),
            )
            .attr("class", "swatch-row"),
        ),
        Box::new(el("div", text("Turns")).attr("class", "side-heading")),
        // The location's time: round from the turn order, ticks from the clock.
        // Only meaningful once a campaign map is active; a bare board keeps no
        // clock, so this line stays quiet there.
        Box::new(
            el(
                "div",
                text(if ui.active_map.is_some() {
                    format!(
                        "round {} · time {} ({})",
                        ui.turns.round() + 1,
                        ui.clock_now(),
                        ui.active_map.as_deref().unwrap_or("?"),
                    )
                } else {
                    format!("round {}", ui.turns.round() + 1)
                }),
            )
            .attr("class", "side-line"),
        ),
        Box::new(
            el(
                "div",
                ui.map
                    .tokens
                    .iter()
                    .map(|t| turn_row(ui, t.id))
                    .collect::<Vec<UiChild>>(),
            )
            .attr("class", "turn-list"),
        ),
        Box::new(
            el(
                "div",
                (
                    action_button("End turn", true, |ui| ui.end_turn()),
                    // The downtime verb: rounds tick the clock by themselves;
                    // these are for the stretches no turn order measures. DM
                    // controls, like the other authoring buttons.
                    action_button("+1 time", ui.can_edit_inventory, |ui| ui.pass_time(1)),
                    action_button("+10 time", ui.can_edit_inventory, |ui| ui.pass_time(10)),
                ),
            )
            .attr("class", "btn-row"),
        ),
        init_controls(ui),
        Box::new(
            el(
                "div",
                (
                    action_button("Sheet", true, |ui| ui.open_or_bind_sheet()),
                    action_button("Bestiary", true, |ui| ui.open_compendium()),
                    action_button("Generate", ui.can_edit_inventory, |ui| ui.open_generator()),
                    action_button("Resolve", ui.governance_conflict.is_some(), |ui| {
                        ui.open_governance_conflict()
                    }),
                ),
            )
            .attr("class", "btn-row"),
        ),
        Box::new(el("div", text("Dice")).attr("class", "side-heading")),
        Box::new(
            el(
                "div",
                DICE.iter()
                    .map(|(label, expr)| dice_button(label, expr))
                    .collect::<Vec<UiChild>>(),
            )
            .attr("class", "btn-row"),
        ),
        Box::new(
            el(
                "div",
                ui.roll_log
                    .iter()
                    .rev()
                    .take(5)
                    .map(|r| {
                        Box::new(
                            el(
                                "div",
                                text(format!("{}: {} = {}", r.by, r.expr, r.total)),
                            )
                            .attr("class", "roll-line"),
                        ) as UiChild
                    })
                    .collect::<Vec<UiChild>>(),
            )
            .attr("class", "roll-log"),
        ),
        Box::new(el("div", text("Measure")).attr("class", "side-heading")),
        measure_controls(ui),
        Box::new(el("div", text("Messages")).attr("class", "side-heading")),
        messages_section(ui),
        Box::new(el("div", text(ui.status.clone())).attr("class", "side-status")),
        Box::new(
            el(
                "div",
                text("arrows: pan / r: face / enter: end turn / f: fog view"),
            )
            .attr("class", "side-hint"),
        ),
    ];
    Box::new(el("div", children).attr("class", "side"))
}
