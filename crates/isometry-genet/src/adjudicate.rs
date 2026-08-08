//! Action adjudication: the host rolls, and everyone applies the verdict.
//!
//! One entry point. A swing reaches it from the combat self-test and from a
//! player's request alike, so there is a single place where dice are rolled and
//! a single event shape the table replicates. Peers never roll.
//!
//! Split out of `main.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl App {
    /// Resolve one action request and commit its outcome. The only path from
    /// "I swing at that goblin" to the goblin being hurt, taken by the DM's own
    /// swings and by a player's request alike.
    pub(crate) fn adjudicate(&mut self, (actor, target, key): (TokenId, TokenId, String)) {
        let Some(system) = self.system.as_mut() else {
            return;
        };
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        // The host validates what the substrate can see (both tokens exist, it
        // is the actor's turn, the victim is in reach), and the system decides
        // everything else. Only the resolved outcome is replicated, so peers
        // apply it without rerunning a line of Lua.
        {
            let (
                actor_sheet,
                actor_inv,
                target_sheet,
                target_inv,
                tiles,
                turn_ok,
                under_initiative,
            ) = {
                let s = runner.state();
                let tiles = match (s.map.token(actor), s.map.token(target)) {
                    (Some(a), Some(t)) => Some((a.at, t.at)),
                    _ => None,
                };
                // An empty turn order means free play (the editor, a hot-seat
                // skirmish before initiative); once initiative exists, only the
                // active token may act.
                let turn_ok = s.turns.active().map_or(true, |active| active == actor);
                // Per-turn counters (the action economy, the multiple-attack
                // penalty) only exist *within* initiative: with no turn order
                // there is no turn to reset against, so the afford rule must not
                // bite and the spend must not accumulate -- otherwise free play
                // would silently cap a PF2e token at three strikes forever. Under
                // initiative, turn_ok already proves the actor is the active one.
                let under_initiative = s.turns.active() == Some(actor);
                (
                    s.map.sheet(actor).cloned(),
                    s.inventories.get(&actor).cloned(),
                    s.map.sheet(target).cloned(),
                    s.inventories.get(&target).cloned(),
                    tiles,
                    turn_ok,
                    under_initiative,
                )
            };

            let outcome: Result<_, String> = (|| {
                let (actor_at, target_at) = tiles.ok_or_else(|| "no such target".to_owned())?;
                if !turn_ok {
                    return Err("not your turn".to_owned());
                }
                let actor_sheet = actor_sheet.ok_or_else(|| "attacker has no sheet".to_owned())?;
                let target_sheet = target_sheet.ok_or_else(|| "target has no sheet".to_owned())?;
                // Equipment counts: resolve against the effective sheets, so a
                // magic sword's bonus lands and armour raises the AC it is
                // compared against. Conditions ride along as boolean fields, so
                // the rules can read `t.prone` and the resolver can tell "apply
                // prone" from "already prone".
                let (actor_conds, target_conds, actor_counters) = {
                    let s = runner.state();
                    (
                        s.map.conditions.get(&actor).cloned().unwrap_or_default(),
                        s.map.conditions.get(&target).cloned().unwrap_or_default(),
                        // Free play carries no per-turn counters into the sheet,
                        // so the afford rule sees a fresh economy every swing.
                        if under_initiative {
                            s.map.turn_counters.get(&actor).cloned().unwrap_or_default()
                        } else {
                            Default::default()
                        },
                    )
                };
                // The actor's per-turn counters ride the sheet as `turn_<key>`
                // fields, the same channel conditions use, so the afford rule and
                // any script folding a multiple-attack penalty read them with a
                // plain `c.turn_strikes`. Only the actor spends, so only the actor
                // carries them; the target's affordability is never asked.
                let actor_eff = sheet_with_turn_counters(
                    &sheet_with_conditions(
                        &system.effective_sheet(&actor_sheet, actor_inv.as_ref()),
                        actor_conds.iter(),
                    ),
                    actor_counters.iter(),
                );
                let target_eff = sheet_with_conditions(
                    &system.effective_sheet(&target_sheet, target_inv.as_ref()),
                    target_conds.iter(),
                );
                system
                    .resolve_action(
                        &key,
                        actor,
                        &actor_eff,
                        actor_at,
                        target,
                        &target_eff,
                        target_at,
                        &mut self.action_rng,
                    )
                    .map_err(|e| match e {
                        ActionError::OutOfRange { range, distance } => {
                            format!("out of reach ({distance} tiles, reach {range})")
                        }
                        ActionError::SelfTarget => "cannot target yourself".to_owned(),
                        ActionError::AlreadyDefeated => "that one is already down".to_owned(),
                        ActionError::NotTargeted(key) => format!("{key} needs no target"),
                        ActionError::UnknownAction(key) => format!("no such action: {key}"),
                        // The system's afford rule refused it -- out of actions
                        // this turn, not enough mana, whatever the ruleset gates
                        // on. The substrate names the action, not the resource.
                        ActionError::CannotAfford(key) => format!("can't afford {key} right now"),
                        // A script or dice-expression fault is the system's bug,
                        // not the player's; name it rather than hiding it.
                        ActionError::ScriptFailed(f) => format!("system script failed: {f}"),
                        ActionError::BadDice(expr) => format!("system rolled bad dice: {expr}"),
                    })
            })();

            let label = system
                .actions
                .iter()
                .find(|a| a.key == key)
                .map(|a| a.label.clone())
                .unwrap_or_else(|| key.clone());

            runner.update(|ui| {
                ui.action_intent = None;
                let resolution = match outcome {
                    Ok(r) => r,
                    Err(reason) => {
                        // A refused intent changes nothing at all: no dice, no
                        // deltas, no turn spent.
                        ui.status = reason;
                        return;
                    }
                };

                // Where does a shove actually land? The rules said how hard and
                // which way; the *board* rules on the rest, because a wall, a map
                // edge, or another body stops a push short and the system does
                // not know the map. This is truth, so it is decided once and
                // replicated, unlike the stagger beat riding alongside it.
                let mut displaced = Vec::new();
                if let Some((step, tiles)) = resolution.push {
                    if let Some(from) = ui.map.token(target).map(|t| t.at) {
                        let occupied: Vec<TileCoord> = ui.map.tokens.iter().map(|t| t.at).collect();
                        let (w, h) = (ui.map.ground.width(), ui.map.ground.height());
                        let landing = isometry_core::push_path(from, step, tiles, |at| {
                            at.0 >= 0
                                && at.1 >= 0
                                && (at.0 as u32) < w
                                && (at.1 as u32) < h
                                && !occupied.contains(&at)
                        });
                        if let Some(to) = landing {
                            displaced.push((target, to));
                        }
                    }
                }
                // A landed recruit becomes an owner change, ruled here because
                // owners and the party cap are the map's, not the rules'. The
                // winner's side is the actor's owner; a player's party has a cap
                // (the DM, owner None, is uncapped); a creature already on that
                // side needs nothing.
                let mut owner_changes = Vec::new();
                let mut recruit_note = "";
                if let Some(recruited) = resolution.recruited {
                    let new_owner = ui.map.token(actor).and_then(|t| t.owner.clone());
                    let current = ui.map.token(recruited).and_then(|t| t.owner.clone());
                    if current == new_owner {
                        recruit_note = " (already at your side)";
                    } else if let Some(owner) = new_owner.clone() {
                        // Count the owner's *whole* party, not just the active
                        // map: a split party (C3) keeps tokens on stored maps,
                        // and the cap is a limit on people, not on this board.
                        let owned = owner_token_count(ui, &owner);
                        if owned >= ui.party_cap {
                            recruit_note = " but your party is full";
                        } else {
                            owner_changes.push((recruited, Some(owner)));
                            recruit_note = " and it joins you";
                        }
                    } else {
                        owner_changes.push((recruited, None));
                        recruit_note = " and it joins you";
                    }
                }

                let victim = ui
                    .map
                    .token(target)
                    .and_then(|_| ui.map.sheet(target))
                    .and_then(|s| s.text("name").map(str::to_owned))
                    .unwrap_or_else(|| "target".to_owned());
                let down = !resolution.defeated.is_empty();
                ui.status = if resolution.recruited.is_some() {
                    // A social action; "hits for 0 damage" would read wrong.
                    let verb = if owner_changes.is_empty() {
                        "sways"
                    } else {
                        "wins over"
                    };
                    format!(
                        "{} {verb} {victim} ({}){recruit_note}",
                        resolution.attack.by, resolution.attack.total
                    )
                } else if resolution.hit {
                    let dmg = resolution.damage.as_ref().map_or(0, |d| d.total);
                    let felled = if down { " and drops it" } else { "" };
                    format!(
                        "{} hits {victim} ({}) for {dmg}{felled}",
                        resolution.attack.by, resolution.attack.total
                    )
                } else {
                    format!(
                        "{} misses {victim} ({})",
                        resolution.attack.by, resolution.attack.total
                    )
                };

                let event = GameEvent::ActionResolved(ActionResolved {
                    actor,
                    target,
                    action_key: key.clone(),
                    label,
                    attack: resolution.attack,
                    hit: resolution.hit,
                    damage: resolution.damage,
                    deltas: resolution.deltas,
                    beats: resolution.beats,
                    defeated: resolution.defeated,
                    displaced,
                    conditions: resolution.conditions,
                    mobility: resolution.mobility,
                    owner_changes,
                    // Symmetric with the injection: outside initiative there is
                    // no turn to reset against, so the spend is not recorded at
                    // all. Nothing to read, nothing to accumulate -- free play
                    // stays inert for per-turn resources.
                    turn_counters: if under_initiative {
                        resolution.turn_counters
                    } else {
                        Vec::new()
                    },
                });
                if ui.net_mode == NetMode::Remote {
                    // In session the authority applies it and mirrors the
                    // snapshot back, which is where the beats get staged (so a
                    // joined player sees the exchange too, not just the DM).
                    //
                    // But fold this recruit's owner change into the local map
                    // *now*, before the mirror returns: pump_sheets adjudicates a
                    // whole batch of queued intents in one pass, and the cap
                    // above counts `ui.map`. Without this, two same-owner
                    // recruits in one batch would both see the pre-recruit count
                    // and both pass, blowing past the cap. The authority applies
                    // the identical change on mirror-back, so this is idempotent.
                    if let GameEvent::ActionResolved(res) = &event {
                        for (token, owner) in &res.owner_changes {
                            if let Some(t) = ui.map.tokens.iter_mut().find(|t| t.id == *token) {
                                t.owner = owner.clone();
                            }
                        }
                        if !res.owner_changes.is_empty() {
                            ui.recompute_fog();
                            ui.recompute_reach();
                        }
                        // Same batch-window reason as the owner change: the next
                        // queued intent's afford rule reads these counters from
                        // local state, so fold the spend now instead of waiting
                        // for the mirror. The authority applies the identical
                        // bump and mirrors back a whole map (self.map = snap.map),
                        // which *replaces* this, so the additive delta is never
                        // double-counted.
                        for (token, key, delta) in &res.turn_counters {
                            ui.map.bump_turn_counter(*token, key, *delta);
                        }
                    }
                    ui.net_outbox.push(event);
                } else if let GameEvent::ActionResolved(res) = &event {
                    // Solo: no authority to route through, so apply it here.
                    for delta in &res.deltas {
                        ui.map.apply_delta(delta);
                    }
                    for (token, to) in &res.displaced {
                        if let Some(t) = ui.map.tokens.iter_mut().find(|t| t.id == *token) {
                            t.at = *to;
                        }
                    }
                    for token in &res.defeated {
                        ui.map.set_defeated(*token, true);
                    }
                    for (token, name, value) in &res.conditions {
                        ui.map.set_condition(*token, name, *value);
                    }
                    for (token, mobility) in &res.mobility {
                        ui.map.set_mobility(*token, *mobility);
                    }
                    for (token, owner) in &res.owner_changes {
                        if let Some(t) = ui.map.tokens.iter_mut().find(|t| t.id == *token) {
                            t.owner = owner.clone();
                        }
                    }
                    // Solo: no authority to route through, so the per-turn spend
                    // lands in the local ledger here (action economy, MAP).
                    for (token, key, delta) in &res.turn_counters {
                        ui.map.bump_turn_counter(*token, key, *delta);
                    }
                    if !res.conditions.is_empty()
                        || !res.displaced.is_empty()
                        || !res.owner_changes.is_empty()
                    {
                        // A condition, a shove, or a change of allegiance all
                        // change what can be seen and reached; recompute so the
                        // board tells the truth now, not on the next click.
                        ui.recompute_fog();
                        ui.recompute_reach();
                    }
                    ui.push_roll(res.attack.clone());
                    if let Some(damage) = &res.damage {
                        ui.push_roll(damage.clone());
                    }
                    let seq = ui.beat_seq.wrapping_add(1);
                    ui.stage_beats(seq, &res.beats);
                }
                ui.sheet_effective = None;
            });
        }
    }
}
