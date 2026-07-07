//! Board-to-text narration: a deterministic serializer that turns board
//! state into prose facts. Pure substrate geometry, alongside
//! [`template`](crate::template) and [`visibility`](crate::visibility): no
//! UI, no I/O, no model. This is the factual layer; an optional model
//! fluency pass (behind the host's provider seam) rephrases it later. It is
//! the shared perception primitive: accessibility and text-only play,
//! session recap, and a model's grounding context all read the same facts.
//!
//! The world compass matches [`Facing`]: north is decreasing row, south
//! increasing row, east increasing col, west decreasing col.

use crate::iso::TileCoord;
use crate::map::{Facing, MapDocument, Token, TokenId};
use crate::template::distance;
use crate::visibility::{visible_from, SightRules};

/// The world direction a facing points, matching [`Facing`]'s axes.
pub fn facing_word(f: Facing) -> &'static str {
    match f {
        Facing::North => "north",
        Facing::South => "south",
        Facing::East => "east",
        Facing::West => "west",
    }
}

/// Eight-way world bearing from `from` to `to`, on the same axes as
/// [`Facing`] (north = decreasing row, east = increasing col). Returns
/// "here" when the tiles coincide.
pub fn bearing(from: TileCoord, to: TileCoord) -> &'static str {
    use std::cmp::Ordering::{Equal, Greater, Less};
    let (dcol, drow) = (to.0 - from.0, to.1 - from.1);
    let ns = match drow.cmp(&0) {
        Less => "north",
        Greater => "south",
        Equal => "",
    };
    let ew = match dcol.cmp(&0) {
        Greater => "east",
        Less => "west",
        Equal => "",
    };
    match (ns, ew) {
        ("", "") => "here",
        (ns, "") => ns,
        ("", ew) => ew,
        ("north", "east") => "northeast",
        ("north", "west") => "northwest",
        ("south", "east") => "southeast",
        ("south", "west") => "southwest",
        // ns/ew each range over a fixed set, so no other pair occurs.
        _ => "here",
    }
}

fn tiles_word(d: u32) -> &'static str {
    if d == 1 {
        "tile"
    } else {
        "tiles"
    }
}

fn owner_word(t: &Token) -> &str {
    t.owner.as_deref().unwrap_or("DM")
}

fn label(t: &Token) -> String {
    format!("{} {}", t.sprite, t.id.0)
}

/// A terrain/elevation note for the tile at `at`: the ground kind (unless
/// empty) and the height (unless flat). Empty string when there is nothing
/// to say (empty ground at elevation 0, or out of bounds).
fn tile_note(map: &MapDocument, at: TileCoord) -> String {
    if at.0 < 0 || at.1 < 0 {
        return String::new();
    }
    let (c, r) = (at.0 as u32, at.1 as u32);
    let ground = map.ground.get(c, r).copied().unwrap_or_default();
    let kind = map
        .tile_kinds
        .get(ground.0 as usize)
        .map(String::as_str)
        .unwrap_or("empty");
    let elev = map.elevation.get(c, r).copied().unwrap_or(0);
    match (kind, elev) {
        ("empty", 0) => String::new(),
        ("empty", e) => format!(", elevation {e}"),
        (k, 0) => format!(", on {k}"),
        (k, e) => format!(", on {k} at elevation {e}"),
    }
}

fn describe(map: &MapDocument, t: &Token) -> String {
    format!(
        "{} ({}), facing {}, at ({}, {}){}",
        label(t),
        owner_word(t),
        facing_word(t.facing),
        t.at.0,
        t.at.1,
        tile_note(map, t.at),
    )
}

/// One token described in absolute terms: label, owner, facing, position,
/// and a terrain/elevation note. `None` if the id is not on the board.
pub fn describe_token(map: &MapDocument, id: TokenId) -> Option<String> {
    map.token(id).map(|t| describe(map, t))
}

/// The whole board described omnisciently (no fog): name, size, and every
/// token. The factual layer for recap and DM-side narration.
pub fn describe_scene(map: &MapDocument) -> String {
    let mut lines = vec![format!(
        "{}, a {}x{} board.",
        map.name,
        map.ground.width(),
        map.ground.height()
    )];
    if map.tokens.is_empty() {
        lines.push("No tokens on the board.".to_owned());
    } else {
        let n = map.tokens.len();
        lines.push(format!("{n} token{}:", if n == 1 { "" } else { "s" }));
        for t in &map.tokens {
            lines.push(format!("  {}", describe(map, t)));
        }
    }
    lines.join("\n")
}

/// The board from one token's eyes, fog-aware: what `viewer` can see out to
/// its sight, with every other visible token placed by distance and
/// bearing relative to the viewer. Tokens on unseen tiles are omitted (the
/// factual half of fog of war). `None` if `viewer` is not on the board.
pub fn describe_from(map: &MapDocument, viewer: TokenId, sight: &SightRules) -> Option<String> {
    let v = map.token(viewer)?;
    let seen = visible_from(map, v.at, sight);
    let mut lines = vec![format!(
        "From {}'s view at ({}, {}), facing {}:",
        label(v),
        v.at.0,
        v.at.1,
        facing_word(v.facing),
    )];
    let mut any = false;
    for t in &map.tokens {
        if t.id == viewer || !seen.contains(&t.at) {
            continue;
        }
        any = true;
        let d = distance(v.at, t.at);
        lines.push(format!(
            "  {} ({}) is {} {} to the {}, facing {}.",
            label(t),
            owner_word(t),
            d,
            tiles_word(d),
            bearing(v.at, t.at),
            facing_word(t.facing),
        ));
    }
    if !any {
        lines.push("  No other tokens are in sight.".to_owned());
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Facing, Token, TokenId};

    fn field(w: u32, h: u32) -> MapDocument {
        let mut m = MapDocument::new("Testfield", w, h);
        let grass = m.intern_tile_kind("grass");
        for r in 0..h {
            for c in 0..w {
                m.ground.set(c, r, grass);
            }
        }
        m
    }

    fn token(id: u32, at: TileCoord, facing: Facing, owner: Option<&str>) -> Token {
        Token {
            id: TokenId(id),
            at,
            facing,
            sprite: "knight".to_owned(),
            owner: owner.map(str::to_owned),
        }
    }

    #[test]
    fn bearing_is_eight_way_on_facing_axes() {
        assert_eq!(bearing((5, 5), (5, 5)), "here");
        assert_eq!(bearing((5, 5), (5, 2)), "north"); // decreasing row
        assert_eq!(bearing((5, 5), (5, 8)), "south");
        assert_eq!(bearing((5, 5), (8, 5)), "east"); // increasing col
        assert_eq!(bearing((5, 5), (2, 5)), "west");
        assert_eq!(bearing((5, 5), (8, 2)), "northeast");
        assert_eq!(bearing((5, 5), (2, 2)), "northwest");
        assert_eq!(bearing((5, 5), (8, 8)), "southeast");
        assert_eq!(bearing((5, 5), (2, 8)), "southwest");
    }

    #[test]
    fn describe_token_carries_the_facts() {
        let mut m = field(8, 8);
        m.elevation.set(2, 3, 2);
        m.tokens.push(token(1, (2, 3), Facing::East, Some("mark")));
        let s = describe_token(&m, TokenId(1)).unwrap();
        assert!(s.contains("knight 1"), "{s}");
        assert!(s.contains("(mark)"), "{s}");
        assert!(s.contains("facing east"), "{s}");
        assert!(s.contains("at (2, 3)"), "{s}");
        assert!(s.contains("on grass at elevation 2"), "{s}");
        assert!(describe_token(&m, TokenId(99)).is_none());
    }

    #[test]
    fn dm_owned_token_reads_as_dm() {
        let mut m = field(8, 8);
        m.tokens.push(token(2, (1, 1), Facing::South, None));
        let s = describe_token(&m, TokenId(2)).unwrap();
        assert!(s.contains("(DM)"), "{s}");
    }

    #[test]
    fn scene_lists_board_and_tokens() {
        let mut m = field(24, 24);
        m.tokens.push(token(1, (2, 3), Facing::East, Some("mark")));
        m.tokens.push(token(2, (10, 10), Facing::South, None));
        let s = describe_scene(&m);
        assert!(s.contains("Testfield, a 24x24 board."), "{s}");
        assert!(s.contains("2 tokens:"), "{s}");
        assert!(s.contains("knight 1"), "{s}");
        assert!(s.contains("knight 2"), "{s}");
    }

    #[test]
    fn empty_board_says_so() {
        let m = field(4, 4);
        assert!(describe_scene(&m).contains("No tokens on the board."));
    }

    fn rules(radius: u32) -> SightRules<'static> {
        SightRules {
            radius,
            opaque: &|kind| kind == "wall",
        }
    }

    #[test]
    fn from_view_places_a_visible_token_by_range_and_bearing() {
        let mut m = field(20, 20);
        m.tokens.push(token(1, (5, 5), Facing::North, Some("mark"))); // viewer
        m.tokens.push(token(2, (8, 5), Facing::West, None)); // 3 east, in sight
        let s = describe_from(&m, TokenId(1), &rules(4)).unwrap();
        assert!(s.contains("From knight 1's view at (5, 5), facing north:"), "{s}");
        assert!(s.contains("knight 2 (DM) is 3 tiles to the east, facing west."), "{s}");
    }

    #[test]
    fn from_view_omits_out_of_range_tokens() {
        let mut m = field(30, 30);
        m.tokens.push(token(1, (5, 5), Facing::North, None)); // viewer, radius 3
        m.tokens.push(token(2, (25, 25), Facing::South, None)); // far away
        let s = describe_from(&m, TokenId(1), &rules(3)).unwrap();
        assert!(s.contains("No other tokens are in sight."), "{s}");
    }

    #[test]
    fn from_view_omits_tokens_behind_a_wall() {
        let mut m = field(20, 20);
        let wall = m.intern_tile_kind("wall");
        m.props.set(7, 5, wall); // wall due east of the viewer
        m.tokens.push(token(1, (5, 5), Facing::East, None)); // viewer
        m.tokens.push(token(2, (9, 5), Facing::West, None)); // behind the wall
        let s = describe_from(&m, TokenId(1), &rules(6)).unwrap();
        assert!(s.contains("No other tokens are in sight."), "{s}");
    }

    #[test]
    fn singular_tile_reads_naturally() {
        let mut m = field(10, 10);
        m.tokens.push(token(1, (5, 5), Facing::North, None));
        m.tokens.push(token(2, (6, 5), Facing::West, None)); // 1 east, adjacent
        let s = describe_from(&m, TokenId(1), &rules(3)).unwrap();
        assert!(s.contains("is 1 tile to the east"), "{s}");
    }
}
