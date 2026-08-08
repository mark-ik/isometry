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
//! Isometry adapts the campaign's pointcrawl to Scenograph's portable score and
//! realizes it with Scenomise. The final unit-box fit remains here because it is
//! a property of Cambium's swatch viewport, not of the score or scene. The leaf
//! key and the swatch model are shared with the host through [`overmap_swatch`],
//! so the painted leaf and these hit targets project through one identical
//! layout.

use std::collections::BTreeMap;

use cambium::{
    clickable, el, graph_canvas_swatch_with_drag_and_relations, lens, segmented_control, slider,
    text, GraphCanvasNode, GraphCanvasRelation, GraphCanvasSubgraph, GraphCanvasSwatch, Slider,
};
use isometry_core::{Overmap, OvermapEdge};
use sceno::{
    Arrangement, Footprint, Geographic, Placement, Representation, Score, ScoreItem, SourceRef,
    Spiral, Vec2,
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

/// The adapter id persisted in a score's opaque source refs. It identifies a
/// boundary, not an Isometry type in the portable scene schema.
pub const ISOMETRY_OVERMAP_ADAPTER: &str = "isometry.overmap";

/// A local relation-cell id for the present overmap projection. `OvermapEdge`
/// does not yet carry the campaign route id, so the source order distinguishes
/// parallel routes. A future projection with campaign ids can replace this
/// adapter-local identity without changing Cambium's relation contract.
fn overmap_relation_id(index: usize, edge: &OvermapEdge) -> String {
    format!(
        "route:{index}:{}:{}:{}:{}",
        edge.from, edge.to, edge.weight, edge.directed
    )
}

/// Adapt a pointcrawl to the shared score contract.
///
/// Authored coordinates choose the geographic arrangement. Uniform default
/// coordinates instead choose the generic spiral: the campaign owns the choice
/// of whether locations were authored, while Scenomise owns the analytic layout.
pub fn overmap_score(overmap: &Overmap) -> Score {
    let authored = overmap
        .nodes
        .first()
        .is_some_and(|first| overmap.nodes.iter().any(|node| node.at != first.at));
    let arrangement = if authored {
        Arrangement::Geographic(Geographic {
            invert_y: false,
            ..Geographic::default()
        })
    } else {
        Arrangement::Spiral(Spiral::default())
    };
    let mut score = Score::new(arrangement);
    score.items = overmap
        .nodes
        .iter()
        .enumerate()
        .map(|(ordinal, node)| ScoreItem {
            source: SourceRef::new(ISOMETRY_OVERMAP_ADAPTER, &node.id),
            ordinal: ordinal as u32,
            footprint: Footprint::Circle { radius: 6.0 },
            representation: Representation::Glyph,
            placement: if authored {
                Placement::Coordinate(Vec2::new(node.at.0 as f32, node.at.1 as f32))
            } else {
                Placement::Ordinal
            },
            layer: 0,
            visible: true,
        })
        .collect();
    score
}

/// Realize an overmap score, then fit the scene's product-neutral coordinates
/// into Cambium's normalized swatch viewport. This is deliberately a view
/// adapter, replacing the old `Overmap::layout` solver rather than relocating it
/// into the campaign model.
pub fn overmap_positions(overmap: &Overmap) -> BTreeMap<String, (f32, f32)> {
    overmap_positions_relaxed(overmap, None)
}

/// The same overmap, optionally loosened by the shared relaxation before it is
/// normalized: sites push apart, routes pull toward their rest length, and the
/// arrangement keeps drawing them back toward the slots it chose. Physics is a
/// capability of any graph surface, not of the one canvas that owns a rigid-body
/// world — a swatch gets the same reading at swatch cost, and `None` keeps the
/// purely analytic placement.
pub fn overmap_positions_relaxed(
    overmap: &Overmap,
    relaxation: Option<scenomise::Relaxation>,
) -> BTreeMap<String, (f32, f32)> {
    let mut scene = scenomise::solve(&overmap_score(overmap));
    if let Some(settings) = relaxation {
        scenomise::relax(&mut scene, &settings);
    }
    normalize_for_swatch(&scene)
}

fn normalize_for_swatch(scene: &sceno::Scene) -> BTreeMap<String, (f32, f32)> {
    const MARGIN: f32 = 0.08;
    let raw: Vec<_> = scene
        .items
        .iter()
        .filter_map(|item| {
            scene
                .sources
                .get(item.source.0 as usize)
                .map(|source| (source.id.clone(), item.transform.translate))
        })
        .collect();
    if raw.is_empty() {
        return BTreeMap::new();
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (_, point) in &raw {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    let span = 1.0 - 2.0 * MARGIN;
    let fit = |value: f32, low: f32, high: f32| {
        if high - low < 1e-4 {
            0.5
        } else {
            MARGIN + (value - low) / (high - low) * span
        }
    };
    raw.into_iter()
        .map(|(id, point)| (id, (fit(point.x, min_x, max_x), fit(point.y, min_y, max_y))))
        .collect()
}

/// Build the graph-canvas swatch for the party's discovered overmap. Both the
/// view (native hit targets + the `custom_leaf`) and the host (the registered
/// `paint_leaf`) call this, so they agree on node identity, order, and layout.
/// Returns `None` when the party has discovered nothing (no leaf to paint).
pub fn overmap_swatch(ui: &UiState) -> Option<GraphCanvasSwatch<String, OvermapNodeKind>> {
    // Whose party? The viewer's; the DM (no viewer) watches the "dm" party.
    let party = ui.viewer.as_deref().unwrap_or("dm");
    // Only what the party has discovered (E6): the unfound map is not drawn.
    let world = ui.overmap_source_world();
    let overmap = world.overmap_for(party);
    if overmap.nodes.is_empty() {
        return None;
    }
    let here = world.party_at(party).map(str::to_owned);

    // Shared score -> scene realization; the final viewport fit is local to the
    // Cambium swatch and does not alter campaign or scene data.
    let placed = overmap_positions(&overmap);
    let nodes: Vec<GraphCanvasNode<String, OvermapNodeKind>> = overmap
        .nodes
        .iter()
        .map(|node| {
            let position = ui
                .overmap_position_overrides
                .get(&node.id)
                .copied()
                .or_else(|| placed.get(&node.id).copied())
                .unwrap_or((0.5, 0.5));
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
    let relations: Vec<GraphCanvasRelation<String>> = overmap
        .edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            let id = overmap_relation_id(index, edge);
            let from = overmap
                .node(&edge.from)
                .map(|node| node.name.clone())
                .unwrap_or_else(|| edge.from.clone());
            let to = overmap
                .node(&edge.to)
                .map(|node| node.name.clone())
                .unwrap_or_else(|| edge.to.clone());
            GraphCanvasRelation {
                id: id.clone(),
                from: edge.from.clone(),
                to: edge.to.clone(),
                kind: if edge.directed {
                    "One-way route".to_owned()
                } else {
                    "Route".to_owned()
                },
                label: format!("{from} to {to}, travel weight {}", edge.weight),
                visible: !ui.overmap_hidden_relations.contains(&id),
                emphasized: ui.overmap_selected_relation.as_deref() == Some(id.as_str())
                    || ui.overmap_hovered_relation.as_deref() == Some(id.as_str()),
            }
        })
        .collect();

    let mut swatch = GraphCanvasSwatch::new(
        OVERMAP_LEAF_KEY,
        GraphCanvasSubgraph {
            nodes,
            edges: Vec::new(),
        },
    )
    .with_relations(relations)
    .with_size(OVERMAP_CANVAS.0, OVERMAP_CANVAS.1)
    .with_label("Overmap")
    // The overmap *is* the full view; there is nothing to expand into. The
    // chip used to render and get hidden with `display: none`, which left a
    // focusable, screen-reader-announced control that did nothing.
    .with_expand(false)
    // A pointcrawl is unreadable as unnamed dots, and since cambium 0.3.2 the
    // visible labels carry each node's `selected` / `hovered` state, so "here"
    // can be emphasized without a parallel hand-rolled label layer.
    .with_node_labels(true);
    // A larger ring for a full-panel canvas than the card default.
    swatch.node_radius = 6.0;
    swatch.edge_width = 1.5;
    // The party's place reads as the selected node (its emphasis ring); the node
    // under the pointer reads as hovered.
    swatch.selected = here;
    swatch.hovered = ui.overmap_hover.clone();
    Some(swatch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use isometry_core::OvermapNode;

    fn node(id: &str, at: (i32, i32)) -> OvermapNode {
        OvermapNode {
            id: id.to_owned(),
            name: id.to_owned(),
            at,
            site: None,
        }
    }

    #[test]
    fn authored_overmap_uses_geographic_score_and_viewport_fit() {
        let mut overmap = Overmap::new("shore");
        overmap.nodes = vec![node("west", (0, 4)), node("east", (10, 0))];
        let score = overmap_score(&overmap);
        assert!(matches!(score.arrangement, Arrangement::Geographic(_)));
        let positions = overmap_positions(&overmap);
        assert_eq!(positions["west"], (0.08, 0.92));
        assert_eq!(positions["east"], (0.92, 0.08));
    }

    #[test]
    fn uniform_overmap_uses_portable_spiral_and_product_free_types() {
        let mut overmap = Overmap::new("unplaced");
        overmap.nodes = vec![node("a", (0, 0)), node("b", (0, 0)), node("c", (0, 0))];
        let score = overmap_score(&overmap);
        assert!(matches!(score.arrangement, Arrangement::Spiral(_)));
        let scene = scenomise::solve(&score);
        assert!(std::any::type_name::<Score>().starts_with("sceno::"));
        assert!(std::any::type_name::<sceno::Scene>().starts_with("sceno::"));
        let score_json = serde_json::to_string(&score).unwrap();
        let scene_json = serde_json::to_string(&scene).unwrap();
        assert_eq!(serde_json::from_str::<Score>(&score_json).unwrap(), score);
        assert_eq!(
            serde_json::from_str::<sceno::Scene>(&scene_json).unwrap(),
            scene
        );
        assert_eq!(overmap_positions(&overmap).len(), 3);
    }

    /// The Expand chip is off at the model, not hidden at the sheet. The old
    /// `display: none` left a real button in the tree: tabbable, announced as
    /// "Expand graph", and wired to a closure that did nothing. Switching it
    /// back on without also wiring a full-canvas route re-creates that, and CSS
    /// would no longer hide the evidence.
    #[test]
    fn overmap_renders_no_expand_affordance() {
        let swatch = overmap_swatch(&discovered_one_place()).expect("a discovered place draws");
        assert!(
            !swatch.show_expand,
            "the overmap has no fuller view to expand into; wire the route before re-enabling"
        );
    }

    /// The names come from the component, not a parallel layer. Isometry drew its
    /// own `overmap-label*` layer until cambium 0.3.2 taught visible labels the
    /// node's state; turning `show_labels` back off would render an unnamed
    /// pointcrawl rather than fall back to anything, since the hand-rolled layer
    /// is deleted. Cambium's own suite pins that the labels carry `selected` /
    /// `hovered`, which is the emphasis "here" needs.
    #[test]
    fn the_component_draws_the_place_names() {
        let ui = discovered_one_place();
        let swatch = overmap_swatch(&ui).expect("a discovered place draws");
        assert!(
            swatch.show_labels,
            "the pointcrawl reads as unnamed dots without the component's labels"
        );
        assert_eq!(
            swatch.graph.nodes[0].label, "Here",
            "the label the component renders is the place's name"
        );
        assert_eq!(
            swatch.selected.as_deref(),
            Some("here"),
            "the party's place is the selected node, which is what emphasizes its label"
        );
    }

    #[test]
    fn overmap_drag_is_local_curation_and_consumes_its_release_click() {
        let mut ui = discovered_one_place();
        let source_position = ui.world.places["here"].position.clone();

        ui.drag_overmap_node(cambium::GraphCanvasNodeDrag {
            id: "here".to_owned(),
            phase: cambium::PointerPhase::Down,
            position: (0.5, 0.5),
        });
        ui.drag_overmap_node(cambium::GraphCanvasNodeDrag {
            id: "here".to_owned(),
            phase: cambium::PointerPhase::Move,
            position: (0.78, 0.22),
        });
        ui.drag_overmap_node(cambium::GraphCanvasNodeDrag {
            id: "here".to_owned(),
            phase: cambium::PointerPhase::Up,
            position: (0.78, 0.22),
        });

        let swatch = overmap_swatch(&ui).expect("a discovered place draws");
        assert_eq!(swatch.graph.nodes[0].position, (0.78, 0.22));
        assert_eq!(
            ui.world.places["here"].position, source_position,
            "pulling a Swatch node does not edit campaign geography"
        );

        ui.activate_overmap_node("here".to_owned());
        assert!(
            ui.overmap_travel_request.is_none(),
            "the click following a drag is consumed rather than becoming travel"
        );
        ui.activate_overmap_node("here".to_owned());
        assert_eq!(ui.overmap_travel_request.as_deref(), Some("here"));
    }

    #[test]
    fn parallel_route_cells_remain_independently_selectable_and_hideable() {
        let first = OvermapEdge {
            from: "here".to_owned(),
            to: "there".to_owned(),
            weight: 1,
            directed: false,
        };
        let second = OvermapEdge {
            weight: 3,
            ..first.clone()
        };
        let first_id = overmap_relation_id(0, &first);
        let second_id = overmap_relation_id(1, &second);
        assert_ne!(first_id, second_id, "parallel routes are distinct cells");

        let mut ui = discovered_one_place();
        ui.select_overmap_relation(first_id.clone());
        ui.toggle_overmap_relation_visibility(first_id.clone());
        assert_eq!(
            ui.overmap_selected_relation.as_deref(),
            Some(first_id.as_str())
        );
        assert!(ui.overmap_hidden_relations.contains(&first_id));
        assert!(
            !ui.overmap_hidden_relations.contains(&second_id),
            "hiding one relation cell never hides its parallel sibling"
        );
        ui.toggle_overmap_relation_visibility(first_id.clone());
        assert!(
            !ui.overmap_hidden_relations.contains(&first_id),
            "show restores only the local view cell"
        );
    }

    fn discovered_one_place() -> UiState {
        let mut ui = UiState::new(isometry_core::MapDocument::new("t", 2, 2));
        ui.world.places.insert(
            "here".to_owned(),
            isometry_campaign::WorldPlace {
                id: "here".to_owned(),
                name: "Here".to_owned(),
                tags: Vec::new(),
                map: None,
                position: None,
            },
        );
        ui.world.reveal("dm", "here");
        ui.world
            .apply(&isometry_campaign::WorldEvent::PartyMoved {
                party: "dm".to_owned(),
                node: "here".to_owned(),
            })
            .expect("the party stands on a discovered place");
        ui
    }
}

pub fn overmap_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.overmap_open {
        return None;
    }
    let party = ui.viewer.as_deref().unwrap_or("dm");
    let world = ui.overmap_source_world();
    let overmap = world.overmap_for(party);
    let here = world.party_at(party).map(str::to_owned);
    let historical = ui.overmap_is_historical();

    let mut actions: Vec<UiChild> = Vec::new();
    if historical {
        actions.push(Box::new(clickable(
            el::<_, UiState, ()>("span", text("return live")).attr("class", "btn btn-mini"),
            |ui: &mut UiState, _| ui.return_overmap_to_live(),
        )));
    } else {
        actions.push(Box::new(clickable(
            el::<_, UiState, ()>("span", text("study map")).attr("class", "btn btn-mini"),
            |ui: &mut UiState, _| ui.request_map_read(),
        )));
    }
    actions.push(Box::new(clickable(
        el::<_, UiState, ()>("span", text("close")).attr("class", "btn btn-mini"),
        |ui: &mut UiState, _| ui.close_overmap(),
    )));

    let mut body: Vec<UiChild> = Vec::new();

    if let Some(label) = ui.overmap_source_time_label() {
        if ui.overmap_source_time_available() {
            body.push(Box::new(
                el(
                    "section",
                    (
                        el("div", text(label)).attr("class", "source-time-label"),
                        lens(
                            |state: &mut Slider| slider(state),
                            |ui: &mut UiState| &mut ui.overmap_source_slider,
                        ),
                    ),
                )
                .attr("class", "source-time"),
            ));
        }
    }

    // The painted graph, when there is one; else the "find your way" hint.
    match overmap_swatch(ui) {
        Some(swatch) => {
            // The swatch draws its own named nodes (`with_node_labels`), so the
            // place names ride the same projection as the painted dots by
            // construction rather than by a second layer agreeing with the first.
            body.push(Box::new(
                el(
                    "div",
                    graph_canvas_swatch_with_drag_and_relations(
                        &swatch,
                        // A click asks the host to travel there; the drag path
                        // consumes its own release-click so a pull stays local
                        // curation rather than becoming a trip request.
                        |ui: &mut UiState, id: String| {
                            if !ui.overmap_is_historical() {
                                ui.activate_overmap_node(id);
                            }
                        },
                        // Enter/leave lifts the hovered node on the painted leaf.
                        |ui: &mut UiState, id: Option<String>| ui.hover_overmap(id),
                        // Pulling a site changes only this view's local
                        // placement override. Campaign geography remains the
                        // source of truth until a separate authoring command.
                        |ui: &mut UiState, event| ui.drag_overmap_node(event),
                        // Relation cells keep their own stable local identity,
                        // even where two routes share the same endpoints.
                        |ui: &mut UiState, id: String| ui.select_overmap_relation(id),
                        |ui: &mut UiState, id: Option<String>| ui.hover_overmap_relation(id),
                        // Never called: the swatch renders no Expand chip
                        // (`with_expand(false)`).
                        |_ui: &mut UiState| {},
                    ),
                )
                .attr("class", "overmap-graph"),
            ));
        }
        None => {
            body.push(Box::new(
                el(
                    "div",
                    text("no map yet — travel or ask around to find your way"),
                )
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
    for (index, edge) in overmap.edges.iter().enumerate() {
        let relation = overmap_relation_id(index, edge);
        let selected = ui.overmap_selected_relation.as_deref() == Some(relation.as_str());
        let hidden = ui.overmap_hidden_relations.contains(&relation);
        let mut class = String::from("side-line overmap-route");
        if selected {
            class.push_str(" selected");
        }
        if hidden {
            class.push_str(" hidden");
        }
        let row_relation = relation.clone();
        body.push(Box::new(clickable(
            el(
                "button",
                text(format!(
                    "{} to {} ({}){}",
                    name_of(&edge.from),
                    name_of(&edge.to),
                    edge.weight,
                    if hidden { " [hidden]" } else { "" },
                )),
            )
            .attr("class", class)
            .attr("type", "button")
            .attr("aria-current", if selected { "true" } else { "false" }),
            move |ui: &mut UiState, _| ui.select_overmap_relation(row_relation.clone()),
        )));
    }

    if let Some((index, edge)) = overmap.edges.iter().enumerate().find(|(index, edge)| {
        let relation = overmap_relation_id(*index, edge);
        ui.overmap_selected_relation.as_deref() == Some(relation.as_str())
    }) {
        let relation = overmap_relation_id(index, edge);
        let hidden = ui.overmap_hidden_relations.contains(&relation);
        let toggle_relation = relation.clone();
        body.push(Box::new(
            el(
                "section",
                (
                    el("strong", text("Route")),
                    el(
                        "div",
                        text(format!(
                            "{} to {} · travel weight {}{}",
                            name_of(&edge.from),
                            name_of(&edge.to),
                            edge.weight,
                            if edge.directed { " · one way" } else { "" },
                        )),
                    ),
                    clickable(
                        el(
                            "button",
                            text(if hidden { "Show route" } else { "Hide route" }),
                        )
                        .attr("class", "btn btn-mini")
                        .attr("type", "button"),
                        move |ui: &mut UiState, _| {
                            ui.toggle_overmap_relation_visibility(toggle_relation.clone())
                        },
                    ),
                ),
            )
            .attr("class", "overmap-link-card"),
        ));
    }

    // Pace (E1) and stance (E3): the party's marching orders. The chosen pace is
    // marked; a stance is set on the lead token by the host.
    // Pace and stance are catalog `segmented_control` rows: one-of-N choice, which
    // is exactly the component's shape. The row writes only its own
    // `SelectionState`; `pump_selection_rows` notices the divergence from the
    // world and dispatches the real request, so a click still travels the
    // ordinary host-adjudicated path.
    if !historical {
        let pace_items = crate::state::pace_items();
        body.push(Box::new(
            el::<_, UiState, ()>(
                "div",
                lens(
                    move |sel: &mut cambium::SelectionState| segmented_control(sel, &pace_items),
                    |ui: &mut UiState| &mut ui.pace_selection,
                ),
            )
            .attr("class", "overmap-controls"),
        ));

        let stance_items = crate::state::stance_items();
        body.push(Box::new(
            el::<_, UiState, ()>(
                "div",
                lens(
                    move |sel: &mut cambium::SelectionState| segmented_control(sel, &stance_items),
                    |ui: &mut UiState| &mut ui.stance_selection,
                ),
            )
            .attr("class", "overmap-controls"),
        ));
    }

    let hint = if historical {
        "historical projection: travel and exploration orders are disabled".to_owned()
    } else {
        match &here {
        Some(node) => format!("here: {} — click a place to travel", name_of(node)),
        None => "the party is not on the overmap yet".to_owned(),
        }
    };
    body.push(Box::new(el("div", text(hint)).attr("class", "side-hint")));

    Some(crate::widgets::overlay_panel(
        "overmap",
        "Overmap".to_owned(),
        actions,
        body,
    ))
}

#[cfg(test)]
mod relax_tests {
    use super::*;
    use isometry_core::OvermapNode;

    fn node(id: &str, at: (i32, i32)) -> OvermapNode {
        OvermapNode {
            id: id.to_owned(),
            name: id.to_owned(),
            at,
            site: None,
        }
    }

    /// A swatch can run physics. The shared relaxation loosens the overmap in
    /// place -- no rigid-body world at swatch scale -- and placement changes
    /// while membership and the normalized viewport fit do not.
    #[test]
    fn a_relaxed_overmap_still_places_every_site() {
        let mut overmap = Overmap::new("shore");
        overmap.nodes = vec![
            node("west", (0, 4)),
            node("east", (10, 0)),
            node("north", (5, 9)),
        ];
        let analytic = overmap_positions(&overmap);
        let relaxed = overmap_positions_relaxed(&overmap, Some(scenomise::Relaxation::default()));

        assert_eq!(
            analytic.keys().collect::<Vec<_>>(),
            relaxed.keys().collect::<Vec<_>>(),
            "relaxation changes placement, never membership"
        );
        for (_, (x, y)) in &relaxed {
            assert!(
                (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y),
                "a relaxed site still fits the swatch viewport"
            );
        }
        assert_eq!(
            relaxed,
            overmap_positions_relaxed(&overmap, Some(scenomise::Relaxation::default())),
            "swatch physics is deterministic"
        );
    }
}
