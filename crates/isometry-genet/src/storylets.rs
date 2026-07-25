//! Storylets, faction turns, and the host-event seam.
//!
//! The storylet surface refreshes through a gate: `refresh_storylet_rows`
//! rebuilds only when `(world, secret_ids)` changed, because it runs after
//! every dispatch while the surface is open (see the 2026-07-20 perf plan).
//! `emit_host_events` lives here too, as the one place a burst of events
//! becomes a single authority step.
//!
//! Split out of `main.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl App {
    /// The storylet surface (C6). While the DM has it open, resolve each
    /// campaign storylet against the current world and the host-private secrets
    /// and hand the view rows; when the DM plays one, commit its effects.
    ///
    /// Host-only: matching reads secret facts, and committing is authoring. A
    /// client's `open_storylets`/`play_storylet` are gated on `can_edit_inventory`
    /// upstream, so this never runs for a joined player.
    /// Rebuild the storylet rows, but only when the answer can have changed.
    ///
    /// This runs after every dispatch while the surface is open. It used to
    /// clone the whole `CampaignWorld` and re-resolve every storylet each time,
    /// almost always producing rows identical to the ones already on screen. At
    /// 400 places / 200 storylets the 07-20 plan's scaled receipt puts that at
    /// ~623us of clone plus ~42us of resolve, per click. The clone was the
    /// larger half by an order of magnitude and was not needed at all: the rows
    /// can be built from a borrow.
    ///
    /// The gate compares the inputs the rows were built from rather than a world
    /// revision counter. A counter is a new invariant every mutation site has to
    /// honor, and one missed bump leaves the surface silently stale, which is a
    /// worse failure than the cost it saves. `(world, secret_ids)` is the
    /// complete input set (`resolve_storylet` reads factions, laws, and
    /// characters, all of them world state), so a compare cannot go stale. It is
    /// also the shape the overmap's leaf gate already uses.
    pub(crate) fn refresh_storylet_rows(&mut self) {
        let secret_ids: Vec<String> = self.campaign.secret_ids().map(str::to_owned).collect();
        let Some(runner) = self.runner.as_ref() else {
            return;
        };
        let world = &runner.state().world;
        if self
            .last_storylet_inputs
            .as_ref()
            .is_some_and(|(seen, ids)| seen == world && ids == &secret_ids)
        {
            return;
        }

        let rows = storylet_rows(world, &secret_ids);
        // The one clone, on the path that actually changed something.
        self.last_storylet_inputs = Some((world.clone(), secret_ids));

        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                // Only replace on change, to keep the DM's selection stable.
                if ui.storylets != rows {
                    ui.storylets = rows;
                    if ui.storylet_selected >= ui.storylets.len() {
                        ui.storylet_selected = 0;
                    }
                }
            });
        }
    }

    pub(crate) fn pump_storylets(&mut self) {
        // Cheap flags first: with the surface closed and no request pending
        // (every ordinary dispatch), this returns before touching the world.
        let (open, request, can_edit) = match self.runner.as_ref() {
            Some(r) => {
                let s = r.state();
                (s.storylet_open, s.storylet_request.clone(), s.can_edit_inventory)
            }
            None => return,
        };
        if !can_edit || (!open && request.is_none()) {
            return;
        }

        // Refresh the rows while the surface is open: which storylets are
        // playable now (requirements met, roles cast) and which are still locked.
        if open {
            self.refresh_storylet_rows();
        }

        // Play a storylet: commit its effects. A storylet Item effect wants a
        // recipient, so pass the active token (else the first), like campaign.
        let Some(key) = request else {
            return;
        };
        let (item_owner, snapshot, any_storylets) = match self.runner.as_ref() {
            Some(r) => {
                let s = r.state();
                let owner = s.turns.active().or_else(|| s.map.tokens.first().map(|t| t.id));
                (owner, self.snapshot_of(s), !s.world.storylets.is_empty())
            }
            None => return,
        };
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.storylet_request = None);
        }
        if !any_storylets {
            return;
        }
        let remote = matches!(self.runner.as_ref().map(|r| r.state().net_mode), Some(NetMode::Remote));
        if remote {
            if let Some(net) = self.net.as_mut() {
                let request = net.commit_storylet(key.clone(), item_owner);
                if let Some(runner) = self.runner.as_mut() {
                    runner.update(|ui| {
                        ui.status = match request {
                            Some(request) => format!("playing storylet (request {request})"),
                            None => "storylet authority actor stopped".to_owned(),
                        };
                    });
                }
            }
        } else {
            let mut host = HostSession::with_history(
                snapshot,
                self.campaign.clone(),
                self.history.clone(),
            );
            match host.commit_storylet(&key, item_owner) {
                Ok(_) => {
                    self.campaign = host.campaign().clone();
                    self.history = host.history().clone();
                    self.journal = host.state().journal.clone();
                    let snapshot = host.state().clone();
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| {
                            ui.apply_snapshot(snapshot);
                            ui.status = format!("played storylet: {key}");
                        });
                    }
                }
                Err(error) => {
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| ui.status = format!("storylet failed: {error}"));
                    }
                }
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// The downtime surface (C7). While the DM has it open, roll a faction tick
    /// on request and hand the view its moves; when the DM commits, keep the
    /// un-struck moves and apply them as replicated world events.
    ///
    /// Host-only, like storylets: rolling reads the world and spends host
    /// entropy, gated on `can_edit_inventory` upstream. In a live session the DM
    /// hosts (net_mode Remote), so the commit routes through the bridge; solo it
    /// runs a local HostSession. Either way the moves land as ordinary events.
    pub(crate) fn pump_faction_turn(&mut self) {
        // Cheap flags first: neither a roll nor a commit pending is the ordinary
        // case for every dispatch, and it must not cost a world clone.
        let (roll, commit, tick, can_edit, remote) = match self.runner.as_ref() {
            Some(r) => {
                let s = r.state();
                let tick = s
                    .active_map
                    .as_ref()
                    .and_then(|m| s.clocks.get(m))
                    .copied()
                    .unwrap_or(0) as i64;
                (
                    s.downtime_roll_request,
                    s.downtime_commit_request,
                    tick,
                    s.can_edit_inventory,
                    s.net_mode == NetMode::Remote,
                )
            }
            None => return,
        };
        if !can_edit || (!roll && !commit) {
            return;
        }
        let world = match self.runner.as_ref() {
            Some(r) => r.state().world.clone(),
            None => return,
        };

        // Roll a fresh tick: draw a move per faction (proportional to banked
        // time) and hand the view display rows, keeping the real moves aligned.
        if roll {
            let moves = world.faction_turn(tick, &mut self.generation_tape);
            let rows: Vec<FactionMoveRow> = moves
                .iter()
                .map(|m| FactionMoveRow {
                    faction: world
                        .factions
                        .get(&m.faction)
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| m.faction.clone()),
                    verb: m.verb.label().to_owned(),
                    text: m.history.text.clone(),
                    has_change: m.change.is_some(),
                    struck: false,
                })
                .collect();
            self.faction_turn_batch = moves;
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| {
                    ui.downtime_roll_request = false;
                    ui.downtime_selected = 0;
                    ui.status = if rows.is_empty() {
                        "no factions to move".to_owned()
                    } else {
                        format!("rolled {} downtime move(s)", rows.len())
                    };
                    ui.faction_moves = rows;
                });
            }
        }

        if !commit {
            return;
        }

        // Commit: keep the moves whose row the DM did not strike, drop the rest.
        // The rows are index-aligned with the batch, so a struck row drops its
        // move. Extra batch entries (should not happen) default to kept.
        let struck: Vec<bool> = self
            .runner
            .as_ref()
            .map(|r| r.state().faction_moves.iter().map(|m| m.struck).collect())
            .unwrap_or_default();
        let kept: Vec<FactionMove> = std::mem::take(&mut self.faction_turn_batch)
            .into_iter()
            .enumerate()
            .filter(|(index, _)| !struck.get(*index).copied().unwrap_or(false))
            .map(|(_, m)| m)
            .collect();
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                ui.downtime_commit_request = false;
                ui.faction_moves.clear();
                ui.downtime_open = false;
            });
        }
        if kept.is_empty() {
            return;
        }

        if remote {
            if let Some(net) = self.net.as_mut() {
                let request = net.commit_faction_turn(kept);
                if let Some(runner) = self.runner.as_mut() {
                    runner.update(|ui| {
                        ui.status = match request {
                            Some(request) => format!("downtime committed (request {request})"),
                            None => "downtime authority actor stopped".to_owned(),
                        };
                    });
                }
            }
        } else {
            let snapshot = match self.runner.as_ref() {
                Some(r) => self.snapshot_of(r.state()),
                None => return,
            };
            let mut host =
                HostSession::with_history(snapshot, self.campaign.clone(), self.history.clone());
            match host.commit_faction_turn(kept) {
                Ok(_) => {
                    self.campaign = host.campaign().clone();
                    self.history = host.history().clone();
                    self.journal = host.state().journal.clone();
                    let snapshot = host.state().clone();
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| {
                            ui.apply_snapshot(snapshot);
                            ui.status = "downtime committed".to_owned();
                        });
                    }
                }
                Err(error) => {
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| ui.status = format!("downtime failed: {error}"));
                    }
                }
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// Commit one host-authored game event: solo through a local `HostSession`
    /// (copying its campaign/history/journal back and mirroring the snapshot),
    /// in a live session through the host bridge. Leaves `status` alone, which
    /// `apply_snapshot` preserves, so a caller may set it before or after.
    pub(crate) fn emit_host_event(&mut self, event: GameEvent) {
        self.emit_host_events(vec![event]);
    }

    /// Emit several events as one authority step.
    ///
    /// The per-event path pays a snapshot clone, a `HostSession` build, and an
    /// `apply_snapshot` (which recomputes fog and reach) for *each* event, so a
    /// burst of N costs N full round-trips. A map read revealing a dozen places
    /// is exactly that burst. Batching applies them to one session and lands one
    /// snapshot, so the fog and reach recompute happens once.
    ///
    /// Order is preserved and each event still goes through `local_event`, so
    /// this is the same adjudication the single-event path performs, not a
    /// shortcut around it.
    pub(crate) fn emit_host_events(&mut self, events: Vec<GameEvent>) {
        if events.is_empty() {
            return;
        }
        let remote = self
            .runner
            .as_ref()
            .map(|r| r.state().net_mode == NetMode::Remote)
            .unwrap_or(false);
        if remote {
            if let Some(net) = self.net.as_mut() {
                // The bridge orders submissions; batching is a host-side saving
                // only, and the wire sees the same sequence either way.
                for event in events {
                    net.submit(event);
                }
            }
        } else {
            let snapshot = match self.runner.as_ref() {
                Some(r) => self.snapshot_of(r.state()),
                None => return,
            };
            let mut host =
                HostSession::with_history(snapshot, self.campaign.clone(), self.history.clone());
            for event in events {
                let _ = host.local_event(event);
            }
            self.campaign = host.campaign().clone();
            self.history = host.history().clone();
            self.journal = host.state().journal.clone();
            let snapshot = host.state().clone();
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| ui.apply_snapshot(snapshot));
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

}

/// A DM-facing reason a storylet is not yet playable.
fn describe_storylet_error(error: &isometry_campaign::StoryletError) -> String {
    use isometry_campaign::StoryletError::*;
    match error {
        MissingFactionTag(tag) => format!("needs a faction tagged '{tag}'"),
        MissingHiddenFact(id) => format!("needs the secret '{id}' to be true"),
        MissingWorldLaw(id) => format!("needs the law '{id}'"),
        UncastRole(role) => format!("no character fits the role '{role}'"),
    }
}

/// Which storylets are playable now and which are still locked, given the world
/// and the host's secret-fact ids.
///
/// Free and pure on purpose. `refresh_storylet_rows` caches exactly these two
/// arguments and skips the call when neither changed, and that gate is sound
/// only if the rows depend on nothing else. Keeping the builder a function of
/// its inputs makes that structural rather than a promise in a comment: adding
/// a third input here without adding it to the cache will not compile past the
/// call site.
fn storylet_rows(world: &CampaignWorld, secret_ids: &[String]) -> Vec<StoryletRow> {
    world
        .storylets
        .iter()
        .map(|(key, storylet)| {
            match world.resolve_storylet(storylet, secret_ids.iter().map(String::as_str)) {
                Ok(resolved) => StoryletRow {
                    key: key.clone(),
                    entry: storylet.entry.clone(),
                    available: true,
                    status: "ready".to_owned(),
                    cast: resolved
                        .cast
                        .into_iter()
                        .map(|(role, character_id)| {
                            let name = world
                                .characters
                                .get(&character_id)
                                .map(|c| c.name.clone())
                                .unwrap_or(character_id);
                            (role, name)
                        })
                        .collect(),
                },
                Err(error) => StoryletRow {
                    key: key.clone(),
                    entry: storylet.entry.clone(),
                    available: false,
                    status: describe_storylet_error(&error),
                    cast: Vec::new(),
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use isometry_views::synth_world;

    /// The storylet refresh gate skips the rebuild when `(world, secret_ids)` is
    /// unchanged. That is only safe while the rows are a pure function of those
    /// two, so pin it: identical inputs must give identical rows, or the gate
    /// starts serving a stale surface and nothing reports it.
    #[test]
    fn storylet_rows_are_a_function_of_world_and_secrets() {
        let world = synth_world(24, 12);
        let secrets: Vec<String> = vec!["secret4".to_owned()];
        assert_eq!(
            storylet_rows(&world, &secrets),
            storylet_rows(&world, &secrets),
            "unchanged inputs must give unchanged rows, or the refresh gate is unsound"
        );
    }

    /// The mirror: each cached input has to actually move the rows, or it is
    /// dead weight in the compare and the real input is missing from it.
    #[test]
    fn storylet_rows_track_both_cached_inputs() {
        let world = synth_world(24, 12);
        let none: Vec<String> = Vec::new();
        let base = storylet_rows(&world, &none);

        // A secret the world does not hold unlocks a hidden-fact storylet. Index
        // 2 is the hidden-fact shape in synth_world's rotation.
        let with_secret = storylet_rows(&world, &["secret2".to_owned()]);
        assert_ne!(
            base, with_secret,
            "a revealed secret must change the rows, or secret_ids need not be cached"
        );

        // Removing the cast breaks every role-bearing storylet.
        let mut thinner = world.clone();
        thinner.characters.clear();
        assert_ne!(
            base,
            storylet_rows(&thinner, &none),
            "a changed world must change the rows"
        );
    }
}
