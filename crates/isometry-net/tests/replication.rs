//! End-to-end replication over the in-process sim: the same routing the
//! iroh transport performs, minus the wire. Proves the I4 done-condition
//! (mid-session join, no divergence: state and log hashes match) without
//! two machines.

use std::collections::BTreeMap;

use isometry_campaign::{
    EntropyTape, EquipmentSlot, GenValue, GenerationRecord, GeneratorRequest,
    HiddenItemModifier, Inventory, ItemId, ItemInstance, ItemModifier, ItemModifierKind,
    ItemProposal, RevealCondition,
};
use isometry_core::{Facing, MapDocument, SessionEvent, Token, TokenId, TurnList};
use isometry_net::sim::Sim;
use isometry_net::{GameEvent, GameSnapshot, HostSession, PeerId};

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
    }
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

    sim.client_intent(PeerId(10), GameEvent::Generation(generation_record("forged")));
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
