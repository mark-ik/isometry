//! The overmap surface (C8, exploration mode): the party's pointcrawl, drawn.
//!
//! The board above the tactical maps. The host projects the world's places and
//! routes into an `Overmap` (`CampaignWorld::overmap`); this draws that graph
//! through Cambium's catalog `graph_canvas_swatch` -- sites and waypoints as
//! nodes, routes as edges, the party's current node highlighted -- and lets the
//! table click a place to travel there. The click only *asks*; the host rolls
//! the navigation, spends the time, and moves the party (`resolve_travel` ->
//! `TravelResolved`), so the view never decides a trip's outcome.
//!
//! Node positions are laid out on a circle here, because the projection sets
//! none yet; an authored or force-directed layout is a later refinement.

use cambium::{
    clickable, el, graph_canvas_swatch, text, GraphCanvasEdge, GraphCanvasNode,
    GraphCanvasSubgraph, GraphCanvasSwatch,
};

use crate::board::UiChild;
use crate::state::UiState;

/// A stable retained-leaf key for the overmap canvas (C8).
const OVERMAP_KEY: u64 = 0xC8;

pub fn overmap_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.overmap_open {
        return None;
    }
    let overmap = ui.world.overmap();
    // Whose party? The viewer's; the DM (no viewer) watches the "dm" party.
    let party = ui.viewer.as_deref().unwrap_or("dm");
    let here = ui.world.party_at(party).map(str::to_owned);

    // Lay the nodes on a circle in normalized (0..1) space, which the swatch
    // expects: the projection carries no positions yet.
    let count = overmap.nodes.len().max(1) as f32;
    let nodes: Vec<GraphCanvasNode<String, &'static str>> = overmap
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let angle = (i as f32) / count * std::f32::consts::TAU;
            GraphCanvasNode {
                id: node.id.clone(),
                kind: if node.site.is_some() { "site" } else { "waypoint" },
                position: (0.5 + 0.4 * angle.cos(), 0.5 + 0.4 * angle.sin()),
                label: node.name.clone().into(),
                key: None,
            }
        })
        .collect();
    let edges = overmap
        .edges
        .iter()
        .map(|edge| GraphCanvasEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
        })
        .collect();

    let mut swatch = GraphCanvasSwatch::new(OVERMAP_KEY, GraphCanvasSubgraph { nodes, edges })
        .with_size(380, 300)
        .with_label("Overmap");
    swatch.selected = here.clone();

    let canvas = graph_canvas_swatch::<UiState, (), String, &'static str, _, _, _>(
        &swatch,
        |ui: &mut UiState, id: String| ui.request_travel(id),
        |_ui: &mut UiState, _id: Option<String>| {},
        |_ui: &mut UiState| {},
    );

    let actions: Vec<UiChild> = vec![Box::new(clickable(
        el::<_, UiState, ()>("span", text("close")).attr("class", "btn btn-mini"),
        |ui: &mut UiState, _| ui.close_overmap(),
    ))];
    let hint = match &here {
        Some(node) => format!("here: {node} — click a place to travel"),
        None => "the party is not on the overmap yet".to_owned(),
    };
    let body: Vec<UiChild> = vec![
        Box::new(canvas),
        Box::new(el("div", text(hint)).attr("class", "side-hint")),
    ];
    Some(crate::widgets::overlay_panel(
        "overmap",
        "Overmap".to_owned(),
        actions,
        body,
    ))
}
