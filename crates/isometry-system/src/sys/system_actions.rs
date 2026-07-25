//! Derived stats, action expressions, and adjudication.
//!
//! `resolve_action` is the resolver the host calls to turn an intent into a
//! verdict. It rolls through the plugin, so a system decides outcomes and the
//! substrate only records them.
//!
//! Split out of `lib.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl System {
    /// Every derived stat's current value for `sheet`.
    pub fn derived(&mut self, sheet: &SheetData) -> BTreeMap<String, i64> {
        let defs: Vec<(String, String)> = self
            .derived
            .iter()
            .map(|d| (d.key.clone(), d.func.clone()))
            .collect();
        let mut out = BTreeMap::new();
        for (key, func) in defs {
            if let Some(v) = self.call_int(&func, sheet) {
                out.insert(key, v);
            }
        }
        out
    }

    /// The dice expression an action rolls for `sheet`: its base die plus
    /// the Lua-computed signed bonus (e.g. `1d20+5`).
    pub fn action_expr(&mut self, action_key: &str, sheet: &SheetData) -> Option<String> {
        let (base, func) = self
            .actions
            .iter()
            .find(|a| a.key == action_key)
            .map(|a| (a.base.clone(), a.func.clone()))?;
        let bonus = self.call_int(&func, sheet)?;
        Some(format!("{base}{bonus:+}"))
    }

    /// Whether an action names a victim (and so must be resolved rather than
    /// merely rolled). The view uses this to decide if clicking the button
    /// enters target-pick mode.
    pub fn is_targeted(&self, action_key: &str) -> bool {
        self.actions
            .iter()
            .any(|a| a.key == action_key && a.target.is_some())
    }

    /// Adjudicate one action of `actor` against `target`, `distance` tiles away.
    ///
    /// This is the whole of "the app adjudicates". It rolls the attack, asks the
    /// system's script whether that roll lands, rolls the effect, and returns the
    /// typed consequences plus the beats that represent them. It is the *only*
    /// path from an intent to a change in game state.
    ///
    /// Determinism: every die comes from `rng`, so a fixed entropy tape yields a
    /// byte-identical `Resolution`. That is what lets one machine resolve and
    /// every other machine merely apply.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve_action(
        &mut self,
        action_key: &str,
        actor: TokenId,
        actor_sheet: &SheetData,
        actor_at: TileCoord,
        target: TokenId,
        target_sheet: &SheetData,
        target_at: TileCoord,
        rng: &mut Rng,
    ) -> Result<Resolution, ActionError> {
        // The rules see where the two of them are standing, not merely how far
        // apart. Reach needs the distance; a shove needs the direction; flanking
        // and backstabs would need both, and can now have them.
        let distance = isometry_core::distance(actor_at, target_at);
        if actor == target {
            return Err(ActionError::SelfTarget);
        }
        // Checked before anything is rolled: a corpse is not a target, so a swing
        // at one costs nothing and changes nothing.
        if self.is_defeated(target_sheet) {
            return Err(ActionError::AlreadyDefeated);
        }
        let Some(def) = self.actions.iter().find(|a| a.key == action_key) else {
            return Err(ActionError::UnknownAction(action_key.to_owned()));
        };
        let Some(spec) = def.target.as_ref() else {
            return Err(ActionError::NotTargeted(action_key.to_owned()));
        };
        if distance > spec.range {
            return Err(ActionError::OutOfRange {
                range: spec.range,
                distance,
            });
        }
        // Copy out of the borrow so the Lua calls can take `&mut self`.
        let (base, func) = (def.base.clone(), def.func.clone());
        let spec = TargetSpec {
            range: spec.range,
            hit_func: spec.hit_func.clone(),
            damage: spec.damage.clone(),
            damage_func: spec.damage_func.clone(),
            damage_mult_func: spec.damage_mult_func.clone(),
            damage_field: spec.damage_field.clone(),
            actor_beat: spec.actor_beat.clone(),
            hit_beat: spec.hit_beat.clone(),
            miss_beat: spec.miss_beat.clone(),
            fall_beat: spec.fall_beat.clone(),
            stagger_func: spec.stagger_func.clone(),
            stagger_beat: spec.stagger_beat.clone(),
            push_func: spec.push_func.clone(),
            push_beat: spec.push_beat.clone(),
            condition_on_hit: spec.condition_on_hit.clone(),
            condition_value_func: spec.condition_value_func.clone(),
            recruit_on_hit: spec.recruit_on_hit,
            afford_func: spec.afford_func.clone(),
            turn_effect: spec.turn_effect.clone(),
        };
        let by = actor_sheet.text("name").unwrap_or("?").to_owned();

        // Can the actor afford it, given its per-turn counters (injected into
        // the sheet by the host)? Checked before any die, so an unaffordable
        // action -- out of actions this turn -- costs nothing. The rule is the
        // system's; the substrate only stored the counters it reads.
        if let Some(afford) = spec.afford_func.clone() {
            let ok = self
                .call_int(&afford, actor_sheet)
                .ok_or_else(|| ActionError::ScriptFailed(afford.clone()))?
                != 0;
            if !ok {
                return Err(ActionError::CannotAfford(action_key.to_owned()));
            }
        }

        // 1. The attack: base die plus the actor's Lua bonus.
        let bonus = self
            .call_int(&func, actor_sheet)
            .ok_or_else(|| ActionError::ScriptFailed(func.clone()))?;
        let (raw, dice) = roll(&base, rng).ok_or_else(|| ActionError::BadDice(base.clone()))?;
        let total = raw + bonus as i32;
        let attack = RollRecord {
            by: by.clone(),
            expr: format!("{base}{bonus:+}"),
            dice,
            total,
        };

        // 2. The verdict. The script owns it, seeing both sheets and the roll,
        //    so "beats AC" is a rule and not a Rust branch.
        // The verdict sees the total *and* the natural die, so a script can
        // treat a 20 or a 1 specially (5e crits on a natural 20; PF2e shifts the
        // degree one rung either way). A single-die base gives `Some(die)`; a
        // multi-die base has no one natural roll, so it gives nil.
        let natural = (attack.dice.len() == 1).then(|| attack.dice[0] as i64);
        let degree = self
            .call_int_ctx2(
                &spec.hit_func,
                actor_sheet,
                Some(target_sheet),
                Some(total as i64),
                natural,
            )
            .ok_or_else(|| ActionError::ScriptFailed(spec.hit_func.clone()))?;
        // Anything at or above a plain success landed. A binary system returns
        // 1 or 0 and this is exactly its old meaning; a four-tier one also says
        // *how well*, and the multiplier below is what reads that.
        let hit = degree >= 1;

        // 3. The consequence.
        let mut damage = None;
        let mut deltas = Vec::new();
        if hit {
            let dmg_bonus = self
                .call_int_ctx(&spec.damage_func, actor_sheet, Some(target_sheet), None)
                .ok_or_else(|| ActionError::ScriptFailed(spec.damage_func.clone()))?;
            let (dmg_raw, dmg_dice) = roll(&spec.damage, rng)
                .ok_or_else(|| ActionError::BadDice(spec.damage.clone()))?;
            // The degree scales the whole effect, dice and modifiers together:
            // a PF2e critical doubles it, a 5e save-for-half halves it. Percent
            // because the Lua boundary carries integers; 100 when unspecified.
            let mult = match spec.damage_mult_func.as_ref() {
                Some(func) => self
                    .call_int_ctx(func, actor_sheet, Some(target_sheet), Some(degree))
                    .ok_or_else(|| ActionError::ScriptFailed(func.clone()))?,
                None => 100,
            };
            // Damage never heals: a big negative modifier floors at zero rather
            // than restoring the victim.
            let dmg_total = (dmg_raw + dmg_bonus as i32).max(0);
            let dmg_total = ((dmg_total as i64 * mult.max(0)) / 100) as i32;
            damage = Some(RollRecord {
                by,
                expr: if mult == 100 {
                    format!("{}{dmg_bonus:+}", spec.damage)
                } else {
                    format!("({}{dmg_bonus:+}) x{mult}%", spec.damage)
                },
                dice: dmg_dice,
                total: dmg_total,
            });
            // Only touch the target when there is damage to do. A zero (a shove,
            // a convince, a trip) must not push a `field -= 0` delta: applying it
            // to a sheet that lacks the field would *create* it at 0, and a
            // hp_current invented at 0 then reads as defeated.
            if dmg_total > 0 {
                deltas.push(SheetDelta {
                    token: target,
                    key: spec.damage_field.clone(),
                    add: -(dmg_total as i64),
                });
            }
        }

        // 4. Did that put anyone down? Ask the system, against the sheet as it
        //    will be *after* the deltas land rather than as it is now. The
        //    substrate is never consulted and never has to understand the answer.
        let mut defeated = Vec::new();
        if hit {
            let mut after = target_sheet.clone();
            for delta in deltas.iter().filter(|d| d.token == target) {
                after.add_int(&delta.key, delta.add);
            }
            if self.is_defeated(&after) {
                defeated.push(target);
            }
        }

        // 5. Force. Two different things share a direction and must not be
        //    confused: how far the blow *actually* moves the victim (truth, and
        //    so replicated and geometry-bound), and whether it merely rocks it
        //    off its feet (representation, and so free).
        let dealt = damage.as_ref().map_or(0, |d| d.total) as i64;
        let step = isometry_core::away(actor_at, target_at);
        let push_tiles = match (&spec.push_func, hit) {
            (Some(func), true) => self
                .call_int_ctx(func, actor_sheet, Some(target_sheet), Some(dealt))
                .ok_or_else(|| ActionError::ScriptFailed(func.clone()))?
                .max(0) as u32,
            _ => 0,
        };
        let staggered = match (&spec.stagger_func, hit) {
            (Some(func), true) => {
                self.call_int_ctx(func, actor_sheet, Some(target_sheet), Some(dealt))
                    .ok_or_else(|| ActionError::ScriptFailed(func.clone()))?
                    != 0
            }
            _ => false,
        };

        // 6. The representation. The beat follows the outcome, which is how a
        //    richer vocabulary grows later without this code changing at all.
        let dir = isometry_core::compass(step);
        let suffix = |base: &str| match dir {
            Some(d) => format!("{base}-{d}"),
            None => base.to_owned(),
        };
        let victim_beat = if !defeated.is_empty() {
            // Dropped where it stood. A corpse does not stagger.
            spec.fall_beat.clone()
        } else if push_tiles > 0 {
            // Actually moved: the board will place it on the new tile, so the
            // beat slides it in from the old one.
            suffix(&spec.push_beat)
        } else if staggered {
            // Rocked back and recovers. Nothing moved.
            suffix(&spec.stagger_beat)
        } else if hit {
            spec.hit_beat.clone()
        } else {
            spec.miss_beat.clone()
        };
        let beats = vec![
            Beat::new(actor, spec.actor_beat.clone()),
            Beat::new(target, victim_beat),
        ];

        // 7. Conditions. A corpse does not need `prone` on top of being dead,
        //    and re-applying a condition the victim already has is noise (the
        //    caller passes the target sheet already carrying its condition
        //    booleans, so "already has it" is one field read).
        let mut conditions = Vec::new();
        let mut mobility = Vec::new();
        if hit && defeated.is_empty() {
            if let Some(name) = spec.condition_on_hit.clone() {
                // The magnitude: a value func riding the degree ladder (PF2e's
                // Demoralize -> frightened 2 on a crit, 1 on a plain success), or
                // a plain 1 for an on/off condition. The substrate stores
                // whatever number the rules pick; nothing here knows "frightened"
                // from "prone", or that 2 is worse than 1.
                let value = match spec.condition_value_func.as_ref() {
                    Some(f) => self
                        .call_int_ctx(f, actor_sheet, Some(target_sheet), Some(degree))
                        .unwrap_or(0),
                    None => 1,
                };
                // Applying on a hit can only *worsen* a condition, never lift it:
                // re-tripping the prone is noise, and a weaker fear must not undo
                // a stronger one already in force. Clearing is a separate path
                // (standing up). The current magnitude is one field read, because
                // the caller passed the target sheet already carrying it.
                let current = match target_sheet.fields.get(&name) {
                    Some(FieldValue::Int(n)) => *n,
                    _ => 0,
                };
                if value > current {
                    conditions.push((target, name.clone(), value));
                    // The mobility projection, computed against the sheet as it
                    // will be: carrying the condition at its new magnitude.
                    let conditioned =
                        sheet_with_conditions(target_sheet, std::iter::once((&name, &value)));
                    mobility.push((target, self.mobility_for(&conditioned, true)));
                }
            }
        }

        // A hit that wins the target over. Not a corpse (you cannot recruit the
        // dead) and not something you already command. The host takes it from
        // here: allegiance is an owner change, and owners are the map's, not the
        // rules'.
        let recruited = if hit && defeated.is_empty() && spec.recruit_on_hit {
            Some(target)
        } else {
            None
        };

        let turn_counters: Vec<(TokenId, String, i64)> = spec
            .turn_effect
            .iter()
            .map(|(key, delta)| (actor, key.clone(), *delta))
            .collect();

        Ok(Resolution {
            attack,
            degree,
            hit,
            damage,
            deltas,
            beats,
            defeated,
            push: (push_tiles > 0).then_some((step, push_tiles)),
            conditions,
            mobility,
            recruited,
            turn_counters,
        })
    }
}
