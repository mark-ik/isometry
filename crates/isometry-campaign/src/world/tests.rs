//! Tests for this module, split out on 2026-07-24; unchanged.

use super::*;

#[test]
fn storylet_requires_private_fact_and_casts_existing_character() {
    let mut world = CampaignWorld::default();
    world.factions.insert(
        "tide".into(),
        WorldFaction {
            id: "tide".into(),
            name: "Tide Court".into(),
            tags: vec!["river".into()],
            claims: vec![],
        },
    );
    world.characters.insert(
        "mara".into(),
        WorldCharacter {
            id: "mara".into(),
            name: "Mara".into(),
            tags: vec!["warden".into()],
            faction: Some("tide".into()),
            place: None,
        },
    );
    world.laws.insert(
        "iron-remembers".into(),
        WorldLaw {
            id: "iron-remembers".into(),
            name: "Iron remembers".into(),
            text: "Iron keeps the name of its maker.".into(),
            tags: vec!["magic".into()],
            parameters: BTreeMap::new(),
        },
    );
    let storylet = StoryletProposal {
        key: "sunken-vow".into(),
        entry: "The old oath surfaces.".into(),
        tags: vec![],
        requirements: StoryletRequirements {
            faction_tags: vec!["river".into()],
            hidden_facts: vec!["vow.secret".into()],
            world_laws: vec!["iron-remembers".into()],
        },
        roles: vec![RoleSlot {
            key: "warden".into(),
            tags: vec!["warden".into()],
        }],
        effects: vec![],
    };
    assert!(matches!(
        world.resolve_storylet(&storylet, []),
        Err(StoryletError::MissingHiddenFact(_))
    ));
    let resolved = world.resolve_storylet(&storylet, ["vow.secret"]).unwrap();
    assert_eq!(resolved.cast["warden"], "mara");
}

#[test]
fn campaign_draft_rejects_duplicate_map_ids_before_commit() {
    let mut world = CampaignWorld::default();
    world.storylets.insert(
        "finale".into(),
        StoryletProposal {
            key: "finale".into(),
            entry: "Finale".into(),
            tags: vec![],
            requirements: Default::default(),
            roles: vec![],
            effects: vec![],
        },
    );
    let map = LocalMapProposal {
        id: "same".into(),
        name: "Same".into(),
        width: 2,
        height: 2,
        default_ground: "grass".into(),
        cells: vec![],
        spawn_zones: vec![],
        transitions: vec![],
        encounter_anchors: vec![],
    };
    let draft = CampaignDraft {
        id: "draft".into(),
        name: "Draft".into(),
        world,
        maps: vec![
            DraftMap {
                scale: MapScale::Region,
                map: map.clone(),
            },
            DraftMap {
                scale: MapScale::Local,
                map,
            },
        ],
        secrets: vec![],
        rewards: vec![],
        starting_map: "same".into(),
        final_storylet: "finale".into(),
    };
    assert_eq!(
        draft.validate(),
        Err(WorldError::DuplicateMap("same".into()))
    );
}

fn place(id: &str, name: &str) -> WorldPlace {
    WorldPlace {
        id: id.into(),
        name: name.into(),
        tags: vec![],
        map: None,
        position: None,
    }
}

fn route(id: &str, from: &str, to: &str, weight: u32) -> WorldRoute {
    WorldRoute {
        id: id.into(),
        from: from.into(),
        to: to.into(),
        tags: vec![],
        weight,
    }
}

#[test]
fn the_overmap_projects_from_places_and_routes() {
    let mut world = CampaignWorld::default();
    for (id, name) in [("village", "Village"), ("forest", "Forest"), ("ruins", "Ruins")] {
        world.places.insert(id.into(), place(id, name));
    }
    // The forest opens into a tactical map; the node carries it as its site.
    world.places.get_mut("forest").unwrap().map = Some("forest-map".into());
    world.routes.insert("r1".into(), route("r1", "village", "forest", 2));
    world.routes.insert("r2".into(), route("r2", "forest", "ruins", 3));
    // An unweighted route (weight 0) still costs 1 once projected.
    world.routes.insert("r3".into(), route("r3", "village", "ruins", 0));

    let overmap = world.overmap();
    assert_eq!(overmap.nodes.len(), 3, "a node per place");
    assert_eq!(
        overmap.node("forest").and_then(|n| n.site.as_deref()),
        Some("forest-map"),
        "a place's tactical map becomes the node's site"
    );
    // The direct village->ruins route projects to cost 1 (weight 0 -> 1),
    // cheaper than through the forest (5). Pathfinding runs on the projection.
    let (path, cost) = overmap.route("village", "ruins").expect("the ruins are reachable");
    assert_eq!(path, vec!["village", "ruins"]);
    assert_eq!(cost, 1, "an unweighted route projects to unit cost");
}

#[test]
fn a_party_sits_on_an_overmap_node_and_travels() {
    let mut world = CampaignWorld::default();
    world.places.insert("village".into(), place("village", "Village"));
    world.places.insert("forest".into(), place("forest", "Forest"));
    world.routes.insert("r1".into(), route("r1", "village", "forest", 2));

    assert_eq!(world.party_at("A"), None, "the party starts off the map");
    world
        .apply(&WorldEvent::PartyMoved {
            party: "A".into(),
            node: "village".into(),
        })
        .unwrap();
    assert_eq!(world.party_at("A"), Some("village"));
    // The projected overmap says the forest is reachable, so travel there.
    assert!(world.overmap().route("village", "forest").is_some());
    world
        .apply(&WorldEvent::PartyMoved {
            party: "A".into(),
            node: "forest".into(),
        })
        .unwrap();
    assert_eq!(world.party_at("A"), Some("forest"), "the party travelled the edge");
}

#[test]
fn pace_scales_the_travel_cost() {
    let mut world = CampaignWorld::default();
    world.places.insert("village".into(), place("village", "Village"));
    world.places.insert("forest".into(), place("forest", "Forest"));
    world.routes.insert("r1".into(), route("r1", "village", "forest", 4));

    // Default pace is normal (100%): the cost is the route's weight.
    assert_eq!(world.pace("A"), 100);
    assert_eq!(world.travel_cost("A", "village", "forest"), Some(4));

    // Fast (50%) halves the time; slow (200%) doubles it. Same edge, same
    // party, different ticks.
    world
        .apply(&WorldEvent::PartyPaceSet { party: "A".into(), pace: 50 })
        .unwrap();
    assert_eq!(world.travel_cost("A", "village", "forest"), Some(2), "fast is half the time");
    world
        .apply(&WorldEvent::PartyPaceSet { party: "A".into(), pace: 200 })
        .unwrap();
    assert_eq!(world.travel_cost("A", "village", "forest"), Some(8), "slow is double");

    // A cost never rounds to zero, and an unreachable destination has none.
    assert_eq!(world.travel_cost("A", "village", "atlantis"), None);
}

#[test]
fn a_party_discovers_the_overmap_as_it_travels() {
    let mut world = CampaignWorld::default();
    for id in ["village", "forest", "ruins", "island"] {
        world.places.insert(id.into(), place(id, id));
    }
    world.routes.insert("r1".into(), route("r1", "village", "forest", 2));
    world.routes.insert("r2".into(), route("r2", "forest", "ruins", 2));
    // The island has no route to it.

    // A party that knows nothing sees an empty overmap.
    assert!(world.overmap_for("A").nodes.is_empty(), "the unfound map is dark");
    assert!(!world.knows("A", "village"));

    // Arriving at the village discovers it and its neighbour (the forest),
    // but not what is two steps on (the ruins).
    world
        .apply(&WorldEvent::PartyMoved { party: "A".into(), node: "village".into() })
        .unwrap();
    assert!(world.knows("A", "village"));
    assert!(world.knows("A", "forest"), "and one step on");
    assert!(!world.knows("A", "ruins"), "but not two steps on");
    // The known overmap shows only what has been found, and refuses to route
    // through the dark.
    let known = world.overmap_for("A");
    assert_eq!(known.nodes.len(), 2);
    assert!(known.route("village", "ruins").is_none(), "cannot plot a course into the unknown");

    // Travel on to the forest, and the ruins come into view.
    world
        .apply(&WorldEvent::PartyMoved { party: "A".into(), node: "forest".into() })
        .unwrap();
    assert!(world.knows("A", "ruins"), "arriving at the forest reveals the ruins");

    // A rumour reveals the island directly, though no road leads there.
    world
        .apply(&WorldEvent::NodeRevealed { party: "A".into(), node: "island".into() })
        .unwrap();
    assert!(world.knows("A", "island"), "word of mouth reaches the unreachable");
}
