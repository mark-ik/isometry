//! Initiative, dice, fog, and the authority snapshot.
//!
//! The viewer's eyes and the shared log. `apply_snapshot` is the seam where the
//! authority's state replaces the local view, and it recomputes fog and reach,
//! which is why emitting a burst of events through one snapshot matters.
//!
//! Split out of `state.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl UiState {
    /// The tiles the current area template covers, aimed at the hovered
    /// tile from the anchor. Empty unless in Measure mode with an anchor.
    pub fn template_preview(&self) -> std::collections::HashSet<TileCoord> {
        if self.mode != EditMode::Measure {
            return std::collections::HashSet::new();
        }
        let Some(anchor) = self.measure_anchor else {
            return std::collections::HashSet::new();
        };
        let toward = self.hover_tile.unwrap_or(anchor);
        template_tiles(
            &self.map,
            anchor,
            self.template_kind,
            self.template_size,
            toward,
        )
    }

    /// The measured distance from the anchor to the hovered tile, if both
    /// are set (Measure mode).
    pub fn measured_distance(&self) -> Option<u32> {
        match (self.measure_anchor, self.hover_tile) {
            (Some(a), Some(h)) => Some(distance(a, h)),
            _ => None,
        }
    }

    /// A short display label for a token, e.g. "knight 1".
    pub(crate) fn token_label(&self, id: TokenId) -> String {
        match self.map.token(id) {
            Some(t) => format!("{} {}", t.sprite, id.0),
            None => format!("token {}", id.0),
        }
    }

    /// Roll initiative and reorder the turn list. Individual mode rolls a
    /// d20 per token and sorts high-to-low; side mode rolls a d20 per side
    /// and groups them. The rolls go to the shared roll log; the new order
    /// replicates (Remote) or applies locally.
    pub fn roll_initiative(&mut self) {
        let ids: Vec<TokenId> = if self.turns.is_empty() {
            self.map.tokens.iter().map(|t| t.id).collect()
        } else {
            self.turns.entries().to_vec()
        };
        if ids.is_empty() {
            self.status = "no tokens to order".to_owned();
            return;
        }
        let mut records: Vec<RollRecord> = Vec::new();
        let order: Vec<TokenId> = match self.initiative_mode {
            InitiativeMode::Individual => {
                let mut rolled: Vec<(i32, usize, TokenId)> = ids
                    .iter()
                    .enumerate()
                    .map(|(i, &id)| {
                        let (total, dice) = roll("1d20", &mut self.rng).unwrap();
                        records.push(RollRecord {
                            by: self.token_label(id),
                            expr: "init".to_owned(),
                            dice,
                            total,
                        });
                        (total, i, id)
                    })
                    .collect();
                // High roll first; ties keep input order.
                rolled.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                rolled.into_iter().map(|(_, _, id)| id).collect()
            }
            InitiativeMode::SideBased => {
                // Group tokens by owner, preserving order within a side.
                let mut sides: Vec<(String, Vec<TokenId>)> = Vec::new();
                for &id in &ids {
                    let owner = self
                        .map
                        .token(id)
                        .and_then(|t| t.owner.clone())
                        .unwrap_or_else(|| "dm".to_owned());
                    match sides.iter_mut().find(|(o, _)| *o == owner) {
                        Some(s) => s.1.push(id),
                        None => sides.push((owner, vec![id])),
                    }
                }
                let mut rolled: Vec<(i32, usize, Vec<TokenId>)> = sides
                    .into_iter()
                    .enumerate()
                    .map(|(i, (owner, toks))| {
                        let (total, dice) = roll("1d20", &mut self.rng).unwrap();
                        records.push(RollRecord {
                            by: format!("side {owner}"),
                            expr: "init".to_owned(),
                            dice,
                            total,
                        });
                        (total, i, toks)
                    })
                    .collect();
                rolled.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                rolled.into_iter().flat_map(|(_, _, toks)| toks).collect()
            }
        };
        self.status = format!("rolled initiative ({})", self.initiative_mode.label());
        if self.net_mode == NetMode::Remote {
            for r in records {
                self.net_outbox.push(GameEvent::Rolled(r));
            }
            self.net_outbox.push(GameEvent::TurnSetOrder(order));
        } else {
            self.roll_log.extend(records);
            let overflow = self.roll_log.len().saturating_sub(ROLL_LOG_CAP);
            if overflow > 0 {
                self.roll_log.drain(0..overflow);
            }
            self.turns.set_order(order);
            self.recompute_reach();
        }
    }

    /// Reseed the dice generator (the host does this with real entropy so
    /// rolls differ per launch).
    pub fn reseed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    /// The name shown as the roller: the viewer's player name in a
    /// session, else "dm".
    pub(crate) fn roller_name(&self) -> String {
        self.viewer.clone().unwrap_or_else(|| "dm".to_owned())
    }

    /// Roll a dice expression (e.g. "1d20+5"). The result is shared: in a
    /// session it goes to the host as a `Rolled` event and returns via the
    /// snapshot; solo it appends to the local log. Bad expressions set a
    /// status and do nothing.
    pub fn roll_dice(&mut self, expr: &str) {
        let by = self.roller_name();
        self.roll_labeled(&by, expr, expr);
    }

    /// Whether fog of war is being applied (a viewer is set).
    pub fn fog_active(&self) -> bool {
        self.viewer.is_some()
    }

    /// The fog presentation of tile `at` for the current viewer.
    pub fn fog_level(&self, at: TileCoord) -> FogLevel {
        if !self.fog_active() {
            FogLevel::Clear
        } else if self.visible.contains(&at) {
            FogLevel::Clear
        } else if self.explored.contains(&at) {
            FogLevel::Dim
        } else {
            FogLevel::Hidden
        }
    }

    /// Whether a token should be drawn: always when omniscient, always if
    /// it is the viewer's own, otherwise only while in current sight (you
    /// see foes only when they are lit).
    /// Whether the local viewer commands a token with this `owner`: it is
    /// theirs, or it belongs to a faction whose channel they have been granted.
    /// Additive over direct ownership, so a viewer with no faction grant behaves
    /// exactly as before. The DM (no viewer) is omniscient by other paths, so
    /// this stays false for them.
    pub fn commands(&self, owner: Option<&str>) -> bool {
        let Some(viewer) = self.viewer.as_deref() else {
            return false;
        };
        let Some(owner) = owner else {
            return false;
        };
        owner == viewer || self.world.faction_controller(owner) == Some(viewer)
    }

    pub fn token_visible(&self, token: &Token) -> bool {
        if !self.fog_active() {
            return true;
        }
        self.commands(token.owner.as_deref()) || self.visible.contains(&token.at)
    }

    /// Recompute the visible set from the viewer's tokens and fold it into
    /// explored memory. No-op (and clears) when omniscient.
    pub fn recompute_fog(&mut self) {
        if self.viewer.is_none() {
            self.visible.clear();
            self.explored.clear();
            return;
        }
        // Each token sees with its *own* effective sight (system-driven via
        // conditions), so a blinded scout goes dark without dimming its allies.
        let origins: Vec<(TileCoord, u32)> = self
            .map
            .tokens
            .iter()
            .filter(|t| self.commands(t.owner.as_deref()))
            .map(|t| {
                let (_, sight) = self
                    .map
                    .effective_mobility(t.id, (MOVE_BUDGET, self.sight_radius));
                (t.at, sight)
            })
            .collect();
        let mut visible = HashSet::new();
        for (at, radius) in origins {
            let rules = SightRules {
                radius,
                opaque: &|kind| kind == "tree" || kind == "wall",
            };
            visible.extend(visible_from(&self.map, at, &rules));
        }
        self.visible = visible;
        self.explored.extend(self.visible.iter().copied());
    }

    /// Cycle the viewer for previewing fog: omniscient, then each token
    /// owner in turn, then back. Explored memory resets per viewer.
    pub fn cycle_viewer(&mut self) {
        let mut owners: Vec<String> = Vec::new();
        for t in &self.map.tokens {
            if let Some(o) = &t.owner {
                if !owners.contains(o) {
                    owners.push(o.clone());
                }
            }
        }
        let next = match &self.viewer {
            None => owners.first().cloned(),
            Some(cur) => {
                let idx = owners.iter().position(|o| o == cur);
                match idx {
                    Some(i) => owners.get(i + 1).cloned(),
                    None => None,
                }
            }
        };
        self.viewer = next;
        self.explored.clear();
        self.recompute_fog();
        self.status = match &self.viewer {
            Some(v) => format!("view: {v} (fog)"),
            None => "view: all".to_owned(),
        };
    }

    /// Mirror a replicated snapshot into the view (Remote mode): the map
    /// and turn order become the host's authoritative copy, then reach
    /// recomputes for whatever token is selected. Selection and camera
    /// are local and survive.
    pub fn apply_snapshot(&mut self, snap: GameSnapshot) {
        // Beats first: a client renders from the snapshot, so this is the only
        // place it learns a flourish happened and can play it. Keyed on
        // `beat_seq`, so mirroring the same snapshot twice does not re-strike.
        if !snap.last_beats.is_empty() {
            self.stage_beats(snap.beat_seq, &snap.last_beats);
        }
        self.map = snap.map;
        self.turns = snap.turns;
        self.roll_log = snap.roll_log;
        self.inventories = snap.inventories;
        self.generations = snap.generations;
        self.campaign_maps = snap.maps;
        self.active_map = snap.active_map;
        self.world = snap.world;
        // A joined client mirrors these too, or its panel shows the wrong split-
        // party time (a C3 omission) and its cap disagrees with the host.
        self.clocks = snap.clocks;
        self.party_cap = snap.party_cap;
        self.sheet_effective = None;
        if let Some(id) = self.selected_token {
            if self.map.token(id).is_none() {
                self.selected_token = None;
            }
        }
        if self.status == "connecting..." {
            self.status = "in session".to_owned();
        }
        self.recompute_fog();
        self.recompute_reach();
        // The authority may have moved pace, stance, or the lead token. Push
        // that into the selection rows so the host's compare stays meaningful:
        // after this, any disagreement is the user having moved a control.
        self.sync_selection_rows();
    }

    /// In Remote mode, queue a game event for the session instead of
    /// mutating locally. Returns true when it was queued (so callers
    /// skip the local path).
    pub(crate) fn net_emit(&mut self, event: GameEvent) -> bool {
        if self.net_mode == NetMode::Remote {
            self.net_outbox.push(event);
            true
        } else {
            false
        }
    }
}
