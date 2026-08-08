//! Doorway crossings as resolved payloads (H1).
//!
//! The receipts for H1's done-conditions: one crossing produces one verdict
//! naming every consequence; peers apply it without deriving anything; a peer
//! that arrives after the crossing rebuilds the same state from the log alone;
//! and taking the same crossing twice takes it once.
//!
//! They live apart from `replication.rs` because they prove properties of the
//! travel *payload* rather than of replication: what the authority says, and
//! what a peer is thereby spared from working out.

use isometry_campaign::{
    CampaignMap, CampaignStore, EquipmentSlot, Inventory, ItemId, ItemInstance, MapPoint, MapScale,
    MapTransition,
};
use isometry_core::{
    Facing, MapDocument, SessionEvent, SheetData, Token, TokenId, TurnList,
};
use isometry_net::sim::Sim;
use isometry_net::{
    GameError, GameEvent, GameSnapshot, GameSourceHistory, HostSession, PeerId, RequestId,
    TransitionResolved, apply_game, resolve_transition,
};

/// Two prepared maps joined by a door. `field` holds A's knight and a goblin
/// that is DM furniture, so the knight is the last player out and the board
/// follows it through; `hut`'s entry door faces back.
fn origin() -> GameSnapshot {
    let mut field_doc = MapDocument::new("field", 8, 8);
    let grass = field_doc.intern_tile_kind("grass");
    for r in 0..8 {
        for c in 0..8 {
            field_doc.ground.set(c, r, grass);
        }
    }
    field_doc.tokens.push(Token {
        id: TokenId(1),
        at: (1, 1),
        facing: Facing::South,
        sprite: "knight".to_owned(),
        owner: Some("A".to_owned()),
    });
    field_doc.tokens.push(Token {
        id: TokenId(2),
        at: (6, 6),
        facing: Facing::North,
        sprite: "goblin".to_owned(),
        owner: None,
    });
    let mut hut_doc = MapDocument::new("hut", 6, 6);
    let stone = hut_doc.intern_tile_kind("stone");
    for r in 0..6 {
        for c in 0..6 {
            hut_doc.ground.set(c, r, stone);
        }
    }

    let field = CampaignMap {
        id: "field".to_owned(),
        scale: MapScale::Local,
        document: field_doc.clone(),
        spawn_zones: Vec::new(),
        transitions: vec![MapTransition {
            id: "field-gate".to_owned(),
            at: MapPoint { col: 3, row: 3 },
            target_map: "hut".to_owned(),
            target_entry: Some("hut-door".to_owned()),
        }],
        encounter_anchors: Vec::new(),
    };
    let hut = CampaignMap {
        id: "hut".to_owned(),
        scale: MapScale::Local,
        document: hut_doc,
        spawn_zones: Vec::new(),
        transitions: vec![MapTransition {
            id: "hut-door".to_owned(),
            at: MapPoint { col: 1, row: 1 },
            target_map: "field".to_owned(),
            target_entry: Some("field-gate".to_owned()),
        }],
        encounter_anchors: Vec::new(),
    };

    let mut state = GameSnapshot {
        map: field_doc,
        turns: TurnList::new(),
        roll_log: Vec::new(),
        journal: Vec::new(),
        inventories: Default::default(),
        generations: Vec::new(),
        maps: Default::default(),
        active_map: Some("field".to_owned()),
        world: Default::default(),
        clocks: Default::default(),
        party_cap: isometry_net::default_party_cap(),
        last_beats: Vec::new(),
        beat_seq: 0,
        applied_actions: Default::default(),
    };
    state.maps.insert("field".to_owned(), field);
    state.maps.insert("hut".to_owned(), hut);
    state
}

fn sheet(name: &str, hp: i64) -> SheetData {
    let mut s = SheetData::new("5e-srd");
    s.set_text("name", name);
    s.set_int("hp_current", hp);
    s.set_int("hp_max", hp);
    s.set_int("ac", 16);
    s
}

fn sword() -> Inventory {
    let mut inventory = Inventory::default();
    inventory
        .insert(ItemInstance {
            id: ItemId::new("kit.sword"),
            template: "srd5e:longsword".to_owned(),
            name: "Fine Longsword".to_owned(),
            quantity: 1,
            tags: vec!["weapon".to_owned()],
            modifiers: Vec::new(),
            appearance_layers: vec!["weapon:longsword".to_owned()],
        })
        .unwrap();
    inventory
        .equip(EquipmentSlot::MainHand, ItemId::new("kit.sword"))
        .unwrap();
    inventory
}

fn walk_to_the_gate(sim: &mut Sim) {
    sim.host_event(GameEvent::Map(SessionEvent::TokenMoved {
        id: TokenId(1),
        to: (3, 3),
    }));
}

/// Rule the crossing the knight is standing in, without committing it.
fn rule(sim: &Sim, nonce: u64) -> TransitionResolved {
    resolve_transition(sim.host.state(), TokenId(1), RequestId::host(nonce))
        .expect("the knight is standing on the gate")
}

fn assert_converged(sim: &Sim) {
    for (peer, client) in &sim.clients {
        assert_eq!(
            client.state(),
            Some(sim.host.state()),
            "client {peer:?} state diverged"
        );
        assert_eq!(
            client.log_hash(),
            sim.host.log_hash(),
            "client {peer:?} log hash diverged"
        );
        assert_eq!(
            client.applied(),
            sim.host.seq(),
            "client {peer:?} seq diverged"
        );
    }
}

#[test]
fn a_doorway_crossing_names_every_consequence() {
    let mut sim = Sim::new(HostSession::new(origin()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(1),
        sheet: sheet("Knight", 12),
    });
    sim.host_event(GameEvent::ConditionSet {
        token: TokenId(1),
        condition: "prone".to_owned(),
        value: 1,
        mobility: Some((2, 6)),
    });
    // Five ticks pass in the field while the hut sits at zero.
    sim.host_event(GameEvent::TimeAdvanced { ticks: 5 });
    walk_to_the_gate(&mut sim);

    // Every field below is one thing a peer used to work out for itself from
    // `Traveled { token }`: which door, which map, which tile, which id, which
    // inventories, what the clocks became, and whether the board followed.
    let ruled = rule(&sim, 1);
    assert_eq!(
        ruled,
        TransitionResolved {
            request: RequestId::host(1),
            token: TokenId(1),
            from_map: "field".to_owned(),
            to_map: "hut".to_owned(),
            // The hut's named entry door, free, so the outward scan stops there.
            landing: (1, 1),
            // Nobody in the hut holds id 1, so the knight keeps its own.
            arrival: TokenId(1),
            // Nothing carried, nothing to re-key.
            inventory_remaps: Vec::new(),
            // Nobody arrives before they left: the hut is pulled up to 5.
            destination_clock: 5,
            // The goblin is DM furniture, so the knight is the last player out.
            activated: Some("hut".to_owned()),
        },
        "the crossing names every consequence the old derivation computed"
    );

    // And applying it lands exactly what the derivation used to land.
    sim.host_event(GameEvent::TransitionResolved(ruled));
    let host = sim.host.state();
    assert_eq!(host.active_map.as_deref(), Some("hut"));
    let knight = host.map.token(TokenId(1)).expect("knight in the hut");
    assert_eq!(knight.at, (1, 1));
    assert_eq!(
        host.map.sheet(TokenId(1)).and_then(|s| s.int("hp_current")),
        Some(12),
        "the traveler is still itself on the other side of the door"
    );
    assert!(host.map.has_condition(TokenId(1), "prone"));
    assert_eq!(host.map.effective_mobility(TokenId(1), (5, 6)), (2, 6));
    assert_eq!(host.clocks.get("hut").copied(), Some(5));
    let field = &host.maps["field"].document;
    assert!(field.token(TokenId(1)).is_none(), "it left the field");
    assert!(field.token(TokenId(2)).is_some(), "the furniture stayed home");
    assert_converged(&sim);
}

#[test]
fn an_identity_collision_is_named_with_the_inventory_that_follows_it() {
    let mut base = origin();
    // The hut already has a resident holding the knight's id.
    base.maps.get_mut("hut").unwrap().document.tokens.push(Token {
        id: TokenId(1),
        at: (4, 4),
        facing: Facing::South,
        sprite: "goblin".to_owned(),
        owner: None,
    });
    let mut sim = Sim::new(HostSession::new(base));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::InventorySet {
        token: TokenId(1),
        inventory: sword(),
    });
    walk_to_the_gate(&mut sim);

    let ruled = rule(&sim, 1);
    // Ids are per-map but inventories key on them globally, so the replacement
    // and the re-key are two named consequences of one crossing.
    assert_eq!(ruled.arrival, TokenId(3), "minted above every id in play");
    assert_eq!(ruled.inventory_remaps, vec![(TokenId(1), TokenId(3))]);
    assert_eq!(ruled.landing, (1, 1), "the entry door is still free");

    sim.host_event(GameEvent::TransitionResolved(ruled));
    let host = sim.host.state();
    assert!(host.inventories.contains_key(&TokenId(3)), "the sword crossed");
    assert!(!host.inventories.contains_key(&TokenId(1)));
    let resident = host.map.token(TokenId(1)).expect("the resident kept its id");
    assert_eq!(resident.sprite, "goblin");
    assert_eq!(host.map.token(TokenId(3)).map(|t| t.sprite.as_str()), Some("knight"));
    assert_converged(&sim);
}

#[test]
fn a_late_joiner_reconstructs_the_crossing_from_the_log_alone() {
    // H1's headline receipt, and the one the adjudication plan never landed. A
    // crossing is now a fact in the log rather than an instruction to re-derive
    // one, so a peer that was not in the room when it happened reaches the same
    // state two independent ways: seeded from a snapshot and fed the tail, and
    // replayed from the origin over nothing but the ordered log.
    let mut sim = Sim::new(HostSession::new(origin()));
    sim.connect(PeerId(10));
    sim.client_hello(PeerId(10), "A");
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(1),
        sheet: sheet("Knight", 12),
    });
    sim.host_event(GameEvent::TimeAdvanced { ticks: 5 });
    walk_to_the_gate(&mut sim);
    sim.host_event(GameEvent::TransitionResolved(rule(&sim, 1)));
    assert_eq!(sim.host.state().active_map.as_deref(), Some("hut"));

    // One: a peer that arrives *after* the crossing.
    let hash_at_join = sim.host.log_hash();
    let seq_at_join = sim.host.seq();
    sim.connect(PeerId(20));
    let joiner = &sim.clients[&PeerId(20)];
    assert_eq!(joiner.state(), Some(sim.host.state()), "the arrival is state");
    assert_eq!(joiner.log_hash(), hash_at_join);
    assert_eq!(joiner.applied(), seq_at_join);

    // Two: the log alone. Origin plus entries, replayed by a machine that saw
    // none of it happen, with no snapshot to copy from.
    let history = sim.host.history().clone();
    let source = GameSourceHistory::new(origin(), history.clone());
    let replayed = source
        .snapshot_at(source.live_cursor())
        .expect("the log replays over its own origin");
    assert_eq!(
        &replayed,
        sim.host.state(),
        "log-alone replay diverged from the authority"
    );
    let rebuilt = HostSession::with_history(replayed, CampaignStore::new(), history);
    assert_eq!(
        rebuilt.log_hash(),
        sim.host.log_hash(),
        "the rebuilt authority's log hash diverged"
    );
    assert_eq!(rebuilt.seq(), sim.host.seq());

    // And play carries on, with the late joiner converged on the peer that saw
    // the whole thing.
    sim.host_event(GameEvent::TurnAdd(TokenId(1)));
    sim.host_event(GameEvent::TurnAdvance);
    assert_converged(&sim);
}

#[test]
fn applying_a_crossing_reads_no_door_table() {
    // The sharp edge of apply-only. Re-author the doors out of the world and
    // replay: the derivation would have refused (nobody is standing on a door
    // any more, and there is no target to look up), while a payload that names
    // its own ends lands identically. Which is the proof that applying one
    // consults no transition table at all.
    let mut sim = Sim::new(HostSession::new(origin()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::TimeAdvanced { ticks: 5 });
    walk_to_the_gate(&mut sim);
    sim.host_event(GameEvent::TransitionResolved(rule(&sim, 1)));

    let mut doorless = origin();
    for map in doorless.maps.values_mut() {
        map.transitions.clear();
    }
    let replayed = GameSourceHistory::new(doorless, sim.host.history().clone())
        .snapshot_at(sim.host.seq())
        .expect("a crossing applies over a world with no doors left");

    assert_eq!(replayed.active_map.as_deref(), Some("hut"));
    assert_eq!(
        replayed.map.token(TokenId(1)).map(|t| t.at),
        Some((1, 1)),
        "the landing tile came from the payload, not from a door lookup"
    );
    assert_eq!(replayed.clocks.get("hut").copied(), Some(5));
    assert!(replayed.maps["field"].document.token(TokenId(1)).is_none());
}

#[test]
fn a_repeated_crossing_is_a_no_op() {
    // The H0 ledger, over the H1 payload: a crossing is taken once, by the
    // identity the authority stamped on it. Without this a retransmit would
    // walk the knight through the same door twice, and the second walk would
    // find it standing somewhere else entirely.
    let mut sim = Sim::new(HostSession::new(origin()));
    sim.connect(PeerId(10));
    walk_to_the_gate(&mut sim);
    let ruled = rule(&sim, 1);
    sim.host_event(GameEvent::TransitionResolved(ruled.clone()));

    let after_first = sim.host.state().clone();
    let mut again = after_first.clone();
    apply_game(&mut again, &GameEvent::TransitionResolved(ruled.clone()))
        .expect("a repeat is accepted and ignored, not rejected");
    assert_eq!(again, after_first, "the second application moved something");

    // And by identity, not by content: the same crossing renumbered is a second
    // verdict, so it is judged on its merits rather than swallowed. Here it is
    // refused because the knight now stands in the hut and no longer departs
    // the field -- a refusal, which a duplicate would never have produced.
    let mut renumbered = ruled;
    renumbered.request = RequestId::host(2);
    let mut third = after_first.clone();
    assert_eq!(
        apply_game(&mut third, &GameEvent::TransitionResolved(renumbered)),
        Err(GameError::UnknownMap("field".to_owned())),
        "a fresh request id must be judged, not swallowed as a duplicate"
    );
    assert_eq!(third, after_first, "a refused crossing half-applied");
}
