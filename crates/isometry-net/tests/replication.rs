//! End-to-end replication over the in-process sim: the same routing the
//! iroh transport performs, minus the wire. Proves the I4 done-condition
//! (mid-session join, no divergence: state and log hashes match) without
//! two machines.

use isometry_core::{Facing, MapDocument, SessionEvent, Token, TokenId, TurnList};
use isometry_net::{GameEvent, GameSnapshot, HostSession, PeerId};
use isometry_net::sim::Sim;

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
    GameEvent::Map(SessionEvent::TokenMoved { id: TokenId(id), to })
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
    use isometry_campaign::{CampaignStore, RevealCondition, SecretFact};

    const SECRET_TEXT: &str = "OATHBOUND-RIVER-SECRET";

    // The GM layer: host-private, beside the session, never inside it.
    let mut campaign = CampaignStore::new();
    campaign.insert_secret(SecretFact {
        id: "sword-01.curse".to_owned(),
        text: SECRET_TEXT.to_owned(),
        tags: vec!["item:sword-01".to_owned()],
        reveal: RevealCondition::Identify,
    });

    let mut sim = Sim::new(HostSession::new(snapshot()));
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

    // The DM reveals: the fact leaves the GM store and is committed as
    // an ordinary event; every journal converges on the public face.
    let fact = campaign.reveal("sword-01.curse").expect("secret exists");
    assert!(campaign.is_empty(), "revealed fact left the GM layer");
    sim.host_event(GameEvent::Fact(fact));

    assert_eq!(sim.host.state().journal.len(), 1);
    assert_eq!(sim.host.state().journal[0].text, SECRET_TEXT);
    for client in sim.clients.values() {
        assert_eq!(client.state().unwrap().journal.len(), 1);
        assert_eq!(client.state().unwrap().journal[0].kind, "reveal");
    }
    assert_converged(&sim);
}
