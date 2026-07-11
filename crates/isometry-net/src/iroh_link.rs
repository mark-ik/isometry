//! The iroh QUIC transport: a pump that carries [`NetMessage`]s between
//! machines and drives the same [`HostSession`] / [`ClientSession`] state
//! machines the in-process [`sim`](crate::sim) does. Behind the `iroh`
//! feature so the default build stays dep-light.
//!
//! Wire shape: one bidirectional QUIC stream per peer, length-prefixed
//! postcard frames. The **host opens** the stream and writes the snapshot
//! first, so the client's `accept_bi` resolves on that data (opening a
//! stream is lazy in QUIC — whoever has something to send first must
//! open). The client then sends its intents back on the same stream's
//! reverse half. Turn-based play means this single ordered stream per
//! peer is all the ordering guarantee the protocol needs.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr};
use iroh_tickets::endpoint::EndpointTicket;
use tokio::sync::{mpsc, Mutex};

use codicil::Codicil;
use isometry_campaign::CampaignStore;
use crate::protocol::{fnv1a, GameEvent, GameSnapshot, NetMessage, Outbound, PeerId, Recipient, FNV_OFFSET};
use crate::session::{ClientSession, HostSession};

/// The session ALPN. Bumping it is a protocol break (old clients can't
/// dial a new host).
pub const ALPN: &[u8] = b"isometry/session/v1";

type PeerMap = Arc<Mutex<HashMap<PeerId, mpsc::UnboundedSender<NetMessage>>>>;

/// Derive a routing [`PeerId`] from the connection's remote endpoint id.
fn peer_of(conn: &Connection) -> PeerId {
    PeerId(fnv1a(FNV_OFFSET, conn.remote_id().as_bytes()))
}

async fn write_frame(send: &mut SendStream, msg: &NetMessage) -> Result<(), String> {
    let body = postcard::to_allocvec(msg).map_err(|e| format!("encode: {e}"))?;
    let len = (body.len() as u32).to_le_bytes();
    send.write_all(&len).await.map_err(|e| format!("write len: {e}"))?;
    send.write_all(&body).await.map_err(|e| format!("write body: {e}"))?;
    Ok(())
}

async fn read_frame(recv: &mut RecvStream) -> Result<NetMessage, String> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await.map_err(|e| format!("read len: {e}"))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body).await.map_err(|e| format!("read body: {e}"))?;
    postcard::from_bytes(&body).map_err(|e| format!("decode: {e}"))
}

/// Route the session's outbound messages to peer channels. `Host` is a
/// no-op on the host side (it never addresses itself).
async fn dispatch(peers: &PeerMap, out: Vec<Outbound>) {
    if out.is_empty() {
        return;
    }
    let map = peers.lock().await;
    for (to, msg) in out {
        match to {
            Recipient::All => {
                for tx in map.values() {
                    let _ = tx.send(msg.clone());
                }
            }
            Recipient::One(peer) => {
                if let Some(tx) = map.get(&peer) {
                    let _ = tx.send(msg);
                }
            }
            Recipient::Host => {}
        }
    }
}

/// This endpoint's dialable address, waited for briefly so it carries LAN
/// candidates, with a loopback rewrite so same-machine peers connect even
/// with no network up (the mere transport pattern).
async fn dialable_addr(endpoint: &Endpoint) -> EndpointAddr {
    let mut addr = endpoint.addr();
    for _ in 0..40 {
        if addr.ip_addrs().any(|a| !a.ip().is_loopback()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        addr = endpoint.addr();
    }
    for sock in endpoint.bound_sockets() {
        let dial = if sock.ip().is_unspecified() {
            let ip = if sock.is_ipv4() {
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            } else {
                IpAddr::V6(Ipv6Addr::LOCALHOST)
            };
            SocketAddr::new(ip, sock.port())
        } else {
            sock
        };
        addr = addr.with_ip_addr(dial);
    }
    addr
}

/// The host endpoint: accepts joiners and replicates the authoritative
/// session to them.
pub struct HostNet {
    endpoint: Endpoint,
    session: Arc<Mutex<HostSession>>,
    peers: PeerMap,
}

impl HostNet {
    /// Bind the host endpoint over the authoritative `state`.
    pub async fn bind(state: GameSnapshot) -> Result<Self, String> {
        Self::bind_with_campaign(state, CampaignStore::new()).await
    }

    /// Bind a host restored with its GM-private campaign state.
    pub async fn bind_with_campaign(
        state: GameSnapshot,
        campaign: CampaignStore,
    ) -> Result<Self, String> {
        Self::bind_with_history(state, campaign, Codicil::new()).await
    }

    /// Bind a host restored from a checkpoint's complete ordered history.
    pub async fn bind_with_history(
        state: GameSnapshot,
        campaign: CampaignStore,
        history: Codicil<GameEvent>,
    ) -> Result<Self, String> {
        let endpoint = Endpoint::builder(presets::N0)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| format!("host bind: {e}"))?;
        let mut session = HostSession::with_history(state, campaign, history);
        // A checkpoint may have landed between preparing and finalizing a
        // reveal. Reconcile before any peer can receive the initial snapshot.
        session.reconcile_pending_reveals()?;
        Ok(Self {
            endpoint,
            session: Arc::new(Mutex::new(session)),
            peers: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// The shareable join ticket (a base32 string a player pastes).
    pub async fn ticket(&self) -> String {
        let addr = dialable_addr(&self.endpoint).await;
        EndpointTicket::from(addr).to_string()
    }

    /// Spawn the accept loop. Each joiner gets the snapshot, then the
    /// live `Applied` tail; its intents flow back into the host session.
    pub fn spawn_accept(&self) {
        let endpoint = self.endpoint.clone();
        let session = self.session.clone();
        let peers = self.peers.clone();
        tokio::spawn(async move {
            while let Some(connecting) = endpoint.accept().await {
                let Ok(conn) = connecting.await else { continue };
                let peer = peer_of(&conn);
                // Host opens the stream and speaks first (the snapshot).
                let Ok((send, recv)) = conn.open_bi().await else {
                    continue;
                };
                let (tx, rx) = mpsc::unbounded_channel();
                peers.lock().await.insert(peer, tx);
                tokio::spawn(writer_task(send, rx));
                // Snapshot handshake, then the live tail follows via the
                // same per-peer channel as `Applied` broadcasts arrive.
                let out = session.lock().await.on_connect(peer);
                dispatch(&peers, out).await;
                tokio::spawn(reader_task_host(
                    conn,
                    recv,
                    peer,
                    session.clone(),
                    peers.clone(),
                ));
            }
        });
    }

    /// The host's own move (the DM plays): validate, order, broadcast.
    pub async fn local_event(&self, event: GameEvent) {
        let out = self.session.lock().await.local_event(event);
        dispatch(&self.peers, out).await;
    }

    /// The DM whispers to a named player (directed, not broadcast).
    pub async fn whisper(&self, from: &str, to: &str, text: &str) {
        let out = self.session.lock().await.whisper(from, to, text);
        dispatch(&self.peers, out).await;
    }

    /// Connected player names (whisper targets), from their `Hello`s.
    pub async fn player_names(&self) -> Vec<String> {
        self.session.lock().await.peer_names()
    }

    pub async fn snapshot(&self) -> GameSnapshot {
        self.session.lock().await.state().clone()
    }

    /// A host-only copy for durable save or future host handoff. This never
    /// travels through the public session stream.
    pub async fn campaign(&self) -> CampaignStore {
        self.session.lock().await.campaign().clone()
    }

    /// The authoritative append-only history, for checkpoint persistence and
    /// later host handoff. It never travels in a normal peer snapshot.
    pub async fn history(&self) -> Codicil<GameEvent> {
        self.session.lock().await.history().clone()
    }

    pub async fn seq(&self) -> u64 {
        self.session.lock().await.seq()
    }

    pub async fn log_hash(&self) -> u64 {
        self.session.lock().await.log_hash()
    }
}

async fn writer_task(mut send: SendStream, mut rx: mpsc::UnboundedReceiver<NetMessage>) {
    while let Some(msg) = rx.recv().await {
        if write_frame(&mut send, &msg).await.is_err() {
            break;
        }
    }
    let _ = send.finish();
}

async fn reader_task_host(
    _conn: Connection,
    mut recv: RecvStream,
    peer: PeerId,
    session: Arc<Mutex<HostSession>>,
    peers: PeerMap,
) {
    loop {
        match read_frame(&mut recv).await {
            Ok(msg) => {
                let out = session.lock().await.on_message(peer, msg);
                dispatch(&peers, out).await;
            }
            Err(_) => break,
        }
    }
    peers.lock().await.remove(&peer);
}

/// A player's endpoint: dials a host ticket and replays its stream.
pub struct ClientNet {
    session: Arc<Mutex<ClientSession>>,
    send: Arc<Mutex<SendStream>>,
    _conn: Connection,
    _endpoint: Endpoint,
}

impl ClientNet {
    /// Dial a host by ticket, announce `name`, receive the snapshot, and
    /// start replaying.
    pub async fn join(ticket: &str, name: &str) -> Result<Self, String> {
        let endpoint = Endpoint::bind(presets::N0)
            .await
            .map_err(|e| format!("client bind: {e}"))?;
        let ticket: EndpointTicket = ticket
            .trim()
            .parse()
            .map_err(|e| format!("parse ticket: {e}"))?;
        let addr = EndpointAddr::from(ticket);
        let conn = endpoint
            .connect(addr, ALPN)
            .await
            .map_err(|e| format!("connect: {e}"))?;
        // The host opened the stream; accept it and read the snapshot.
        let (mut send, mut recv) = conn
            .accept_bi()
            .await
            .map_err(|e| format!("accept_bi: {e}"))?;
        let session = Arc::new(Mutex::new(ClientSession::new()));
        // Announce our player name so the DM can whisper to us.
        let hello = session.lock().await.hello(name).1;
        let _ = write_frame(&mut send, &hello).await;
        let reader_session = session.clone();
        tokio::spawn(async move {
            loop {
                match read_frame(&mut recv).await {
                    Ok(msg) => {
                        reader_session.lock().await.on_message(msg);
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            session,
            send: Arc::new(Mutex::new(send)),
            _conn: conn,
            _endpoint: endpoint,
        })
    }

    /// Propose an event to the host.
    pub async fn intent(&self, event: GameEvent) -> Result<(), String> {
        let (_, msg) = self.session.lock().await.intent(event);
        let mut send = self.send.lock().await;
        write_frame(&mut send, &msg).await
    }

    pub async fn state(&self) -> Option<GameSnapshot> {
        self.session.lock().await.state().cloned()
    }

    pub async fn applied(&self) -> u64 {
        self.session.lock().await.applied()
    }

    pub async fn log_hash(&self) -> u64 {
        self.session.lock().await.log_hash()
    }

    /// Take and clear whispers received from the DM.
    pub async fn take_whispers(&self) -> Vec<(String, String)> {
        self.session.lock().await.drain_inbox()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isometry_core::{Facing, MapDocument, SessionEvent, Token, TokenId, TurnList};

    fn snapshot() -> GameSnapshot {
        let mut map = MapDocument::new("iroh demo", 8, 8);
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
            owner: None,
        });
        map.tokens.push(Token {
            id: TokenId(2),
            at: (6, 6),
            facing: Facing::North,
            sprite: "goblin".to_owned(),
            owner: None,
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

    async fn wait_until<F, Fut>(what: &str, mut cond: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        for _ in 0..200 {
            if cond().await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("timed out waiting for: {what}");
    }

    #[tokio::test]
    async fn loopback_session_converges_over_quic() {
        let host = HostNet::bind(snapshot()).await.expect("host bind");
        host.spawn_accept();
        let ticket = host.ticket().await;

        let client = ClientNet::join(&ticket, "tester").await.expect("client join");
        // Snapshot handshake arrives.
        wait_until("snapshot", || async { client.state().await.is_some() }).await;

        // Host and client both play; the host orders the log.
        host.local_event(GameEvent::Map(SessionEvent::TokenMoved {
            id: TokenId(1),
            to: (2, 1),
        }))
        .await;
        client
            .intent(GameEvent::Map(SessionEvent::TokenMoved {
                id: TokenId(2),
                to: (5, 6),
            }))
            .await
            .expect("intent");
        host.local_event(GameEvent::TurnAdd(TokenId(1))).await;

        wait_until("client caught up", || async {
            client.applied().await == host.seq().await && host.seq().await == 3
        })
        .await;

        assert_eq!(client.state().await.as_ref(), Some(&host.snapshot().await));
        assert_eq!(client.log_hash().await, host.log_hash().await);
    }
}
