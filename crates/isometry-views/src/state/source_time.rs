//! Read-only source-time selection for the overmap Swatch.
//!
//! The view retains a separate replayed snapshot while a historical prefix is
//! selected. `UiState::world` remains the live authority projection throughout,
//! so returning to live is a pointer change rather than a lossy restore.

use super::*;

const SOURCE_TIME_TICKS: usize = 64;

impl UiState {
    /// Install the host-provided authority origin and immutable event log.
    ///
    /// A selected prefix survives appended events when it remains valid. Live
    /// stays live as the log advances. An absent source means the host did not
    /// retain a truthful origin, so the view exposes no historical control.
    pub fn set_overmap_source_history(&mut self, source: Option<isometry_net::GameSourceHistory>) {
        let selected = self.overmap_source_cursor;
        self.overmap_source = source;
        self.overmap_source_cursor = selected.filter(|cursor| {
            self.overmap_source
                .as_ref()
                .is_some_and(|source| *cursor < source.live_cursor())
        });
        self.refresh_overmap_source_snapshot();
    }

    /// Whether the overmap has more than its live source point to scrub.
    pub fn overmap_source_time_available(&self) -> bool {
        self.overmap_source
            .as_ref()
            .is_some_and(|source| source.live_cursor() > source.earliest_cursor())
    }

    /// Whether the present overmap is a read-only historical projection.
    pub fn overmap_is_historical(&self) -> bool {
        self.overmap_source_snapshot.is_some()
    }

    /// The campaign world selected for the Swatch. Historical source data is
    /// explicitly separate from `self.world`, which is always the live truth.
    pub fn overmap_source_world(&self) -> &CampaignWorld {
        self.overmap_source_snapshot
            .as_ref()
            .map(|snapshot| &snapshot.world)
            .unwrap_or(&self.world)
    }

    /// A compact source-time label for the Swatch control.
    pub fn overmap_source_time_label(&self) -> Option<String> {
        let source = self.overmap_source.as_ref()?;
        let live = source.live_cursor();
        Some(match self.overmap_source_cursor {
            Some(cursor) => format!("history: event {cursor} of {live}"),
            None => format!("live: event {live}"),
        })
    }

    /// Return the Swatch to the live authority projection without changing any
    /// graph curation, camera, selection, or event-source truth.
    pub fn return_overmap_to_live(&mut self) {
        self.overmap_source_cursor = None;
        self.refresh_overmap_source_snapshot();
    }

    /// Convert Cambium's normalized slider state into the nearest stable
    /// prefix cursor. Called after each UI dispatch, rather than from a widget
    /// callback, so keyboard and pointer input share exactly one transition.
    pub fn sync_overmap_source_time(&mut self) {
        let Some(source) = self.overmap_source.as_ref() else {
            return;
        };
        let ticks = source.sequence_ticks(SOURCE_TIME_TICKS);
        let Some(last) = ticks.len().checked_sub(1) else {
            return;
        };
        let index =
            (self.overmap_source_slider.value.clamp(0.0, 1.0) * last as f32).round() as usize;
        let cursor = ticks[index.min(last)];
        self.overmap_source_cursor = (cursor < source.live_cursor()).then_some(cursor);
        self.refresh_overmap_source_snapshot();
    }

    fn refresh_overmap_source_snapshot(&mut self) {
        let Some(source) = self.overmap_source.as_ref() else {
            self.overmap_source_snapshot = None;
            self.overmap_source_slider.value = 1.0;
            return;
        };
        let live = source.live_cursor();
        let cursor = self.overmap_source_cursor.unwrap_or(live);
        self.overmap_source_snapshot = if cursor == live {
            None
        } else {
            match source.snapshot_at(cursor) {
                Ok(snapshot) => Some(snapshot),
                Err(error) => {
                    self.status = format!("history preview unavailable: {error:?}");
                    self.overmap_source_cursor = None;
                    None
                }
            }
        };

        let ticks = source.sequence_ticks(SOURCE_TIME_TICKS);
        let denominator = ticks.len().saturating_sub(1).max(1) as f32;
        self.overmap_source_slider.step = 1.0 / denominator;
        self.overmap_source_slider.page_step = (10.0 / denominator).min(1.0);
        self.overmap_source_slider.value = ticks
            .iter()
            .position(|tick| *tick == cursor)
            .map(|index| index as f32 / denominator)
            .unwrap_or(1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codicil::Codicil;
    use isometry_campaign::WorldFact;

    #[test]
    fn scrubbing_uses_a_disposable_historical_snapshot_and_returns_live() {
        let origin = GameSnapshot {
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
            party_cap: isometry_net::default_party_cap(),
            last_beats: Vec::new(),
            beat_seq: 0,
        };
        let mut history = Codicil::new();
        history.append(GameEvent::Fact(WorldFact {
            id: "mark".to_owned(),
            kind: "history".to_owned(),
            text: "The road was marked.".to_owned(),
            tags: Vec::new(),
        }));
        let mut ui = UiState::new(origin.map.clone());
        let live_world = CampaignWorld::default();
        ui.world = live_world.clone();
        ui.set_overmap_source_history(Some(isometry_net::GameSourceHistory::new(
            origin.clone(),
            history.clone(),
        )));

        ui.overmap_source_slider.value = 0.0;
        ui.sync_overmap_source_time();
        assert!(ui.overmap_is_historical());
        assert_eq!(ui.overmap_source_cursor, Some(0));
        assert_eq!(ui.world, live_world, "live truth stayed untouched");

        history.append(GameEvent::Fact(WorldFact {
            id: "return".to_owned(),
            kind: "history".to_owned(),
            text: "The lantern came home.".to_owned(),
            tags: Vec::new(),
        }));
        ui.set_overmap_source_history(Some(isometry_net::GameSourceHistory::new(origin, history)));
        assert_eq!(
            ui.overmap_source_cursor,
            Some(0),
            "a historical prefix survives a later live event"
        );

        ui.return_overmap_to_live();
        assert!(!ui.overmap_is_historical());
        assert_eq!(ui.overmap_source_cursor, None);
    }
}
