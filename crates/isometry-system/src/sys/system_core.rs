//! Loading a system and the sheet projections it drives.
//!
//! `System` is the plugin seam: schema plus scripted rules. The substrate never
//! learns what a hit point is, so everything numeric here comes back from the
//! plugin's Lua.
//!
//! Split out of `lib.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl System {
    /// Build a system, loading `script` (which defines the derived/action
    /// functions) into a fresh sandboxed Lua.
    pub fn load(
        id: impl Into<String>,
        name: impl Into<String>,
        fields: Vec<FieldDef>,
        derived: Vec<DerivedDef>,
        actions: Vec<ActionDef>,
        script: &str,
    ) -> Result<Self, String> {
        let mut lua = Lua::core();
        let ex = lua
            .try_enter(|ctx| {
                let closure = Closure::load(ctx, Some("system"), script.as_bytes())?;
                Ok(ctx.stash(Executor::start(ctx, closure.into(), ())))
            })
            .map_err(|e| format!("load system script: {e}"))?;
        lua.execute::<()>(&ex)
            .map_err(|e| format!("run system script: {e}"))?;
        Ok(Self {
            id: id.into(),
            name: name.into(),
            fields,
            derived,
            actions,
            defeat_func: None,
            speed_func: None,
            sight_func: None,
            nav_func: None,
            toll_func: None,
            encounter_func: None,
            map_read_func: None,
            forage_func: None,
            lua,
        })
    }

    /// Declare the system's out-of-play rule: a Lua `f(c) -> 1|0`. Systems
    /// without a notion of defeat simply never call this.
    pub fn with_defeat(mut self, func: impl Into<String>) -> Self {
        self.defeat_func = Some(func.into());
        self
    }

    /// Declare the movement and sight projections.
    pub fn with_mobility(
        mut self,
        speed_func: impl Into<String>,
        sight_func: impl Into<String>,
    ) -> Self {
        self.speed_func = Some(speed_func.into());
        self.sight_func = Some(sight_func.into());
        self
    }

    /// Declare the overmap navigation rule: a Lua `f(c, t, roll, weight) ->
    /// percent`. A system with no wilderness travel simply never calls this.
    pub fn with_nav(mut self, func: impl Into<String>) -> Self {
        self.nav_func = Some(func.into());
        self
    }

    /// Declare the march-toll rule: a Lua `f(c, t, ticks) -> exhaustion`. A
    /// system with no attrition simply never calls this.
    pub fn with_toll(mut self, func: impl Into<String>) -> Self {
        self.toll_func = Some(func.into());
        self
    }

    /// Declare the wandering-encounter rule: a Lua `f(c, t, ticks) -> 1|0`. A
    /// system with safe roads simply never calls this.
    pub fn with_encounters(mut self, func: impl Into<String>) -> Self {
        self.encounter_func = Some(func.into());
        self
    }

    /// Declare the map-reading rule: a Lua `f(c, t, roll) -> 1|0`. A system where
    /// anyone can read a map simply never calls this.
    pub fn with_map_reading(mut self, func: impl Into<String>) -> Self {
        self.map_read_func = Some(func.into());
        self
    }

    /// Declare the foraging rule: a Lua `f(c, t, roll) -> food`. A system where
    /// travel gathers no food simply never calls this.
    pub fn with_foraging(mut self, func: impl Into<String>) -> Self {
        self.forage_func = Some(func.into());
        self
    }

    /// Can `reader` make sense of a map? Rolls the reader's literacy/skill check;
    /// on a pass the host reveals what the map shows, on a failure it learns
    /// nothing -- an unskilled party holds a map it cannot use. With no
    /// `map_read_func`, any reader succeeds.
    pub fn read_map(&mut self, reader: &SheetData, rng: &mut Rng) -> bool {
        let Some(func) = self.map_read_func.clone() else {
            return true;
        };
        let (raw, _) = roll("1d20", rng).unwrap_or((0, vec![0]));
        self.call_int_ctx(&func, reader, None, Some(raw as i64))
            .unwrap_or(0)
            != 0
    }

    /// Resolve one leg of overmap travel: roll the party's navigator against the
    /// route, and rule how long it takes. The base time is E1's cost (the route
    /// weight scaled by pace); the system's `nav_func` decides whether the party
    /// travels it smoothly or loses the way and pays more. The travel analogue of
    /// [`Self::resolve_action`]: the system judges once, and the substrate applies
    /// the ticks and moves the party. Absent `nav_func`, the party always finds
    /// its way at base cost.
    pub fn resolve_travel(
        &mut self,
        navigator: &SheetData,
        weight: u32,
        pace: i64,
        rng: &mut Rng,
    ) -> TravelResolution {
        let (raw, dice) = roll("1d20", rng).unwrap_or((0, vec![0]));
        let base = ((weight as u64 * pace.max(1) as u64) / 100).max(1);
        let nav_pct = match self.nav_func.clone() {
            Some(func) => self
                .call_int_ctx2(&func, navigator, None, Some(raw as i64), Some(weight as i64))
                .unwrap_or(100),
            None => 100,
        };
        let ticks = ((base * nav_pct.max(0) as u64) / 100).max(1);
        // The toll of the march: how tired `ticks` of travel leaves the party.
        // The system reads the *actual* time (lost trips are longer and tire
        // more), so exhaustion follows navigation without the toll rule knowing
        // it. Absent `toll_func`, travel never tires.
        let exhaustion = match self.toll_func.clone() {
            Some(func) => self
                .call_int_ctx(&func, navigator, None, Some(ticks as i64))
                .unwrap_or(0)
                .max(0),
            None => 0,
        };
        // Did the road throw a peril? A fresh d20 makes it a *chance*, and the
        // check reads the time too, so a longer (or lost) trip is more dangerous.
        // A safe-road system that declares no rule never rolls one.
        let encounter = match self.encounter_func.clone() {
            Some(func) => {
                let (peril_roll, _) = roll("1d20", rng).unwrap_or((0, vec![0]));
                self.call_int_ctx2(&func, navigator, None, Some(peril_roll as i64), Some(ticks as i64))
                    .unwrap_or(0)
                    != 0
            }
            None => false,
        };
        // What the party foraged on the road. The rule reads the navigator's own
        // stance, so it yields only when Foraging; a fresh d20 is the check.
        let forage = match self.forage_func.clone() {
            Some(func) => {
                let (forage_roll, _) = roll("1d20", rng).unwrap_or((0, vec![0]));
                self.call_int_ctx(&func, navigator, None, Some(forage_roll as i64))
                    .unwrap_or(0)
                    .max(0)
            }
            None => 0,
        };
        TravelResolution {
            roll: RollRecord {
                by: navigator.text("name").unwrap_or("party").to_owned(),
                expr: "1d20".to_owned(),
                dice,
                total: raw,
            },
            ticks,
            lost: nav_pct > 100,
            exhaustion,
            encounter,
            forage,
        }
    }

    /// The system's mechanical ruling for a character *as conditioned*: pass a
    /// sheet already augmented via [`sheet_with_conditions`]. Returns `None`
    /// when the system declares no projection, or when no condition is in force
    /// (the base numbers stand, so the substrate stores no override at all).
    pub fn mobility_for(
        &mut self,
        conditioned: &SheetData,
        any_condition: bool,
    ) -> Option<(u32, u32)> {
        if !any_condition {
            return None;
        }
        let (speed_func, sight_func) =
            (self.speed_func.clone()?, self.sight_func.clone()?);
        let speed = self.call_int(&speed_func, conditioned)?.max(0) as u32;
        let sight = self.call_int(&sight_func, conditioned)?.max(0) as u32;
        Some((speed, sight))
    }

    /// Ask the system whether `sheet` is out of play. False when the system
    /// declares no such rule.
    pub fn is_defeated(&mut self, sheet: &SheetData) -> bool {
        let Some(func) = self.defeat_func.clone() else {
            return false;
        };
        self.call_int(&func, sheet).is_some_and(|v| v != 0)
    }

    /// A fresh sheet with the schema's default field values.
    pub fn default_sheet(&self) -> SheetData {
        let mut sheet = SheetData::new(&self.id);
        for f in &self.fields {
            sheet.fields.insert(f.key.clone(), f.default.clone());
        }
        sheet
    }

    /// Build a transient rules input from a stored sheet plus its equipped
    /// public items. Modifier stat keys belong to the system/pack vocabulary;
    /// integer fields add cumulatively, while unsupported field types are left
    /// unchanged. The stored sheet never absorbs equipment bonuses.
    pub fn effective_sheet(&self, sheet: &SheetData, inventory: Option<&Inventory>) -> SheetData {
        let mut effective = sheet.clone();
        let Some(inventory) = inventory else {
            return effective;
        };
        for item_id in inventory.equipped.values() {
            let Some(item) = inventory.items.get(item_id) else {
                continue;
            };
            for modifier in &item.modifiers {
                for (key, bonus) in &modifier.stats {
                    match effective.fields.get_mut(key) {
                        Some(FieldValue::Int(value)) => *value += bonus,
                        None => {
                            effective
                                .fields
                                .insert(key.clone(), FieldValue::Int(*bonus));
                        }
                        Some(_) => {}
                    }
                }
            }
        }
        effective
    }

    /// Call a Lua function `func(character)` returning an int.
    pub(crate) fn call_int(&mut self, func: &str, sheet: &SheetData) -> Option<i64> {
        self.call_int_ctx(func, sheet, None, None)
    }

    /// Call `func(c, t, n) -> int`, where `t` is an optional target sheet and
    /// `n` an optional scalar (the actor's total roll).
    ///
    /// Lua discards arguments a function does not declare, so the existing
    /// one-argument scripts (`m_str(c)`, `a_attack(c)`) are unaffected by the
    /// extra parameters, while a targeted script can read `t.ac`. That is the
    /// whole ABI widening: no tagged returns, no new marshalling, one call path.
    pub(crate) fn call_int_ctx(
        &mut self,
        func: &str,
        sheet: &SheetData,
        target: Option<&SheetData>,
        extra: Option<i64>,
    ) -> Option<i64> {
        self.call_int_ctx2(func, sheet, target, extra, None)
    }

    /// `f(c, t, n, m)`: the same call with a second scalar. Lua discards
    /// arguments a function does not declare, so every existing `f(c, t, n)`
    /// script is unaffected by the extra slot.
    pub(crate) fn call_int_ctx2(
        &mut self,
        func: &str,
        sheet: &SheetData,
        target: Option<&SheetData>,
        extra: Option<i64>,
        extra2: Option<i64>,
    ) -> Option<i64> {
        // The `try_enter` closure is higher-ranked over `'gc`, so it can
        // capture only owned data; copy the sheets and the name in.
        let func = func.to_owned();
        let own = |s: &SheetData| -> Vec<(String, FieldValue)> {
            s.fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        let fields = own(sheet);
        let target_fields = target.map(own);
        let ex = self
            .lua
            .try_enter(move |ctx| {
                // Intern each key into a `'gc` Lua string so no borrow of the
                // owned field vectors crosses the higher-ranked `'gc` boundary.
                let build = |fields: &[(String, FieldValue)]| -> Result<Table<'_>, piccolo::Error<'_>> {
                    let table = Table::new(&ctx);
                    for (k, v) in fields {
                        let key = piccolo::String::from_slice(&ctx, k.as_bytes());
                        match v {
                            FieldValue::Int(n) => table.set(ctx, key, *n)?,
                            FieldValue::Bool(b) => table.set(ctx, key, *b)?,
                            FieldValue::Text(s) => {
                                let ls = piccolo::String::from_slice(&ctx, s.as_bytes());
                                table.set(ctx, key, ls)?
                            }
                            FieldValue::Float(f) => table.set(ctx, key, *f)?,
                            // Nested values reach Lua with the W2 generator
                            // ABI (worldbuilding plan); scalar rules don't
                            // see them yet.
                            FieldValue::List(_) | FieldValue::Map(_) => Value::Nil,
                        };
                    }
                    Ok(table)
                };
                let table = build(&fields)?;
                let t = match &target_fields {
                    Some(f) => Value::Table(build(f)?),
                    None => Value::Nil,
                };
                let n = match extra {
                    Some(n) => Value::Integer(n),
                    None => Value::Nil,
                };
                let m = match extra2 {
                    Some(m) => Value::Integer(m),
                    None => Value::Nil,
                };
                let fname = piccolo::String::from_slice(&ctx, func.as_bytes());
                let f = ctx.globals().get(ctx, fname);
                let Value::Function(f) = f else {
                    return Err("not a function".into_value(ctx).into());
                };
                Ok(ctx.stash(Executor::start(ctx, f, (table, t, n, m))))
            })
            .ok()?;
        self.lua.execute::<i64>(&ex).ok()
    }

}
