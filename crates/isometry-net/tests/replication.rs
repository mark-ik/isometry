//! End-to-end replication over the in-process sim: the same routing the
//! iroh transport performs, minus the wire. Proves the I4 done-condition
//! (mid-session join, no divergence: state and log hashes match) without
//! two machines.

use std::collections::BTreeMap;

use isometry_campaign::{
    CampaignDraft, CampaignMap, CampaignWorld, DraftMap, EncounterAnchor, EntropyTape,
    EquipmentSlot, GenValue, GenerationRecord, GeneratorRequest, HiddenItemModifier, HistoryEvent,
    Inventory, ItemId, ItemInstance, ItemModifier, ItemModifierKind, ItemProposal,
    LocalMapProposal, MapCellProposal, MapPoint, MapScale, MapTransition, RevealCondition,
    RoleSlot, SecretFact, SpawnZone, StoryletEffect, StoryletProposal, StoryletRequirements,
    WorldCharacter, WorldEvent, WorldFact, WorldFaction, WorldLaw,
};
use isometry_core::{
    Beat, Facing, MapDocument, RollRecord, SessionEvent, SheetData, SheetDelta, Token, TokenId,
    TurnList,
};
use isometry_net::sim::Sim;
use isometry_net::{ActionIntent, ActionResolved, GameEvent, GameSnapshot, HostSession, PeerId};

fn snapshot() -> GameSnapshot {
    let mut map = MapDocument::new("net demo", 8, 8);
    let grass = map.intern_tile_kind("grass");
    for r in 0..8 {
        for c in 0..8 {
            map.ground.set(c, r, grass);
        }
    }
    map.tokens.push(Token {
        id: TokenId(1),
        at: (1, 1),
        facing: Facing::South,
        sprite: "knight".to_owned(),
        owner: Some("A".to_owned()),
    });
    map.tokens.push(Token {
        id: TokenId(2),
        at: (6, 6),
        facing: Facing::North,
        sprite: "goblin".to_owned(),
        owner: Some("B".to_owned()),
    });
    GameSnapshot {
        map,
        turns: TurnList::new(),
        roll_log: Vec::new(),
        journal: Vec::new(),
        inventories: Default::default(),
        generations: Vec::new(),
        maps: Default::default(),
        active_map: None,
        world: Default::default(),
        clocks: Default::default(),

        party_cap: isometry_net::default_party_cap(),
        last_beats: Vec::new(),
        beat_seq: 0,
    }
}

/// Bind a sheet to a token so it can take part in an adjudicated action.
fn sheet(name: &str, hp: i64, ac: i64) -> SheetData {
    let mut s = SheetData::new("5e-srd");
    s.set_text("name", name);
    s.set_int("hp_current", hp);
    s.set_int("hp_max", hp);
    s.set_int("ac", ac);
    s
}

/// A hit for `damage` on the goblin, shaped exactly as the system resolver
/// produces it. The net crate never builds one of these itself: it only carries
/// what the rules decided.
fn attack_hit(damage: i64) -> GameEvent {
    GameEvent::ActionResolved(ActionResolved {
        actor: TokenId(1),
        target: TokenId(2),
        action_key: "attack".to_owned(),
        label: "Attack".to_owned(),
        attack: RollRecord {
            by: "Knight".to_owned(),
            expr: "1d20+5".to_owned(),
            dice: vec![14],
            total: 19,
        },
        hit: true,
        damage: Some(RollRecord {
            by: "Knight".to_owned(),
            expr: "1d8+3".to_owned(),
            dice: vec![4],
            total: damage as i32,
        }),
        deltas: vec![SheetDelta {
            token: TokenId(2),
            key: "hp_current".to_owned(),
            add: -damage,
        }],
        beats: vec![
            Beat::new(TokenId(1), "strike"),
            Beat::new(TokenId(2), "recoil"),
        ],
        defeated: Vec::new(),
        displaced: Vec::new(),
        conditions: Vec::new(),
        mobility: Vec::new(),
        owner_changes: Vec::new(),
        turn_counters: Vec::new(),
    })
}

fn generation_record(id: &str) -> GenerationRecord {
    GenerationRecord {
        id: id.to_owned(),
        request: GeneratorRequest {
            generator: "demo:forge-item".to_owned(),
            args: GenValue::Text {
                value: "river".to_owned(),
            },
            locks: Default::default(),
        },
        entropy: EntropyTape::from_seed(7).draw(),
        proposal: GenValue::Item {
            item: ItemProposal {
                template: "demo:river-blade".to_owned(),
                name: "River Blade".to_owned(),
                tags: vec!["fixture".to_owned()],
            },
        },
    }
}

/// Every connected client holds exactly the host's state, hash, and seq.
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

fn mv(id: u32, to: (i32, i32)) -> GameEvent {
    GameEvent::Map(SessionEvent::TokenMoved {
        id: TokenId(id),
        to,
    })
}

fn sword_inventory() -> Inventory {
    let sword = ItemInstance {
        id: ItemId::new("reward-03.sword"),
        template: "srd5e:longsword".to_owned(),
        name: "Fine Longsword".to_owned(),
        quantity: 1,
        tags: vec!["weapon".to_owned()],
        modifiers: Vec::new(),
        appearance_layers: vec!["weapon:longsword".to_owned()],
    };
    let mut inventory = Inventory::default();
    inventory.insert(sword).unwrap();
    inventory
        .equip(EquipmentSlot::MainHand, ItemId::new("reward-03.sword"))
        .unwrap();
    inventory
}

#[test]
fn from_start_clients_converge_on_host_ordering() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.connect(PeerId(11));

    // Host and both clients each propose moves; the host orders them.
    sim.host_event(mv(1, (2, 1)));
    sim.client_intent(PeerId(10), mv(2, (5, 6)));
    sim.client_intent(PeerId(11), mv(1, (2, 2)));
    sim.host_event(GameEvent::TurnAdd(TokenId(1)));

    assert_eq!(sim.host.seq(), 4);
    assert_eq!(sim.host.state().map.token(TokenId(1)).unwrap().at, (2, 2));
    assert_eq!(sim.host.state().map.token(TokenId(2)).unwrap().at, (5, 6));
    assert_converged(&sim);
}

#[test]
fn late_joiner_gets_snapshot_plus_tail_and_converges() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));

    // Play happens before the second player joins.
    sim.host_event(mv(1, (3, 1)));
    sim.client_intent(PeerId(10), mv(2, (4, 6)));
    sim.host_event(GameEvent::TurnAdd(TokenId(1)));
    let hash_at_join = sim.host.log_hash();
    let seq_at_join = sim.host.seq();

    // Mid-session join: snapshot carries the 3 prior events' state+hash.
    sim.connect(PeerId(20));
    let joiner = &sim.clients[&PeerId(20)];
    assert_eq!(joiner.applied(), seq_at_join);
    assert_eq!(joiner.log_hash(), hash_at_join);
    assert_eq!(joiner.state(), Some(sim.host.state()));

    // More play; the late joiner replays the tail and stays converged
    // with the peer that saw everything.
    sim.client_intent(PeerId(20), mv(1, (3, 2)));
    sim.host_event(GameEvent::TurnAdvance);
    assert_eq!(sim.host.seq(), 5);
    assert_converged(&sim);
}

#[test]
fn invalid_intent_is_rejected_without_divergence() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.host_event(mv(1, (2, 1)));
    let seq = sim.host.seq();
    let hash = sim.host.log_hash();

    // Out-of-bounds move and a move of a nonexistent token: both must
    // fail validation on the host and never enter the log.
    sim.client_intent(PeerId(10), mv(1, (99, 0)));
    sim.client_intent(PeerId(10), mv(7, (2, 2)));
    // A turn-add for a missing token is rejected too.
    sim.client_intent(PeerId(10), GameEvent::TurnAdd(TokenId(7)));

    assert_eq!(sim.host.seq(), seq, "rejected intents changed the log");
    assert_eq!(sim.host.log_hash(), hash);
    assert_converged(&sim);
}

#[test]
fn a_resolved_attack_replicates_and_lands_on_every_peer() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(1),
        sheet: sheet("Knight", 12, 16),
    });
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(2),
        sheet: sheet("Goblin", 7, 15),
    });

    sim.host_event(attack_hit(5));

    // The goblin took the hit on the host and on the client, identically.
    let hp = |s: &GameSnapshot| s.map.sheet(TokenId(2)).unwrap().int("hp_current");
    assert_eq!(hp(sim.host.state()), Some(2), "7 hp less 5 damage");
    assert_eq!(hp(sim.clients[&PeerId(10)].state().unwrap()), Some(2));
    // The attacker is untouched: a resolution changes only what it addresses.
    assert_eq!(sim.host.state().map.sheet(TokenId(1)).unwrap().int("hp_current"), Some(12));
    // Both rolls reached the shared log, and the beats reached the client so it
    // can play the exchange rather than merely read about it.
    assert_eq!(sim.host.state().roll_log.len(), 2);
    let beats = &sim.clients[&PeerId(10)].state().unwrap().last_beats;
    assert_eq!(beats.len(), 2, "the client must see the exchange to play it");
    assert_eq!(beats[1], Beat::new(TokenId(2), "recoil"));
    assert_converged(&sim);
}

#[test]
fn a_killing_blow_replicates_and_the_fallen_lose_their_turn() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(1),
        sheet: sheet("Knight", 12, 16),
    });
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(2),
        sheet: sheet("Goblin", 7, 15),
    });
    sim.host_event(GameEvent::TurnAdd(TokenId(1)));
    sim.host_event(GameEvent::TurnAdd(TokenId(2)));

    // A blow that drops the goblin. The system judged it; the substrate obeys.
    let mut lethal = attack_hit(7);
    if let GameEvent::ActionResolved(res) = &mut lethal {
        res.defeated = vec![TokenId(2)];
        res.beats[1] = Beat::new(TokenId(2), "fall");
    }
    sim.host_event(lethal);

    let down = |s: &GameSnapshot| s.map.is_defeated(TokenId(2));
    assert!(down(sim.host.state()));
    assert!(
        down(sim.clients[&PeerId(10)].state().unwrap()),
        "the client must know it fell, or it will still let you swing at it"
    );

    // The turn passes from the knight straight back to the knight: the corpse
    // does not get a turn, and every peer computes that skip from state it
    // already has rather than being told about it.
    assert_eq!(sim.host.state().turns.active(), Some(TokenId(1)));
    sim.host_event(GameEvent::TurnAdvance);
    assert_eq!(sim.host.state().turns.active(), Some(TokenId(1)));
    assert_converged(&sim);
}

#[test]
fn a_player_may_emote_for_itself_without_the_host_adjudicating() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    // Token 2 belongs to player B; token 1 to player A.
    sim.client_hello(PeerId(10), "B");

    // Unlike an attack, a client's own emote is accepted: there is no verdict to
    // forge and no state to change, so the worst a liar can do is wave.
    sim.client_intent(
        PeerId(10),
        GameEvent::Emoted {
            token: TokenId(2),
            beat: "cheer".to_owned(),
        },
    );

    let beats = &sim.host.state().last_beats;
    assert_eq!(beats, &[Beat::new(TokenId(2), "cheer")]);
    assert_eq!(
        &sim.clients[&PeerId(10)].state().unwrap().last_beats,
        beats,
        "everyone at the table sees the cheer"
    );
    // It is a flourish, not a fact: no roll, no delta, nothing to undo.
    assert!(sim.host.state().roll_log.is_empty());
    assert_converged(&sim);

    // But only your own: a wave is harmless, and puppeteering someone else's
    // token (or the DM's monsters) is not.
    let seq = sim.host.seq();
    sim.client_intent(
        PeerId(10),
        GameEvent::Emoted {
            token: TokenId(1), // player A's knight
            beat: "taunt".to_owned(),
        },
    );
    assert_eq!(sim.host.seq(), seq, "B puppeteered A's knight");

    // And an emote for a token that does not exist is still refused.
    sim.client_intent(
        PeerId(10),
        GameEvent::Emoted {
            token: TokenId(99),
            beat: "cheer".to_owned(),
        },
    );
    assert_eq!(sim.host.seq(), seq);
    assert_converged(&sim);
}

#[test]
fn forced_movement_is_truth_and_lands_on_the_same_tile_everywhere() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(2),
        sheet: sheet("Goblin", 7, 15),
    });
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(1),
        sheet: sheet("Knight", 12, 16),
    });

    // A shove: the goblin genuinely relocates. Unlike a stagger beat, which
    // peers may render however they like, this changes what the goblin can
    // reach and see, so every peer must land it on exactly the same tile.
    let mut shove = attack_hit(0);
    if let GameEvent::ActionResolved(res) = &mut shove {
        res.action_key = "shove".to_owned();
        res.deltas.clear();
        res.displaced = vec![(TokenId(2), (7, 6))];
        res.beats[1] = Beat::new(TokenId(2), "shoved-e");
    }
    sim.host_event(shove);

    let at = |s: &GameSnapshot| s.map.token(TokenId(2)).unwrap().at;
    assert_eq!(at(sim.host.state()), (7, 6), "the goblin was pushed");
    assert_eq!(
        at(sim.clients[&PeerId(10)].state().unwrap()),
        (7, 6),
        "forced movement is game truth, so it cannot be left to each peer"
    );
    assert_converged(&sim);
}

/// Two prepared maps joined by a door: `field` (the snapshot's demo board,
/// promoted to a stored map) and `hut`, whose entry door faces back.
fn two_map_snapshot() -> GameSnapshot {
    let mut snap = snapshot();
    let field = CampaignMap {
        id: "field".to_owned(),
        scale: MapScale::Local,
        document: snap.map.clone(),
        spawn_zones: Vec::new(),
        transitions: vec![MapTransition {
            id: "field-gate".to_owned(),
            at: MapPoint { col: 3, row: 3 },
            target_map: "hut".to_owned(),
            target_entry: Some("hut-door".to_owned()),
        }],
        encounter_anchors: Vec::new(),
    };
    let mut hut_doc = MapDocument::new("hut", 6, 6);
    let floor = hut_doc.intern_tile_kind("stone");
    for r in 0..6 {
        for c in 0..6 {
            hut_doc.ground.set(c, r, floor);
        }
    }
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
    snap.maps.insert("field".to_owned(), field);
    snap.maps.insert("hut".to_owned(), hut);
    snap.active_map = Some("field".to_owned());
    snap
}

#[test]
fn walking_through_a_door_crosses_maps_and_the_board_follows_the_party() {
    // The goblin is DM furniture (owner: None), so the knight is the last
    // player out and the board follows it through the door.
    let mut base = two_map_snapshot();
    base.map.tokens[1].owner = None; // goblin: DM furniture
    if let Some(field) = base.maps.get_mut("field") {
        field.document = base.map.clone();
    }
    let mut sim = Sim::new(HostSession::new(base));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(1),
        sheet: sheet("Knight", 12, 16),
    });
    sim.host_event(GameEvent::ConditionSet {
        token: TokenId(1),
        condition: "prone".to_owned(),
        value: 1,
        mobility: Some((2, 6)),
    });

    // Walk onto the gate, then through it.
    sim.host_event(GameEvent::Map(SessionEvent::TokenMoved {
        id: TokenId(1),
        to: (3, 3),
    }));
    sim.host_event(GameEvent::Traveled { token: TokenId(1) });

    let host = sim.host.state();
    // The board followed the last player out.
    assert_eq!(host.active_map.as_deref(), Some("hut"));
    // The knight arrived at the hut's entry door, carrying everything it is:
    // sheet, condition, and the condition's numbers.
    let knight = host.map.token(TokenId(1)).expect("knight in the hut");
    assert_eq!(knight.at, (1, 1), "landed at the named entry");
    assert_eq!(host.map.sheet(TokenId(1)).and_then(|s| s.int("hp_current")), Some(12));
    assert!(host.map.has_condition(TokenId(1), "prone"), "still prone: travel is not a cure");
    assert_eq!(host.map.effective_mobility(TokenId(1), (5, 6)), (2, 6));
    // And left the field entirely (the stored copy, since field is no longer
    // the active board).
    let field = &host.maps["field"].document;
    assert!(field.token(TokenId(1)).is_none());
    assert!(field.sheets.get(&TokenId(1)).is_none());
    // The goblin furniture stayed home.
    assert!(field.token(TokenId(2)).is_some());
    assert_converged(&sim);
}

#[test]
fn arriving_where_your_id_is_taken_mints_a_new_one_and_carries_the_inventory() {
    let mut base = two_map_snapshot();
    base.map.tokens[1].owner = None;
    // The hut already has a resident with the knight's id.
    if let Some(hut) = base.maps.get_mut("hut") {
        hut.document.tokens.push(Token {
            id: TokenId(1),
            at: (4, 4),
            facing: Facing::South,
            sprite: "goblin".to_owned(),
            owner: None,
        });
    }
    if let Some(field) = base.maps.get_mut("field") {
        field.document = base.map.clone();
    }
    let mut sim = Sim::new(HostSession::new(base));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::InventorySet {
        token: TokenId(1),
        inventory: sword_inventory(),
    });
    sim.host_event(GameEvent::Map(SessionEvent::TokenMoved {
        id: TokenId(1),
        to: (3, 3),
    }));
    sim.host_event(GameEvent::Traveled { token: TokenId(1) });

    let host = sim.host.state();
    assert_eq!(host.active_map.as_deref(), Some("hut"));
    // The resident kept its id; the traveler was minted a fresh one, and the
    // inventory followed the new id (they key globally).
    let arrivals: Vec<_> = host
        .map
        .tokens
        .iter()
        .filter(|t| t.sprite == "knight")
        .collect();
    assert_eq!(arrivals.len(), 1);
    let new_id = arrivals[0].id;
    assert_ne!(new_id, TokenId(1));
    assert!(host.inventories.contains_key(&new_id), "the sword crossed too");
    assert!(!host.inventories.contains_key(&TokenId(1)));
    assert_converged(&sim);
}

#[test]
fn split_party_time_drifts_freely_and_travel_reconciles_it() {
    // The knight fights in the field while the hut sits quiet: the two
    // locations' clocks drift apart, and nothing needs to agree until someone
    // crosses. Simultaneity is presentation; the door is where timelines meet.
    let mut base = two_map_snapshot();
    base.map.tokens[1].owner = None;
    if let Some(field) = base.maps.get_mut("field") {
        field.document = base.map.clone();
    }
    let mut sim = Sim::new(HostSession::new(base));
    sim.connect(PeerId(10));

    // Three rounds of fighting in the field: knight and goblin trade turns.
    sim.host_event(GameEvent::TurnAdd(TokenId(1)));
    sim.host_event(GameEvent::TurnAdd(TokenId(2)));
    for _ in 0..6 {
        sim.host_event(GameEvent::TurnAdvance);
    }
    // And the DM declares a rest on top.
    sim.host_event(GameEvent::TimeAdvanced { ticks: 4 });

    let clock = |s: &GameSnapshot, id: &str| s.clocks.get(id).copied().unwrap_or(0);
    assert_eq!(clock(sim.host.state(), "field"), 7, "3 rounds + 4 declared");
    assert_eq!(clock(sim.host.state(), "hut"), 0, "nobody home: no time passes");

    // The knight walks through the gate. Nobody arrives before they left: the
    // hut's clock catches up to the traveler's, on every peer.
    sim.host_event(GameEvent::Map(SessionEvent::TokenMoved {
        id: TokenId(1),
        to: (3, 3),
    }));
    sim.host_event(GameEvent::Traveled { token: TokenId(1) });
    assert_eq!(clock(sim.host.state(), "hut"), 7);
    assert_eq!(
        clock(sim.clients[&PeerId(10)].state().unwrap(), "hut"),
        7,
        "the reconciled clock is truth, so the client holds it too"
    );

    // A player does not declare hours passing.
    let seq = sim.host.seq();
    sim.client_intent(PeerId(10), GameEvent::TimeAdvanced { ticks: 99 });
    assert_eq!(sim.host.seq(), seq, "a client kept the clock");
    assert_converged(&sim);
}

#[test]
fn travel_off_a_door_is_refused_and_clients_cannot_rule_it() {
    let mut sim = Sim::new(HostSession::new(two_map_snapshot()));
    sim.connect(PeerId(10));
    let seq = sim.host.seq();

    // Not standing on a transition point: nothing happens.
    sim.host_event(GameEvent::Traveled { token: TokenId(1) });
    assert_eq!(sim.host.seq(), seq, "an off-door travel entered the log");

    // And travel is the host's ruling: a client walks, it does not ask in words.
    sim.client_intent(PeerId(10), GameEvent::Traveled { token: TokenId(1) });
    assert_eq!(sim.host.seq(), seq);
    assert_converged(&sim);
}

#[test]
fn allegiance_replicates_and_a_convinced_creature_joins_your_side() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));

    // Token 2 (the goblin) belongs to player B. A convince, ruled by the host,
    // hands it to player A. Owner changes are truth, so every peer applies it.
    let mut won = attack_hit(0);
    if let GameEvent::ActionResolved(res) = &mut won {
        res.action_key = "convince".to_owned();
        res.deltas.clear();
        res.owner_changes = vec![(TokenId(2), Some("A".to_owned()))];
        res.beats[1] = Beat::new(TokenId(2), "cheer");
    }
    sim.host_event(won);

    let owner = |s: &GameSnapshot| s.map.token(TokenId(2)).unwrap().owner.clone();
    assert_eq!(owner(sim.host.state()).as_deref(), Some("A"), "the goblin joined A");
    assert_eq!(
        owner(sim.clients[&PeerId(10)].state().unwrap()).as_deref(),
        Some("A"),
        "allegiance is game truth, so the client holds it too"
    );
    // It did no damage: convince changes sides, not hit points.
    assert_eq!(
        sim.host.state().map.sheet(TokenId(2)).and_then(|s| s.int("hp_current")),
        None,
        "no sheet was bound, and none was needed to change owner"
    );
    assert_converged(&sim);
}

#[test]
fn a_condition_and_its_numbers_replicate_and_standing_up_restores_them() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(2),
        sheet: sheet("Goblin", 7, 15),
    });

    // A trip lands: prone plus the rules' recomputed numbers, one event.
    let mut trip = attack_hit(0);
    if let GameEvent::ActionResolved(res) = &mut trip {
        res.action_key = "trip".to_owned();
        res.deltas.clear();
        res.conditions = vec![(TokenId(2), "prone".to_owned(), 1)];
        res.mobility = vec![(TokenId(2), Some((2, 6)))];
    }
    sim.host_event(trip);

    let check = |s: &GameSnapshot| {
        (
            s.map.has_condition(TokenId(2), "prone"),
            s.map.effective_mobility(TokenId(2), (5, 6)),
        )
    };
    assert_eq!(check(sim.host.state()), (true, (2, 6)));
    assert_eq!(
        check(sim.clients[&PeerId(10)].state().unwrap()),
        (true, (2, 6)),
        "the client computes fog and reach locally, so it must hold the numbers"
    );

    // Standing up: the condition clears and the override clears with it, so the
    // sheet's base values stand again.
    sim.host_event(GameEvent::ConditionSet {
        token: TokenId(2),
        condition: "prone".to_owned(),
        value: 0,
        mobility: None,
    });
    assert_eq!(check(sim.host.state()), (false, (5, 6)));
    assert_eq!(check(sim.clients[&PeerId(10)].state().unwrap()), (false, (5, 6)));
    assert_converged(&sim);

    // A client may not pronounce a condition: that is a rules ruling.
    let seq = sim.host.seq();
    sim.client_intent(
        PeerId(10),
        GameEvent::ConditionSet {
            token: TokenId(2),
            condition: "blinded".to_owned(),
            value: 1,
            mobility: Some((5, 0)),
        },
    );
    assert_eq!(sim.host.seq(), seq, "a client ruled on a condition");
    assert_converged(&sim);
}

#[test]
fn a_client_asks_and_the_host_adjudicates() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.client_hello(PeerId(10), "A"); // token 1 is A's knight

    // The player swings. What crosses the wire is a *request*: no roll, no
    // damage, no verdict. The host holds the rules; the client holds none.
    sim.client_action(
        PeerId(10),
        ActionIntent {
            actor: TokenId(1),
            target: TokenId(2),
            action_key: "attack".to_owned(),
        },
    );

    // It changes nothing by itself. It is not an event and never enters the log;
    // it waits for the host's rules system to answer it.
    assert_eq!(sim.host.seq(), 0, "an ask is not a fact");
    let queued = sim.host.take_action_intents();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].actor, TokenId(1));
    assert_eq!(queued[0].action_key, "attack");
    // Drained exactly once: the host app resolves it, and it does not linger to
    // be resolved twice.
    assert!(sim.host.take_action_intents().is_empty());
    assert_converged(&sim);
}

#[test]
fn a_client_may_only_act_with_its_own_tokens() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.client_hello(PeerId(10), "B"); // B owns token 2, not token 1

    // Swinging *someone else's* sword is refused before the rules are consulted:
    // ownership is one of the two things the rules-blind session can check.
    sim.client_action(
        PeerId(10),
        ActionIntent {
            actor: TokenId(1), // player A's knight
            target: TokenId(2),
            action_key: "attack".to_owned(),
        },
    );
    assert!(
        sim.host.take_action_intents().is_empty(),
        "B queued an action for A's knight"
    );
    assert_converged(&sim);
}

#[test]
fn a_client_cannot_pronounce_its_own_verdict() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(1),
        sheet: sheet("Knight", 12, 16),
    });
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(2),
        sheet: sheet("Goblin", 7, 15),
    });
    let seq = sim.host.seq();
    let hash = sim.host.log_hash();

    // A client proposing a resolution is proposing that it hit and for how
    // much. The rules run on the sequencer; a client asks, it never decides.
    sim.client_intent(PeerId(10), attack_hit(999));

    assert_eq!(sim.host.seq(), seq, "a forged verdict entered the log");
    assert_eq!(sim.host.log_hash(), hash);
    assert_eq!(
        sim.host.state().map.sheet(TokenId(2)).unwrap().int("hp_current"),
        Some(7),
        "the goblin took damage from an unadjudicated claim"
    );
    assert_converged(&sim);
}

#[test]
fn a_resolution_addressing_an_unsheeted_token_is_refused_whole() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    // Only the attacker is statted; the goblin was never bound a sheet.
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(1),
        sheet: sheet("Knight", 12, 16),
    });
    let seq = sim.host.seq();

    sim.host_event(attack_hit(5));

    assert_eq!(
        sim.host.seq(),
        seq,
        "a half-appliable resolution entered the log"
    );
    assert_converged(&sim);
}

#[test]
fn a_graded_condition_replicates_at_its_magnitude() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(2),
        sheet: sheet("Goblin", 7, 15),
    });

    // A Demoralize critical: frightened 2, no damage. The magnitude is truth,
    // so every peer must hold the same number -- "frightened 1" and
    // "frightened 2" are different states and only one of them is real here.
    let mut fear = attack_hit(0);
    if let GameEvent::ActionResolved(res) = &mut fear {
        res.action_key = "demoralize".to_owned();
        res.deltas.clear();
        res.conditions = vec![(TokenId(2), "frightened".to_owned(), 2)];
    }
    sim.host_event(fear);

    let value = |s: &GameSnapshot| s.map.condition_value(TokenId(2), "frightened");
    assert_eq!(value(sim.host.state()), 2);
    assert_eq!(
        value(sim.clients[&PeerId(10)].state().unwrap()),
        2,
        "the magnitude is game truth, so the client holds 2, not merely 'frightened'"
    );
    assert_converged(&sim);
}

#[test]
fn per_turn_counters_replicate_and_reset_when_the_turn_comes_round() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(1),
        sheet: sheet("Knight", 12, 16),
    });
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(2),
        sheet: sheet("Goblin", 7, 15),
    });
    sim.host_event(GameEvent::TurnAdd(TokenId(1)));
    sim.host_event(GameEvent::TurnAdd(TokenId(2)));

    // Two strikes in the knight's turn, each spending an action and adding to
    // the multiple-attack tally. The net layer carries the integers the rules
    // decided; it never learns they mean "actions" or "attacks".
    let strike = || {
        let mut e = attack_hit(3);
        if let GameEvent::ActionResolved(res) = &mut e {
            res.turn_counters = vec![
                (TokenId(1), "actions_spent".to_owned(), 1),
                (TokenId(1), "strikes".to_owned(), 1),
            ];
        }
        e
    };
    sim.host_event(strike());
    sim.host_event(strike());

    // The ledger accumulated, and the client holds exactly the same count: a
    // per-turn resource is truth, applied verbatim like any sheet delta.
    let spent = |s: &GameSnapshot| s.map.turn_counter(TokenId(1), "actions_spent");
    assert_eq!(spent(sim.host.state()), 2);
    assert_eq!(spent(sim.clients[&PeerId(10)].state().unwrap()), 2);
    assert_eq!(sim.host.state().map.turn_counter(TokenId(1), "strikes"), 2);

    // The knight's turn ends and the goblin's begins. A turn-start wipes the
    // *incoming* token's counters, so the goblin's clear (they were empty), but
    // the knight keeps its spend while someone else acts.
    sim.host_event(GameEvent::TurnAdvance);
    assert_eq!(sim.host.state().turns.active(), Some(TokenId(2)));
    assert_eq!(
        spent(sim.host.state()),
        2,
        "another token's turn does not refill yours"
    );

    // The turn comes back to the knight: now its counters wipe, so it has its
    // whole action economy again. Every peer computes the identical reset from
    // the same TurnAdvance -- nobody is told the counts separately.
    sim.host_event(GameEvent::TurnAdvance);
    assert_eq!(sim.host.state().turns.active(), Some(TokenId(1)));
    assert_eq!(
        spent(sim.host.state()),
        0,
        "the knight's own turn refilled its actions"
    );
    assert_eq!(spent(sim.clients[&PeerId(10)].state().unwrap()), 0);
    assert_converged(&sim);
}

#[test]
fn turn_order_replicates() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::TurnAdd(TokenId(1)));
    sim.host_event(GameEvent::TurnAdd(TokenId(2)));
    sim.client_intent(PeerId(10), GameEvent::TurnAdvance);

    assert_eq!(sim.host.state().turns.active(), Some(TokenId(2)));
    sim.host_event(GameEvent::TurnRemove(TokenId(2)));
    assert_eq!(sim.host.state().turns.active(), Some(TokenId(1)));
    assert_converged(&sim);
}

#[test]
fn whisper_reaches_only_the_named_player() {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.connect(PeerId(20));
    sim.client_hello(PeerId(10), "alice");
    sim.client_hello(PeerId(20), "bob");

    sim.host_whisper("dm", "alice", "the door is trapped");
    assert_eq!(
        sim.clients[&PeerId(10)].inbox(),
        &[("dm".to_owned(), "the door is trapped".to_owned())]
    );
    assert!(
        sim.clients[&PeerId(20)].inbox().is_empty(),
        "bob does not see alice's whisper"
    );

    // A whisper to a name nobody announced goes nowhere.
    sim.host_whisper("dm", "carol", "hello?");
    assert_eq!(sim.clients[&PeerId(10)].inbox().len(), 1);
    // Whispers are directed, so they never touch the replicated log.
    assert_converged(&sim);
}

#[test]
fn move_and_facing_batch_orders_atomically() {
    // A play move is two events (move + face); interleaving from two
    // clients must not split a pair, because the host applies each
    // intent fully before the next.
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.client_intent(PeerId(10), mv(1, (2, 1)));
    sim.client_intent(
        PeerId(10),
        GameEvent::Map(SessionEvent::TokenFaced {
            id: TokenId(1),
            facing: Facing::East,
        }),
    );
    let t = sim.host.state().map.token(TokenId(1)).unwrap();
    assert_eq!((t.at, t.facing), ((2, 1), Facing::East));
    assert_converged(&sim);
}

/// The W0 done-condition (worldbuilding plan, decision 8): a GM-only
/// fact lives host-side only, a peer's snapshot bytes provably contain
/// no unrevealed secret, a reveal publishes it to every journal as an
/// ordinary logged event, and a client cannot fabricate a fact.
#[test]
fn secrets_stay_host_side_until_revealed() {
    use isometry_campaign::{RevealCondition, SecretFact};

    const SECRET_TEXT: &str = "OATHBOUND-RIVER-SECRET";

    let mut host = HostSession::new(snapshot());
    // The GM layer belongs to the host session, never inside its snapshot.
    host.campaign_mut().insert_secret(SecretFact {
        id: "sword-01.curse".to_owned(),
        text: SECRET_TEXT.to_owned(),
        tags: vec!["item:sword-01".to_owned()],
        reveal: RevealCondition::Identify,
    });

    let mut sim = Sim::new(host);
    sim.connect(PeerId(10));
    sim.connect(PeerId(11));

    // Unrevealed: no peer's replicated state carries the secret, byte-wise.
    let needle = SECRET_TEXT.as_bytes();
    for (peer, client) in &sim.clients {
        let bytes = postcard::to_allocvec(client.state().unwrap()).unwrap();
        assert!(
            !bytes.windows(needle.len()).any(|w| w == needle),
            "client {peer:?} snapshot bytes contain the unrevealed secret"
        );
        assert!(client.state().unwrap().journal.is_empty());
    }

    // A client cannot make something true by proposing it.
    sim.client_intent(
        PeerId(10),
        GameEvent::Fact(isometry_campaign::WorldFact {
            id: "forged".to_owned(),
            kind: "reveal".to_owned(),
            text: "the king owes me gold".to_owned(),
            tags: Vec::new(),
        }),
    );
    assert!(sim.host.state().journal.is_empty());
    assert_converged(&sim);

    // The DM reveals through the host transaction. The private record only
    // finalizes after its public fact commits.
    sim.host_reveal_secret("sword-01.curse")
        .expect("secret exists");
    assert!(
        sim.host.campaign().is_empty(),
        "revealed fact left the GM layer"
    );

    assert_eq!(sim.host.state().journal.len(), 1);
    assert_eq!(sim.host.state().journal[0].text, SECRET_TEXT);
    for client in sim.clients.values() {
        assert_eq!(client.state().unwrap().journal.len(), 1);
        assert_eq!(client.state().unwrap().journal[0].kind, "reveal");
    }
    assert_converged(&sim);
}

#[test]
fn failed_reveal_restores_the_private_secret() {
    use isometry_campaign::{RevealCondition, SecretFact, WorldFact};

    let mut host = HostSession::new(snapshot());
    host.local_event(GameEvent::Fact(WorldFact {
        id: "sword-01.curse".to_owned(),
        kind: "history".to_owned(),
        text: "A different public fact already owns this id.".to_owned(),
        tags: Vec::new(),
    }));
    host.campaign_mut().insert_secret(SecretFact {
        id: "sword-01.curse".to_owned(),
        text: "The sword is cursed.".to_owned(),
        tags: Vec::new(),
        reveal: RevealCondition::Manual,
    });

    assert!(host.reveal_secret("sword-01.curse").is_err());
    assert!(host.campaign().secret("sword-01.curse").is_some());
    assert!(host.campaign().pending_reveal("sword-01.curse").is_none());
}

#[test]
fn restored_host_reconciles_a_pending_reveal() {
    use isometry_campaign::{CampaignStore, RevealCondition, SecretFact};

    let mut campaign = CampaignStore::new();
    campaign.insert_secret(SecretFact {
        id: "sword-01.curse".to_owned(),
        text: "The sword is cursed.".to_owned(),
        tags: Vec::new(),
        reveal: RevealCondition::Manual,
    });
    campaign
        .begin_reveal("sword-01.curse")
        .expect("secret exists");

    let mut host = HostSession::with_campaign(snapshot(), campaign);
    let out = host
        .reconcile_pending_reveals()
        .expect("reconciles pending fact");
    assert_eq!(out.len(), 1);
    assert_eq!(host.state().journal.len(), 1);
    assert!(host.campaign().is_empty());
}

#[test]
fn restored_history_rebuilds_sequence_and_convergence_hash() {
    use isometry_campaign::CampaignStore;

    let mut host = HostSession::new(snapshot());
    host.local_event(mv(1, (2, 1)));
    host.local_event(GameEvent::TurnAdd(TokenId(1)));
    let state = host.state().clone();
    let history = host.history().clone();
    let hash = host.log_hash();

    let mut restored = HostSession::with_history(state, CampaignStore::new(), history);
    assert_eq!(restored.seq(), 2);
    assert_eq!(restored.log_hash(), hash);

    restored.local_event(GameEvent::TurnAdvance);
    assert_eq!(restored.seq(), 3);
    assert_eq!(restored.history().len(), 3);
}

/// W1's visibility guard: players receive equipped public items, but a
/// generated curse does not enter their snapshots until the DM reveals it.
#[test]
fn hidden_item_modifiers_stay_private_until_dm_reveal() {
    const CURSE_NAME: &str = "VOID-THIRST-CURSE";

    let mut state = snapshot();
    state.inventories.insert(TokenId(1), sword_inventory());
    let mut host = HostSession::new(state);
    let hidden = HiddenItemModifier {
        id: "reward-03.sword.curse".to_owned(),
        item: ItemId::new("reward-03.sword"),
        modifier: ItemModifier {
            id: "reward-03.sword.curse.void-thirst".to_owned(),
            kind: ItemModifierKind::Curse,
            name: CURSE_NAME.to_owned(),
            stats: BTreeMap::from([("attack_bonus".to_owned(), -1)]),
            appearance_layer: Some("effect:void".to_owned()),
        },
        reveal: RevealCondition::Identify,
    };
    host.campaign_mut()
        .insert_hidden_item_modifier(hidden.clone());

    let mut sim = Sim::new(host);
    sim.connect(PeerId(10));
    sim.connect(PeerId(11));

    // A player gets the sword and its equipped slot, but cannot learn or
    // forge the still-private curse.
    for client in sim.clients.values() {
        let state = client.state().unwrap();
        assert_eq!(
            state.inventories[&TokenId(1)].equipped[&EquipmentSlot::MainHand],
            ItemId::new("reward-03.sword")
        );
        let bytes = postcard::to_allocvec(state).unwrap();
        assert!(!bytes
            .windows(CURSE_NAME.len())
            .any(|w| w == CURSE_NAME.as_bytes()));
    }
    sim.client_intent(
        PeerId(10),
        GameEvent::ItemModifierRevealed(hidden.public_face()),
    );
    sim.client_intent(
        PeerId(10),
        GameEvent::InventorySet {
            token: TokenId(1),
            inventory: Inventory::default(),
        },
    );
    sim.client_intent(
        PeerId(10),
        GameEvent::ItemTransfer {
            from: TokenId(1),
            to: TokenId(2),
            item: ItemId::new("reward-03.sword"),
        },
    );
    assert!(
        sim.host.state().inventories[&TokenId(1)].items[&ItemId::new("reward-03.sword")]
            .modifiers
            .is_empty()
    );
    assert!(sim.host.state().inventories[&TokenId(1)]
        .items
        .contains_key(&ItemId::new("reward-03.sword")));

    sim.host_reveal_item_modifier("reward-03.sword.curse")
        .expect("DM reveals the curse");
    assert!(sim.host.campaign().is_empty());
    for client in sim.clients.values() {
        let item = &client.state().unwrap().inventories[&TokenId(1)].items
            [&ItemId::new("reward-03.sword")];
        assert_eq!(item.modifiers[0].name, CURSE_NAME);
        assert_eq!(
            item.appearance_layers().collect::<Vec<_>>(),
            vec!["weapon:longsword", "effect:void"]
        );
    }
    assert_converged(&sim);
}

#[test]
fn host_transfers_an_equipped_item_atomically() {
    let mut state = snapshot();
    state.inventories.insert(TokenId(1), sword_inventory());
    let mut sim = Sim::new(HostSession::new(state));
    sim.connect(PeerId(10));
    sim.connect(PeerId(11));

    sim.host_event(GameEvent::ItemTransfer {
        from: TokenId(1),
        to: TokenId(2),
        item: ItemId::new("reward-03.sword"),
    });

    let source = &sim.host.state().inventories[&TokenId(1)];
    let target = &sim.host.state().inventories[&TokenId(2)];
    assert!(!source.items.contains_key(&ItemId::new("reward-03.sword")));
    assert!(source.equipped.is_empty());
    assert!(target.items.contains_key(&ItemId::new("reward-03.sword")));
    assert!(target.equipped.is_empty());
    assert_converged(&sim);
}

#[test]
fn restored_host_reconciles_a_pending_item_modifier_once() {
    let mut campaign = isometry_campaign::CampaignStore::new();
    let hidden = HiddenItemModifier {
        id: "reward-03.sword.curse".to_owned(),
        item: ItemId::new("reward-03.sword"),
        modifier: ItemModifier {
            id: "reward-03.sword.curse.void-thirst".to_owned(),
            kind: ItemModifierKind::Curse,
            name: "Void Thirst".to_owned(),
            stats: BTreeMap::new(),
            appearance_layer: None,
        },
        reveal: RevealCondition::Manual,
    };
    campaign.insert_hidden_item_modifier(hidden);
    campaign
        .begin_item_modifier_reveal("reward-03.sword.curse")
        .expect("modifier exists");

    let mut state = snapshot();
    state.inventories.insert(TokenId(1), sword_inventory());
    let mut host = HostSession::with_campaign(state, campaign);
    let out = host
        .reconcile_pending_reveals()
        .expect("reconciles pending modifier");
    assert_eq!(out.len(), 1);
    assert_eq!(host.history().len(), 1);
    assert_eq!(
        host.state().inventories[&TokenId(1)].items[&ItemId::new("reward-03.sword")]
            .modifiers
            .len(),
        1
    );
    assert!(host.campaign().is_empty());
}

/// W2 commit-result mode: peers store the host-selected typed output but do
/// not execute its pack script. A client cannot forge a generation record.
#[test]
fn committed_generation_replicates_without_client_authority() {
    let record = generation_record("generated.river-blade.1");
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));

    sim.host_event(GameEvent::Generation(record.clone()));
    assert_eq!(sim.host.state().generations, vec![record.clone()]);
    assert_eq!(
        sim.clients[&PeerId(10)].state().unwrap().generations,
        vec![record.clone()]
    );

    sim.connect(PeerId(20));
    assert_eq!(
        sim.clients[&PeerId(20)].state().unwrap().generations,
        vec![record.clone()],
        "a late joiner receives committed results in its snapshot"
    );

    sim.client_intent(
        PeerId(10),
        GameEvent::Generation(generation_record("forged")),
    );
    assert_eq!(sim.host.state().generations, vec![record]);
    assert_converged(&sim);
}

#[test]
fn host_rejects_malformed_generation_before_it_enters_history() {
    let mut host = HostSession::new(snapshot());
    let mut malformed = generation_record("");
    malformed.request.generator.clear();

    assert!(host.commit_generation(malformed).is_err());
    assert!(host.state().generations.is_empty());
    assert!(host.history().is_empty());
}

#[test]
fn generated_map_stores_activates_edits_and_replicates_as_result_data() {
    let map = LocalMapProposal {
        id: "demo:river-cache".to_owned(),
        name: "River Cache".to_owned(),
        width: 5,
        height: 4,
        default_ground: "grass".to_owned(),
        cells: vec![MapCellProposal {
            col: 2,
            row: 2,
            ground: Some("stone".to_owned()),
            prop: None,
            elevation: Some(1),
        }],
        spawn_zones: vec![SpawnZone {
            id: "party".to_owned(),
            cells: vec![MapPoint { col: 0, row: 1 }],
        }],
        transitions: Vec::new(),
        encounter_anchors: vec![EncounterAnchor {
            id: "guardian".to_owned(),
            at: MapPoint { col: 3, row: 2 },
            tags: vec!["guardian".to_owned()],
        }],
    }
    .lower(MapScale::Local)
    .unwrap();
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));

    sim.host_event(GameEvent::MapStored(map.clone()));
    sim.host_event(GameEvent::MapActivated { id: map.id.clone() });
    assert_eq!(
        sim.host.state().active_map.as_deref(),
        Some("demo:river-cache")
    );
    assert_eq!(sim.host.state().map.ground.width(), 5);

    let stone = sim
        .host
        .state()
        .map
        .tile_kinds
        .iter()
        .position(|kind| kind == "stone")
        .unwrap() as u16;
    sim.host_event(GameEvent::Map(SessionEvent::TilePlaced {
        layer: isometry_core::Layer::Ground,
        at: (1, 1),
        kind: isometry_core::TileKindId(stone),
    }));
    assert_eq!(
        sim.host.state().maps["demo:river-cache"]
            .document
            .ground
            .get(1, 1),
        Some(&isometry_core::TileKindId(stone))
    );

    sim.connect(PeerId(20));
    assert_eq!(
        sim.clients[&PeerId(20)].state().unwrap().maps,
        sim.host.state().maps
    );
    sim.client_intent(PeerId(10), GameEvent::MapStored(map));
    assert_eq!(sim.host.seq(), 3, "client map authoring entered the log");
    assert_converged(&sim);
}

#[test]
fn a_faction_turn_commits_and_every_peer_lives_in_the_changed_world() {
    let mut snap = snapshot();
    snap.world.factions.insert(
        "tide".to_owned(),
        WorldFaction {
            id: "tide".into(),
            name: "Tide Court".into(),
            tags: vec!["river".into()],
            claims: vec![],
        },
    );
    let mut sim = Sim::new(HostSession::new(snap));
    sim.connect(PeerId(10));

    // The DM rolls a downtime tick from the host's own world and tape, then
    // commits the batch. (A real DM edits it first; here we commit as rolled.)
    let mut tape = EntropyTape::from_seed(7);
    let moves = sim.host.state().world.faction_turn(4, &mut tape);
    assert_eq!(moves.len(), 1, "one move for the one faction");
    let logged: usize = moves.iter().map(|m| m.clone().into_events().len()).sum();
    sim.host_faction_turn(moves).expect("the tick commits");

    // Every peer holds the faction-turn history at the tick, identically: a
    // faction acting on the world is ordinary replicated truth, not a host-only
    // record. And the whole batch (each move's history plus its change) reached
    // the ordered log.
    let meanwhile = |s: &GameSnapshot| {
        s.world
            .history
            .iter()
            .filter(|h| h.kind == "faction-turn" && h.time == 4)
            .count()
    };
    assert_eq!(meanwhile(sim.host.state()), 1);
    assert_eq!(meanwhile(sim.clients[&PeerId(10)].state().unwrap()), 1);
    assert_eq!(sim.host.seq() as usize, logged, "every move event entered the log");
    assert_converged(&sim);
}

#[test]
fn a_granted_player_plays_a_faction_and_a_stranger_may_not() {
    let mut snap = snapshot();
    // The goblin (token 2) is the Tide Court's furniture, not any player's.
    snap.map.tokens[1].owner = Some("tide".to_owned());
    snap.world.factions.insert(
        "tide".to_owned(),
        WorldFaction {
            id: "tide".into(),
            name: "Tide Court".into(),
            tags: vec!["river".into()],
            claims: vec![],
        },
    );
    let mut sim = Sim::new(HostSession::new(snap));
    sim.connect(PeerId(10));
    sim.client_hello(PeerId(10), "B");

    // Ungranted, B cannot even emote the faction's token: it is not B's, and no
    // channel has been handed over.
    sim.client_intent(
        PeerId(10),
        GameEvent::Emoted {
            token: TokenId(2),
            beat: "cheer".to_owned(),
        },
    );
    assert!(
        sim.host.state().last_beats.is_empty(),
        "a stranger cannot command a faction's token"
    );

    // The DM grants B the Tide Court's channel. Now B plays the faction: a
    // faction is an owner name, and the grant is the per-channel permission.
    sim.host_event(GameEvent::World(WorldEvent::FactionControlSet {
        faction: "tide".to_owned(),
        player: Some("B".to_owned()),
    }));
    sim.client_intent(
        PeerId(10),
        GameEvent::Emoted {
            token: TokenId(2),
            beat: "cheer".to_owned(),
        },
    );
    assert_eq!(
        sim.host.state().last_beats,
        &[Beat::new(TokenId(2), "cheer")],
        "a granted player commands the faction's token as its own"
    );

    // Revoke, and the faction returns to the DM: B is a stranger again, so a
    // fresh emote is refused and the last beat is still the granted one.
    sim.host_event(GameEvent::World(WorldEvent::FactionControlSet {
        faction: "tide".to_owned(),
        player: None,
    }));
    sim.client_intent(
        PeerId(10),
        GameEvent::Emoted {
            token: TokenId(2),
            beat: "taunt".to_owned(),
        },
    );
    assert_eq!(
        sim.host.state().last_beats,
        &[Beat::new(TokenId(2), "cheer")],
        "after revoke the channel is the DM's again, so the taunt never played"
    );
    assert_converged(&sim);
}

#[test]
fn banked_time_makes_a_bigger_tick_and_the_commit_empties_the_bank() {
    let mut snap = snapshot();
    snap.world.factions.insert(
        "tide".to_owned(),
        WorldFaction {
            id: "tide".into(),
            name: "Tide Court".into(),
            tags: vec!["river".into()],
            claims: vec![],
        },
    );
    // The table spent a long scene away: 25 units banked toward this faction.
    snap.world
        .faction_sheets
        .insert("tide".to_owned(), BTreeMap::from([("banked_time".to_owned(), 25)]));
    let mut sim = Sim::new(HostSession::new(snap));
    sim.connect(PeerId(10));

    let mut tape = EntropyTape::from_seed(3);
    let moves = sim.host.state().world.faction_turn(5, &mut tape);
    assert_eq!(moves.len(), 3, "banked 25 => one baseline plus two earned moves");
    sim.host_faction_turn(moves).expect("the tick commits");

    // The tick was proportional (3 faction-turn history events), and acting
    // emptied the bank -- on every peer, so the same time cannot be spent twice.
    let banked = |s: &GameSnapshot| {
        s.world
            .faction_sheet("tide")
            .and_then(|m| m.get("banked_time"))
            .copied()
    };
    let logged = |s: &GameSnapshot| {
        s.world
            .history
            .iter()
            .filter(|h| h.kind == "faction-turn" && h.time == 5)
            .count()
    };
    assert_eq!(logged(sim.host.state()), 3);
    assert_eq!(banked(sim.host.state()), Some(0), "the bank emptied on the host");
    assert_eq!(
        banked(sim.clients[&PeerId(10)].state().unwrap()),
        Some(0),
        "and on the client -- the spend is replicated truth"
    );
    assert_converged(&sim);
}

#[test]
fn storylet_matches_private_fact_casts_existing_role_and_commits_effects() {
    let mut host = HostSession::new(snapshot());
    host.campaign_mut().insert_secret(SecretFact {
        id: "ford.secret".into(),
        text: "The ford remembers a drowned oath.".into(),
        tags: vec!["river".into()],
        reveal: RevealCondition::Manual,
    });
    host.local_event(GameEvent::World(WorldEvent::Faction(WorldFaction {
        id: "tide".into(),
        name: "Tide Court".into(),
        tags: vec!["river".into()],
        claims: vec![],
    })));
    host.local_event(GameEvent::World(WorldEvent::Character(WorldCharacter {
        id: "mara".into(),
        name: "Mara".into(),
        tags: vec!["warden".into()],
        faction: Some("tide".into()),
        place: None,
    })));
    host.local_event(GameEvent::World(WorldEvent::Law(WorldLaw {
        id: "iron-remembers".into(),
        name: "Iron remembers".into(),
        text: "Iron keeps its maker's name.".into(),
        tags: vec!["magic".into()],
        parameters: BTreeMap::new(),
    })));
    let encounter = LocalMapProposal {
        id: "oath-encounter".into(),
        name: "Drowned Ford".into(),
        width: 3,
        height: 3,
        default_ground: "water".into(),
        cells: vec![],
        spawn_zones: vec![],
        transitions: vec![],
        encounter_anchors: vec![],
    };
    host.local_event(GameEvent::World(WorldEvent::Storylet(StoryletProposal {
        key: "drowned-oath".into(),
        entry: "The drowned oath surfaces.".into(),
        tags: vec!["encounter".into()],
        requirements: StoryletRequirements {
            faction_tags: vec!["river".into()],
            hidden_facts: vec!["ford.secret".into()],
            world_laws: vec!["iron-remembers".into()],
        },
        roles: vec![RoleSlot {
            key: "warden".into(),
            tags: vec!["warden".into()],
        }],
        effects: vec![
            StoryletEffect::History {
                event: HistoryEvent {
                    id: "oath-returned".into(),
                    time: 4,
                    kind: "omen".into(),
                    text: "The oath returned.".into(),
                    participants: vec!["mara".into()],
                    place: None,
                    tags: vec![],
                },
            },
            StoryletEffect::Item {
                item: ItemProposal {
                    template: "demo:oath-blade".into(),
                    name: "Oath Blade".into(),
                    tags: vec!["weapon".into()],
                },
            },
            StoryletEffect::LocalMap { map: encounter },
            StoryletEffect::Fact {
                fact: WorldFact {
                    id: "oath.public".into(),
                    kind: "storylet".into(),
                    text: "The oath has returned.".into(),
                    tags: vec!["river".into()],
                },
            },
        ],
    })));

    host.commit_storylet("drowned-oath", Some(TokenId(1)))
        .unwrap();
    assert_eq!(host.state().world.history[0].id, "oath-returned");
    assert!(host.state().inventories[&TokenId(1)]
        .items
        .values()
        .any(|item| item.name == "Oath Blade"));
    assert!(host.state().maps.contains_key("oath-encounter"));
    assert!(host
        .state()
        .journal
        .iter()
        .any(|fact| fact.id == "oath.public"));

    // A storylet re-lights while its requirements hold, so it can be played
    // again. The Item effect must not collide with its first grant: a second
    // play yields a second blade rather than failing the whole commit.
    host.commit_storylet("drowned-oath", Some(TokenId(1)))
        .expect("a repeat play must not error on a duplicate item id");
    let blades = host.state().inventories[&TokenId(1)]
        .items
        .values()
        .filter(|item| item.name == "Oath Blade")
        .count();
    assert_eq!(blades, 2, "each play grants a fresh instance");
}

#[test]
fn campaign_commit_keeps_secrets_private_and_applies_public_draft() {
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
    world.storylets.insert(
        "finale".into(),
        StoryletProposal {
            key: "finale".into(),
            entry: "The oath returns.".into(),
            tags: vec![],
            requirements: Default::default(),
            roles: vec![],
            effects: vec![],
        },
    );
    let draft = CampaignDraft {
        id: "oath".into(),
        name: "River Oath".into(),
        world,
        maps: vec![DraftMap {
            scale: MapScale::Region,
            map: LocalMapProposal {
                id: "march".into(),
                name: "River March".into(),
                width: 3,
                height: 2,
                default_ground: "grass".into(),
                cells: vec![],
                spawn_zones: vec![],
                transitions: vec![],
                encounter_anchors: vec![],
            },
        }],
        secrets: vec![SecretFact {
            id: "oath.secret".into(),
            text: "The witness lied.".into(),
            tags: vec![],
            reveal: RevealCondition::Manual,
        }],
        rewards: vec![ItemProposal {
            template: "demo:witness".into(),
            name: "Witness Blade".into(),
            tags: vec!["weapon".into()],
        }],
        starting_map: "march".into(),
        final_storylet: "finale".into(),
    };
    let record = GenerationRecord {
        id: "generated.campaign.1".into(),
        request: GeneratorRequest {
            generator: "demo:campaign".into(),
            args: GenValue::Text {
                value: "river".into(),
            },
            locks: BTreeMap::new(),
        },
        entropy: 7,
        proposal: GenValue::Campaign { campaign: draft },
    };
    let mut host = HostSession::new(snapshot());
    host.commit_campaign(record, Some(TokenId(1))).unwrap();

    assert!(host.campaign().secret("oath.secret").is_some());
    assert!(host.state().world.factions.contains_key("tide"));
    assert_eq!(host.state().active_map.as_deref(), Some("march"));
    assert!(host.state().inventories[&TokenId(1)]
        .items
        .values()
        .any(|item| item.name == "Witness Blade"));
    assert!(host
        .state()
        .journal
        .iter()
        .all(|fact| fact.text != "The witness lied."));
}
