//! The hardened action envelope (H0): protocol version, request identity,
//! idempotency, and version refusal.
//!
//! These are the receipts for the three H0 done-conditions. They live apart
//! from `replication.rs` because they prove properties of the envelope rather
//! than of replication: what a peer refuses, and what it declines to do twice.

use isometry_core::{
    Beat, Facing, MapDocument, RollRecord, SheetData, SheetDelta, Token, TokenId, TurnList,
};
use isometry_net::sim::Sim;
use isometry_net::{
    ActionIntent, ActionResolved, ClientSession, GameEvent, GameSnapshot, HostSession, NetMessage,
    PeerId, Recipient, RequestId, PROTOCOL_VERSION,
};

/// A small board: A's knight and B's goblin, in reach of nothing in
/// particular. The rules never run here; only resolved verdicts arrive.
fn snapshot() -> GameSnapshot {
    let mut map = MapDocument::new("envelope", 4, 4);
    let grass = map.intern_tile_kind("grass");
    for r in 0..4 {
        for c in 0..4 {
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
        at: (2, 2),
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
        applied_actions: Default::default(),
    }
}

fn sheet(name: &str, hp: i64) -> SheetData {
    let mut s = SheetData::new("5e-srd");
    s.set_text("name", name);
    s.set_int("hp_current", hp);
    s.set_int("hp_max", hp);
    s.set_int("ac", 13);
    s
}

fn roll(total: i32) -> RollRecord {
    RollRecord {
        by: "Knight".to_owned(),
        expr: "1d20+5".to_owned(),
        dice: vec![14],
        total,
    }
}

/// A hit answering `request`, shaped as the rules system produces it.
fn strike(request: RequestId, damage: i64) -> GameEvent {
    GameEvent::ActionResolved(ActionResolved {
        request,
        actor: TokenId(1),
        target: TokenId(2),
        action_key: "attack".to_owned(),
        label: "Attack".to_owned(),
        attack: roll(19),
        hit: true,
        damage: Some(RollRecord {
            by: "Knight".to_owned(),
            expr: "1d8+3".to_owned(),
            dice: vec![damage as u16],
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

fn armed_sim() -> Sim {
    let mut sim = Sim::new(HostSession::new(snapshot()));
    sim.connect(PeerId(10));
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(1),
        sheet: sheet("Knight", 20),
    });
    sim.host_event(GameEvent::SheetSet {
        token: TokenId(2),
        sheet: sheet("Goblin", 12),
    });
    sim
}

fn goblin_hp(state: &GameSnapshot) -> Option<i64> {
    state.map.sheet(TokenId(2)).and_then(|s| s.int("hp_current"))
}

/// H0 done-condition: applying the same resolution twice is a proven no-op.
#[test]
fn a_duplicate_resolution_is_a_no_op() {
    let mut sim = armed_sim();

    let blow = strike(RequestId::host(1), 5);
    sim.host_event(blow.clone());
    let after_once = sim.host.state().clone();
    assert_eq!(goblin_hp(&after_once), Some(7), "the blow landed");
    assert_eq!(after_once.roll_log.len(), 2, "an attack and a damage roll");

    // The same verdict a second time. It names the same request, so it has
    // already been answered and nothing about the state may move: not the hit
    // points, not the shared roll log, not the beat sequence that decides
    // whether the board plays the flourish again.
    sim.host_event(blow);
    assert_eq!(goblin_hp(sim.host.state()), Some(7), "the blow landed twice");
    assert_eq!(
        sim.host.state().roll_log.len(),
        2,
        "the duplicate re-logged its dice"
    );
    assert_eq!(
        sim.host.state().beat_seq,
        after_once.beat_seq,
        "the duplicate replayed its flourish"
    );
    assert_eq!(
        sim.host.state(),
        &after_once,
        "the whole state must be identical after the second apply"
    );

    // Every peer took the duplicate the same way, so the log hashes still
    // agree: a no-op that only one side performed would be a divergence.
    let client = &sim.clients[&PeerId(10)];
    assert_eq!(client.state(), Some(&after_once));
    assert_eq!(client.log_hash(), sim.host.log_hash());
}

/// The no-op is keyed on identity, not on content: two identical strikes are
/// two verdicts, and both must land.
#[test]
fn two_identical_strikes_are_two_verdicts() {
    let mut sim = armed_sim();

    sim.host_event(strike(RequestId::host(1), 5));
    sim.host_event(strike(RequestId::host(2), 5));

    assert_eq!(
        goblin_hp(sim.host.state()),
        Some(2),
        "identical content is not a duplicate; only an identical request is"
    );
    assert_eq!(sim.host.state().applied_actions.len(), 2);
}

/// Request identity is the authority's word. The asker numbers its own ask; the
/// host says whose it was.
#[test]
fn the_host_attributes_an_ask_to_the_peer_it_arrived_on() {
    let mut sim = armed_sim();
    sim.client_hello(PeerId(10), "A"); // token 1 is A's knight

    sim.client_action(
        PeerId(10),
        ActionIntent::new(TokenId(1), TokenId(2), "attack"),
    );
    sim.client_action(
        PeerId(10),
        ActionIntent::new(TokenId(1), TokenId(2), "attack"),
    );

    let queued = sim.host.take_action_intents();
    assert_eq!(queued.len(), 2);
    assert_eq!(
        queued[0].request.peer,
        PeerId(10),
        "an ask is attributed to the connection it arrived on"
    );
    assert_ne!(
        queued[0].request, queued[1].request,
        "two asks are two requests, or the second answer would be swallowed"
    );

    // A peer that stamps somebody else's identity on its ask is corrected, not
    // believed: otherwise it could poison another player's request numbers and
    // have their next verdict discarded as a duplicate.
    sim.host.on_message(
        PeerId(10),
        NetMessage::Action(ActionIntent {
            request: RequestId {
                peer: PeerId(99),
                nonce: 1,
            },
            actor: TokenId(1),
            target: TokenId(2),
            action_key: "attack".to_owned(),
        }),
    );
    let queued = sim.host.take_action_intents();
    assert_eq!(
        queued[0].request.peer,
        PeerId(10),
        "a peer asked as somebody else"
    );
}

/// H0 done-condition: a version neither side can speak is refused legibly, with
/// the offered and supported numbers, and nothing is applied.
#[test]
fn an_unsupported_version_is_refused_with_a_receipt() {
    let stranger = PROTOCOL_VERSION + 1;
    let receipt = NetMessage::VersionRefused {
        offered: stranger,
        supported: PROTOCOL_VERSION,
    };

    // The host's side: a client announcing a dialect it cannot read.
    let peer = PeerId(10);
    let mut host = HostSession::new(snapshot());
    assert_eq!(
        host.on_message(
            peer,
            NetMessage::Hello {
                version: stranger,
                name: "A".to_owned(),
            }
        ),
        vec![(Recipient::One(peer), receipt.clone())],
        "the refusal names both versions"
    );
    assert_eq!(host.refused_version(peer), Some(stranger));
    assert!(
        host.peer_names().is_empty(),
        "a refused peer never became a player"
    );

    // It stays refused, and nothing it sends afterwards reaches the log, not
    // even the one event this session accepts from anybody.
    assert_eq!(
        host.on_message(
            peer,
            NetMessage::Intent {
                event: GameEvent::Rolled(roll(11)),
            }
        ),
        vec![(Recipient::One(peer), receipt.clone())],
        "a refused peer must keep being refused"
    );
    assert_eq!(host.seq(), 0, "a refused peer wrote to the log");
    assert!(host.state().roll_log.is_empty());

    // The client's side: a host whose snapshot speaks a dialect the client
    // cannot read. It adopts no state at all rather than applying what parses.
    let mut client = ClientSession::new();
    assert_eq!(
        client.on_message(NetMessage::Snapshot {
            version: stranger,
            seq: 9,
            log_hash: 7,
            state: snapshot(),
        }),
        vec![(Recipient::Host, receipt)],
        "the refusal travels back to the host, naming both versions"
    );
    assert_eq!(client.refused(), Some(stranger));
    assert!(
        client.state().is_none(),
        "state was adopted from a version this peer cannot speak"
    );
    assert_eq!(client.applied(), 0);

    // A refused session reads nothing further, so no later event can slip past
    // the refusal on its own.
    assert!(client
        .on_message(NetMessage::Applied {
            seq: 1,
            event: GameEvent::Rolled(roll(11)),
        })
        .is_empty());
    assert!(client.state().is_none());
}

/// The matching version passes, so the refusal is a gate and not a wall.
#[test]
fn the_supported_version_is_admitted() {
    let mut sim = armed_sim();
    sim.client_hello(PeerId(10), "A");

    assert_eq!(sim.host.peer_names(), vec!["A".to_owned()]);
    assert_eq!(sim.host.refused_version(PeerId(10)), None);
    assert_eq!(sim.clients[&PeerId(10)].refused(), None);
    assert!(sim.clients[&PeerId(10)].state().is_some());
}
