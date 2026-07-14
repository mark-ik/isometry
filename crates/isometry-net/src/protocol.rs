use std::collections::BTreeMap;

use isometry_campaign::{
    CampaignMap, CampaignWorld, GenerationRecord, Inventory, ItemId, ItemModifierReveal,
    WorldEvent, WorldFact,
};
use isometry_core::{
    Beat, MapDocument, RollRecord, SessionEvent, SheetData, SheetDelta, TileCoord, TokenId,
    TurnList,
};
use serde::{Deserialize, Serialize};

/// A peer's identity within a session. For the iroh transport this wraps
/// the remote node id; the pure-sync core only needs it to route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PeerId(pub u64);

/// The replicated game state: exactly the substrate document plus the
/// turn order. View concerns (camera, undo, selection) never cross the
/// wire; each peer keeps its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameSnapshot {
    pub map: MapDocument,
    pub turns: TurnList,
    /// The shared roll log, most recent last, capped at
    /// [`ROLL_LOG_CAP`]. Everyone at the table sees every roll.
    #[serde(default)]
    pub roll_log: Vec<RollRecord>,
    /// The campaign journal: every public [`WorldFact`] committed so far,
    /// oldest first. Uncapped by design: entries are small text and each
    /// is campaign state (a revealed secret, a history event), so
    /// dropping old ones would silently delete facts, unlike the roll
    /// log's noise. Only public faces ever land here; the GM layer lives
    /// in the host's private `CampaignStore` (worldbuilding decision 8).
    #[serde(default)]
    pub journal: Vec<WorldFact>,
    /// Public carried/equipped item instances, keyed by owning token. Hidden
    /// modifiers remain in the host-private `CampaignStore` until revealed.
    #[serde(default)]
    pub inventories: BTreeMap<TokenId, Inventory>,
    /// Host-accepted generator results, stored as result data rather than
    /// peer-rerunnable scripts. A later type-specific operation lowers a
    /// record into items, map changes, cast NPCs, or story state.
    #[serde(default)]
    pub generations: Vec<GenerationRecord>,
    /// Named authored/generated maps retained by the campaign. `map` is the
    /// active editable projection; edits mirror back into this registry when
    /// `active_map` is set.
    #[serde(default)]
    pub maps: BTreeMap<String, CampaignMap>,
    #[serde(default)]
    pub active_map: Option<String>,
    /// Public world state. Secret fact bodies stay in `CampaignStore`.
    #[serde(default)]
    pub world: CampaignWorld,
    /// The beats of the most recently applied event, kept so that *every* peer
    /// can play them and not only the peer that produced them. A client renders
    /// from the snapshot, so without this the defender's recoil would be seen on
    /// the host alone.
    ///
    /// Deliberately a bare beat list rather than "the last action": an emote has
    /// no resolution behind it, and the board should not have to care which kind
    /// of event asked for a flourish. This is representation, not truth.
    /// `beat_seq` exists so a view can tell a new flourish from the same snapshot
    /// arriving twice, and so two identical consecutive strikes each play.
    /// Neither field feeds a rule.
    #[serde(default)]
    pub last_beats: Vec<Beat>,
    #[serde(default)]
    pub beat_seq: u64,
}

/// Rolls kept in the shared log; older ones drop off.
pub const ROLL_LOG_CAP: usize = 50;

/// One adjudicated action, resolved by whoever held the sequencer, applied by
/// everyone.
///
/// This is the fact that a rules system produced; it is *not* the rules system.
/// Every field is substrate vocabulary (tokens, rolls, integer deltas, beat
/// names), so this crate replicates a resolved attack without knowing that
/// `hp_current` means hit points or that `1d8` is a longsword. Peers apply the
/// deltas verbatim: they never rerun the script and never reroll, which is what
/// keeps one machine's Lua the only Lua that runs and the convergence hash
/// meaningful.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionResolved {
    pub actor: TokenId,
    pub target: TokenId,
    pub action_key: String,
    /// Human label for the log ("Attack").
    pub label: String,
    /// The public attack roll. Everyone sees the dice that decided it.
    pub attack: RollRecord,
    pub hit: bool,
    /// The effect roll, present only on a hit.
    pub damage: Option<RollRecord>,
    /// The consequences. Empty on a miss: a miss changes nothing.
    pub deltas: Vec<SheetDelta>,
    /// How to show it. Purely representational; a peer that ignores every beat
    /// still converges on the same state and the same hash.
    pub beats: Vec<Beat>,
    /// Tokens this action put out of play, as judged by the rules system. Unlike
    /// the beats, this *is* state: applying it marks them defeated, and the
    /// substrate then skips their turns and refuses them as targets.
    #[serde(default)]
    pub defeated: Vec<TokenId>,
    /// Forced movement: tokens this action actually relocated, and to where.
    ///
    /// The counterpart to a stagger beat, and the reason the two are separate
    /// fields. A stagger is a flourish that peers may render differently and even
    /// skip; **this** is game truth, so the landing tile is decided once, by the
    /// board, and every peer applies exactly it. Reach, line of sight, and the
    /// next player's options all change because of it.
    #[serde(default)]
    pub displaced: Vec<(TokenId, TileCoord)>,
}

/// The replicated unit: a map mutation or a turn-order change. The host
/// orders these into one log every peer replays; `MapDocument` and
/// `TurnList` together are the whole shared state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GameEvent {
    /// A substrate document mutation (token move, tile paint, ...).
    Map(SessionEvent),
    /// Add a token to the turn order.
    TurnAdd(TokenId),
    /// Drop a token from the turn order (free movement thereafter).
    TurnRemove(TokenId),
    /// Advance to the next turn.
    TurnAdvance,
    /// Replace the whole turn order (initiative roll result).
    TurnSetOrder(Vec<TokenId>),
    /// A resolved dice roll to append to the shared log.
    Rolled(RollRecord),
    /// Bind or replace a token's character sheet.
    SheetSet { token: TokenId, sheet: SheetData },
    /// A public campaign fact committed to the journal: a revealed
    /// secret, a generated object's public face, narration, a history
    /// event. Host-committed only; the host rejects client intents of
    /// this variant (the DM is the authority over what becomes true).
    Fact(WorldFact),
    /// Replace one token's public inventory/equipment state. A rules plugin
    /// interprets modifiers; the substrate only stores and replicates them.
    InventorySet {
        token: TokenId,
        inventory: Inventory,
    },
    /// Move one whole public item instance between tokens atomically. Applying
    /// it also clears any source equipment slot pointing at that instance.
    ItemTransfer {
        from: TokenId,
        to: TokenId,
        item: ItemId,
    },
    /// Publish a previously hidden modifier into a public item instance.
    /// Host-committed only, like [`GameEvent::Fact`].
    ItemModifierRevealed(ItemModifierReveal),
    /// Record a typed generator result selected by the host. It has no direct
    /// map or inventory effect: those lowerings remain explicit events.
    Generation(GenerationRecord),
    /// Store one generated/authored map without changing the active board.
    /// Host-committed only.
    MapStored(CampaignMap),
    /// Swap the active editable board to a named campaign map.
    /// Host-committed only.
    MapActivated { id: String },
    /// Apply one idempotent public world-state change. Host-committed only.
    World(WorldEvent),
    /// One adjudicated action: the only event by which one token changes
    /// another. Applying it appends its rolls to the shared log and its deltas
    /// to the addressed sheets.
    ///
    /// Appended at the end deliberately. Postcard encodes the variant index, so
    /// inserting this next to `Rolled` (where it belongs by meaning) would
    /// silently re-tag every later variant and misread existing saved
    /// checkpoints.
    ActionResolved(ActionResolved),
    /// A token plays a beat for its own sake: a cheer, a shrug, a taunt.
    ///
    /// The same primitive as a combat beat, with no resolution behind it and no
    /// state to change. That is the whole of the emote system: it needs no
    /// rules, no dice, and no new rendering, because a beat already exists.
    /// Unlike an action, a player may throw this for themselves, since the worst
    /// a liar can do is wave.
    Emoted { token: TokenId, beat: String },
}

/// One message on the wire. The host is the authority: clients send
/// `Intent`, the host validates and rebroadcasts `Applied` with a
/// sequence number, and every peer applies the same ordered log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NetMessage {
    /// Host to a joining client: full state as of `seq` applied events,
    /// plus the host's rolling log hash at that point so a late joiner
    /// seeds its own hash to match and converges on the tail.
    Snapshot {
        seq: u64,
        log_hash: u64,
        state: GameSnapshot,
    },
    /// Host to all: the next ordered, host-validated event.
    Applied { seq: u64, event: GameEvent },
    /// Client to host: a proposed event (may be rejected).
    Intent { event: GameEvent },
    /// Host to the proposer: the intent failed validation.
    Rejected { reason: String },
    /// Client to host on connect: announce the player name, so the host
    /// can address whispers to it.
    Hello { name: String },
    /// Host to one peer: a private message (a GM whisper). Directed, not
    /// broadcast, so it never enters the replicated log.
    Whisper { from: String, text: String },
    /// Client to host: "I swing at that goblin." The *ask*, not the answer.
    ///
    /// This is deliberately not a `GameEvent`. A client may not propose an
    /// `ActionResolved`, because that is a verdict and a peer cannot pronounce
    /// its own. It asks, and the host's rules system decides. The host queues
    /// these for its app to drain, because this crate is rules-blind and holds no
    /// `System` to resolve them with.
    ///
    /// Appended at the end: postcard tags variants by index.
    Action(ActionIntent),
}

/// A player asking to act. Everything about the outcome is the host's to say.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionIntent {
    pub actor: TokenId,
    pub target: TokenId,
    pub action_key: String,
}

/// Where a produced message goes. The transport resolves this to actual
/// peers; the session core stays routing-agnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recipient {
    /// Every connected peer (used for `Applied`, so ordering is uniform).
    All,
    /// One specific peer (snapshot to a joiner, reject to a proposer).
    One(PeerId),
    /// The host (a client's `Intent`).
    Host,
}

/// A message the session wants sent, paired with its destination.
pub type Outbound = (Recipient, NetMessage);

/// FNV-1a over bytes. A fixed, std-independent hash so the log-hash
/// convergence check holds across machines and std versions (unlike
/// `DefaultHasher`, whose SipHash keys are unspecified across builds).
pub(crate) fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Starting basis for the rolling log hash.
pub(crate) const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// Fold one applied `(seq, event)` into the rolling log hash via its
/// postcard bytes — the same byte form both host and client hash, so
/// equal logs give equal hashes regardless of platform.
pub(crate) fn fold_event(hash: u64, seq: u64, event: &GameEvent) -> u64 {
    let mut h = fnv1a(hash, &seq.to_le_bytes());
    if let Ok(bytes) = postcard::to_allocvec(event) {
        h = fnv1a(h, &bytes);
    }
    h
}
