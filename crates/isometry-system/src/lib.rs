//! Isometry's system-plugin lane.
//!
//! A game **system** is a schema (what fields a character has) plus Lua
//! scripts (how derived stats compute and what an action rolls). The
//! substrate stores [`SheetData`](isometry_core::SheetData); this crate
//! interprets it. The scripting engine is piccolo (pure-Rust Lua),
//! sandboxed, behind the [`System`] type so a host never touches Lua
//! directly.
//!
//! The Lua boundary is deliberately narrow: every script function takes a
//! character table and returns an **integer** (a modifier or bonus). The
//! dice expression an action rolls is assembled in Rust (`base` die +
//! signed bonus), so no Lua string ever has to cross the GC boundary.
//! Rich content generators can loosen this later; formulas do not need
//! it.

use std::collections::BTreeMap;

use isometry_core::{FieldValue, SheetData};
use piccolo::{Closure, Executor, IntoValue, Lua, Table, Value};

/// A schema field: an editable value on the sheet.
pub struct FieldDef {
    pub key: String,
    pub label: String,
    pub default: FieldValue,
}

/// A derived stat: a display value computed by a Lua function of the
/// sheet (e.g. an ability modifier).
pub struct DerivedDef {
    pub key: String,
    pub label: String,
    /// Lua function name; takes the character table, returns an int.
    pub func: String,
}

/// An action: a roll of `base` (a dice expression) plus a Lua-computed
/// bonus (e.g. attack = `1d20` + str-mod + proficiency).
pub struct ActionDef {
    pub key: String,
    pub label: String,
    pub base: String,
    /// Lua function name; takes the character table, returns the bonus.
    pub func: String,
}

/// A loaded game system: schema + a live sandboxed Lua interpreter with
/// the system's script defining its functions.
pub struct System {
    pub id: String,
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub derived: Vec<DerivedDef>,
    pub actions: Vec<ActionDef>,
    lua: Lua,
}

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
            lua,
        })
    }

    /// A fresh sheet with the schema's default field values.
    pub fn default_sheet(&self) -> SheetData {
        let mut sheet = SheetData::new(&self.id);
        for f in &self.fields {
            sheet.fields.insert(f.key.clone(), f.default.clone());
        }
        sheet
    }

    /// Call a Lua function `func(character)` returning an int.
    fn call_int(&mut self, func: &str, sheet: &SheetData) -> Option<i64> {
        // The `try_enter` closure is higher-ranked over `'gc`, so it can
        // capture only owned data; copy the sheet fields and the name in.
        let func = func.to_owned();
        let fields: Vec<(String, FieldValue)> = sheet
            .fields
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let ex = self
            .lua
            .try_enter(move |ctx| {
                let table = Table::new(&ctx);
                for (k, v) in &fields {
                    // Intern the key into a `'gc` Lua string so no borrow
                    // of `fields` crosses the higher-ranked `'gc` boundary.
                    let key = piccolo::String::from_slice(&ctx, k.as_bytes());
                    match v {
                        FieldValue::Int(n) => table.set(ctx, key, *n)?,
                        FieldValue::Bool(b) => table.set(ctx, key, *b)?,
                        FieldValue::Text(s) => {
                            let ls = piccolo::String::from_slice(&ctx, s.as_bytes());
                            table.set(ctx, key, ls)?
                        }
                    };
                }
                let fname = piccolo::String::from_slice(&ctx, func.as_bytes());
                let f = ctx.globals().get(ctx, fname);
                let Value::Function(f) = f else {
                    return Err("not a function".into_value(ctx).into());
                };
                Ok(ctx.stash(Executor::start(ctx, f, (table,))))
            })
            .ok()?;
        self.lua.execute::<i64>(&ex).ok()
    }

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
}

/// The 5e SRD system (CC-BY-4.0 material): six ability scores, level,
/// proficiency, HP, AC. Derived: the six ability modifiers. Actions: an
/// attack (d20 + str-mod + proficiency) and a d20 check per ability.
pub fn srd_5e() -> System {
    let ability = |key: &str, label: &str| FieldDef {
        key: key.to_owned(),
        label: label.to_owned(),
        default: FieldValue::Int(10),
    };
    let fields = vec![
        FieldDef {
            key: "name".to_owned(),
            label: "Name".to_owned(),
            default: FieldValue::Text("Hero".to_owned()),
        },
        ability("str", "STR"),
        ability("dex", "DEX"),
        ability("con", "CON"),
        ability("int", "INT"),
        ability("wis", "WIS"),
        ability("cha", "CHA"),
        FieldDef {
            key: "prof".to_owned(),
            label: "Proficiency".to_owned(),
            default: FieldValue::Int(2),
        },
        FieldDef {
            key: "level".to_owned(),
            label: "Level".to_owned(),
            default: FieldValue::Int(1),
        },
        FieldDef {
            key: "hp".to_owned(),
            label: "HP".to_owned(),
            default: FieldValue::Int(10),
        },
        FieldDef {
            key: "ac".to_owned(),
            label: "AC".to_owned(),
            default: FieldValue::Int(12),
        },
    ];
    let m = |ab: &str| DerivedDef {
        key: format!("{ab}_mod"),
        label: format!("{} mod", ab.to_uppercase()),
        func: format!("m_{ab}"),
    };
    let derived = vec![
        m("str"),
        m("dex"),
        m("con"),
        m("int"),
        m("wis"),
        m("cha"),
    ];
    let check = |ab: &str| ActionDef {
        key: format!("{ab}_check"),
        label: format!("{} check", ab.to_uppercase()),
        base: "1d20".to_owned(),
        func: format!("m_{ab}"),
    };
    let actions = vec![
        ActionDef {
            key: "attack".to_owned(),
            label: "Attack".to_owned(),
            base: "1d20".to_owned(),
            func: "a_attack".to_owned(),
        },
        check("str"),
        check("dex"),
        check("con"),
        check("int"),
        check("wis"),
        check("cha"),
    ];
    // 5e ability modifier = floor((score - 10) / 2). piccolo's `//`
    // truncates toward zero rather than flooring, so normalize the
    // remainder to make the division exact (works for either sign
    // convention). Every function returns an integer.
    let script = r#"
        function ab_mod(s)
            local x = s - 10
            local r = ((x % 2) + 2) % 2
            return (x - r) // 2
        end
        function m_str(c) return ab_mod(c.str) end
        function m_dex(c) return ab_mod(c.dex) end
        function m_con(c) return ab_mod(c.con) end
        function m_int(c) return ab_mod(c.int) end
        function m_wis(c) return ab_mod(c.wis) end
        function m_cha(c) return ab_mod(c.cha) end
        function a_attack(c) return ab_mod(c.str) + c.prof end
    "#;
    System::load("5e-srd", "5e SRD", fields, derived, actions, script)
        .expect("builtin 5e system loads")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sheet_has_schema_defaults() {
        let sys = srd_5e();
        let sheet = sys.default_sheet();
        assert_eq!(sheet.system, "5e-srd");
        assert_eq!(sheet.int("str"), Some(10));
        assert_eq!(sheet.int("prof"), Some(2));
        assert_eq!(sheet.text("name"), Some("Hero"));
    }

    #[test]
    fn ability_modifiers_follow_5e() {
        let mut sys = srd_5e();
        let mut sheet = sys.default_sheet();
        sheet.set_int("str", 16); // +3
        sheet.set_int("dex", 7); //  -2 (floor)
        sheet.set_int("con", 10); //  0
        let d = sys.derived(&sheet);
        assert_eq!(d.get("str_mod"), Some(&3));
        assert_eq!(d.get("dex_mod"), Some(&-2));
        assert_eq!(d.get("con_mod"), Some(&0));
    }

    #[test]
    fn attack_expr_folds_str_mod_and_proficiency() {
        let mut sys = srd_5e();
        let mut sheet = sys.default_sheet();
        sheet.set_int("str", 18); // +4
        sheet.set_int("prof", 3);
        // 1d20 + 4 + 3 = 1d20+7
        assert_eq!(sys.action_expr("attack", &sheet).as_deref(), Some("1d20+7"));
        // A negative total still formats correctly.
        sheet.set_int("str", 6); // -2
        sheet.set_int("prof", 0);
        assert_eq!(sys.action_expr("attack", &sheet).as_deref(), Some("1d20-2"));
    }
}
