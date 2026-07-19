//! The overmap surface (C8, exploration mode): the party's pointcrawl, drawn.
//!
//! The board above the tactical maps. The host projects the world's places and
//! routes into an `Overmap` (`CampaignWorld::overmap`), filtered to what the
//! party has discovered (`overmap_for`); this draws that graph as positioned,
//! clickable place markers with a route list, and lets the table click a place
//! to travel there. The click only *asks*; the host rolls the navigation, spends
//! the time, and moves the party (`resolve_travel` -> `TravelResolved`), so the
//! view never decides a trip's outcome.
//!
//! The nodes are laid out on a circle (the projection carries no positions yet;
//! an authored or force-directed layout is a later refinement). Cambium's catalog
//! `graph_canvas_swatch` is the eventual home for this -- a real graph canvas with
//! painted edges -- but it is a retained paint leaf, and wiring sprigging leaves
//! into the app's paint pipeline is provider-side work not yet done, so this
//! draws with the plain element path every other surface uses.

use cambium::{clickable, el, text};

use crate::board::UiChild;
use crate::state::UiState;

pub fn overmap_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.overmap_open {
        return None;
    }
    // Whose party? The viewer's; the DM (no viewer) watches the "dm" party.
    let party = ui.viewer.as_deref().unwrap_or("dm");
    // Only what the party has discovered: the overmap as it knows it (E6). The
    // unfound map is not drawn and cannot be clicked to travel to.
    let overmap = ui.world.overmap_for(party);
    let here = ui.world.party_at(party).map(str::to_owned);

    let actions: Vec<UiChild> = vec![
        Box::new(clickable(
            el::<_, UiState, ()>("span", text("study map")).attr("class", "btn btn-mini"),
            |ui: &mut UiState, _| ui.request_map_read(),
        )),
        Box::new(clickable(
            el::<_, UiState, ()>("span", text("close")).attr("class", "btn btn-mini"),
            |ui: &mut UiState, _| ui.close_overmap(),
        )),
    ];

    let mut body: Vec<UiChild> = Vec::new();
    if overmap.nodes.is_empty() {
        body.push(Box::new(
            el("div", text("no map yet — travel or ask around to find your way"))
                .attr("class", "side-hint"),
        ));
        return Some(crate::widgets::overlay_panel(
            "overmap",
            "Overmap".to_owned(),
            actions,
            body,
        ));
    }

    // Lay the places on a circle inside the canvas, each a clickable marker. The
    // party's current place is marked; clicking another arms a travel to it.
    let (w, h) = (420.0f32, 320.0f32);
    let (cx, cy, radius) = (w / 2.0, h / 2.0, h * 0.36);
    let count = overmap.nodes.len().max(1) as f32;
    let mut markers: Vec<UiChild> = Vec::new();
    for (i, node) in overmap.nodes.iter().enumerate() {
        let angle = (i as f32) / count * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let (x, y) = (cx + radius * angle.cos(), cy + radius * angle.sin());
        let is_here = here.as_deref() == Some(node.id.as_str());
        let mark = if is_here { "◉ " } else { "○ " };
        let class = if is_here {
            "overmap-node overmap-here"
        } else {
            "overmap-node"
        };
        let id = node.id.clone();
        markers.push(Box::new(clickable(
            el::<_, UiState, ()>("div", text(format!("{mark}{}", node.name)))
                .attr("class", class)
                .attr(
                    "style",
                    format!("position:absolute; left:{:.0}px; top:{:.0}px;", x, y),
                ),
            move |ui: &mut UiState, _| ui.request_travel(id.clone()),
        )));
    }
    body.push(Box::new(
        el("div", markers).attr("class", "overmap-canvas").attr(
            "style",
            format!("position:relative; width:{:.0}px; height:{:.0}px;", w, h),
        ),
    ));

    // The routes, listed, so the connectivity is legible without painted edges.
    let name_of = |id: &str| {
        overmap
            .node(id)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| id.to_owned())
    };
    for edge in &overmap.edges {
        body.push(Box::new(
            el(
                "div",
                text(format!(
                    "{} — {} ({})",
                    name_of(&edge.from),
                    name_of(&edge.to),
                    edge.weight
                )),
            )
            .attr("class", "side-line"),
        ));
    }

    // Pace (E1) and stance (E3): the party's marching orders. The chosen pace is
    // marked; a stance is set on the lead token by the host.
    let pace = ui.world.pace(party);
    let pace_row: Vec<UiChild> = [("Fast", 50i64), ("Normal", 100), ("Slow", 200)]
        .into_iter()
        .map(|(label, pct)| {
            let class = if pace == pct { "btn btn-attack" } else { "btn" };
            Box::new(clickable(
                el::<_, UiState, ()>("span", text(label)).attr("class", class),
                move |ui: &mut UiState, _| ui.request_pace(pct),
            )) as UiChild
        })
        .collect();
    body.push(Box::new(
        el("div", pace_row).attr("class", "overmap-controls"),
    ));

    let stance_row: Vec<UiChild> = [
        ("Scout", "scout"),
        ("Search", "search"),
        ("Forage", "forage"),
        ("Walk", ""),
    ]
    .into_iter()
    .map(|(label, stance)| {
        Box::new(clickable(
            el::<_, UiState, ()>("span", text(label)).attr("class", "btn"),
            move |ui: &mut UiState, _| ui.request_stance(stance),
        )) as UiChild
    })
    .collect();
    body.push(Box::new(
        el("div", stance_row).attr("class", "overmap-controls"),
    ));

    let hint = match &here {
        Some(node) => format!("here: {} — click a place to travel", name_of(node)),
        None => "the party is not on the overmap yet".to_owned(),
    };
    body.push(Box::new(
        el("div", text(hint)).attr("class", "side-hint"),
    ));

    Some(crate::widgets::overlay_panel(
        "overmap",
        "Overmap".to_owned(),
        actions,
        body,
    ))
}
