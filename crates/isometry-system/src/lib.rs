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
use std::path::{Path, PathBuf};

use isometry_campaign::{
    ContentPackManifest, EntropyTape, GenValue, GenerationRecord, GeneratorFixture,
    GeneratorRequest, Inventory,
};
use isometry_core::{FieldValue, SheetData};
use piccolo::{Closure, Executor, Fuel, IntoValue, Lua, StashedExecutor, Table, Value};

mod bestiary;
mod items;
mod spells;
pub use bestiary::{srd_bestiary, Monster, MonsterAction};
pub use items::{srd_items, Item};
pub use spells::{srd_spells, Spell};

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

/// Limits applied to every pack-generator invocation. These are host policy,
/// not content-pack metadata: a pack may ask for less work but cannot raise a
/// table's cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneratorLimits {
    pub fuel: i32,
    pub max_output_bytes: usize,
    pub max_value_depth: usize,
}

impl Default for GeneratorLimits {
    fn default() -> Self {
        Self {
            fuel: 4_096,
            max_output_bytes: 64 * 1024,
            max_value_depth: 16,
        }
    }
}

/// A bounded Piccolo host for one content pack's generator script.
///
/// The pack defines `call_gen(request_json, entropy, request) -> result_json`.
/// `request_json` preserves the stable serialized ABI, while `request` is its
/// structured Lua-table form: `{ generator, args, locks }`, where every value
/// retains the tagged [`GenValue`] shape. `entropy` is host-supplied and
/// recorded. The returned JSON decodes to a typed [`GenValue`]. This runtime
/// only makes proposals. It has no campaign, network, filesystem, or commit
/// capability.
pub struct GeneratorRuntime {
    lua: Lua,
    limits: GeneratorLimits,
}

/// The validated result of one generator call. The corresponding draw is also
/// appended to the supplied [`EntropyTape`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratorResult {
    pub value: GenValue,
    pub entropy: u64,
}

/// One content pack loaded from a directory. Its manifest declares every Lua
/// script and fixture the host may open; callers cannot point execution at an
/// arbitrary sibling file after the pack has been validated.
pub struct GeneratorPack {
    root: PathBuf,
    manifest: ContentPackManifest,
}

impl GeneratorPack {
    pub const MANIFEST_FILE: &'static str = "isometry-pack.json";

    /// Load a pack directory and validate its manifest before any generator
    /// assets are read. The canonical root also prevents a declared symlink
    /// from escaping the pack when the asset is opened.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, String> {
        let root = dir
            .as_ref()
            .canonicalize()
            .map_err(|error| format!("open content-pack root: {error}"))?;
        let manifest_path = root.join(Self::MANIFEST_FILE);
        let manifest_json = std::fs::read_to_string(&manifest_path)
            .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
        let manifest: ContentPackManifest = serde_json::from_str(&manifest_json)
            .map_err(|error| format!("parse {}: {error}", manifest_path.display()))?;
        manifest
            .validate()
            .map_err(|error| format!("validate {}: {error}", manifest_path.display()))?;
        Ok(Self { root, manifest })
    }

    pub fn manifest(&self) -> &ContentPackManifest {
        &self.manifest
    }

    /// Load a bounded runtime for the generator named by a fully-qualified
    /// request id such as `demo:forge_item`.
    pub fn runtime_for(
        &self,
        request: &GeneratorRequest,
        limits: GeneratorLimits,
    ) -> Result<GeneratorRuntime, String> {
        let entry = self
            .manifest
            .generator(&request.generator)
            .ok_or_else(|| format!("generator is not declared by this pack: {}", request.generator))?;
        let script = self.read_asset(&entry.script)?;
        GeneratorRuntime::load(&script, limits)
    }

    /// Evaluate one declared generator into a public commit-result record.
    /// The desktop host owns the tape and then passes this record to
    /// `HostSession::commit_generation`; the net crate deliberately does not
    /// depend on this Lua runtime.
    pub fn generate(
        &self,
        record_id: impl Into<String>,
        request: &GeneratorRequest,
        tape: &mut EntropyTape,
        limits: GeneratorLimits,
    ) -> Result<GenerationRecord, String> {
        let mut runtime = self.runtime_for(request, limits)?;
        let result = runtime.call(request, tape)?;
        let record = GenerationRecord {
            id: record_id.into(),
            request: request.clone(),
            entropy: result.entropy,
            proposal: result.value,
        };
        record
            .validate(limits.max_value_depth)
            .map_err(|error| format!("validate generation record: {error}"))?;
        Ok(record)
    }

    /// Load and run a fixture declared for one pack generator. The fixture's
    /// request must name that same fully-qualified generator, keeping fixtures
    /// from silently testing a script they do not describe.
    pub fn run_fixture(
        &self,
        generator: &str,
        fixture_path: &str,
        limits: GeneratorLimits,
    ) -> Result<(), String> {
        let entry = self
            .manifest
            .generator(generator)
            .ok_or_else(|| format!("generator is not declared by this pack: {generator}"))?;
        if !entry.fixtures.iter().any(|fixture| fixture == fixture_path) {
            return Err(format!(
                "fixture is not declared for generator {generator}: {fixture_path}"
            ));
        }
        let fixture_json = self.read_asset(fixture_path)?;
        let fixture: GeneratorFixture = serde_json::from_str(&fixture_json)
            .map_err(|error| format!("parse fixture {fixture_path}: {error}"))?;
        if fixture.request.generator != generator {
            return Err(format!(
                "fixture {fixture_path} names {}, expected {generator}",
                fixture.request.generator
            ));
        }
        let mut runtime = self.runtime_for(&fixture.request, limits)?;
        runtime.run_fixture(&fixture)
    }

    fn read_asset(&self, relative: &str) -> Result<String, String> {
        let path = self.root.join(relative);
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("open pack asset {relative}: {error}"))?;
        if !canonical.starts_with(&self.root) {
            return Err(format!("pack asset escapes root: {relative}"));
        }
        std::fs::read_to_string(&canonical)
            .map_err(|error| format!("read {}: {error}", canonical.display()))
    }
}

impl GeneratorRuntime {
    pub fn load(script: &str, limits: GeneratorLimits) -> Result<Self, String> {
        if limits.fuel <= 0 {
            return Err("generator fuel must be positive".to_owned());
        }
        let mut lua = Lua::core();
        let ex = lua
            .try_enter(|ctx| {
                let closure = Closure::load(ctx, Some("generator"), script.as_bytes())?;
                Ok(ctx.stash(Executor::start(ctx, closure.into(), ())))
            })
            .map_err(|e| format!("load generator script: {e}"))?;
        execute_bounded::<()>(&mut lua, &ex, limits.fuel)?;
        Ok(Self { lua, limits })
    }

    /// Execute a generator once with one host-owned entropy draw. Lua receives
    /// the draw as an `i64`, hence the high bit is cleared without changing the
    /// deterministic tape record.
    pub fn call(
        &mut self,
        request: &GeneratorRequest,
        tape: &mut EntropyTape,
    ) -> Result<GeneratorResult, String> {
        let args = serde_json::to_string(request)
            .map_err(|e| format!("serialize generator request: {e}"))?;
        let entropy = tape.draw();
        let lua_entropy = (entropy >> 1) as i64;
        let ex = self
            .lua
            .try_enter(move |ctx| {
                let request_table = generator_request_table(ctx, &request);
                let name = piccolo::String::from_slice(&ctx, b"call_gen");
                let Value::Function(function) = ctx.globals().get(ctx, name) else {
                    return Err("generator script does not define call_gen"
                        .into_value(ctx)
                        .into());
                };
                Ok(ctx.stash(Executor::start(
                    ctx,
                    function,
                    (args, lua_entropy, request_table),
                )))
            })
            .map_err(|e| format!("start generator: {e}"))?;
        let output: String = execute_bounded(&mut self.lua, &ex, self.limits.fuel)?;
        if output.len() > self.limits.max_output_bytes {
            return Err(format!(
                "generator output exceeds {} byte limit",
                self.limits.max_output_bytes
            ));
        }
        let value: GenValue = serde_json::from_str(&output)
            .map_err(|e| format!("generator returned invalid GenValue JSON: {e}"))?;
        value
            .validate_depth(self.limits.max_value_depth)
            .map_err(|e| format!("generator returned invalid GenValue: {e}"))?;
        Ok(GeneratorResult { value, entropy })
    }

    /// Run one authored fixture without any campaign state. Both the proposal
    /// and entropy trace must match, so a changed number/order of random draws
    /// is visible even when it happens to produce the same proposal text.
    pub fn run_fixture(&mut self, fixture: &GeneratorFixture) -> Result<(), String> {
        let mut tape = EntropyTape::from_seed(fixture.seed);
        let result = self.call(&fixture.request, &mut tape)?;
        if result.value != fixture.expected {
            return Err(format!(
                "fixture {} returned a different proposal",
                fixture.name
            ));
        }
        if tape.draws != fixture.expected_draws {
            return Err(format!(
                "fixture {} recorded different entropy",
                fixture.name
            ));
        }
        Ok(())
    }
}

/// Marshal one generator request into deterministic, tagged Lua data. Tables
/// are built from `BTreeMap` iteration and list order, so pack code sees the
/// same structure for a given request on every host.
fn generator_request_table<'gc>(
    ctx: piccolo::Context<'gc>,
    request: &GeneratorRequest,
) -> Table<'gc> {
    let table = Table::new(&ctx);
    set_lua_string(table, ctx, "generator", &request.generator);
    table
        .set(ctx, "args", gen_value_table(ctx, &request.args))
        .expect("static generator request key is valid");
    let locks = Table::new(&ctx);
    for (key, value) in &request.locks {
        locks
            .set(ctx, lua_string(ctx, key), gen_value_table(ctx, value))
            .expect("non-empty lock keys are valid Lua table keys");
    }
    table
        .set(ctx, "locks", locks)
        .expect("static generator request key is valid");
    table
}

fn gen_value_table<'gc>(ctx: piccolo::Context<'gc>, value: &GenValue) -> Table<'gc> {
    let table = Table::new(&ctx);
    match value {
        GenValue::Text { value } => {
            table.set(ctx, "type", "text").unwrap();
            set_lua_string(table, ctx, "value", value);
        }
        GenValue::Object { fields } => {
            table.set(ctx, "type", "object").unwrap();
            let values = Table::new(&ctx);
            for (key, value) in fields {
                values
                    .set(ctx, lua_string(ctx, key), gen_value_table(ctx, value))
                    .unwrap();
            }
            table.set(ctx, "fields", values).unwrap();
        }
        GenValue::List { values } => {
            table.set(ctx, "type", "list").unwrap();
            let list = Table::new(&ctx);
            for (index, value) in values.iter().enumerate() {
                list.set(ctx, index as i64 + 1, gen_value_table(ctx, value))
                    .unwrap();
            }
            table.set(ctx, "values", list).unwrap();
        }
        GenValue::Item { item } => {
            table.set(ctx, "type", "item").unwrap();
            let item_table = Table::new(&ctx);
            set_lua_string(item_table, ctx, "template", &item.template);
            set_lua_string(item_table, ctx, "name", &item.name);
            item_table.set(ctx, "tags", lua_strings(ctx, &item.tags)).unwrap();
            table.set(ctx, "item", item_table).unwrap();
        }
        GenValue::Npc { npc } => {
            table.set(ctx, "type", "npc").unwrap();
            let npc_table = Table::new(&ctx);
            set_lua_string(npc_table, ctx, "key", &npc.key);
            set_lua_string(npc_table, ctx, "name", &npc.name);
            npc_table.set(ctx, "tags", lua_strings(ctx, &npc.tags)).unwrap();
            table.set(ctx, "npc", npc_table).unwrap();
        }
        GenValue::MapPatch { patch } => {
            table.set(ctx, "type", "map_patch").unwrap();
            let patch_table = Table::new(&ctx);
            set_lua_string(patch_table, ctx, "target", &patch.target);
            let operations = Table::new(&ctx);
            for (index, value) in patch.operations.iter().enumerate() {
                operations
                    .set(ctx, index as i64 + 1, gen_value_table(ctx, value))
                    .unwrap();
            }
            patch_table.set(ctx, "operations", operations).unwrap();
            table.set(ctx, "patch", patch_table).unwrap();
        }
        GenValue::WorldFact { fact } => {
            table.set(ctx, "type", "world_fact").unwrap();
            let fact_table = Table::new(&ctx);
            set_lua_string(fact_table, ctx, "id", &fact.id);
            set_lua_string(fact_table, ctx, "kind", &fact.kind);
            set_lua_string(fact_table, ctx, "text", &fact.text);
            fact_table.set(ctx, "tags", lua_strings(ctx, &fact.tags)).unwrap();
            table.set(ctx, "fact", fact_table).unwrap();
        }
        GenValue::Storylet { storylet } => {
            table.set(ctx, "type", "storylet").unwrap();
            let storylet_table = Table::new(&ctx);
            set_lua_string(storylet_table, ctx, "key", &storylet.key);
            set_lua_string(storylet_table, ctx, "entry", &storylet.entry);
            storylet_table
                .set(ctx, "tags", lua_strings(ctx, &storylet.tags))
                .unwrap();
            table.set(ctx, "storylet", storylet_table).unwrap();
        }
    }
    table
}

fn lua_strings<'gc>(ctx: piccolo::Context<'gc>, strings: &[String]) -> Table<'gc> {
    let table = Table::new(&ctx);
    for (index, string) in strings.iter().enumerate() {
        table
            .set(ctx, index as i64 + 1, lua_string(ctx, string))
            .unwrap();
    }
    table
}

fn set_lua_string<'gc>(
    table: Table<'gc>,
    ctx: piccolo::Context<'gc>,
    key: &'static str,
    value: &str,
) {
    table.set(ctx, key, lua_string(ctx, value)).unwrap();
}

fn lua_string<'gc>(ctx: piccolo::Context<'gc>, value: &str) -> piccolo::String<'gc> {
    piccolo::String::from_slice(&ctx, value.as_bytes())
}

/// Drive one executor with a finite total fuel budget. `Lua::execute` refuels
/// internally, which is appropriate for rules formulas but not untrusted pack
/// generators, so this path intentionally steps the executor itself.
fn execute_bounded<R: for<'gc> piccolo::FromMultiValue<'gc>>(
    lua: &mut Lua,
    executor: &StashedExecutor,
    total_fuel: i32,
) -> Result<R, String> {
    let mut fuel = Fuel::with(total_fuel);
    loop {
        let complete = lua.enter(|ctx| ctx.fetch(executor).step(ctx, &mut fuel));
        if complete {
            break;
        }
        if !fuel.should_continue() {
            return Err("generator exhausted fuel".to_owned());
        }
    }
    lua.try_enter(|ctx| ctx.fetch(executor).take_result::<R>(ctx)?)
        .map_err(|e| format!("run generator: {e}"))
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
                        FieldValue::Float(f) => table.set(ctx, key, *f)?,
                        // Nested values reach Lua with the W2 generator
                        // ABI (worldbuilding plan); scalar rules don't
                        // see them yet.
                        FieldValue::List(_) | FieldValue::Map(_) => Value::Nil,
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
        FieldDef {
            key: "attack_bonus".to_owned(),
            label: "Attack bonus".to_owned(),
            default: FieldValue::Int(0),
        },
    ];
    let m = |ab: &str| DerivedDef {
        key: format!("{ab}_mod"),
        label: format!("{} mod", ab.to_uppercase()),
        func: format!("m_{ab}"),
    };
    let derived = vec![m("str"), m("dex"), m("con"), m("int"), m("wis"), m("cha")];
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
        function a_attack(c) return ab_mod(c.str) + c.prof + c.attack_bonus end
    "#;
    System::load("5e-srd", "5e SRD", fields, derived, actions, script)
        .expect("builtin 5e system loads")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generator_request() -> GeneratorRequest {
        GeneratorRequest {
            generator: "demo:forge".to_owned(),
            args: GenValue::Text {
                value: "coast".to_owned(),
            },
            locks: BTreeMap::from([(
                "culture".to_owned(),
                GenValue::Text {
                    value: "river-clans".to_owned(),
                },
            )]),
        }
    }

    #[test]
    fn generator_is_deterministic_and_records_host_entropy() {
        let script = r#"
            function call_gen(args, entropy)
                return '{"type":"item","item":{"template":"demo:sword","name":"Blade-' .. entropy .. '","tags":["generated"]}}'
            end
        "#;
        let mut first = GeneratorRuntime::load(script, GeneratorLimits::default()).unwrap();
        let mut second = GeneratorRuntime::load(script, GeneratorLimits::default()).unwrap();
        let mut first_tape = EntropyTape::from_seed(7);
        let mut second_tape = EntropyTape::from_seed(7);

        let first_result = first.call(&generator_request(), &mut first_tape).unwrap();
        let second_result = second.call(&generator_request(), &mut second_tape).unwrap();

        assert_eq!(first_result, second_result);
        assert_eq!(first_tape.draws, second_tape.draws);
        assert_eq!(first_tape.draws, vec![first_result.entropy]);
        assert!(matches!(first_result.value, GenValue::Item { .. }));
    }

    #[test]
    fn generator_fuel_cap_stops_unbounded_scripts() {
        let script = r#"
            function call_gen(args, entropy)
                while true do end
            end
        "#;
        let limits = GeneratorLimits {
            fuel: 128,
            ..GeneratorLimits::default()
        };
        let mut runtime = GeneratorRuntime::load(script, limits).unwrap();
        let mut tape = EntropyTape::from_seed(1);
        assert_eq!(
            runtime.call(&generator_request(), &mut tape).unwrap_err(),
            "generator exhausted fuel"
        );
        assert_eq!(tape.draws.len(), 1);
    }

    #[test]
    fn generator_fixture_checks_proposal_and_entropy_trace() {
        let script = r#"
            function call_gen(args, entropy)
                return '{"type":"text","value":"fixed"}'
            end
        "#;
        let mut runtime = GeneratorRuntime::load(script, GeneratorLimits::default()).unwrap();
        let mut expected_tape = EntropyTape::from_seed(99);
        expected_tape.draw();
        let fixture = GeneratorFixture {
            name: "fixed proposal".to_owned(),
            seed: 99,
            request: generator_request(),
            expected: GenValue::Text {
                value: "fixed".to_owned(),
            },
            expected_draws: expected_tape.draws,
        };
        runtime.run_fixture(&fixture).unwrap();
    }

    #[test]
    fn generator_receives_tagged_request_and_locks_as_lua_tables() {
        let script = r#"
            function call_gen(args_json, entropy, request)
                local culture = request.locks.culture
                if request.generator == "demo:forge"
                    and request.args.type == "text"
                    and request.args.value == "coast"
                    and culture.type == "text"
                    and culture.value == "river-clans" then
                    return '{"type":"text","value":"typed request"}'
                end
                return '{"type":"text","value":"wrong request"}'
            end
        "#;
        let mut runtime = GeneratorRuntime::load(script, GeneratorLimits::default()).unwrap();
        let mut tape = EntropyTape::from_seed(3);
        assert_eq!(
            runtime.call(&generator_request(), &mut tape).unwrap().value,
            GenValue::Text {
                value: "typed request".to_owned()
            }
        );
    }

    #[test]
    fn declared_pack_fixture_runs_without_opening_undeclared_assets() {
        let root = std::env::temp_dir().join(format!(
            "isometry-generator-pack-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("generators")).unwrap();
        std::fs::create_dir_all(root.join("fixtures")).unwrap();
        std::fs::write(
            root.join(GeneratorPack::MANIFEST_FILE),
            r#"{
  "format": 1,
  "id": "demo",
  "name": "Demo Pack",
  "version": "0.1.0",
  "generators": [{
    "id": "forge_item",
    "script": "generators/forge_item.lua",
    "fixtures": ["fixtures/forge_item.json"]
  }]
}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("generators/forge_item.lua"),
            r#"function call_gen(args_json, entropy)
    return '{"type":"text","value":"forge"}'
end"#,
        )
        .unwrap();
        std::fs::write(
            root.join("fixtures/forge_item.json"),
            r#"{
  "name": "declared fixture",
  "seed": 7,
  "request": {
    "generator": "demo:forge_item",
    "args": { "type": "text", "value": "river" },
    "locks": {}
  },
  "expected": { "type": "text", "value": "forge" },
  "expected_draws": [7191089600892374487]
}"#,
        )
        .unwrap();

        let pack = GeneratorPack::load(&root).unwrap();
        assert_eq!(pack.manifest().id, "demo");
        let request = GeneratorRequest {
            generator: "demo:forge_item".to_owned(),
            args: GenValue::Text {
                value: "river".to_owned(),
            },
            locks: BTreeMap::new(),
        };
        let mut tape = EntropyTape::from_seed(7);
        let record = pack
            .generate(
                "generated.forge.1",
                &request,
                &mut tape,
                GeneratorLimits::default(),
            )
            .unwrap();
        assert_eq!(record.request, request);
        assert_eq!(record.proposal, GenValue::Text { value: "forge".to_owned() });
        assert_eq!(record.entropy, tape.draws[0]);
        pack.run_fixture(
            "demo:forge_item",
            "fixtures/forge_item.json",
            GeneratorLimits::default(),
        )
        .unwrap();
        assert!(pack
            .run_fixture(
                "demo:forge_item",
                "fixtures/not-declared.json",
                GeneratorLimits::default(),
            )
            .is_err());

        std::fs::remove_dir_all(root).unwrap();
    }

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

    #[test]
    fn equipped_modifier_changes_effective_attack_without_mutating_sheet() {
        use isometry_campaign::{
            EquipmentSlot, Inventory, ItemId, ItemInstance, ItemModifier, ItemModifierKind,
        };

        let mut system = srd_5e();
        let sheet = system.default_sheet();
        let sword = ItemInstance {
            id: ItemId::new("reward-03.sword"),
            template: "srd5e:longsword".to_owned(),
            name: "Fine Longsword".to_owned(),
            quantity: 1,
            tags: vec!["weapon".to_owned()],
            modifiers: vec![ItemModifier {
                id: "reward-03.sword.fine".to_owned(),
                kind: ItemModifierKind::Quality,
                name: "Fine".to_owned(),
                stats: BTreeMap::from([("attack_bonus".to_owned(), 2)]),
                appearance_layer: None,
            }],
            appearance_layers: vec!["weapon:longsword".to_owned()],
        };
        let mut inventory = Inventory::default();
        inventory.insert(sword).unwrap();
        inventory
            .equip(EquipmentSlot::MainHand, ItemId::new("reward-03.sword"))
            .unwrap();

        let effective = system.effective_sheet(&sheet, Some(&inventory));
        assert_eq!(sheet.int("attack_bonus"), Some(0));
        assert_eq!(effective.int("attack_bonus"), Some(2));
        assert_eq!(
            system.action_expr("attack", &effective).as_deref(),
            Some("1d20+4")
        );
    }
}
