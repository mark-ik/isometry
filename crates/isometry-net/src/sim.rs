//! In-process routing of a session's [`Outbound`] traffic: a host plus
//! any number of clients, all wired through one queue. It is the
//! transport-free driver the replication tests run against, and doubles
//! as a same-machine multi-window harness. The iroh transport does the
//! same routing over QUIC.

use std::collections::{BTreeMap, VecDeque};

use crate::protocol::{GameEvent, NetMessage, Outbound, PeerId, Recipient};
use crate::session::{ClientSession, HostSession};

/// One message in flight, already resolved to a concrete destination.
enum Hop {
    ToHost { from: PeerId, msg: NetMessage },
    ToClient { to: PeerId, msg: NetMessage },
}

/// A host and its clients, routed in-process. Deliveries queue and flush
/// to quiescence via [`Sim::settle`].
pub struct Sim {
    pub host: HostSession,
    pub clients: BTreeMap<PeerId, ClientSession>,
    queue: VecDeque<Hop>,
}

impl Sim {
    pub fn new(host: HostSession) -> Self {
        Self {
            host,
            clients: BTreeMap::new(),
            queue: VecDeque::new(),
        }
    }

    /// Add a client and run the host's connect handshake (snapshot). The
    /// caller picks the `PeerId`; the iroh transport derives it from the
    /// node id.
    pub fn connect(&mut self, peer: PeerId) {
        self.clients.insert(peer, ClientSession::new());
        let out = self.host.on_connect(peer);
        self.enqueue_from_host(out);
        self.settle();
    }

    /// The host proposes an event (the DM's own move).
    pub fn host_event(&mut self, event: GameEvent) {
        let out = self.host.local_event(event);
        self.enqueue_from_host(out);
        self.settle();
    }

    /// A client proposes an event; it reaches the host, is validated,
    /// and the resulting broadcast flows back to every peer.
    pub fn client_intent(&mut self, peer: PeerId, event: GameEvent) {
        if let Some(client) = self.clients.get(&peer) {
            let out = client.intent(event);
            self.enqueue_from_client(peer, vec![out]);
            self.settle();
        }
    }

    /// A client announces its player name to the host.
    pub fn client_hello(&mut self, peer: PeerId, name: &str) {
        if let Some(client) = self.clients.get(&peer) {
            let out = client.hello(name);
            self.enqueue_from_client(peer, vec![out]);
            self.settle();
        }
    }

    /// The DM whispers to a named player.
    pub fn host_whisper(&mut self, from: &str, to: &str, text: &str) {
        let out = self.host.whisper(from, to, text);
        self.enqueue_from_host(out);
        self.settle();
    }

    fn enqueue_from_host(&mut self, out: Vec<Outbound>) {
        for (to, msg) in out {
            match to {
                Recipient::All => {
                    for &peer in self.clients.keys() {
                        self.queue.push_back(Hop::ToClient {
                            to: peer,
                            msg: msg.clone(),
                        });
                    }
                }
                Recipient::One(peer) => {
                    self.queue.push_back(Hop::ToClient { to: peer, msg });
                }
                // The host never addresses itself.
                Recipient::Host => {}
            }
        }
    }

    fn enqueue_from_client(&mut self, from: PeerId, out: Vec<Outbound>) {
        for (to, msg) in out {
            match to {
                Recipient::Host => self.queue.push_back(Hop::ToHost { from, msg }),
                // Clients only ever address the host.
                Recipient::All | Recipient::One(_) => {}
            }
        }
    }

    /// Deliver queued messages until nothing is in flight.
    pub fn settle(&mut self) {
        while let Some(hop) = self.queue.pop_front() {
            match hop {
                Hop::ToHost { from, msg } => {
                    let out = self.host.on_message(from, msg);
                    self.enqueue_from_host(out);
                }
                Hop::ToClient { to, msg } => {
                    if let Some(client) = self.clients.get_mut(&to) {
                        let out = client.on_message(msg);
                        self.enqueue_from_client(to, out);
                    }
                }
            }
        }
    }
}
