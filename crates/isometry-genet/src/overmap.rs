//! The overmap surface (C8): travel, marching orders, and map reads.
//!
//! Clicking a place only *asks*; the host rolls the navigation, spends the
//! time, and moves the party, so the view never decides a trip's outcome. Each
//! pump reads its cheap request flags before touching the world.
//!
//! Split out of `main.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl App {
    /// The overmap surface (C8). When the table clicks a place to travel to,
    /// resolve the trip and move the party. Host-adjudicated, like an action:
    /// the authority rolls the navigation (`resolve_travel`), spends the time,
    /// and emits a `TravelResolved` verdict every peer applies. If the party is
    /// not on the overmap yet, the first click simply places it there.
    pub(crate) fn pump_overmap(&mut self) {
        // Cheap flag first: this pump runs after every dispatch, and cloning the
        // world to then discover there is no request would tax every click.
        let (request, remote) = match self.runner.as_ref() {
            Some(r) => {
                let s = r.state();
                (
                    s.overmap_travel_request.clone(),
                    s.net_mode == NetMode::Remote,
                )
            }
            None => return,
        };
        let Some(to) = request else {
            return;
        };
        let world = match self.runner.as_ref() {
            Some(r) => r.state().world.clone(),
            None => return,
        };
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.overmap_travel_request = None);
        }
        // Only the authority resolves travel. A joined client's click is a look,
        // not a verdict; routing its request as an ask is a later refinement.
        if remote && !self.net_is_host {
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| ui.status = "the DM guides the party".to_owned());
            }
            return;
        }
        let party = self
            .runner
            .as_ref()
            .and_then(|r| r.state().viewer.clone())
            .unwrap_or_else(|| "dm".to_owned());
        let from = world.party_at(&party).map(str::to_owned);

        let event = match from {
            // Not on the overmap yet: the first click places the party.
            None => GameEvent::World(WorldEvent::PartyMoved {
                party: party.clone(),
                node: to.clone(),
            }),
            Some(here) if here == to => return,
            Some(here) => {
                let Some((_, weight)) = world.overmap().route(&here, &to) else {
                    if let Some(runner) = self.runner.as_mut() {
                        runner.update(|ui| ui.status = format!("no route to {to}"));
                    }
                    return;
                };
                let pace = world.pace(&party);
                // The navigator: a token of the party leads the way, else a bare
                // sheet (a party with no statted member just travels at base).
                // The party's navigator (its first token) leads the way, carrying
                // its sheet and its exploration stance (E3), so Scouting or
                // Searching colours the trip. A partyless-of-tokens party travels
                // on a bare sheet at base.
                let (nav_sheet, stance) = match self.runner.as_ref() {
                    Some(r) => {
                        let s = r.state();
                        let tok = s
                            .map
                            .tokens
                            .iter()
                            .find(|t| t.owner.as_deref() == Some(party.as_str()));
                        (
                            tok.and_then(|t| s.map.sheet(t.id).cloned()),
                            tok.and_then(|t| s.map.stance(t.id).map(str::to_owned)),
                        )
                    }
                    None => (None, None),
                };
                let Some(system) = self.system.as_mut() else {
                    return;
                };
                let mut nav = nav_sheet.unwrap_or_else(|| system.default_sheet());
                if let Some(stance) = stance {
                    nav.set_text("stance", stance);
                }
                let res = system.resolve_travel(&nav, weight as u32, pace, &mut self.action_rng);
                let (ticks, lost, exhaustion, encounter, forage) = (
                    res.ticks,
                    res.lost,
                    res.exhaustion,
                    res.encounter,
                    res.forage,
                );
                if let Some(runner) = self.runner.as_mut() {
                    runner.update(|ui| {
                        ui.status = if lost {
                            format!("lost the way to {to} ({ticks})")
                        } else {
                            format!("travelled to {to} ({ticks})")
                        };
                    });
                }
                GameEvent::TravelResolved {
                    party: party.clone(),
                    to: to.clone(),
                    ticks,
                    roll: res.roll,
                    lost,
                    exhaustion,
                    encounter,
                    forage,
                }
            }
        };

        self.emit_host_event(event);
    }

    /// Drain the overmap's pace and stance choices (E1/E3): host-recorded state,
    /// not a travel resolution. A pace sets the party's marching speed; a stance
    /// is set on the party's lead token, which the travel rule then reads.
    pub(crate) fn pump_overmap_orders(&mut self) {
        let (pace_req, stance_req, remote) = match self.runner.as_ref() {
            Some(r) => {
                let s = r.state();
                (
                    s.overmap_pace_request,
                    s.overmap_stance_request.clone(),
                    s.net_mode == NetMode::Remote,
                )
            }
            None => return,
        };
        if pace_req.is_none() && stance_req.is_none() {
            return;
        }
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                ui.overmap_pace_request = None;
                ui.overmap_stance_request = None;
            });
        }
        if remote && !self.net_is_host {
            return;
        }
        let party = self
            .runner
            .as_ref()
            .and_then(|r| r.state().viewer.clone())
            .unwrap_or_else(|| "dm".to_owned());
        if let Some(pace) = pace_req {
            self.emit_host_event(GameEvent::World(WorldEvent::PartyPaceSet {
                party: party.clone(),
                pace,
            }));
        }
        if let Some(stance) = stance_req {
            let nav = self.runner.as_ref().and_then(|r| {
                r.state()
                    .map
                    .tokens
                    .iter()
                    .find(|t| t.owner.as_deref() == Some(party.as_str()))
                    .map(|t| t.id)
            });
            if let Some(token) = nav {
                self.emit_host_event(GameEvent::StanceSet { token, stance });
            }
        }
    }

    /// Study the party's maps (E6). Roll the lead token's literacy; on a pass,
    /// reveal the places just beyond the known frontier, so a lettered party sees
    /// further than it has walked and a dull-witted one learns nothing.
    pub(crate) fn pump_overmap_read(&mut self) {
        // Cheap flag first; the world clone below is only for a live request.
        let (read_req, remote) = match self.runner.as_ref() {
            Some(r) => {
                let s = r.state();
                (s.overmap_read_request, s.net_mode == NetMode::Remote)
            }
            None => return,
        };
        if !read_req {
            return;
        }
        let world = match self.runner.as_ref() {
            Some(r) => r.state().world.clone(),
            None => return,
        };
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.overmap_read_request = false);
        }
        if remote && !self.net_is_host {
            return;
        }
        let party = self
            .runner
            .as_ref()
            .and_then(|r| r.state().viewer.clone())
            .unwrap_or_else(|| "dm".to_owned());
        let reader = self.runner.as_ref().and_then(|r| {
            let s = r.state();
            s.map
                .tokens
                .iter()
                .find(|t| t.owner.as_deref() == Some(party.as_str()))
                .and_then(|t| s.map.sheet(t.id).cloned())
        });
        // Roll the read in a scope so the system borrow ends before emitting.
        let can_read = {
            let Some(system) = self.system.as_mut() else {
                return;
            };
            let reader = reader.unwrap_or_else(|| system.default_sheet());
            system.read_map(&reader, &mut self.action_rng)
        };
        if !can_read {
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| ui.status = "you cannot make sense of the map".to_owned());
            }
            return;
        }
        let known: std::collections::BTreeSet<String> =
            world.party_known.get(&party).cloned().unwrap_or_default();
        let full = world.overmap();
        let mut reveal: Vec<String> = Vec::new();
        let mut push = |node: &str, reveal: &mut Vec<String>| {
            if !known.contains(node) && !reveal.iter().any(|r| r == node) {
                reveal.push(node.to_owned());
            }
        };
        // The frontier: places one route beyond the ones the party knows.
        for node in &known {
            for (neighbour, _) in full.neighbours(node) {
                push(neighbour, &mut reveal);
            }
        }
        // Carried maps: a `map` item in the party's packs shows named places
        // (`ItemInstance::revealed_places`). Reading one is how a bought or looted
        // chart hands over somewhere far off the party has never been near.
        if let Some(runner) = self.runner.as_ref() {
            let s = runner.state();
            for token in s
                .map
                .tokens
                .iter()
                .filter(|t| t.owner.as_deref() == Some(party.as_str()))
            {
                if let Some(inventory) = s.inventories.get(&token.id) {
                    for node in inventory.revealed_places() {
                        push(node, &mut reveal);
                    }
                }
            }
        }
        if reveal.is_empty() {
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| ui.status = "the map shows nothing you do not know".to_owned());
            }
            return;
        }
        // One authority step for the whole read, not one per place: a chart that
        // discloses a dozen sites should cost one fog recompute, not twelve.
        let count = reveal.len();
        self.emit_host_events(
            reveal
                .into_iter()
                .map(|node| {
                    GameEvent::World(WorldEvent::NodeRevealed {
                        party: party.clone(),
                        node,
                    })
                })
                .collect(),
        );
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.status = format!("read the map: {count} place(s) revealed"));
        }
    }
}
