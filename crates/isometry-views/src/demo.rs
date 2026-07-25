//! A hand-authored demo board for the I1 receipts: a grass field with a
//! lake, a stepped hill (probe P3, depth sort under elevation), a stone
//! path, scattered trees, and two tokens.
//!
//! Also the synthetic fixtures the stress receipts scale up: [`synth_map`]
//! sizes the *board*, [`synth_world`] the *campaign*. They measure different
//! costs and neither substitutes for the other.

use isometry_campaign::{
    CampaignWorld, RoleSlot, StoryletProposal, StoryletRequirements, WorldCharacter, WorldFaction,
    WorldLaw, WorldPlace, WorldRoute,
};
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

/// A synthetic stress board: `w` x `h` with every layer loaded (ground
/// everywhere, props on a third of the tiles, elevation over the lower
/// half) plus 20 scattered tokens. At 30x30 this is the ~2,700-element
/// probe P2 board; larger sizes exercise viewport windowing (the emitted
/// element count should stay bounded by the pane, not the board).
/// `ISOMETRY_SYNTH=<n>` loads an n x n board (n>1; default 30).
pub fn synth_map(w: u32, h: u32) -> MapDocument {
    let mut map = MapDocument::new(format!("Synthetic {w}x{h}"), w, h);
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
            if row >= h / 2 {
                map.elevation.set(col, row, ((col + row) % 4) as u8);
            }
        }
    }
    for i in 0..20u32 {
        map.tokens.push(Token {
            id: TokenId(i + 1),
            at: (((i * 7) % w) as i32, ((i * 13) % h) as i32),
            facing: Facing::South,
            sprite: if i % 2 == 0 { "knight" } else { "goblin" }.to_owned(),
            owner: None,
        });
    }
    map
}

/// The party [`synth_world`] builds around: it seeds this owner's position and
/// discovered map, so a scaled receipt reads the same party the host would.
pub const SYNTH_PARTY: &str = "dm";

/// A synthetic stress *world*: `places` sites on a rough square grid, routed to
/// their right and lower neighbors, with one notable per site, a handful of
/// factions and laws, and `storylets` opportunities. [`SYNTH_PARTY`] knows
/// every place and stands on the first, so the whole map is drawable (an
/// undiscovered world would measure the filter, not the projection).
///
/// Sites carry no authored `position`, matching the shipped default: neither
/// the generator nor the demo seed sets one, so the overmap takes the
/// unauthored arrangement, the same layout path a real session runs.
///
/// The demo campaign is five places, which is why the 2026-07-20 perf fixes
/// measured below the noise floor: every cost they removed scales with world
/// size and the demo has none to scale with. This is the world that makes them
/// measurable. `tests/scaled_world.rs` is the receipt.
pub fn synth_world(places: usize, storylets: usize) -> CampaignWorld {
    let mut world = CampaignWorld::default();
    // Grid side, so route count tracks place count (~2 routes per site) rather
    // than exploding quadratically. A pointcrawl is sparse; a dense one would
    // measure a graph nobody plays.
    let side = (places as f64).sqrt().ceil().max(1.0) as usize;

    for i in 0..places {
        let place = WorldPlace {
            id: format!("p{i}"),
            name: format!("Site {i}"),
            tags: vec![["wood", "hill", "water", "ruin"][i % 4].to_owned()],
            map: None,
            position: None,
        };
        world.places.insert(place.id.clone(), place);

        let character = WorldCharacter {
            id: format!("c{i}"),
            name: format!("Notable {i}"),
            // "elder" is deliberately rare: a role asking for it scans deep into
            // the cast before it casts, which is the cost a storylet refresh pays.
            tags: match i % 7 {
                0 => vec!["notable".to_owned(), "elder".to_owned()],
                _ => vec!["notable".to_owned()],
            },
            faction: Some(format!("f{}", i % side)),
            place: Some(format!("p{i}")),
        };
        world.characters.insert(character.id.clone(), character);

        let right = (i % side + 1 < side).then_some(i + 1);
        let down = Some(i + side);
        for (ordinal, neighbor) in [right, down]
            .into_iter()
            .flatten()
            .filter(|&n| n < places)
            .enumerate()
        {
            let route = WorldRoute {
                id: format!("r{i}_{ordinal}"),
                from: format!("p{i}"),
                to: format!("p{neighbor}"),
                tags: Vec::new(),
                weight: (i % 5 + 1) as u32,
            };
            world.routes.insert(route.id.clone(), route);
        }
    }

    for i in 0..side {
        let faction = WorldFaction {
            id: format!("f{i}"),
            name: format!("House {i}"),
            tags: vec!["settled".to_owned()],
            claims: vec![format!("p{i}")],
        };
        world.factions.insert(faction.id.clone(), faction);

        let law = WorldLaw {
            id: format!("l{i}"),
            name: format!("Custom {i}"),
            text: "A rule of the setting.".to_owned(),
            tags: Vec::new(),
            parameters: Default::default(),
        };
        world.laws.insert(law.id.clone(), law);
    }

    for i in 0..storylets {
        // Three shapes in rotation, so a refresh pays the real mix rather than
        // one easy case: casts immediately, casts only after scanning deep, and
        // never casts (the locked row a table still sees listed).
        let (requirements, roles) = match i % 3 {
            0 => (
                StoryletRequirements {
                    faction_tags: vec!["settled".to_owned()],
                    ..StoryletRequirements::default()
                },
                vec![RoleSlot {
                    key: "any".to_owned(),
                    tags: vec!["notable".to_owned()],
                }],
            ),
            1 => (
                StoryletRequirements {
                    world_laws: vec!["l0".to_owned()],
                    ..StoryletRequirements::default()
                },
                vec![
                    RoleSlot {
                        key: "elder".to_owned(),
                        tags: vec!["elder".to_owned()],
                    },
                    RoleSlot {
                        key: "second".to_owned(),
                        tags: vec!["notable".to_owned()],
                    },
                ],
            ),
            _ => (
                StoryletRequirements {
                    hidden_facts: vec![format!("secret{i}")],
                    ..StoryletRequirements::default()
                },
                Vec::new(),
            ),
        };
        let storylet = StoryletProposal {
            key: format!("s{i}"),
            entry: format!("Opportunity {i}"),
            tags: Vec::new(),
            requirements,
            roles,
            effects: Vec::new(),
        };
        world.storylets.insert(storylet.key.clone(), storylet);
    }

    world.party_node.insert(SYNTH_PARTY.to_owned(), "p0".to_owned());
    for i in 0..places {
        world.reveal(SYNTH_PARTY, &format!("p{i}"));
    }
    world
}
