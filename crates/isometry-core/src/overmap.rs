//! The overmap: a pointcrawl graph of sites, the board for exploration mode.
//!
//! Nodes are places; edges are routes carrying an abstract travel weight. This
//! is the map *above* the tactical maps: where `MapDocument` is a tile grid the
//! party fights on, the overmap is a graph the party travels across. Geometry
//! only, like the rest of `isometry-core`: what a weight *costs* in time (5e
//! forced-march, PF2e hexploration), what a site *holds*, and what happens when
//! you arrive are system-plugin and campaign concerns layered on later. The
//! substrate stores the graph and searches it, and knows nothing of any of that.
//!
//! Pathfinding is a bounded Dijkstra rather than the grid's uniform BFS
//! ([`crate::path::reachable`]): overmap routes carry unequal weights, so equal
//! per-step cost cannot serve. The *shape* is the grid's (reachable-within-budget
//! plus a path), the arithmetic is weighted.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap};

use serde::{Deserialize, Serialize};

/// A site on the overmap, addressed by a stable id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OvermapNode {
    pub id: String,
    pub name: String,
    /// Where the node sits when the graph is drawn. Pathfinding ignores it (the
    /// edges carry the weight); it is for rendering, not for measuring travel.
    pub at: (i32, i32),
    /// The prepared or generated tactical map this site opens into, if any.
    /// Entering it is C2's transition; `None` is a waypoint you pass through.
    #[serde(default)]
    pub site: Option<String>,
}

/// A route between two sites, carrying an abstract travel weight (distance,
/// time, difficulty -- the system decides what the number means). Undirected by
/// default; a `directed` edge is a one-way route, like a cliff you descend but
/// cannot climb.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OvermapEdge {
    pub from: String,
    pub to: String,
    pub weight: u32,
    #[serde(default)]
    pub directed: bool,
}

/// A pointcrawl graph: sites and the routes between them. Pure geometry, the
/// travel counterpart of [`crate::map::MapDocument`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overmap {
    pub name: String,
    pub nodes: Vec<OvermapNode>,
    pub edges: Vec<OvermapEdge>,
}

impl Overmap {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn node(&self, id: &str) -> Option<&OvermapNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// The routes leaving `id`, as `(neighbour id, weight)`: the far end of every
    /// undirected edge touching `id`, and the `to` of every directed edge from
    /// it. A directed edge is not a neighbour of its `to`.
    pub fn neighbours(&self, id: &str) -> Vec<(&str, u32)> {
        let mut out = Vec::new();
        for edge in &self.edges {
            if edge.from == id {
                out.push((edge.to.as_str(), edge.weight));
            } else if !edge.directed && edge.to == id {
                out.push((edge.from.as_str(), edge.weight));
            }
        }
        out
    }

    /// Every site reachable from `from` within a total travel `budget`, mapped to
    /// its least cost (including `from` at 0). The travel analogue of the grid's
    /// reachable set, bounded by a budget the same way, but Dijkstra because the
    /// routes are weighted.
    pub fn reachable_within(&self, from: &str, budget: u32) -> BTreeMap<String, u32> {
        let mut best: BTreeMap<String, u32> = BTreeMap::new();
        if self.node(from).is_none() {
            return best;
        }
        best.insert(from.to_owned(), 0);
        let mut heap = BinaryHeap::new();
        heap.push(Reverse((0u32, from.to_owned())));
        while let Some(Reverse((cost, at))) = heap.pop() {
            if cost > *best.get(&at).unwrap_or(&u32::MAX) {
                continue; // a cheaper route to `at` was already settled
            }
            for (neighbour, weight) in self.neighbours(&at) {
                let next = cost.saturating_add(weight);
                if next > budget {
                    continue;
                }
                if next < *best.get(neighbour).unwrap_or(&u32::MAX) {
                    best.insert(neighbour.to_owned(), next);
                    heap.push(Reverse((next, neighbour.to_owned())));
                }
            }
        }
        best
    }

    /// Node positions for drawing, each normalized into the unit box `0..1`.
    ///
    /// When the nodes carry distinct authored `at` coordinates, those are used
    /// (normalized, with a margin); a hand-placed map is honored. Otherwise a
    /// deterministic force-directed layout is relaxed from a circle, so linked
    /// sites pull together and the graph reads at a glance without anyone placing
    /// a node. Deterministic by construction -- circle seed, no clock, no RNG --
    /// so the same graph always draws the same, and replay is unaffected.
    pub fn layout(&self) -> BTreeMap<String, (f32, f32)> {
        use std::f32::consts::TAU;
        let count = self.nodes.len();
        if count == 0 {
            return BTreeMap::new();
        }
        if count == 1 {
            return BTreeMap::from([(self.nodes[0].id.clone(), (0.5, 0.5))]);
        }

        // Authored positions win when they are real and distinct (not the default
        // uniform `(0, 0)` a projection leaves behind).
        let first = self.nodes[0].at;
        if self.nodes.iter().any(|node| node.at != first) {
            return normalize_positions(
                self.nodes
                    .iter()
                    .map(|node| (node.id.clone(), (node.at.0 as f32, node.at.1 as f32)))
                    .collect(),
            );
        }

        // Otherwise relax a Fruchterman-Reingold layout from a deterministic ring.
        let index: HashMap<&str, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.id.as_str(), i))
            .collect();
        let mut pos: Vec<(f32, f32)> = (0..count)
            .map(|i| {
                let angle = i as f32 / count as f32 * TAU;
                (0.5 + 0.4 * angle.cos(), 0.5 + 0.4 * angle.sin())
            })
            .collect();
        let links: Vec<(usize, usize)> = self
            .edges
            .iter()
            .filter_map(|edge| {
                let a = *index.get(edge.from.as_str())?;
                let b = *index.get(edge.to.as_str())?;
                (a != b).then_some((a, b))
            })
            .collect();

        // Ideal separation in unit space, and a cooling cap so the layout settles.
        let k = (1.0 / count as f32).sqrt();
        let iterations = 120;
        for step in 0..iterations {
            let temp = 0.10 * (1.0 - step as f32 / iterations as f32);
            let mut disp = vec![(0.0f32, 0.0f32); count];
            // Every pair repels.
            for i in 0..count {
                for j in (i + 1)..count {
                    let (dx, dy) = (pos[i].0 - pos[j].0, pos[i].1 - pos[j].1);
                    let mut dist = (dx * dx + dy * dy).sqrt();
                    let (ux, uy) = if dist < 1e-4 {
                        // Coincident nodes: a deterministic, index-keyed nudge so
                        // they separate the same way every run (no RNG).
                        dist = 1e-4;
                        ((i as f32 * 1.3).cos(), (j as f32 * 1.7).sin())
                    } else {
                        (dx / dist, dy / dist)
                    };
                    let force = k * k / dist;
                    disp[i].0 += ux * force;
                    disp[i].1 += uy * force;
                    disp[j].0 -= ux * force;
                    disp[j].1 -= uy * force;
                }
            }
            // Linked nodes attract.
            for &(a, b) in &links {
                let (dx, dy) = (pos[a].0 - pos[b].0, pos[a].1 - pos[b].1);
                let dist = (dx * dx + dy * dy).sqrt().max(1e-4);
                let (ux, uy) = (dx / dist, dy / dist);
                let force = dist * dist / k;
                disp[a].0 -= ux * force;
                disp[a].1 -= uy * force;
                disp[b].0 += ux * force;
                disp[b].1 += uy * force;
            }
            // Move each node by its displacement, capped by the cooling temp.
            for i in 0..count {
                let (mx, my) = disp[i];
                let mag = (mx * mx + my * my).sqrt().max(1e-4);
                let capped = mag.min(temp);
                pos[i].0 += mx / mag * capped;
                pos[i].1 += my / mag * capped;
            }
        }

        normalize_positions(
            self.nodes
                .iter()
                .enumerate()
                .map(|(i, node)| (node.id.clone(), pos[i]))
                .collect(),
        )
    }

    /// The least-cost route from `from` to `to` as a site sequence (both ends
    /// included) with its total weight, or `None` when `to` is unreachable or
    /// either id is unknown. Dijkstra.
    pub fn route(&self, from: &str, to: &str) -> Option<(Vec<String>, u32)> {
        if self.node(from).is_none() || self.node(to).is_none() {
            return None;
        }
        let mut best: BTreeMap<String, u32> = BTreeMap::new();
        let mut prev: HashMap<String, String> = HashMap::new();
        best.insert(from.to_owned(), 0);
        let mut heap = BinaryHeap::new();
        heap.push(Reverse((0u32, from.to_owned())));
        while let Some(Reverse((cost, at))) = heap.pop() {
            if at == to {
                let mut path = vec![to.to_owned()];
                let mut cur = to.to_owned();
                while let Some(p) = prev.get(&cur) {
                    path.push(p.clone());
                    cur = p.clone();
                }
                path.reverse();
                return Some((path, cost));
            }
            if cost > *best.get(&at).unwrap_or(&u32::MAX) {
                continue;
            }
            for (neighbour, weight) in self.neighbours(&at) {
                let next = cost.saturating_add(weight);
                if next < *best.get(neighbour).unwrap_or(&u32::MAX) {
                    best.insert(neighbour.to_owned(), next);
                    prev.insert(neighbour.to_owned(), at.clone());
                    heap.push(Reverse((next, neighbour.to_owned())));
                }
            }
        }
        None
    }
}

/// Fit raw positions into the unit box with a margin, preserving aspect by
/// scaling each axis independently to `[margin, 1 - margin]`. A degenerate axis
/// (every node on one line) collapses to the box center on that axis.
fn normalize_positions(raw: Vec<(String, (f32, f32))>) -> BTreeMap<String, (f32, f32)> {
    const MARGIN: f32 = 0.08;
    let (mut min_x, mut min_y, mut max_x, mut max_y) =
        (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (_, (x, y)) in &raw {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }
    let span = 1.0 - 2.0 * MARGIN;
    let fit = |v: f32, lo: f32, hi: f32| {
        if hi - lo < 1e-4 {
            0.5
        } else {
            MARGIN + (v - lo) / (hi - lo) * span
        }
    };
    raw.into_iter()
        .map(|(id, (x, y))| (id, (fit(x, min_x, max_x), fit(y, min_y, max_y))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, at: (i32, i32)) -> OvermapNode {
        OvermapNode {
            id: id.to_owned(),
            name: id.to_owned(),
            at,
            site: None,
        }
    }

    fn edge(from: &str, to: &str, weight: u32) -> OvermapEdge {
        OvermapEdge {
            from: from.to_owned(),
            to: to.to_owned(),
            weight,
            directed: false,
        }
    }

    fn shire() -> Overmap {
        let mut m = Overmap::new("shire");
        m.nodes = vec![
            node("village", (0, 0)),
            node("forest", (2, 0)),
            node("swamp", (0, 2)),
            node("ruins", (3, 2)),
        ];
        // Two ways to the ruins: through the forest (2 + 3 = 5) or the swamp
        // (5 + 1 = 6). The forest is cheaper by one.
        m.edges = vec![
            edge("village", "forest", 2),
            edge("forest", "ruins", 3),
            edge("village", "swamp", 5),
            edge("swamp", "ruins", 1),
        ];
        m
    }

    #[test]
    fn route_takes_the_cheaper_path_by_weight() {
        let m = shire();
        let (path, cost) = m.route("village", "ruins").expect("the ruins are reachable");
        assert_eq!(path, vec!["village", "forest", "ruins"], "the cheaper of two ways");
        assert_eq!(cost, 5);
    }

    #[test]
    fn reachable_within_is_bounded_by_the_travel_budget() {
        let m = shire();
        // Budget 4 reaches the village (0) and the forest (2); both the swamp (5)
        // and the ruins (5 by the cheap path) are past it.
        let near = m.reachable_within("village", 4);
        assert_eq!(near.get("village"), Some(&0));
        assert_eq!(near.get("forest"), Some(&2));
        assert!(!near.contains_key("swamp"), "5 > budget 4");
        assert!(!near.contains_key("ruins"), "5 > budget 4");
        // Raise it and the far sites come into range at their least cost.
        let far = m.reachable_within("village", 6);
        assert_eq!(far.get("ruins"), Some(&5), "the cheap forest route, not the swamp's 6");
        assert_eq!(far.get("swamp"), Some(&5));
    }

    #[test]
    fn a_directed_route_is_one_way() {
        let mut m = Overmap::new("cliff");
        m.nodes = vec![node("top", (0, 0)), node("base", (0, 3))];
        m.edges = vec![OvermapEdge {
            from: "top".to_owned(),
            to: "base".to_owned(),
            weight: 1,
            directed: true,
        }];
        assert!(m.route("top", "base").is_some(), "you can descend the cliff");
        assert!(m.route("base", "top").is_none(), "but not climb back up it");
        assert_eq!(m.neighbours("top"), vec![("base", 1)]);
        assert!(m.neighbours("base").is_empty(), "a directed edge is not a neighbour of its target");
    }

    #[test]
    fn force_directed_layout_is_deterministic_and_inside_the_box() {
        let m = shire(); // four nodes, all at the default uniform position
        let a = m.layout();
        let b = m.layout();
        assert_eq!(a, b, "same graph lays out identically every run");
        assert_eq!(a.len(), 4);
        // Every node lands inside the unit box, and the four are distinct.
        for (_, (x, y)) in &a {
            assert!((0.0..=1.0).contains(x) && (0.0..=1.0).contains(y), "{x},{y} in box");
        }
        let mut points: Vec<_> = a.values().map(|(x, y)| (x.to_bits(), y.to_bits())).collect();
        points.sort();
        points.dedup();
        assert_eq!(points.len(), 4, "the nodes do not all pile onto one point");
    }

    #[test]
    fn authored_positions_override_the_force_layout() {
        // Distinct authored `at` coordinates are honored: the min corner maps to
        // the margin, the max to its complement, so the drawn shape is the
        // authored shape (here a rectangle), not a relaxed circle.
        let mut m = Overmap::new("authored");
        m.nodes = vec![
            node("nw", (0, 0)),
            node("ne", (10, 0)),
            node("sw", (0, 10)),
            node("se", (10, 10)),
        ];
        let laid = m.layout();
        assert_eq!(laid["nw"], (0.08, 0.08));
        assert_eq!(laid["se"], (0.92, 0.92));
        assert_eq!(laid["ne"], (0.92, 0.08));
    }

    #[test]
    fn an_unreachable_or_unknown_site_has_no_route() {
        let mut m = shire();
        m.nodes.push(node("island", (9, 9))); // no edge reaches it
        assert!(m.route("village", "island").is_none(), "no route to the island");
        assert!(m.route("village", "atlantis").is_none(), "unknown site");
        assert!(m.reachable_within("village", 100).get("island").is_none());
    }
}
