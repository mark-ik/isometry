//! Host-only preview surface for W2 generator records.

use isometry_campaign::GenValue;
use cambium::{clickable, el, text};

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
            format!(
                "Map patch: {} ({} operations)",
                patch.target,
                patch.operations.len()
            )
        }
        GenValue::WorldFact { fact } => format!("Fact: {}", fact.text),
        GenValue::Storylet { storylet } => format!("Storylet: {}", storylet.entry),
        GenValue::LocalMap { map } => {
            format!("Map: {} ({}x{})", map.name, map.width, map.height)
        }
        GenValue::Campaign { campaign } => format!(
            "Campaign: {} ({} maps, {} factions, {} secrets)",
            campaign.name,
            campaign.maps.len(),
            campaign.world.factions.len(),
            campaign.secrets.len()
        ),
    }
}

fn proposal_details(value: &GenValue) -> Vec<String> {
    let GenValue::Campaign { campaign } = value else {
        return Vec::new();
    };
    let mut details = vec![
        format!("Start: {}", campaign.starting_map),
        format!("Finale: {}", campaign.final_storylet),
    ];
    details.extend(
        campaign
            .world
            .factions
            .values()
            .map(|faction| format!("Faction: {} [{}]", faction.name, faction.tags.join(", "))),
    );
    details.extend(campaign.maps.iter().map(|map| {
        format!(
            "Map: {} ({:?}, {}x{})",
            map.map.name, map.scale, map.map.width, map.map.height
        )
    }));
    details.extend(
        campaign
            .world
            .laws
            .values()
            .map(|law| format!("Law: {} - {}", law.name, law.text)),
    );
    details.extend(
        campaign
            .world
            .history
            .iter()
            .map(|event| format!("History {}: {}", event.time, event.text)),
    );
    details.extend(
        campaign
            .rewards
            .iter()
            .map(|item| format!("Reward: {}", item.name)),
    );
    details.extend(
        campaign
            .secrets
            .iter()
            .map(|secret| format!("GM secret: {}", secret.text)),
    );
    details
}

/// The first preview-table slice uses the bundled demo item generator. The
/// panel is still record-driven: generator execution and commit live in the
/// desktop host, and player clients never receive this draft state.
pub fn generator_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.generator_open {
        return None;
    }
    let choice = ui.selected_generator()?;
    let preset = choice.lock_presets.first();
    let locked = preset.is_some_and(|preset| ui.generator_locks.contains_key(&preset.key));
    let lock_label = match (preset, locked) {
        (Some(preset), true) => format!("Unlock {}", preset.label),
        (Some(preset), false) => format!("Lock {}", preset.label),
        (None, _) => "No lock presets".to_owned(),
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
            let mut body: Vec<UiChild> = vec![
                Box::new(
                    el("div", text(record.request.generator.clone())).attr("class", "entry-sub"),
                ),
                Box::new(
                    el("div", text(proposal_label(&record.proposal)))
                        .attr("class", "generator-proposal"),
                ),
                Box::new(
                    el("div", text(format!("entropy: {}", record.entropy)))
                        .attr("class", "side-line"),
                ),
            ];
            body.extend(
                proposal_details(&record.proposal)
                    .into_iter()
                    .map(|detail| {
                        Box::new(el("div", text(detail)).attr("class", "side-line")) as UiChild
                    }),
            );
            body.push(Box::new(
                el(
                    "div",
                    (
                        clickable(
                            el("div", text("Reroll")).attr("class", "btn"),
                            |ui: &mut UiState, _| ui.request_generation(),
                        ),
                        clickable(
                            el("div", text(lock_label.clone())).attr("class", "btn"),
                            |ui: &mut UiState, _| ui.toggle_generator_lock(),
                        ),
                        clickable(
                            el("div", text("Discard")).attr("class", "btn"),
                            |ui: &mut UiState, _| ui.discard_generation_preview(),
                        ),
                    ),
                )
                .attr("class", "btn-row"),
            ));
            body
        }
        None => vec![
            Box::new(el("div", text(choice.name.clone())).attr("class", "entry-sub")),
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
                            |ui: &mut UiState, _| ui.toggle_generator_lock(),
                        ),
                    ),
                )
                .attr("class", "btn-row"),
            ),
        ],
    };
    let mut body = body;
    body.insert(
        0,
        Box::new(clickable(
            el("div", text(format!("Generator: {}", choice.name))).attr("class", "btn"),
            |ui: &mut UiState, _| ui.cycle_generator(),
        )),
    );
    Some(crate::widgets::overlay_panel(
        "generator",
        "Generate".to_owned(),
        actions,
        body,
    ))
}
