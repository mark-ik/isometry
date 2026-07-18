//! The downtime surface (C7, "factions as participants"): a DM-only table for
//! the faction turn.
//!
//! The host rolls a tick -- a move per faction, proportional to banked world
//! time -- and hands the view [`FactionMoveRow`]s. This only *displays* the
//! batch and lets the DM edit it: cycle the moves, strike the ones it does not
//! want, reroll for a fresh batch, and commit the keepers. Committing flattens
//! each kept move to ordinary world events on the host, which replicate like any
//! other. A joined client never rolls a tick (the roll reads the world and
//! spends host entropy), so it never receives this surface.
//!
//! The selected move renders through Cambium's catalog `summary_body`, isometry's
//! first use of a catalog composition here: a titled record (the verb) with the
//! faction as its eyebrow and the move's narration as its description.

use cambium::{clickable, el, summary_body, text, SummaryBody};

use crate::board::UiChild;
use crate::state::UiState;

pub fn downtime_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.downtime_open {
        return None;
    }
    let actions: Vec<UiChild> = vec![
        Box::new(clickable(
            el::<_, UiState, ()>("span", text("roll")).attr("class", "btn btn-mini"),
            |ui: &mut UiState, _| ui.reroll_downtime(),
        )),
        Box::new(clickable(
            el::<_, UiState, ()>("span", text("commit")).attr("class", "btn btn-mini"),
            |ui: &mut UiState, _| ui.commit_downtime(),
        )),
        Box::new(clickable(
            el::<_, UiState, ()>("span", text("close")).attr("class", "btn btn-mini"),
            |ui: &mut UiState, _| ui.close_downtime(),
        )),
    ];

    let mut body: Vec<UiChild> = Vec::new();
    if ui.faction_moves.is_empty() {
        body.push(Box::new(
            el("div", text("no faction stirred this tick")).attr("class", "side-hint"),
        ));
    } else {
        let total = ui.faction_moves.len();
        let kept = ui.faction_moves.iter().filter(|m| !m.struck).count();

        // The selector line cycles through the batch, marking each move kept or
        // struck, so the DM edits without leaving the one row.
        let selector = ui
            .selected_downtime_move()
            .map(|row| {
                let mark = if row.struck { "\u{2013}" } else { "\u{2022}" }; // – struck / • kept
                format!(
                    "Move {}/{total}: {mark} {} {}",
                    ui.downtime_selected + 1,
                    row.faction,
                    row.verb
                )
            })
            .unwrap_or_else(|| "Move".to_owned());
        body.push(Box::new(clickable(
            el("div", text(selector)).attr("class", "btn"),
            |ui: &mut UiState, _| ui.cycle_downtime(),
        )));

        if let Some(row) = ui.selected_downtime_move() {
            let summary = SummaryBody::new(
                format!("downtime-move-{}", ui.downtime_selected),
                row.verb.clone(),
            )
            .with_eyebrow(row.faction.clone())
            .with_description(row.text.clone())
            .with_fact(
                "effect",
                if row.has_change {
                    "reshapes the world"
                } else {
                    "a rumor only"
                },
            );
            body.push(Box::new(summary_body::<UiState, ()>(&summary)));

            let toggle_label = if row.struck {
                "keep this move"
            } else {
                "strike this move"
            };
            body.push(Box::new(clickable(
                el("div", text(toggle_label)).attr("class", "btn"),
                |ui: &mut UiState, _| ui.toggle_strike_downtime(),
            )));
        }

        body.push(Box::new(
            el("div", text(format!("{kept} of {total} kept"))).attr("class", "side-line"),
        ));
    }

    Some(crate::widgets::overlay_panel(
        "downtime",
        "Downtime".to_owned(),
        actions,
        body,
    ))
}
