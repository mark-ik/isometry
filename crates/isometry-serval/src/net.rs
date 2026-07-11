//! Bridge between the async iroh session and the synchronous winit loop.
//!
//! The winit kernel owns the view-facing session data. A typed Armillary actor
//! owns Tokio and `HostNet` / `ClientNet`; it receives commands and emits
//! snapshots, campaign state, and status updates for the kernel to drain.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use armillary::{ActorHandle, Emitter, Wake};
use codicil::Codicil;
use isometry_campaign::CampaignStore;
use isometry_net::iroh_link::{ClientNet, HostNet};
use isometry_net::{GameEvent, GameSnapshot};

/// Which side of the session this process runs.
pub enum Role {
    /// The DM: authoritative, prints a join ticket.
    Host {
        state: GameSnapshot,
        campaign: CampaignStore,
        history: Codicil<GameEvent>,
    },
    /// A player: dials the host's ticket, announcing a name.
    Client { ticket: String, name: String },
}

enum BridgeCommand {
    Event(GameEvent),
    Whisper { to: String, text: String },
}

enum BridgeUpdate {
    HostReady {
        ticket: String,
        snapshot: GameSnapshot,
        campaign: CampaignStore,
        history: Codicil<GameEvent>,
    },
    HostState {
        snapshot: GameSnapshot,
        campaign: CampaignStore,
        history: Codicil<GameEvent>,
        players: Vec<String>,
    },
    ClientState(GameSnapshot),
    Whispers(Vec<(String, String)>),
    Failed(String),
}

/// The winit-thread handle to the background session actor.
pub struct NetBridge {
    actor: ActorHandle<BridgeCommand>,
    updates: Receiver<BridgeUpdate>,
    snapshot: Option<GameSnapshot>,
    campaign: Option<CampaignStore>,
    history: Option<Codicil<GameEvent>>,
    version: u64,
    ticket: Option<String>,
    inbox: Vec<(String, String)>,
    players: Vec<String>,
    failure: Option<String>,
}

impl NetBridge {
    /// Spawn the session actor. The app already polls this bridge from the
    /// winit loop, so its wake merely requests the next normal UI turn.
    pub fn spawn(role: Role) -> Self {
        let wake: Wake = std::sync::Arc::new(|| {});
        let (actor, updates) =
            armillary::spawn_named("isometry-session", wake, move |commands, out| {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("session runtime");
                rt.block_on(run(role, commands, out));
            });
        Self {
            actor,
            updates,
            snapshot: None,
            campaign: None,
            history: None,
            version: 0,
            ticket: None,
            inbox: Vec::new(),
            players: Vec::new(),
            failure: None,
        }
    }

    /// Drain actor updates on the winit thread. Returns true when a new
    /// snapshot was accepted and the view needs rebuilding.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.updates.try_recv() {
                Ok(BridgeUpdate::HostReady {
                    ticket,
                    snapshot,
                    campaign,
                    history,
                }) => {
                    self.ticket = Some(ticket);
                    self.campaign = Some(campaign);
                    self.history = Some(history);
                    self.set_snapshot(snapshot);
                    changed = true;
                }
                Ok(BridgeUpdate::HostState {
                    snapshot,
                    campaign,
                    history,
                    players,
                }) => {
                    self.campaign = Some(campaign);
                    self.history = Some(history);
                    self.players = players;
                    self.set_snapshot(snapshot);
                    changed = true;
                }
                Ok(BridgeUpdate::ClientState(snapshot)) => {
                    self.set_snapshot(snapshot);
                    changed = true;
                }
                Ok(BridgeUpdate::Whispers(mut whispers)) => self.inbox.append(&mut whispers),
                Ok(BridgeUpdate::Failed(error)) => self.failure = Some(error),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        changed
    }

    fn set_snapshot(&mut self, snapshot: GameSnapshot) {
        self.snapshot = Some(snapshot);
        self.version = self.version.wrapping_add(1);
    }

    /// Queue a local game event for the session actor.
    pub fn submit(&self, event: GameEvent) {
        let _ = self.actor.command(BridgeCommand::Event(event));
    }

    /// Host: send a directed whisper to a named player.
    pub fn whisper(&self, to: String, text: String) {
        let _ = self.actor.command(BridgeCommand::Whisper { to, text });
    }

    /// Client: take whispers received since the last call.
    pub fn take_whispers(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.inbox)
    }

    /// Host: connected player names (whisper targets).
    pub fn players(&self) -> Vec<String> {
        self.players.clone()
    }

    /// The current change version; the UI redraws when it advances.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The latest replicated snapshot, if one has arrived.
    pub fn latest(&self) -> Option<GameSnapshot> {
        self.snapshot.clone()
    }

    /// Host-only GM state as of the latest session poll.
    pub fn campaign(&self) -> Option<CampaignStore> {
        self.campaign.clone()
    }

    pub fn history(&self) -> Option<Codicil<GameEvent>> {
        self.history.clone()
    }

    /// The host's join ticket, once bound.
    pub fn ticket(&self) -> Option<String> {
        self.ticket.clone()
    }

    /// A background bind/join failure. Reading clears the pending message.
    pub fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }
}

async fn run(role: Role, commands: Receiver<BridgeCommand>, out: Emitter<BridgeUpdate>) {
    match role {
        Role::Host {
            state,
            campaign,
            history,
        } => run_host(state, campaign, history, commands, out).await,
        Role::Client { ticket, name } => run_client(ticket, name, commands, out).await,
    }
}

fn drain_commands(commands: &Receiver<BridgeCommand>) -> Result<Vec<BridgeCommand>, ()> {
    let mut drained = Vec::new();
    loop {
        match commands.try_recv() {
            Ok(command) => drained.push(command),
            Err(TryRecvError::Empty) => return Ok(drained),
            Err(TryRecvError::Disconnected) => return Err(()),
        }
    }
}

async fn run_host(
    state: GameSnapshot,
    campaign: CampaignStore,
    history: Codicil<GameEvent>,
    commands: Receiver<BridgeCommand>,
    out: Emitter<BridgeUpdate>,
) {
    let host = match HostNet::bind_with_history(state, campaign, history).await {
        Ok(host) => host,
        Err(error) => {
            out.emit(BridgeUpdate::Failed(format!("host bind failed: {error}")));
            return;
        }
    };
    host.spawn_accept();
    let ticket = host.ticket().await;
    println!("[isometry] hosting. share this ticket to join:\n\n  {ticket}\n");
    out.emit(BridgeUpdate::HostReady {
        ticket,
        snapshot: host.snapshot().await,
        campaign: host.campaign().await,
        history: host.history().await,
    });

    let mut last_seq = host.seq().await;
    let mut last_players = Vec::new();
    loop {
        let commands = match drain_commands(&commands) {
            Ok(commands) => commands,
            Err(()) => break,
        };
        for command in commands {
            match command {
                BridgeCommand::Event(event) => host.local_event(event).await,
                BridgeCommand::Whisper { to, text } => host.whisper("dm", &to, &text).await,
            }
        }

        let seq = host.seq().await;
        let players = host.player_names().await;
        if seq != last_seq || players != last_players {
            last_seq = seq;
            last_players = players.clone();
            out.emit(BridgeUpdate::HostState {
                snapshot: host.snapshot().await,
                campaign: host.campaign().await,
                history: host.history().await,
                players,
            });
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

async fn run_client(
    ticket: String,
    name: String,
    commands: Receiver<BridgeCommand>,
    out: Emitter<BridgeUpdate>,
) {
    let client = match ClientNet::join(&ticket, &name).await {
        Ok(client) => client,
        Err(error) => {
            out.emit(BridgeUpdate::Failed(format!("join failed: {error}")));
            return;
        }
    };
    println!("[isometry] joined session as {name}; replaying host log.");
    let mut last_applied = u64::MAX;
    loop {
        let commands = match drain_commands(&commands) {
            Ok(commands) => commands,
            Err(()) => break,
        };
        for command in commands {
            if let BridgeCommand::Event(event) = command {
                let _ = client.intent(event).await;
            }
        }

        let applied = client.applied().await;
        if applied != last_applied {
            if let Some(snapshot) = client.state().await {
                last_applied = applied;
                out.emit(BridgeUpdate::ClientState(snapshot));
            }
        }
        let whispers = client.take_whispers().await;
        if !whispers.is_empty() {
            out.emit(BridgeUpdate::Whispers(whispers));
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isometry_core::{MapDocument, TurnList};

    fn snapshot() -> GameSnapshot {
        GameSnapshot {
            map: MapDocument::new("bridge", 2, 2),
            turns: TurnList::new(),
            roll_log: Vec::new(),
            journal: Vec::new(),
            inventories: Default::default(),
            generations: Vec::new(),
        }
    }

    #[test]
    fn host_bridge_delivers_actor_state_to_the_kernel() {
        let mut bridge = NetBridge::spawn(Role::Host {
            state: snapshot(),
            campaign: CampaignStore::new(),
            history: Codicil::new(),
        });

        for _ in 0..100 {
            bridge.poll();
            if bridge.ticket().is_some() && bridge.latest().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            bridge.ticket().is_some(),
            "host actor bound and published a ticket"
        );
        assert_eq!(bridge.latest(), Some(snapshot()));

        let version = bridge.version();
        bridge.submit(GameEvent::TurnAdvance);
        for _ in 0..100 {
            bridge.poll();
            if bridge.version() > version {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("host command did not return through the actor update channel");
    }
}
