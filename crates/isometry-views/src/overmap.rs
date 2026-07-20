//! The overmap surface (C8, exploration mode): the party's pointcrawl, drawn.
//!
//! The board above the tactical maps. The host projects the world's places and
//! routes into an `Overmap` (`CampaignWorld::overmap`), filtered to what the
//! party has discovered (`overmap_for`); this draws that graph through Cambium's
//! `graph_canvas_swatch` -- painted nodes and edges on a retained Sprigging paint
//! leaf, with one native hit target per node -- and lets the table click a place
//! to travel there. The click only *asks*; the host rolls the navigation, spends
//! the time, and moves the party (`resolve_travel` -> `TravelResolved`), so the
//! view never decides a trip's outcome.
//!
//! The nodes are laid out on a circle (the projection carries no positions yet;
//! an authored or force-directed layout is a later refinement). The leaf key and
//! the swatch model are shared with the host through [`overmap_swatch`], so the
//! painted leaf and these hit targets project through one identical layout.

use cambium::{
    GraphCanvasEdge, GraphCanvasNode, GraphCanvasSubgraph, GraphCanvasSwatch, clickable, el,
    graph_canvas_swatch, text,
};

use crate::board::UiChild;
use crate::state::UiState;

/// The Sprigging `LeafRegistry` key the host registers the overmap's painted
/// graph leaf under. Shared so the view's `custom_leaf` and the host's
/// `paint_leaf` name the same leaf.
pub const OVERMAP_LEAF_KEY: u64 = 8001;

/// The two node roles the palette colors: the party's current place, and the
/// rest of what it has discovered. The host resolves these to paint; the plain
/// vocabulary keeps rules and product-specific kinds out of the component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OvermapNodeKind {
    /// Where the party stands now.
    Here,
    /// A discovered place the party is not standing on.
    Elsewhere,
}

/// The canvas size, in logical pixels. Shared by the view (the `custom_leaf`
/// box) and the host (the leaf's intrinsic size), so the two never disagree.
pub const OVERMAP_CANVAS: (u32, u32) = (440, 300);

/// Build the graph-canvas swatch for the party's discovered overmap. Both the
/// view (native hit targets + the `custom_leaf`) and the host (the registered
/// `paint_leaf`) call this, so they agree on node identity, order, and layout.
/// Returns `None` when the party has discovered nothing (no leaf to paint).
pub fn overmap_swatch(ui: &UiState) -> Option<GraphCanvasSwatch<String, OvermapNodeKind>> {
    // Whose party? The viewer's; the DM (no viewer) watches the "dm" party.
    let party = ui.viewer.as_deref().unwrap_or("dm");
    // Only what the party has discovered (E6): the unfound map is not drawn.
    let overmap = ui.world.overmap_for(party);
    if overmap.nodes.is_empty() {
        return None;
    }
    let here = ui.world.party_at(party).map(str::to_owned);

    // Node positions: authored coordinates when the world sets them, else a
    // deterministic force-directed relaxation from the routes (`Overmap::layout`).
    let placed = overmap.layout();
    let nodes: Vec<GraphCanvasNode<String, OvermapNodeKind>> = overmap
        .nodes
        .iter()
        .map(|node| {
            let position = placed.get(&node.id).copied().unwrap_or((0.5, 0.5));
            let kind = if here.as_deref() == Some(node.id.as_str()) {
                OvermapNodeKind::Here
            } else {
                OvermapNodeKind::Elsewhere
            };
            GraphCanvasNode {
                id: node.id.clone(),
                kind,
                position,
                label: node.name.clone(),
                key: Some(node.id.clone()),
            }
        })
        .collect();
    let edges: Vec<GraphCanvasEdge<String>> = overmap
        .edges
        .iter()
        .map(|edge| GraphCanvasEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
        })
        .collect();

    let mut swatch = GraphCanvasSwatch::new(OVERMAP_LEAF_KEY, GraphCanvasSubgraph { nodes, edges })
        .with_size(OVERMAP_CANVAS.0, OVERMAP_CANVAS.1)
        .with_label("Overmap");
    // A larger ring for a full-panel canvas than the card default.
    swatch.node_radius = 6.0;
    swatch.edge_width = 1.5;
    // The party's place reads as the selected node (its emphasis ring); the node
    // under the pointer reads as hovered.
    swatch.selected = here;
    swatch.hovered = ui.overmap_hover.clone();
    Some(swatch)
}

pub fn overmap_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.overmap_open {
        return None;
    }
    let party = ui.viewer.as_deref().unwrap_or("dm");
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

    // The painted graph, when there is one; else the "find your way" hint.
    match overmap_swatch(ui) {
        Some(swatch) => {
            // The swatch paints dots + edges and carries each place name only as
            // an aria-label. A pointcrawl needs the names on screen, so overlay a
            // label layer at the swatch's own projected node positions (same
            // projection as the painted leaf, so labels sit on their dots). The
            // layer is click-through; the swatch's hit targets under it travel.
            let (cw, ch) = OVERMAP_CANVAS;
            let labels: Vec<UiChild> = swatch
                .projected_positions()
                .into_iter()
                .map(|(id, (x, y))| {
                    let here = swatch.selected.as_deref() == Some(id.as_str());
                    let hovered = swatch.hovered.as_deref() == Some(id.as_str());
                    let class = if here {
                        "overmap-label overmap-label-here"
                    } else if hovered {
                        "overmap-label overmap-label-hover"
                    } else {
                        "overmap-label"
                    };
                    let name = swatch
                        .graph
                        .nodes
                        .iter()
                        .find(|n| &n.id == id)
                        .map(|n| n.label.clone())
                        .unwrap_or_else(|| id.clone());
                    Box::new(
                        el::<_, UiState, ()>("div", text(name)).attr("class", class).attr(
                            "style",
                            format!("position:absolute; left:{x:.0}px; top:{y:.0}px;"),
                        ),
                    ) as UiChild
                })
                .collect();
            body.push(Box::new(
                el(
                    "div",
                    (
                        graph_canvas_swatch(
                            &swatch,
                            // A node click asks the host to travel there.
                            |ui: &mut UiState, id: String| ui.request_travel(id),
                            // Enter/leave lifts the hovered node on the painted leaf.
                            |ui: &mut UiState, id: Option<String>| ui.hover_overmap(id),
                            // No full-canvas route yet; Expand is hidden in CSS.
                            |_ui: &mut UiState| {},
                        ),
                        el::<_, UiState, ()>("div", labels)
                            .attr("class", "overmap-labels")
                            .attr(
                                "style",
                                format!("width:{cw}px; height:{ch}px;"),
                            ),
                    ),
                )
                .attr("class", "overmap-graph")
                .attr(
                    "style",
                    format!("position:relative; width:{cw}px; height:{ch}px;"),
                ),
            ));
        }
        None => {
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
    }

    // The routes, listed with weights: painted edges show connectivity, but the
    // costs that drive travel time (weight x pace) still need to be legible.
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
