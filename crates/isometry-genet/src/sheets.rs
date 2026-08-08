//! Character sheets and the compendium-driven spawn lane.
//!
//! `pump_sheets` is the host side of "a token gets stats": it binds stat blocks
//! from the system's bestiary, answers `>spawn` queries, and keeps sheet edits
//! flowing to the authority. Cheap flag checks come first, so an ordinary frame
//! does no work here.
//!
//! Split out of `main.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl App {
    /// Drain the UI's sheet requests (bind / edit / action) and evaluate
    /// them through the game system: bind a default sheet, apply a field
    /// edit, or roll an action; then recompute the open sheet's derived
    /// stats. Cheap-checks first so a normal frame does no work.
    pub(crate) fn pump_sheets(&mut self) {
        if self.system.is_none() {
            return;
        }
        let (bind, edit, action, inventory_request, open, intent, spawn_sheet, clear_condition) =
            match self.runner.as_ref() {
                Some(r) => {
                    let s = r.state();
                    (
                        s.bind_sheet_request,
                        s.sheet_edit.clone(),
                        s.sheet_action.clone(),
                        s.inventory_request.clone(),
                        s.open_sheet,
                        s.action_intent.clone(),
                        s.spawn_sheet_request.clone(),
                        s.clear_condition_request.clone(),
                    )
                }
                None => return,
            };
        let open_changed = open != self.last_sheet_open;
        let effective_missing = self
            .runner
            .as_ref()
            .is_some_and(|runner| open.is_some() && runner.state().sheet_effective.is_none());
        if bind.is_none()
            && edit.is_none()
            && action.is_none()
            && inventory_request.is_none()
            && intent.is_none()
            && spawn_sheet.is_none()
            && clear_condition.is_none()
            && !open_changed
            && !effective_missing
        {
            return;
        }
        self.last_sheet_open = open;
        let system = self.system.as_mut().expect("system present");
        let Some(runner) = self.runner.as_mut() else {
            return;
        };

        // Bind a fresh default sheet.
        if let Some(tok) = bind {
            let sheet = system.default_sheet();
            runner.update(|ui| {
                ui.bind_sheet_request = None;
                ui.map.set_sheet(tok, sheet.clone());
                if ui.net_mode == NetMode::Remote {
                    ui.net_outbox.push(GameEvent::SheetSet {
                        token: tok,
                        sheet: sheet.clone(),
                    });
                }
            });
        }

        // Apply a field edit (clamped non-negative), then replicate.
        if let Some((tok, key, delta)) = edit {
            let mut updated = None;
            runner.update(|ui| {
                ui.sheet_edit = None;
                if let Some(sheet) = ui.map.sheets.get_mut(&tok) {
                    let cur = sheet.int(&key).unwrap_or(0);
                    sheet.set_int(&key, (cur + delta).max(0));
                    updated = Some(sheet.clone());
                }
            });
            if let Some(sheet) = updated {
                runner.update(|ui| {
                    if ui.net_mode == NetMode::Remote {
                        ui.net_outbox
                            .push(GameEvent::SheetSet { token: tok, sheet });
                    }
                });
            }
        }

        // Item instances are minted and equipped on the host. The request only
        // carries pack/template data; the authoritative inventory replacement
        // is what enters the replicated log.
        if let Some(request) = inventory_request {
            let mut event = None;
            runner.update(|ui| {
                ui.inventory_request = None;
                match request {
                    InventoryRequest::AddCompendiumItem {
                        token,
                        template,
                        name,
                        category,
                    } => {
                        if ui.map.token(token).is_none() {
                            ui.status = "cannot add item to a missing token".to_owned();
                            return;
                        }
                        let inventory = ui.inventories.entry(token).or_default();
                        let mut ordinal = inventory.items.len();
                        let id = loop {
                            let id = ItemId::new(format!("token-{}.item-{ordinal}", token.0));
                            if !inventory.items.contains_key(&id) {
                                break id;
                            }
                            ordinal += 1;
                        };
                        let appearance_layers = if category == "Weapon" {
                            vec![format!("weapon:{template}")]
                        } else {
                            Vec::new()
                        };
                        let item = ItemInstance {
                            id,
                            template: format!("srd5e:{template}"),
                            name: name.clone(),
                            quantity: 1,
                            tags: vec![category.to_lowercase()],
                            modifiers: Vec::new(),
                            appearance_layers,
                        };
                        if inventory.insert(item).is_ok() {
                            event = Some(GameEvent::InventorySet {
                                token,
                                inventory: inventory.clone(),
                            });
                            ui.status = format!("added {name}");
                        }
                    }
                    InventoryRequest::Equip { token, slot, item } => {
                        if let Some(inventory) = ui.inventories.get_mut(&token) {
                            if inventory.equip(slot, item).is_ok() {
                                event = Some(GameEvent::InventorySet {
                                    token,
                                    inventory: inventory.clone(),
                                });
                                ui.status = "equipped item".to_owned();
                            }
                        }
                    }
                    InventoryRequest::Unequip { token, slot } => {
                        if let Some(inventory) = ui.inventories.get_mut(&token) {
                            inventory.equipped.remove(&slot);
                            event = Some(GameEvent::InventorySet {
                                token,
                                inventory: inventory.clone(),
                            });
                            ui.status = "unequipped item".to_owned();
                        }
                    }
                    InventoryRequest::Transfer { from, to, item } => {
                        let destination_has_item = ui
                            .inventories
                            .get(&to)
                            .is_some_and(|inventory| inventory.items.contains_key(&item));
                        if !destination_has_item {
                            let moved = ui
                                .inventories
                                .get_mut(&from)
                                .and_then(|inventory| inventory.take(&item).ok());
                            if let Some(moved) = moved {
                                if ui.inventories.entry(to).or_default().insert(moved).is_ok() {
                                    event = Some(GameEvent::ItemTransfer { from, to, item });
                                    ui.status = "transferred item".to_owned();
                                }
                            }
                        }
                    }
                }
                ui.sheet_effective = None;
            });
            if let Some(event) = event {
                runner.update(|ui| {
                    if ui.net_mode == NetMode::Remote {
                        ui.net_outbox.push(event);
                    }
                });
            }
        }

        // Roll an action: evaluate its dice expression against the sheet.
        if let Some((tok, key)) = action {
            let (sheet, inventory) = {
                let state = runner.state();
                (
                    state.map.sheet(tok).cloned(),
                    state.inventories.get(&tok).cloned(),
                )
            };
            if let Some(sheet) = sheet {
                let effective = system.effective_sheet(&sheet, inventory.as_ref());
                if let Some(expr) = system.action_expr(&key, &effective) {
                    let by = sheet.text("name").unwrap_or("?").to_owned();
                    let label = system
                        .actions
                        .iter()
                        .find(|a| a.key == key)
                        .map(|a| a.label.clone())
                        .unwrap_or(key);
                    runner.update(|ui| {
                        ui.sheet_action = None;
                        ui.roll_labeled(&by, &label, &expr);
                    });
                }
            }
        }

        // A spawned monster becomes statted: the compendium stat block reaches a
        // real sheet, which is what makes it a thing that can be fought rather
        // than a sprite standing on a diamond.
        if let Some((token, key)) = spawn_sheet {
            let sheet = srd_bestiary()
                .iter()
                .find(|m| m.key == key)
                .map(monster_sheet);
            runner.update(|ui| {
                ui.spawn_sheet_request = None;
                let Some(sheet) = sheet else {
                    ui.status = format!("no stat block for {key}");
                    return;
                };
                ui.map.set_sheet(token, sheet.clone());
                if ui.net_mode == NetMode::Remote {
                    ui.net_outbox.push(GameEvent::SheetSet { token, sheet });
                }
            });
        }

        // Clear one condition: the ask half of standing up. The rules recompute
        // what the token can do without it; if no condition remains, the
        // mobility override clears entirely and the sheet's base numbers stand.
        if let Some((token, name)) = clear_condition {
            let (sheet, remaining) = {
                let s = runner.state();
                let mut set = s.map.conditions.get(&token).cloned().unwrap_or_default();
                set.remove(&name);
                (s.map.sheet(token).cloned(), set)
            };
            let mobility = match (&sheet, remaining.is_empty()) {
                (_, true) => None,
                (Some(sheet), false) => {
                    let conditioned = sheet_with_conditions(sheet, remaining.iter());
                    system.mobility_for(&conditioned, true)
                }
                (None, false) => None,
            };
            runner.update(|ui| {
                ui.clear_condition_request = None;
                let event = GameEvent::ConditionSet {
                    token,
                    condition: name.clone(),
                    value: 0,
                    mobility,
                };
                if ui.net_mode == NetMode::Remote {
                    ui.net_outbox.push(event);
                } else {
                    ui.map.set_condition(token, &name, 0);
                    ui.map.set_mobility(token, mobility);
                    ui.recompute_fog();
                    ui.recompute_reach();
                    ui.status = format!("cleared {name}");
                }
            });
        }

        // Where does an action get adjudicated? On the authority, always.
        //
        // A joined player *asks*: the intent goes over the wire as an
        // `ActionIntent` carrying no roll and no verdict, and the host's rules
        // system answers. If a client resolved its own swing it would be rolling
        // its own dice and choosing its own damage, which is precisely what the
        // host refuses elsewhere.
        //
        // Everyone else (the DM, and solo play) *is* the authority, so they fall
        // through to the resolver below.
        let mut intent = intent;
        if let Some((actor, target, key)) = intent.clone() {
            if self.net.is_some() && !self.net_is_host {
                if let Some(net) = self.net.as_ref() {
                    // Unnumbered: the client session stamps the nonce on the way
                    // out and the host stamps whose ask it was on the way in.
                    net.submit_action(ActionIntent::new(actor, target, key));
                }
                runner.update(|ui| {
                    ui.action_intent = None;
                    ui.status = "asking the host...".to_owned();
                });
                intent = None;
            }
        }

        // This process's own ask (the DM's swing, or solo play) arrives over no
        // connection, so nobody else can attribute it: number it here, against
        // the reserved host identity.
        let own = intent.map(|(actor, target, key)| {
            self.own_requests += 1;
            (RequestId::host(self.own_requests), actor, target, key)
        });

        // The host also adjudicates its players' requests, through this same
        // path: one resolver, one entropy source, one set of rules, whoever asked.
        let mut queued: Vec<(RequestId, TokenId, TokenId, String)> = Vec::new();
        if self.net_is_host {
            if let Some(net) = self.net.as_mut() {
                queued = net
                    .take_action_intents()
                    .into_iter()
                    .map(|i| (i.request, i.actor, i.target, i.action_key))
                    .collect();
            }
        }
        for pending in own.into_iter().chain(queued) {
            self.adjudicate(pending);
        }

        // Recompute derived stats for the open sheet.
        let Some(system) = self.system.as_mut() else {
            return;
        };
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        if let Some(tok) = open {
            let (sheet, inventory) = {
                let state = runner.state();
                (
                    state.map.sheet(tok).cloned(),
                    state.inventories.get(&tok).cloned(),
                )
            };
            if let Some(sheet) = sheet {
                let effective = system.effective_sheet(&sheet, inventory.as_ref());
                let derived = system.derived(&effective);
                runner.update(|ui| {
                    ui.sheet_effective = Some(effective);
                    ui.sheet_derived = derived;
                });
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}
