//! Host-only preview surface for W2 generator records.

use isometry_campaign::GenValue;
use xilem_serval::{clickable, el, text};

use crate::board::UiChild;
use crate::state::UiState;

fn proposal_label(value: &GenValue) -> String {
    match value {
        GenValue::Text { value } => value.clone(),
        GenValue::Object { .. } => "object proposal".to_owned(),
        GenValue::List { values } => format!("{} proposed values", values.len()),
        GenValue::Item { item } => format!("Item: {} ({})", item.name, item.template),
        GenValue::Npc { npc } => format!("NPC: {} ({})", npc.name, npc.key),
        GenValue::MapPatch { patch } => {
            format!("Map patch: {} ({} operations)", patch.target, patch.operations.len())
        }
        GenValue::WorldFact { fact } => format!("Fact: {}", fact.text),
        GenValue::Storylet { storylet } => format!("Storylet: {}", storylet.entry),
    }
}

/// The first preview-table slice uses the bundled demo item generator. The
/// panel is still record-driven: generator execution and commit live in the
/// desktop host, and player clients never receive this draft state.
pub fn generator_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.generator_open {
        return None;
    }
    let locked = ui.generator_locks.contains_key("culture");
    let lock_label = if locked {
        "Unlock culture"
    } else {
        "Lock culture"
    };
    let mut actions: Vec<UiChild> = vec![Box::new(clickable(
        el::<_, UiState, ()>("span", text("close")).attr("class", "btn btn-mini"),
        |ui: &mut UiState, _| ui.close_generator(),
    ))];
    let body: Vec<UiChild> = match &ui.generator_preview {
        Some(record) => {
            actions.insert(
                0,
                Box::new(clickable(
                    el::<_, UiState, ()>("span", text("commit")).attr("class", "btn btn-mini"),
                    |ui: &mut UiState, _| ui.commit_generation_preview(),
                )),
            );
            vec![
                Box::new(el("div", text(record.request.generator.clone())).attr("class", "entry-sub")),
                Box::new(el("div", text(proposal_label(&record.proposal))).attr("class", "generator-proposal")),
                Box::new(el("div", text(format!("entropy: {}", record.entropy))).attr("class", "side-line")),
                Box::new(
                    el(
                        "div",
                        (
                            clickable(
                                el("div", text("Reroll")).attr("class", "btn"),
                                |ui: &mut UiState, _| ui.request_generation(),
                            ),
                            clickable(
                                el("div", text(lock_label)).attr("class", "btn"),
                                |ui: &mut UiState, _| ui.toggle_demo_culture_lock(),
                            ),
                            clickable(
                                el("div", text("Discard")).attr("class", "btn"),
                                |ui: &mut UiState, _| ui.discard_generation_preview(),
                            ),
                        ),
                    )
                    .attr("class", "btn-row"),
                ),
            ]
        }
        None => vec![
            Box::new(
                el("div", text("Demo forge-item generator")).attr("class", "entry-sub"),
            ),
            Box::new(
                el(
                    "div",
                    (
                        clickable(
                            el("div", text("Generate")).attr("class", "btn"),
                            |ui: &mut UiState, _| ui.request_generation(),
                        ),
                        clickable(
                            el("div", text(lock_label)).attr("class", "btn"),
                            |ui: &mut UiState, _| ui.toggle_demo_culture_lock(),
                        ),
                    ),
                )
                .attr("class", "btn-row"),
            ),
        ],
    };
    Some(crate::widgets::overlay_panel(
        "generator",
        "Generate".to_owned(),
        actions,
        body,
    ))
}
