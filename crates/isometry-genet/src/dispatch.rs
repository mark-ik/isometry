//! The post-dispatch tail and the session bridge.
//!
//! `after_dispatch` runs after every click, key, and drag-step, which is why
//! each pump it calls checks its cheap request flags before cloning anything.
//! `pump_net` moves events between the local state and the authority.
//!
//! Split out of `main.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl App {
    /// Consume one-shot state requests (save/load) and repaint: the
    /// tail of every dispatch.
    pub(crate) fn after_dispatch(&mut self) {
        if let Some(window) = self.window.as_ref() {
            window.set_ime_allowed(
                self.runner
                    .as_ref()
                    .is_some_and(|runner| command_field_node(runner).is_some()),
            );
        }
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.sync_overmap_source_time());
        }
        // Cheap flags first: this tail runs after every dispatch, and the save
        // path below starts by cloning the journal. An ordinary click asks for
        // neither and must not pay for either.
        let wants_save_or_load = self
            .runner
            .as_ref()
            .is_some_and(|r| r.state().save_requested || r.state().load_requested);
        if !wants_save_or_load {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
            self.pump_selection_rows();
            self.pump_sheets();
            self.pump_generators();
            self.pump_storylets();
            self.pump_faction_turn();
            self.pump_overmap();
            self.pump_overmap_orders();
            self.pump_overmap_read();
            self.pump_net();
            self.refresh_source_history();
            return;
        }
        let mut save: Option<(std::path::PathBuf, String, String, GameSnapshot)> = None;
        let mut load: Option<(std::path::PathBuf, String)> = None;
        let journal = self.journal.clone();
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                if std::mem::take(&mut ui.save_requested) {
                    match serde_json::to_string_pretty(&ui.map) {
                        Ok(json) => {
                            let name = ui.map.name.clone();
                            save = Some((
                                map_path(&name),
                                json,
                                name,
                                GameSnapshot {
                                    map: ui.map.clone(),
                                    turns: ui.turns.clone(),
                                    roll_log: ui.roll_log.clone(),
                                    journal: journal.clone(),
                                    inventories: ui.inventories.clone(),
                                    generations: ui.generations.clone(),
                                    maps: ui.campaign_maps.clone(),
                                    active_map: ui.active_map.clone(),
                                    world: ui.world.clone(),
                                    clocks: ui.clocks.clone(),

                                    party_cap: ui.party_cap,
                                    last_beats: Vec::new(),
                                    beat_seq: 0,
                                    applied_actions: Default::default(),
                                },
                            ));
                        }
                        Err(e) => ui.status = format!("save failed: {e}"),
                    }
                }
                if std::mem::take(&mut ui.load_requested) {
                    load = Some((map_path(&ui.map.name), ui.map.name.clone()));
                }
            });
        }
        if let Some((path, json, name, local_public)) = save {
            let map_result =
                std::fs::create_dir_all("maps").and_then(|_| std::fs::write(&path, json));
            let campaign = self
                .net
                .as_ref()
                .and_then(NetBridge::campaign)
                .unwrap_or_else(|| self.campaign.clone());
            let public = self
                .net
                .as_ref()
                .and_then(NetBridge::latest)
                .unwrap_or(local_public);
            let history = self
                .net
                .as_ref()
                .and_then(NetBridge::history)
                .unwrap_or_else(|| self.history.clone());
            let checkpoint =
                CampaignCheckpoint::new(public, campaign, history, self.history_origin.clone());
            let campaign_result = CampaignRepository::open(campaign_path(&name))
                .and_then(|repository| repository.save_checkpoint(&checkpoint));
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| {
                    ui.status = match (map_result.as_ref(), campaign_result.as_ref()) {
                        (Ok(()), Ok(())) => format!("saved {}", path.display()),
                        (Err(error), Ok(())) => {
                            format!("checkpoint saved, map export failed: {error}")
                        }
                        (Err(error), Err(_)) => format!("map save failed: {error}"),
                        (Ok(()), Err(error)) => {
                            format!("map saved, private campaign save failed: {error}")
                        }
                    };
                });
            }
        }
        if let Some((path, name)) = load {
            let checkpoint = CampaignRepository::open(campaign_path(&name))
                .and_then(|repository| repository.load_checkpoint());
            if let Ok(Some(checkpoint)) = checkpoint {
                self.campaign = checkpoint.private;
                self.journal = checkpoint.public.journal.clone();
                self.history = checkpoint.history;
                self.history_origin = checkpoint.history_origin;
                self.source_history_len = None;
                self.source_history_attached = false;
                if let Some(runner) = self.runner.as_mut() {
                    runner.update(|ui| {
                        ui.apply_snapshot(checkpoint.public);
                        ui.status = format!("loaded checkpoint {}", path.display());
                    });
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
                self.refresh_source_history();
                return;
            }
            let checkpoint_error = checkpoint.err();
            let loaded = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|json| {
                    serde_json::from_str::<isometry_core::MapDocument>(&json)
                        .map_err(|e| e.to_string())
                });
            match loaded {
                Ok(map) => {
                    let name = map.name.clone();
                    let campaign = CampaignRepository::open(campaign_path(&name))
                        .and_then(|repository| repository.load_private());
                    if let Ok(campaign) = campaign.as_ref() {
                        self.campaign = campaign.clone();
                    }
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| {
                            ui.replace_map(map);
                            ui.status = match (campaign, checkpoint_error) {
                                (Ok(_), None) => format!("loaded {}", path.display()),
                                (Err(error), _) => {
                                    format!("map loaded, private campaign state failed: {error}")
                                }
                                (_, Some(error)) => format!(
                                    "loaded legacy map after checkpoint read failed: {error}"
                                ),
                            };
                        });
                    }
                }
                Err(error) => {
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| ui.status = format!("load failed: {error}"));
                    }
                }
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        self.pump_selection_rows();
        self.pump_sheets();
        self.pump_generators();
        self.pump_storylets();
        self.pump_faction_turn();
        self.pump_overmap();
        self.pump_overmap_orders();
        self.pump_overmap_read();
        self.pump_net();
        self.refresh_source_history();
    }

    /// In networked mode: ship the UI's queued game events to the
    /// session, and pull the latest replicated snapshot into the view
    /// when the session advanced. No-op when solo.
    pub(crate) fn pump_net(&mut self) {
        if self.net.is_none() {
            return;
        }
        // Drain the outbox and submit each event.
        let mut events = Vec::new();
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| events = std::mem::take(&mut ui.net_outbox));
        }
        // Drain queued whispers (host-side) too.
        let mut whispers = Vec::new();
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| whispers = std::mem::take(&mut ui.whisper_outbox));
        }
        let mut received = Vec::new();
        let mut players = Vec::new();
        let mut campaign_outcomes = Vec::new();
        let mut failure = None;
        if let Some(net) = self.net.as_mut() {
            // Armillary keeps the network runtime off the winit kernel. Drain
            // its typed updates before reading any mirror state.
            net.poll();
            if !events.is_empty() && self.profile {
                eprintln!("[isometry] pump: submitting {} event(s)", events.len());
            }
            for event in events {
                net.submit(event);
            }
            for (to, text) in whispers {
                net.whisper(to, text);
            }
            // Deliver received whispers into the message log, and refresh
            // the whisper-target list from connected players.
            received = net.take_whispers();
            players = net.players();
            campaign_outcomes = net.take_campaign_outcomes();
            failure = net.take_failure();
        }
        if !received.is_empty()
            || !players.is_empty()
            || !campaign_outcomes.is_empty()
            || failure.is_some()
        {
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| {
                    for (from, text) in &received {
                        ui.receive_whisper(from, text);
                    }
                    ui.connected_players = players;
                    if let Some(outcome) = campaign_outcomes.last() {
                        // Campaign drafts and storylets share this one-shot
                        // outcome channel, so the text stays neutral to fit both.
                        ui.status = match &outcome.value {
                            Ok(()) => format!("committed (request {})", outcome.request),
                            Err(error) => {
                                format!("commit failed (request {}): {error}", outcome.request)
                            }
                        };
                    }
                    if let Some(error) = &failure {
                        ui.status = error.clone();
                    }
                });
                if !received.is_empty() || !campaign_outcomes.is_empty() || failure.is_some() {
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
        }
        // Mirror in a new snapshot when the session version bumped.
        let version = self.net.as_ref().map(|n| n.version()).unwrap_or(0);
        if version != self.last_net_version {
            self.last_net_version = version;
            let snap = self.net.as_ref().and_then(|n| n.latest());
            if let (Some(snap), Some(runner)) = (snap, self.runner.as_mut()) {
                self.journal = snap.journal.clone();
                if let Some(history) = self.net.as_ref().and_then(NetBridge::history) {
                    self.history = history;
                }
                // Pull the authoritative host-private campaign too, or storylet
                // availability (which reads secret_ids) resolves against a stale
                // copy: a mid-session reveal would leave a secret-gated storylet
                // wrongly locked, or a removed secret wrongly playable.
                if let Some(campaign) = self.net.as_ref().and_then(NetBridge::campaign) {
                    self.campaign = campaign;
                }
                runner.update(|ui| ui.apply_snapshot(snap));
                // The host's door sweep: any token now standing on a transition
                // point of the active map walks through it. Clients never ask in
                // words; they walk, the move replicates, and this notices. The
                // emitted list keeps one crossing from being ruled twice while
                // its echo is still in flight.
                //
                // The authority *rules* each crossing here and broadcasts the
                // verdict. Peers used to be handed the traveler's id and left to
                // work out the rest; now the payload names it.
                if self.net_is_host {
                    let on_doors: Vec<TokenId> = {
                        let ui = runner.state();
                        ui.map
                            .tokens
                            .iter()
                            .filter(|t| ui.transition_at(t.at))
                            .map(|t| t.id)
                            .collect()
                    };
                    let mut crossings = Vec::new();
                    let mut refusals = Vec::new();
                    {
                        // Not `self.snapshot_of`: `runner` already holds a
                        // mutable borrow of one field of `self`, so the whole-
                        // `self` method is out of reach here.
                        let state = snapshot_with_journal(self.journal.clone(), runner.state());
                        for token in &on_doors {
                            if self.travel_emitted.contains(token) {
                                continue;
                            }
                            self.own_requests += 1;
                            match isometry_net::resolve_transition(
                                &state,
                                *token,
                                RequestId::host(self.own_requests),
                            ) {
                                Ok(res) => crossings.push(GameEvent::TransitionResolved(res)),
                                Err(error) => refusals.push(format!("cannot travel: {error:?}")),
                            }
                        }
                    }
                    if !crossings.is_empty() || !refusals.is_empty() {
                        runner.update(|ui| {
                            ui.net_outbox.extend(crossings);
                            if let Some(last) = refusals.last() {
                                ui.status = last.clone();
                            }
                        });
                    }
                    self.travel_emitted = on_doors;
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
        }
    }

    /// Evaluate or commit a host-owned generator preview. The view asks for a
    /// one-shot action; this desktop layer loads the declared pack and owns the
    /// entropy tape, while `isometry-net` remains scripting-agnostic.
    /// Build a replicated snapshot from the view's current state, for a host
    /// operation that needs to prevalidate against a clone (storylet commit).
    pub(crate) fn snapshot_of(&self, ui: &UiState) -> GameSnapshot {
        snapshot_with_journal(self.journal.clone(), ui)
    }
}

/// The same build, with the journal passed in rather than read off the app.
/// The door sweep needs it while a `&mut` borrow of the runner is live, which
/// puts the whole-`self` method out of reach.
pub(crate) fn snapshot_with_journal(
    journal: Vec<isometry_campaign::WorldFact>,
    ui: &UiState,
) -> GameSnapshot {
    GameSnapshot {
        map: ui.map.clone(),
        turns: ui.turns.clone(),
        roll_log: ui.roll_log.clone(),
        journal,
        inventories: ui.inventories.clone(),
        generations: ui.generations.clone(),
        maps: ui.campaign_maps.clone(),
        active_map: ui.active_map.clone(),
        world: ui.world.clone(),
        clocks: ui.clocks.clone(),
        party_cap: ui.party_cap,
        last_beats: Vec::new(),
        beat_seq: 0,
        applied_actions: Default::default(),
    }
}
