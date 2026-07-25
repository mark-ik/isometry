//! The bridge from the catalog's selection rows to isometry's domain actions.
//!
//! Cambium's `segmented_control` owns a `SelectionState` and writes only that:
//! it moves `selected`, and knows nothing about pace being a replicated world
//! event. Something has to notice and dispatch, and this is that something.
//!
//! The shape is a **pump-side compare**, ruled 2026-07-24 over the
//! `map_action` alternative so the codebase keeps one convention: a control's
//! state is a request the host reconciles each dispatch, exactly as
//! `pump_overmap` and the storylet gate already work.
//!
//! Soundness rests on the sync being one-way. `UiState::sync_selection_rows`
//! pushes truth (the mode, the world) *into* the rows wherever truth can
//! change, so by the time this pump runs, any disagreement can only be the user
//! having moved a control. Without that push the two would drift and a compare
//! could not tell which side moved.

use super::*;

impl App {
    /// Commit any selection row the user moved.
    ///
    /// Cheap first, like every pump in this family: the common dispatch moves
    /// nothing, and comparing three small indices must not cost a clone.
    pub(crate) fn pump_selection_rows(&mut self) {
        let Some(runner) = self.runner.as_ref() else {
            return;
        };
        let ui = runner.state();

        let mode_index = ui.mode_selection.selected.first().copied().unwrap_or(0);
        let wanted_mode = EditMode::ALL.get(mode_index).copied();
        let mode_moved = wanted_mode.is_some_and(|m| m != ui.mode);

        let party = ui.viewer.clone().unwrap_or_else(|| "dm".to_owned());
        let pace_index = ui.pace_selection.selected.first().copied().unwrap_or(1);
        let wanted_pace = isometry_views::PACE_PCTS.get(pace_index).copied();
        let pace_moved = wanted_pace.is_some_and(|p| p != ui.world.pace(&party));

        let stance_index = ui.stance_selection.selected.first().copied().unwrap_or(3);
        let wanted_stance = isometry_views::STANCE_KEYS.get(stance_index).copied();
        let lead = ui
            .selected_token
            .or_else(|| ui.map.tokens.first().map(|t| t.id));
        let current_stance = lead
            .and_then(|id| ui.map.stances.get(&id).cloned())
            .unwrap_or_default();
        let stance_moved = wanted_stance.is_some_and(|s| s != current_stance);

        if !mode_moved && !pace_moved && !stance_moved {
            return;
        }

        // Mode is local view state, so it commits here rather than through the
        // authority. Pace and stance are world state: the row only *asks*, and
        // the existing request flags carry it down the adjudicated path.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                if let Some(mode) = wanted_mode.filter(|_| mode_moved) {
                    ui.mode = mode;
                    ui.status = format!("mode: {}", mode.label());
                }
                if let Some(pace) = wanted_pace.filter(|_| pace_moved) {
                    ui.request_pace(pace);
                }
                if let Some(stance) = wanted_stance.filter(|_| stance_moved) {
                    ui.request_stance(stance);
                }
            });
        }
    }
}
