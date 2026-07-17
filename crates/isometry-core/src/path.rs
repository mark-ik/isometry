//! Grid movement: BFS over 4-connected tiles with a move budget and
//! elevation step limits. Geometry only; what the budget is (a speed
//! stat) and which terrain costs extra are system-plugin concerns
//! layered on later. Uniform cost of 1 per step for now.

use std::collections::HashMap;

use crate::iso::TileCoord;
use crate::map::MapDocument;

/// Substrate movement constraints, deliberately dumb: a tile is
/// enterable if its kind passes `passable`, no other token stands on
/// it, and the elevation change from the previous tile is within the
/// climb/drop limits.
pub struct MoveRules<'a> {
    /// Steps of movement available (BFS depth).
    pub budget: u32,
    /// Max height units climbable in one step.
    pub step_up: u8,
    /// Max height units droppable in one step.
    pub step_down: u8,
    /// Whether a ground kind can be stood on (e.g. water is not).
    pub passable: &'a dyn Fn(&str) -> bool,
}

/// Every tile reachable from `from` under `rules`, mapped to the
/// previous tile on its shortest path (`from` maps to itself). Feed to
/// [`path_to`] for the step list. `ignore` is the moving token (its own
/// tile never blocks it).
pub fn reachable(
    map: &MapDocument,
    from: TileCoord,
    rules: &MoveRules,
    ignore: crate::map::TokenId,
) -> HashMap<TileCoord, TileCoord> {
    let mut prev: HashMap<TileCoord, TileCoord> = HashMap::new();
    if !map.ground.in_bounds(from.0, from.1) {
        return prev;
    }
    prev.insert(from, from);
    let mut frontier = vec![from];
    for _ in 0..rules.budget {
        let mut next = Vec::new();
        for &at in &frontier {
            let h = *map.elevation.get(at.0 as u32, at.1 as u32).unwrap_or(&0);
            // All eight neighbours, to agree with the rest of the substrate.
            // [`crate::distance`] is Chebyshev and [`crate::away`] names eight
            // compass points, so a four-way walk meant a diagonal tile was
            // "one away" -- close enough to hit and to be shoved into, but not
            // to step to. One diagonal step costs one, as Chebyshev says.
            for step in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let to = (at.0 + step.0, at.1 + step.1);
                if prev.contains_key(&to) || !map.ground.in_bounds(to.0, to.1) {
                    continue;
                }
                let (tc, tr) = (to.0 as u32, to.1 as u32);
                let kind = map.ground.get(tc, tr).copied().unwrap_or_default();
                let name = map
                    .tile_kinds
                    .get(kind.0 as usize)
                    .map(String::as_str)
                    .unwrap_or("empty");
                if kind.0 == 0 || !(rules.passable)(name) {
                    continue;
                }
                let th = *map.elevation.get(tc, tr).unwrap_or(&0);
                let ok_climb = th <= h.saturating_add(rules.step_up)
                    && h <= th.saturating_add(rules.step_down);
                if !ok_climb {
                    continue;
                }
                if map.tokens.iter().any(|t| t.at == to && t.id != ignore) {
                    continue;
                }
                prev.insert(to, at);
                next.push(to);
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    prev
}

/// The tile sequence from the BFS origin to `to` (inclusive), or empty
/// when `to` is unreachable. The origin itself is omitted.
pub fn path_to(prev: &HashMap<TileCoord, TileCoord>, to: TileCoord) -> Vec<TileCoord> {
    let mut path = Vec::new();
    let mut cur = to;
    loop {
        let Some(&p) = prev.get(&cur) else {
            return Vec::new();
        };
        if p == cur {
            break;
        }
        path.push(cur);
        cur = p;
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Facing, Token, TokenId};

    fn board() -> MapDocument {
        let mut m = MapDocument::new("t", 8, 8);
        let grass = m.intern_tile_kind("grass");
        let water = m.intern_tile_kind("water");
        for r in 0..8 {
            for c in 0..8 {
                m.ground.set(c, r, grass);
            }
        }
        // A water wall down column 3 with a gap at row 6.
        for r in 0..8 {
            if r != 6 {
                m.ground.set(3, r, water);
            }
        }
        m
    }

    fn rules(budget: u32) -> MoveRules<'static> {
        MoveRules {
            budget,
            step_up: 1,
            step_down: 2,
            passable: &|kind| kind != "water",
        }
    }

    #[test]
    fn water_blocks_and_the_gap_routes_around() {
        let m = board();
        let prev = reachable(&m, (1, 6), &rules(4), TokenId(9));
        // Straight through the gap: (2,6) -> (3,6) -> (4,6).
        let path = path_to(&prev, (4, 6));
        assert_eq!(path, vec![(2, 6), (3, 6), (4, 6)]);
        // Across the wall elsewhere is unreachable at this budget.
        assert!(path_to(&prev, (4, 2)).is_empty());
    }

    #[test]
    fn budget_caps_the_reach() {
        let m = board();
        let prev = reachable(&m, (0, 0), &rules(2), TokenId(9));
        assert!(prev.contains_key(&(2, 0)));
        assert!(prev.contains_key(&(1, 1)));
        // A diagonal step costs one, so the budget buys Chebyshev distance --
        // the same metric `distance()` measures reach with. (2, 1) is two steps
        // (one diagonal, one straight) and (2, 2) is two diagonals.
        assert!(prev.contains_key(&(2, 1)), "one diagonal then one straight");
        assert!(prev.contains_key(&(2, 2)), "two diagonal steps");
        // Three away in any direction is still out of reach. Column 3 is the
        // water wall, so measure up the grass instead.
        assert!(!prev.contains_key(&(0, 3)), "Chebyshev 3 > budget 2");
    }

    #[test]
    fn other_tokens_block_but_self_does_not() {
        let mut m = board();
        m.tokens.push(Token {
            id: TokenId(1),
            at: (1, 0),
            facing: Facing::South,
            sprite: "knight".to_owned(),
            owner: None,
        });
        let prev = reachable(&m, (0, 0), &rules(3), TokenId(2));
        assert!(!prev.contains_key(&(1, 0)), "occupied tile is blocked");
        // The mover itself never blocks its own origin.
        let prev_self = reachable(&m, (1, 0), &rules(1), TokenId(1));
        assert!(prev_self.contains_key(&(2, 0)));
    }

    #[test]
    fn elevation_steps_gate_climbing() {
        let mut m = board();
        m.elevation.set(1, 0, 2); // 2 up from (0,0): too steep
        m.elevation.set(0, 1, 1); // 1 up: fine
        let prev = reachable(&m, (0, 0), &rules(1), TokenId(9));
        assert!(!prev.contains_key(&(1, 0)));
        assert!(prev.contains_key(&(0, 1)));
    }
}
