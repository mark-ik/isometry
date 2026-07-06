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

    // Two hot-seat sides: knights (A) vs goblins (B).
    for (id, at, sprite, owner) in [
        (1, (10, 14), "knight", "A"),
        (3, (9, 15), "knight", "A"),
        (2, (15, 8), "goblin", "B"),
        (4, (16, 9), "goblin", "B"),
    ] {
        map.tokens.push(Token {
            id: TokenId(id),
            at,
            facing: if sprite == "knight" {
                Facing::East
            } else {
                Facing::West
            },
            sprite: sprite.to_owned(),
            owner: Some(owner.to_owned()),
        });
    }
    map
}

/// The probe P2 synthetic: a 30x30 board with every layer loaded
/// (ground everywhere, props on a third of the tiles, elevation over
/// half the board) plus 20 tokens, roughly 2,700 elements once the
/// elevation columns render. `ISOMETRY_SYNTH=1` loads it.
pub fn synth_map() -> MapDocument {
    let (w, h) = (30u32, 30u32);
    let mut map = MapDocument::new("Synthetic 30x30x3", w, h);
    let grass = map.intern_tile_kind("grass");
    let water = map.intern_tile_kind("water");
    let stone = map.intern_tile_kind("stone");
    let tree = map.intern_tile_kind("tree");
    for row in 0..h {
        for col in 0..w {
            let kind = match (col + row) % 5 {
                0 => water,
                1 => stone,
                _ => grass,
            };
            map.ground.set(col, row, kind);
            if (col * 3 + row * 7) % 3 == 0 {
                map.props.set(col, row, tree);
            }
            if row >= 15 {
                map.elevation.set(col, row, ((col + row) % 4) as u8);
            }
        }
    }
    for i in 0..20u32 {
        map.tokens.push(Token {
            id: TokenId(i + 1),
            at: ((i % 10) as i32 * 3, (i / 10) as i32 * 9 + 3),
            facing: Facing::South,
            sprite: if i % 2 == 0 { "knight" } else { "goblin" }.to_owned(),
            owner: None,
        });
    }
    map
}
