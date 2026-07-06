//! Bridge between the async iroh session and the synchronous winit loop.
//!
//! A background OS thread runs a tokio runtime that owns the `HostNet` /
//! `ClientNet`. The winit thread talks to it through plain shared state:
//! it drops local [`GameEvent`]s into an unbounded channel, and reads the
//! latest replicated [`GameSnapshot`] from a mutex, redrawing only when a
//! version counter bumps. This is the meerkat sync-lane pattern, minus
//! the ceremony.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use isometry_net::iroh_link::{ClientNet, HostNet};
use isometry_net::{GameEvent, GameSnapshot};
use tokio::sync::mpsc;

/// Which side of the session this process runs.
pub enum Role {
    /// The DM: authoritative, prints a join ticket.
    Host(GameSnapshot),
    /// A player: dials the host's ticket.
    Client(String),
}

/// The winit-thread handle to the background session.
pub struct NetBridge {
    snapshot: Arc<Mutex<Option<GameSnapshot>>>,
    version: Arc<AtomicU64>,
    ticket: Arc<Mutex<Option<String>>>,
    tx: mpsc::UnboundedSender<GameEvent>,
}

impl NetBridge {
    /// Spawn the session on a background runtime and return the handle.
    pub fn spawn(role: Role) -> Self {
        let snapshot: Arc<Mutex<Option<GameSnapshot>>> = Arc::new(Mutex::new(None));
        let version = Arc::new(AtomicU64::new(0));
        let ticket: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::unbounded_channel();

        let snap = snapshot.clone();
        let ver = version.clone();
        let tick = ticket.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("session runtime");
            rt.block_on(async move {
                match role {
                    Role::Host(state) => run_host(state, rx, snap, ver, tick).await,
                    Role::Client(t) => run_client(t, rx, snap, ver).await,
                }
            });
        });

        Self {
            snapshot,
            version,
            ticket,
            tx,
        }
    }

    /// Queue a local game event for the session.
    pub fn submit(&self, event: GameEvent) {
        let _ = self.tx.send(event);
    }

    /// The current change version; the UI redraws when it advances.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    /// The latest replicated snapshot, if one has arrived.
    pub fn latest(&self) -> Option<GameSnapshot> {
        self.snapshot.lock().unwrap().clone()
    }

    /// The host's join ticket, once bound.
    pub fn ticket(&self) -> Option<String> {
        self.ticket.lock().unwrap().clone()
    }
}

fn publish(
    snapshot: &Arc<Mutex<Option<GameSnapshot>>>,
    version: &Arc<AtomicU64>,
    state: GameSnapshot,
) {
    *snapshot.lock().unwrap() = Some(state);
    version.fetch_add(1, Ordering::Relaxed);
}

async fn run_host(
    state: GameSnapshot,
    mut rx: mpsc::UnboundedReceiver<GameEvent>,
    snapshot: Arc<Mutex<Option<GameSnapshot>>>,
    version: Arc<AtomicU64>,
    ticket: Arc<Mutex<Option<String>>>,
) {
    let host = match HostNet::bind(state).await {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[isometry] host bind failed: {e}");
            return;
        }
    };
    host.spawn_accept();
    let t = host.ticket().await;
    *ticket.lock().unwrap() = Some(t.clone());
    println!("[isometry] hosting. share this ticket to join:\n\n  {t}\n");
    publish(&snapshot, &version, host.snapshot().await);

    let mut last_seq = host.seq().await;
    loop {
        tokio::select! {
            maybe = rx.recv() => match maybe {
                Some(event) => host.local_event(event).await,
                None => break, // UI gone
            },
            _ = tokio::time::sleep(Duration::from_millis(80)) => {}
        }
        // Republish when anything (our move or a client's) advanced the log.
        let seq = host.seq().await;
        if seq != last_seq {
            last_seq = seq;
            publish(&snapshot, &version, host.snapshot().await);
        }
    }
}

async fn run_client(
    ticket: String,
    mut rx: mpsc::UnboundedReceiver<GameEvent>,
    snapshot: Arc<Mutex<Option<GameSnapshot>>>,
    version: Arc<AtomicU64>,
) {
    let client = match ClientNet::join(&ticket).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[isometry] join failed: {e}");
            return;
        }
    };
    println!("[isometry] joined session; replaying host log.");
    let mut last_applied = u64::MAX;
    loop {
        tokio::select! {
            maybe = rx.recv() => match maybe {
                Some(event) => {
                    let _ = client.intent(event).await;
                }
                None => break,
            },
            _ = tokio::time::sleep(Duration::from_millis(60)) => {}
        }
        let applied = client.applied().await;
        if applied != last_applied {
            if let Some(state) = client.state().await {
                last_applied = applied;
                publish(&snapshot, &version, state);
            }
        }
    }
}
