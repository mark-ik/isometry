//! A two-process session smoke over real QUIC. Run the host, copy its
//! ticket, join from another terminal (same machine or another), and
//! watch the client converge on the host's log hash.
//!
//!   cargo run -p isometry-net --features iroh --example session_smoke -- host
//!   cargo run -p isometry-net --features iroh --example session_smoke -- join <TICKET>
//!
//! The host advances the turn order every two seconds; the client prints
//! its applied count and log hash, which must track the host's.

use std::time::Duration;

use isometry_core::{Facing, MapDocument, Token, TokenId, TurnList};
use isometry_net::iroh_link::{ClientNet, HostNet};
use isometry_net::{GameEvent, GameSnapshot};

fn demo_snapshot() -> GameSnapshot {
    let mut map = MapDocument::new("smoke skirmish", 8, 8);
    let grass = map.intern_tile_kind("grass");
    for r in 0..8 {
        for c in 0..8 {
            map.ground.set(c, r, grass);
        }
    }
    let mut turns = TurnList::new();
    for id in 1..=3u32 {
        map.tokens.push(Token {
            id: TokenId(id),
            at: (id as i32, 1),
            facing: Facing::South,
            sprite: "knight".to_owned(),
            owner: None,
        });
        turns.add(TokenId(id));
    }
    GameSnapshot {
        map,
        turns,
        roll_log: Vec::new(),
        journal: Vec::new(),
        inventories: Default::default(),
        generations: Vec::new(),
        maps: Default::default(),
        active_map: None,
        world: Default::default(),
    }
}

#[tokio::main]
async fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    match mode.as_str() {
        "host" => {
            let host = HostNet::bind(demo_snapshot()).await.expect("bind host");
            host.spawn_accept();
            println!(
                "share this ticket with a player:\n\n  {}\n",
                host.ticket().await
            );
            println!("hosting; advancing the turn every 2s. ctrl-c to stop.");
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                host.local_event(GameEvent::TurnAdvance).await;
                let active = host
                    .snapshot()
                    .await
                    .turns
                    .active()
                    .map(|t| t.0)
                    .unwrap_or(0);
                println!(
                    "host  seq={} hash={:016x} active_token={active}",
                    host.seq().await,
                    host.log_hash().await
                );
            }
        }
        "join" => {
            let ticket = std::env::args().nth(2).expect("usage: join <ticket>");
            let client = ClientNet::join(&ticket, "player").await.expect("join host");
            println!("joined; replaying the host's log. ctrl-c to stop.");
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if let Some(state) = client.state().await {
                    let active = state.turns.active().map(|t| t.0).unwrap_or(0);
                    println!(
                        "client applied={} hash={:016x} active_token={active} tokens={}",
                        client.applied().await,
                        client.log_hash().await,
                        state.map.tokens.len()
                    );
                }
            }
        }
        _ => eprintln!("usage: session_smoke host | join <ticket>"),
    }
}
