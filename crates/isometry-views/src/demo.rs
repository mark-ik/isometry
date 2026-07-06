//! A hand-authored demo board for the I1 receipts: a grass field with a
//! lake, a stepped hill (probe P3, depth sort under elevation), a stone
//! path, scattered trees, and two tokens.

use isometry_core::{Facing, MapDocument, Token, TokenId};

pub fn demo_map() -> MapDocument {
    let (w, h) = (24u32, 24u32);
    let mut map = MapDocument::new("Demo skirmish", w, h);
    let grass = map.intern_tile_kind("grass");
    let water = map.intern_tile_kind("water");
    let stone = map.intern_tile_kind("stone");
    let tree = map.intern_tile_kind("tree");

    for row in 0..h {
        for col in 0..w {
            map.ground.set(col, row, grass);
        }
    }

    // A lake in the southwest.
    for row in 14..20 {
        for col in 3..9 {
            let dc = col as i32 - 6;
            let dr = row as i32 - 17;
            if dc * dc + dr * dr <= 7 {
                map.ground.set(col, row, water);
            }
        }
    }

    // A stepped hill in the northeast, tallest at the crown (P3: the
    // crown must cover tiles behind it; the goblin stands on a step).
    for row in 4..12 {
        for col in 12..20 {
            let dc = (col as i32 - 16).abs();
            let dr = (row as i32 - 8).abs();
            let d = dc.max(dr);
            if d <= 3 {
                map.elevation.set(col, row, (3 - d) as u8 + 1);
            }
        }
    }

    // A stone path east-west through the middle.
    for col in 0..w {
        map.ground.set(col, 12, stone);
        map.ground.set(col, 13, stone);
    }

    // Scattered trees on flat grass, deterministic.
    for row in 0..h {
        for col in 0..w {
            let flat = *map.elevation.get(col, row).unwrap_or(&0) == 0;
            let grassy = map.ground.get(col, row) == Some(&grass);
            if flat && grassy && (col * 7 + row * 13) % 23 == 0 {
                map.props.set(col, row, tree);
            }
        }
    }

    map.tokens.push(Token {
        id: TokenId(1),
        at: (10, 14),
        facing: Facing::East,
        sprite: "knight".to_owned(),
        owner: Some("player".to_owned()),
    });
    map.tokens.push(Token {
        id: TokenId(2),
        at: (15, 8),
        facing: Facing::West,
        sprite: "goblin".to_owned(),
        owner: None,
    });
    map
}
