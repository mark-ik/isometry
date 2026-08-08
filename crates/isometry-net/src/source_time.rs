//! Read-only snapshots of an authority event log.
//!
//! A [`GameSourceHistory`] pairs the public state from immediately before a
//! Codicil began with that log's immutable entries. Prefix cursors replay into
//! a fresh [`GameSnapshot`]; selecting one never mutates the live authority or
//! changes the log. The owner of the log must retain the origin explicitly:
//! reconstructing an older state from the current materialized snapshot would
//! be a fiction.

use codicil::Codicil;

use crate::{GameError, GameEvent, GameSnapshot, apply_game};

/// A failure while materializing a source-time prefix.
#[derive(Clone, Debug, PartialEq)]
pub enum GameSourceTimeError {
    /// A prefix cursor must be between the empty origin and the live tail.
    CursorOutOfRange { requested: u64, live: u64 },
    /// The durable log is internally inconsistent with its declared origin.
    /// The error carries the failing prefix entry, rather than presenting a
    /// plausible but incomplete historical scene.
    Replay { cursor: u64, error: GameError },
}

/// The event-source authority behind a historical game projection.
///
/// `0` selects the origin before any entry; `live_cursor()` selects every
/// current entry. Cursors are prefix boundaries rather than entry addresses,
/// making a stable cursor remain meaningful when a later event is appended.
#[derive(Clone, Debug)]
pub struct GameSourceHistory {
    origin: GameSnapshot,
    history: Codicil<GameEvent>,
}

impl GameSourceHistory {
    /// Build a replayable source from the public state that existed just before
    /// `history`'s first entry.
    pub fn new(origin: GameSnapshot, history: Codicil<GameEvent>) -> Self {
        Self { origin, history }
    }

    /// The public state before the first logged event.
    pub fn origin(&self) -> &GameSnapshot {
        &self.origin
    }

    /// The immutable ordered authority log.
    pub fn history(&self) -> &Codicil<GameEvent> {
        &self.history
    }

    /// The empty-prefix cursor.
    pub const fn earliest_cursor(&self) -> u64 {
        0
    }

    /// The prefix immediately following the newest authority event.
    pub fn live_cursor(&self) -> u64 {
        self.history.len() as u64
    }

    /// Materialize exactly the prefix ending at `cursor`.
    pub fn snapshot_at(&self, cursor: u64) -> Result<GameSnapshot, GameSourceTimeError> {
        let live = self.live_cursor();
        if cursor > live {
            return Err(GameSourceTimeError::CursorOutOfRange {
                requested: cursor,
                live,
            });
        }
        let mut snapshot = self.origin.clone();
        for (index, event) in self.history.entries()[..cursor as usize].iter().enumerate() {
            apply_game(&mut snapshot, event).map_err(|error| GameSourceTimeError::Replay {
                cursor: index as u64 + 1,
                error,
            })?;
        }
        Ok(snapshot)
    }

    /// Evenly distributed prefix cursors for a compact scrubber.
    ///
    /// The log is ordered but has no required wall-clock timestamp, so this
    /// deliberately samples sequence order. Requests with more room than
    /// entries include every prefix; larger logs retain both endpoints.
    pub fn sequence_ticks(&self, max_points: usize) -> Vec<u64> {
        let live = self.live_cursor();
        if max_points == 0 {
            return Vec::new();
        }
        if max_points == 1 || live == 0 {
            return vec![live];
        }
        let points = max_points.min(live as usize + 1);
        let denominator = (points - 1) as u64;
        (0..points)
            .map(|index| index as u64 * live / denominator)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isometry_campaign::{CampaignWorld, WorldFact};
    use isometry_core::{MapDocument, TurnList};

    fn snapshot() -> GameSnapshot {
        GameSnapshot {
            map: MapDocument::new("source-time", 2, 2),
            turns: TurnList::new(),
            roll_log: Vec::new(),
            journal: Vec::new(),
            inventories: Default::default(),
            generations: Vec::new(),
            maps: Default::default(),
            active_map: None,
            world: CampaignWorld::default(),
            clocks: Default::default(),
            party_cap: crate::default_party_cap(),
            last_beats: Vec::new(),
            beat_seq: 0,
            applied_actions: Default::default(),
        }
    }

    #[test]
    fn prefix_snapshots_replay_without_changing_live_authority() {
        let origin = snapshot();
        let mut history = Codicil::new();
        history.append(GameEvent::Fact(WorldFact {
            id: "first-light".to_owned(),
            kind: "history".to_owned(),
            text: "The lantern was lit.".to_owned(),
            tags: Vec::new(),
        }));
        history.append(GameEvent::Fact(WorldFact {
            id: "second-light".to_owned(),
            kind: "history".to_owned(),
            text: "The road was marked.".to_owned(),
            tags: Vec::new(),
        }));
        let source = GameSourceHistory::new(origin.clone(), history);

        assert_eq!(source.earliest_cursor(), 0);
        assert_eq!(source.live_cursor(), 2);
        assert_eq!(source.sequence_ticks(4), vec![0, 1, 2]);
        assert!(source.snapshot_at(0).unwrap().journal.is_empty());
        assert_eq!(source.snapshot_at(1).unwrap().journal.len(), 1);
        assert_eq!(source.snapshot_at(2).unwrap().journal.len(), 2);
        assert_eq!(source.origin(), &origin, "replay cannot alter its origin");
        assert!(matches!(
            source.snapshot_at(3),
            Err(GameSourceTimeError::CursorOutOfRange {
                requested: 3,
                live: 2
            })
        ));
    }
}
