//! The env-gated self-tests: scripted sessions the host drives itself.
//!
//! Each one arms from an `ISOMETRY_*` env var, waits out a warm-up so the
//! window and the first frames exist, fires once, and prints what it saw. They
//! are how a headed run proves a lane end to end without a human driving the
//! mouse, and they stay out of `main.rs` because they are scaffolding for
//! verification rather than part of the host loop.
//!
//! Split out of `main.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl App {
    /// The env-gated self-test: after a warm-up, emit one end-turn as if
    /// the user pressed it, exercising the full session round-trip.
    pub(crate) fn maybe_selftest(&mut self) {
        if !self.net_selftest || self.selftest_fired {
            return;
        }
        let ready = self
            .started
            .map(|t| t.elapsed() > Duration::from_secs(3))
            .unwrap_or(false);
        if ready {
            self.selftest_fired = true;
            eprintln!("[isometry] selftest: firing end_turn");
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| ui.end_turn());
            }
            self.pump_net();
        }
    }

    /// `ISOMETRY_COMBAT_SELFTEST=1`: a focus-free proof of the adjudication
    /// loop. It stats both duelists, stands the goblin in reach, and swings.
    ///
    /// The app drives itself rather than being driven by synthetic clicks,
    /// because SendKeys loses the foreground race on a machine someone is
    /// actually using and silently types into their editor. Same rationale as
    /// `ISOMETRY_NET_SELFTEST`.
    /// `ISOMETRY_TRAVEL_SELFTEST=1`: prove C2 end to end in the app. The demo
    /// board becomes the stored map `field` with a door; a `hut` map waits on
    /// the other side; the knight (the whole party: everyone else is demoted to
    /// DM furniture) walks onto the door through the normal Play-mode click
    /// path, and the board follows it through.
    pub(crate) fn maybe_travel_selftest(&mut self) {
        if !self.travel_selftest || self.travel_fired {
            return;
        }
        let ready = self
            .started
            .is_some_and(|t| t.elapsed() > Duration::from_secs(2));
        if !ready {
            return;
        }
        self.travel_fired = true;
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        runner.update(|ui| {
            // The party is the knight alone; the rest is the DM's furniture.
            for t in ui.map.tokens.iter_mut() {
                if t.id != TokenId(1) {
                    t.owner = None;
                }
            }
            // The field: the live board, stored, with a door beside the knight.
            let field = isometry_campaign::CampaignMap {
                id: "field".to_owned(),
                scale: isometry_campaign::MapScale::Local,
                document: ui.map.clone(),
                spawn_zones: Vec::new(),
                transitions: vec![isometry_campaign::MapTransition {
                    id: "field-gate".to_owned(),
                    at: isometry_campaign::MapPoint { col: 12, row: 14 },
                    target_map: "hut".to_owned(),
                    target_entry: Some("hut-door".to_owned()),
                }],
                encounter_anchors: Vec::new(),
            };
            // The hut: a small stone room with one resident.
            let mut hut_doc = isometry_core::MapDocument::new("hut", 10, 10);
            let floor = hut_doc.intern_tile_kind("stone");
            for r in 0..10 {
                for c in 0..10 {
                    hut_doc.ground.set(c, r, floor);
                }
            }
            hut_doc.tokens.push(isometry_core::Token {
                id: TokenId(7),
                at: (6, 6),
                facing: isometry_core::Facing::South,
                sprite: "goblin".to_owned(),
                owner: None,
            });
            let hut = isometry_campaign::CampaignMap {
                id: "hut".to_owned(),
                scale: isometry_campaign::MapScale::Local,
                document: hut_doc,
                spawn_zones: Vec::new(),
                transitions: vec![isometry_campaign::MapTransition {
                    id: "hut-door".to_owned(),
                    at: isometry_campaign::MapPoint { col: 2, row: 2 },
                    target_map: "field".to_owned(),
                    target_entry: Some("field-gate".to_owned()),
                }],
                encounter_anchors: Vec::new(),
            };
            ui.campaign_maps.insert("field".to_owned(), field);
            ui.campaign_maps.insert("hut".to_owned(), hut);
            ui.active_map = Some("field".to_owned());
            // Time passes in the field before anyone crosses: the DM declares
            // a rest, so the two locations' clocks drift apart.
            ui.pass_time(4);
            eprintln!(
                "[isometry] travel selftest: on {:?}, knight@{:?}, door at (12, 14) | clocks {:?}",
                ui.active_map,
                ui.map.token(TokenId(1)).map(|t| t.at),
                ui.clocks,
            );
            // Walk through the door via the normal Play-mode click path.
            ui.mode = EditMode::Play;
            ui.select_token(TokenId(1));
            ui.click_tile((12, 14));
            eprintln!(
                "[isometry] travel selftest: {} | active {:?} board '{}' | knight here: {:?} | field still holds knight: {:?} | clocks {:?}",
                ui.status,
                ui.active_map,
                ui.map.name,
                ui.map.tokens.iter().find(|t| t.sprite == "knight").map(|t| t.at),
                ui.campaign_maps["field"].document.token(TokenId(1)).is_some(),
                ui.clocks,
            );
        });
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// `ISOMETRY_CMD_SELFTEST=1` (pair with `ISOMETRY_GEN_SEED` for a fixed
    /// NPC): drive the whole `>` command surface once, focus-free.
    pub(crate) fn maybe_cmd_selftest(&mut self) {
        if !self.cmd_selftest || self.cmd_fired {
            return;
        }
        if !self.started.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            return;
        }
        self.cmd_fired = true;
        let before = self
            .runner
            .as_ref()
            .map(|r| r.state().map.tokens.len())
            .unwrap_or(0);

        // >spawn: a statted goblin, resolved from a free-text query.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.spawn_query("gobl"));
        }
        self.pump_sheets(); // binds the stat block

        // >find: a unified compendium search.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.find_query("sword"));
            eprintln!(
                "[isometry] cmd selftest: find 'sword' -> {} results, first: {:?}",
                runner.state().command_results.len(),
                runner.state().command_results.first(),
            );
        }

        // >gen npc: open the generator, generate a preview, commit it.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.start_generator("npc"));
        }
        self.pump_generators(); // Generate -> preview
        let previewed = self
            .runner
            .as_ref()
            .and_then(|r| r.state().generator_preview.clone());
        eprintln!("[isometry] cmd selftest: gen npc preview = {previewed:?}");
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.commit_generation_preview());
        }
        self.pump_generators(); // Commit -> lower into a statted token
        self.pump_sheets();

        if let Some(runner) = self.runner.as_ref() {
            let ui = runner.state();
            let newest = ui.map.tokens.last();
            eprintln!(
                "[isometry] cmd selftest: tokens {} -> {} | newest {:?} sheet name {:?} hp {:?} | status: {}",
                before,
                ui.map.tokens.len(),
                newest.map(|t| (t.id.0, t.sprite.clone(), t.at)),
                newest.and_then(|t| ui.map.sheet(t.id)).and_then(|s| s.text("name").map(str::to_owned)),
                newest.and_then(|t| ui.map.sheet(t.id)).and_then(|s| s.int("hp_current")),
                ui.status,
            );
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// `ISOMETRY_CONVINCE_SELFTEST=1`: a bard recruits a goblin, then meets the
    /// party cap on the next. Focus-free.
    /// `ISOMETRY_STORYLET_SELFTEST=1`: seed a ready storylet and a locked one,
    /// open the surface, play the ready one, and confirm its fact committed.
    pub(crate) fn maybe_storylet_selftest(&mut self) {
        if !self.storylet_selftest || self.storylet_fired {
            return;
        }
        if !self.started.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            return;
        }
        self.storylet_fired = true;

        use isometry_campaign::{StoryletEffect, StoryletProposal, StoryletRequirements, WorldFact};
        // A ready storylet (no requirements, no roles) and a locked one (needs a
        // faction that does not exist).
        let ready = StoryletProposal {
            key: "gate-greeting".to_owned(),
            entry: "A stranger greets you at the gate.".to_owned(),
            tags: Vec::new(),
            requirements: StoryletRequirements::default(),
            roles: Vec::new(),
            effects: vec![StoryletEffect::Fact {
                fact: WorldFact {
                    id: "gate-met".to_owned(),
                    kind: "event".to_owned(),
                    text: "The party met a stranger at the gate.".to_owned(),
                    tags: Vec::new(),
                },
            }],
        };
        let locked = StoryletProposal {
            key: "cult-rises".to_owned(),
            entry: "The eel cult stirs in the deep.".to_owned(),
            tags: Vec::new(),
            requirements: StoryletRequirements {
                faction_tags: vec!["cult".to_owned()],
                ..Default::default()
            },
            roles: Vec::new(),
            effects: Vec::new(),
        };
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                ui.world.storylets.insert("gate-greeting".to_owned(), ready.clone());
                ui.world.storylets.insert("cult-rises".to_owned(), locked.clone());
                ui.open_storylets();
            });
        }
        // Compute the rows.
        self.pump_storylets();
        if let Some(runner) = self.runner.as_ref() {
            for row in &runner.state().storylets {
                eprintln!(
                    "[isometry] storylet selftest: {} available={} status={:?} entry={:?}",
                    row.key, row.available, row.status, row.entry
                );
            }
        }
        // Select the ready one and play it.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                let idx = ui.storylets.iter().position(|r| r.key == "gate-greeting").unwrap_or(0);
                ui.storylet_selected = idx;
                ui.play_storylet();
            });
        }
        self.pump_storylets();
        let committed = self.journal.iter().any(|f| f.id == "gate-met");
        let status = self
            .runner
            .as_ref()
            .map(|r| r.state().status.clone())
            .unwrap_or_default();
        eprintln!(
            "[isometry] storylet selftest: played | journal has 'gate-met': {committed} | status: {status}"
        );
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// `ISOMETRY_OVERMAP_SELFTEST=1`: seed a small overmap (four places joined by
    /// roads), stand the party at the village, reveal the map it would know, and
    /// open the overmap surface. A focus-free proof the C8 render draws.
    pub(crate) fn maybe_overmap_selftest(&mut self) {
        if !self.overmap_selftest || self.overmap_fired {
            return;
        }
        if !self.started.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            return;
        }
        self.overmap_fired = true;

        use isometry_campaign::{ItemId, ItemInstance, WorldPlace, WorldRoute};
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                // Positions left unset (`None`): the overmap relaxes a
                // force-directed layout from the routes, proving that path.
                let place = |id: &str, name: &str| WorldPlace {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    tags: Vec::new(),
                    map: None,
                    position: None,
                };
                for (id, name) in [
                    ("village", "Village"),
                    ("forest", "Deepwood"),
                    ("ruins", "Old Ruins"),
                    ("keep", "Grey Keep"),
                    ("citadel", "Sky Citadel"),
                ] {
                    ui.world.places.insert(id.to_owned(), place(id, name));
                }
                let route = |id: &str, from: &str, to: &str, weight: u32| WorldRoute {
                    id: id.to_owned(),
                    from: from.to_owned(),
                    to: to.to_owned(),
                    tags: Vec::new(),
                    weight,
                };
                ui.world.routes.insert("r1".to_owned(), route("r1", "village", "forest", 2));
                ui.world.routes.insert("r2".to_owned(), route("r2", "forest", "ruins", 3));
                ui.world.routes.insert("r3".to_owned(), route("r3", "village", "keep", 5));
                ui.world.routes.insert("r4".to_owned(), route("r4", "keep", "citadel", 4));

                let party = ui.viewer.clone().unwrap_or_else(|| "dm".to_owned());
                ui.world.party_node.insert(party.clone(), "village".to_owned());
                // The party knows only its own ground and the near woods. The keep
                // is one route past the known (a frontier the "study map" read
                // finds); the Sky Citadel is two routes out, unreachable by that
                // read -- it is only learned from a *carried* map (below).
                for node in ["village", "forest", "ruins"] {
                    ui.world.reveal(&party, node);
                }
                // A party token to carry the pack: knight 1, deeded to the party,
                // holding a looted chart tagged with the place it depicts.
                if let Some(token) = ui.map.tokens.iter_mut().find(|t| t.id == TokenId(1)) {
                    token.owner = Some(party.clone());
                }
                ui.inventories.entry(TokenId(1)).or_default().items.insert(
                    ItemId::new("citadel-chart"),
                    ItemInstance {
                        id: ItemId::new("citadel-chart"),
                        template: "map".to_owned(),
                        name: "Chart to the Sky Citadel".to_owned(),
                        quantity: 1,
                        tags: vec!["map".to_owned(), "reveals:citadel".to_owned()],
                        modifiers: Vec::new(),
                        appearance_layers: Vec::new(),
                    },
                );
                ui.open_overmap();
                // Study the map at once so the capture shows the outcome: the
                // frontier read finds the keep, and the carried chart discloses
                // the citadel two routes out that no frontier read could reach.
                ui.request_map_read();
            });
        }
        // The `request_map_read` above only arms a read; its pump normally runs
        // on a window event, of which a headless selftest has none. Drive it once
        // here so the seeded chart resolves and the capture shows the outcome.
        self.pump_overmap_read();
        eprintln!(
            "[isometry] overmap selftest: seeded 5 places, party at the village knowing 3, \
             carrying a chart to the citadel; reading the map reveals keep + citadel"
        );
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    pub(crate) fn maybe_convince_selftest(&mut self) {
        if !self.convince_selftest || self.convince_fired {
            return;
        }
        if !self.started.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            return;
        }
        self.convince_fired = true;

        let Some(system) = self.system.as_mut() else {
            return;
        };
        // A silver-tongued bard: CHA 18 (+4) and proficiency, so the pitch is
        // 1d20+6 against a goblin's low resolve.
        let mut bard = system.default_sheet();
        bard.set_text("name", "Bard");
        bard.set_int("cha", 18);
        bard.set_int("prof", 2);
        let goblin = |will: i64| {
            let mut s = srd_bestiary()
                .iter()
                .find(|m| m.key == "goblin")
                .map(monster_sheet)
                .unwrap_or_else(|| system.default_sheet());
            s.set_int("will", will);
            s
        };
        let (g1, g2) = (goblin(4), goblin(4));

        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| {
                // Knight 1 is player A's; make it the bard. Goblins 2 and 4 are
                // the DM's furniture (owner None) standing in talking range.
                ui.map.set_sheet(TokenId(1), bard.clone());
                ui.map.set_sheet(TokenId(2), g1.clone());
                ui.map.set_sheet(TokenId(4), g2.clone());
                let anchor = ui.map.token(TokenId(1)).map(|t| t.at).unwrap_or((10, 14));
                for (id, dx) in [(TokenId(2), 2), (TokenId(4), 3)] {
                    if let Some(g) = ui.map.tokens.iter_mut().find(|t| t.id == id) {
                        g.at = (anchor.0 + dx, anchor.1);
                        g.owner = None; // DM furniture, up for grabs
                    }
                }
                // A owns knight 1 and knight 3 on this board, plus one companion
                // stashed on a *stored* map (a split party, C3). The cap counts
                // the whole campaign, so that third token matters: with cap 4, A
                // can take exactly one goblin before the party fills.
                let mut away = isometry_core::MapDocument::new("waystation", 6, 6);
                away.tokens.push(isometry_core::Token {
                    id: TokenId(50),
                    at: (2, 2),
                    facing: isometry_core::Facing::South,
                    sprite: "knight".to_owned(),
                    owner: Some("A".to_owned()),
                });
                ui.campaign_maps.insert(
                    "waystation".to_owned(),
                    isometry_campaign::CampaignMap {
                        id: "waystation".to_owned(),
                        scale: isometry_campaign::MapScale::Local,
                        document: away,
                        spawn_zones: Vec::new(),
                        transitions: Vec::new(),
                        encounter_anchors: Vec::new(),
                    },
                );
                ui.party_cap = 4;
                ui.viewer = Some("A".to_owned());
                ui.recompute_fog();
                let a_active = ui.map.tokens.iter().filter(|t| t.owner.as_deref() == Some("A")).count();
                eprintln!(
                    "[isometry] convince selftest: A owns {a_active} here + 1 stored = 3 global, cap {}",
                    ui.party_cap
                );
            });
        }

        // First pitch: goblin 2 joins A (A goes 2 -> 3 tokens, at the cap).
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.action_intent = Some((TokenId(1), TokenId(2), "convince".to_owned())));
        }
        self.pump_sheets();
        // Second pitch: goblin 4 would make 4 > cap 3, so it fails to hold.
        if let Some(runner) = self.runner.as_mut() {
            runner.update(|ui| ui.action_intent = Some((TokenId(1), TokenId(4), "convince".to_owned())));
        }
        self.pump_sheets();

        if let Some(runner) = self.runner.as_ref() {
            let ui = runner.state();
            let owner = |id| ui.map.token(id).and_then(|t| t.owner.clone());
            let a_here = ui.map.tokens.iter().filter(|t| t.owner.as_deref() == Some("A")).count();
            let a_global = a_here
                + ui.campaign_maps
                    .values()
                    .flat_map(|m| m.document.tokens.iter())
                    .filter(|t| t.owner.as_deref() == Some("A"))
                    .count();
            eprintln!(
                "[isometry] convince selftest: goblin2 owner {:?} | goblin4 owner {:?} | A owns {a_here} here, {a_global} global (cap 4) | status: {}",
                owner(TokenId(2)),
                owner(TokenId(4)),
                ui.status,
            );
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    pub(crate) fn maybe_combat_selftest(&mut self) {
        if !self.combat_selftest || (self.combat_swings == 0 && self.combat_emoted) {
            return;
        }
        // Wait 2s for the first swing, then one per second: long enough for a
        // 420ms beat to finish and be cleared, so the *next* swing has to
        // genuinely restart the animation rather than find its class still set.
        let due = match self.last_swing {
            None => self
                .started
                .is_some_and(|t| t.elapsed() > Duration::from_secs(2)),
            Some(last) => last.elapsed() > Duration::from_millis(1000),
        };
        if !due {
            return;
        }
        let first = self.last_swing.is_none();
        self.last_swing = Some(Instant::now());

        // The swings are spent: the winner celebrates. An emote is the same beat
        // primitive, with no resolution behind it and nothing to adjudicate.
        if self.combat_swings == 0 {
            self.combat_emoted = true;
            if let Some(runner) = self.runner.as_mut() {
                runner.update(|ui| ui.emote(TokenId(1), "cheer"));
                eprintln!(
                    "[isometry] combat selftest: emote | beats = {:?}",
                    runner.state().beats
                );
            }
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
            return;
        }
        self.combat_swings -= 1;
        let swings_left = self.combat_swings;

        let Some(system) = self.system.as_mut() else {
            return;
        };
        let mut knight = system.default_sheet();
        knight.set_text("name", "Knight");
        knight.set_int("str", 18); // +4
        knight.set_int("prof", 3); // so the swing is 1d20+7 against AC 15
        let Some(goblin) = srd_bestiary().iter().find(|m| m.key == "goblin").map(monster_sheet)
        else {
            eprintln!("[isometry] combat selftest: no goblin in the bestiary");
            return;
        };
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        runner.update(|ui| {
            if first {
                // Stand the goblin within reach of the knight, and stat them
                // both. After that the board carries its own state: each swing
                // hits whatever hit points the last one left behind.
                if let Some(at) = ui.map.token(TokenId(1)).map(|t| t.at) {
                    if let Some(g) = ui.map.tokens.iter_mut().find(|t| t.id == TokenId(2)) {
                        g.at = (at.0 + 1, at.1);
                    }
                }
                ui.map.set_sheet(TokenId(1), knight.clone());
                ui.map.set_sheet(TokenId(2), goblin.clone());
                ui.open_sheet = Some(TokenId(1));
                ui.recompute_fog();
                eprintln!(
                    "[isometry] combat selftest: knight@{:?} vs goblin@{:?} | goblin hp {:?}, ac {:?} (1d20+7 to hit)",
                    ui.map.token(TokenId(1)).map(|t| t.at),
                    ui.map.token(TokenId(2)).map(|t| t.at),
                    ui.map.sheet(TokenId(2)).and_then(|s| s.int("hp_current")),
                    ui.map.sheet(TokenId(2)).and_then(|s| s.int("ac")),
                );
            }
            // Trip first (a condition: prone halves speed, truth on every
            // peer), then attacks. The prone goblin keeps its tile, unlike the
            // shove run: a condition changes what it can DO, not where it is.
            // The fixed tape rolls 11 then 22: the first swing misses whatever
            // it is, so the trip goes second, where it connects.
            let action = if swings_left == 2 { "trip" } else { "attack" };
            ui.action_intent = Some((TokenId(1), TokenId(2), action.to_owned()));
        });
        self.pump_sheets();
        if let Some(runner) = self.runner.as_ref() {
            let ui = runner.state();
            eprintln!(
                "[isometry] combat selftest: {} | goblin hp {:?} conds {:?} mobility {:?} | beats = {:?}",
                ui.status,
                ui.map.sheet(TokenId(2)).and_then(|s| s.int("hp_current")),
                ui.map.conditions.get(&TokenId(2)),
                ui.map.effective_mobility(TokenId(2), (5, 6)),
                ui.beats,
            );
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

}
