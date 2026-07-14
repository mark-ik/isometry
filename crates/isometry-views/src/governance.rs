//! Host-fed resolution surface for competing campaign-governance bindings.

use cambium::{clickable, el, text};

use crate::board::UiChild;
use crate::state::{GovernanceBindingRow, UiState};

fn short_id(id: [u8; 32]) -> String {
    id[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

fn candidate_row(candidate: &GovernanceBindingRow, index: usize, selected: bool) -> UiChild {
    let class = if selected {
        "governance-row governance-row-selected"
    } else {
        "governance-row"
    };
    Box::new(clickable(
        el(
            "div",
            (
                el("div", text(candidate.moot.clone())).attr("class", "governance-moot"),
                el(
                    "div",
                    text(format!(
                        "{} | {}",
                        candidate.policy,
                        short_id(candidate.proposal)
                    )),
                )
                .attr("class", "governance-policy"),
                el(
                    "div",
                    text(format!(
                        "endorsements {}/{} | claims {}",
                        candidate.endorsements, candidate.required, candidate.claims
                    )),
                )
                .attr("class", "governance-counts"),
            ),
        )
        .attr("class", class),
        move |ui: &mut UiState, _| ui.select_governance_candidate(index),
    ))
}

pub fn governance_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.governance_conflict_open {
        return None;
    }
    let conflict = ui.governance_conflict.as_ref()?;
    let mut body: Vec<UiChild> = conflict
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| candidate_row(candidate, index, index == ui.governance_selected))
        .collect();
    if let Some(restriction) = &conflict.restriction {
        body.push(Box::new(
            el("div", text(restriction.clone())).attr("class", "governance-restriction"),
        ));
    }
    body.push(Box::new(
        el(
            "div",
            (
                clickable(
                    el("div", text("Adopt selected")).attr(
                        "class",
                        if conflict.can_adopt {
                            "btn"
                        } else {
                            "btn btn-dim"
                        },
                    ),
                    |ui: &mut UiState, _| ui.request_governance_adopt(),
                ),
                clickable(
                    el("div", text("Branch all")).attr(
                        "class",
                        if conflict.can_branch {
                            "btn"
                        } else {
                            "btn btn-dim"
                        },
                    ),
                    |ui: &mut UiState, _| ui.request_governance_branch(),
                ),
            ),
        )
        .attr("class", "btn-row governance-actions"),
    ));

    Some(crate::widgets::overlay_panel(
        "governance",
        "Campaign binding conflict".to_owned(),
        vec![Box::new(clickable(
            el::<_, UiState, ()>("span", text("close")).attr("class", "btn btn-mini"),
            |ui: &mut UiState, _| ui.close_governance_conflict(),
        ))],
        body,
    ))
}
