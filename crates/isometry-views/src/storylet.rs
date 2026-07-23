//! The storylet surface (C6, "dialogue"): a DM-only menu of the campaign's
//! narrative opportunities.
//!
//! A storylet is a quality-based scene — an entry line, requirements, cast
//! roles, and effects. The host resolves each against the current world and its
//! private secrets and hands the view [`StoryletRow`]s; this only *displays*
//! them and lets the DM play a playable one. Committing its effects (facts,
//! history, items, maps) is the host's, and they replicate like any other event.
//! Player clients never receive this state, because matching reads secret facts.

use cambium::{clickable, el, text};

use crate::board::UiChild;
use crate::state::UiState;

pub fn storylet_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.storylet_open {
        return None;
    }
    let actions: Vec<UiChild> = vec![Box::new(clickable(
        el::<_, UiState, ()>("span", text("close")).attr("class", "btn btn-mini"),
        |ui: &mut UiState, _| ui.close_storylets(),
    ))];

    let mut body: Vec<UiChild> = Vec::new();
    if ui.storylets.is_empty() {
        body.push(Box::new(
            el("div", text("no storylets in this campaign yet")).attr("class", "side-hint"),
        ));
    } else {
        // The selector line: cycle through storylets, showing which is playable.
        let selected = ui.selected_storylet();
        let selector = selected
            .map(|row| {
                let mark = if row.available {
                    "\u{2022}"
                } else {
                    "\u{2013}"
                }; // • / –
                format!(
                    "Storylet {}: {mark} {}",
                    ui.storylet_selected + 1,
                    row.entry
                )
            })
            .unwrap_or_else(|| "Storylet".to_owned());
        body.push(Box::new(clickable(
            el("div", text(selector)).attr("class", "btn"),
            |ui: &mut UiState, _| ui.cycle_storylet(),
        )));

        if let Some(row) = selected {
            body.push(Box::new(
                el("div", text(row.entry.clone())).attr("class", "storylet-entry"),
            ));
            // The cast: who fills each role.
            for (role, character) in &row.cast {
                body.push(Box::new(
                    el("div", text(format!("{role}: {character}"))).attr("class", "side-line"),
                ));
            }
            if row.available {
                body.push(Box::new(clickable(
                    el("div", text("Play")).attr("class", "btn btn-attack"),
                    |ui: &mut UiState, _| ui.play_storylet(),
                )));
            } else {
                // Why it is locked, so the DM knows what unlocks it.
                body.push(Box::new(
                    el("div", text(format!("locked: {}", row.status))).attr("class", "side-hint"),
                ));
            }
        }
    }

    Some(crate::widgets::overlay_panel(
        "storylet",
        "Storylets".to_owned(),
        actions,
        body,
    ))
}
