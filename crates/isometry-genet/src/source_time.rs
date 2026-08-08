//! Desktop ownership of an event log's replay origin.
//!
//! `Codicil` alone is not enough to show history: the current materialized
//! snapshot cannot be played backwards. The app captures and persists the
//! public state immediately before the first authority event, then hands a
//! read-only `GameSourceHistory` to the view. Older checkpoints without that
//! origin stay honestly live-only.

use super::*;

impl App {
    /// Capture the source origin once, before an empty authority log receives
    /// its first event. A populated log with no origin is deliberately left
    /// unavailable rather than replayed from the wrong state.
    pub(crate) fn ensure_history_origin(&mut self, snapshot: &GameSnapshot) {
        if self.history_origin.is_none() && self.history.is_empty() {
            self.history_origin = Some(snapshot.clone());
            self.source_history_len = None;
            self.source_history_attached = false;
        }
    }

    /// Give the view a fresh source adapter only when its append-only log has
    /// advanced (or a checkpoint changed origin). The view retains a selected
    /// historical cursor across this refresh.
    pub(crate) fn refresh_source_history(&mut self) {
        let source = self
            .history_origin
            .clone()
            .map(|origin| isometry_net::GameSourceHistory::new(origin, self.history.clone()));
        let next_len = source.as_ref().map(|source| source.live_cursor() as usize);
        if self.source_history_attached && next_len == self.source_history_len {
            return;
        }
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.set_overmap_source_history(source));
        }
        self.source_history_len = next_len;
        self.source_history_attached = true;
    }
}
