//! Isometry's system-plugin lane.
//!
//! A game **system** is a schema (what fields a character has) plus Lua
//! scripts (how derived stats compute and what an action rolls). The
//! substrate stores [`SheetData`](isometry_core::SheetData); this crate
//! interprets it. The scripting engine is piccolo (pure-Rust Lua),
//! sandboxed, behind the [`System`] type so a host never touches Lua
//! directly.
//!
//! The Lua boundary stays narrow: every script function returns an **integer**,
//! and the dice expressions are assembled in Rust, so no Lua string has to cross
//! the GC boundary. It now takes up to three arguments, `f(c, t, n)`: the actor's
//! character table, an optional *target* table, and an optional scalar (a roll).
//! Lua ignores arguments a function does not declare, so `m_str(c)` is unchanged
//! by the widening while `a_attack_hit(c, t, roll)` can compare a roll against a
//! defender's AC. That target context is what lets the *system* decide what a hit
//! is, instead of hardcoding d20-versus-AC into Rust.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use isometry_campaign::{
    CampaignDraft, ContentPackManifest, EncounterAnchor, EntropyTape, GenValue, GenerationRecord,
    GeneratorChoice, GeneratorFixture, GeneratorRequest, Inventory, ItemProposal, LocalMapProposal,
    MapCellProposal, MapPatchProposal, MapPoint, MapTransition, NpcProposal, SpawnZone,
    StoryletProposal, WorldFact,
};
use isometry_core::{
    roll, Beat, FieldValue, Rng, RollRecord, SheetData, SheetDelta, TileCoord, TokenId,
};
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
    /// `None` for an untargeted roll (an ability check): it produces a number
    /// for the table to read and changes nothing. `Some` makes the action
    /// *adjudicated*: it names a victim, asks the system whether it lands, and
    /// resolves into typed deltas.
    pub target: Option<TargetSpec>,
}

/// What an adjudicated action needs in order to resolve against a defender.
///
/// Every rule here is data or Lua, never Rust. The resolver rolls the dice, asks
/// the script whether the roll lands, and writes the answer to the named field.
/// Swapping d20-versus-AC for a roll-under, a degrees-of-success ladder, or a
/// non-d20 system is a different script and a different `base`, not a code change.
pub struct TargetSpec {
    /// Maximum Chebyshev distance in tiles. 1 is adjacent melee.
    pub range: u32,
    /// Lua `f(c, t, roll) -> 1|0`: given the actor, the target, and the actor's
    /// total roll, did it land? This is where "beats AC" lives.
    pub hit_func: String,
    /// Dice rolled for effect on a hit.
    pub damage: String,
    /// Lua `f(c, t) -> int`: the effect's flat bonus.
    pub damage_func: String,
    /// The target-sheet field the effect subtracts from.
    pub damage_field: String,
    /// Beat played by the actor, and by the target on a hit or a miss. Pack
    /// vocabulary; the substrate never looks inside these names.
    pub actor_beat: String,
    pub hit_beat: String,
    pub miss_beat: String,
    /// Played by a victim this action puts out of play, instead of `hit_beat`.
    pub fall_beat: String,
    /// Lua `f(c, t, damage) -> 1|0`: does a blow of that size rock the victim
    /// off its feet? **Cosmetic.** The victim is knocked out of place and walks
    /// back; its tile never changes.
    ///
    /// This is the line the whole design rests on. A stagger needs no
    /// pathfinding, no ordering, and no agreement between peers, because it is a
    /// beat: two machines may disagree about exactly where the sprite is
    /// mid-flinch and still hold identical game state. What it may never do is
    /// feed a rule.
    pub stagger_func: Option<String>,
    /// Beat a staggered victim plays. The resolver suffixes the direction, so
    /// `staggered` becomes `staggered-ne`; a pack supplies one rule per compass
    /// point.
    pub stagger_beat: String,
    /// Lua `f(c, t, damage) -> tiles`: how far this action **actually** moves the
    /// victim. **Truth.** Thunderwave, a shove, a repelling blast: the token
    /// relocates and stays there, so it is replicated, validated against the
    /// board's geometry, and it changes reach and line of sight.
    ///
    /// Zero, and `None`, mean the usual answer: nobody moves.
    pub push_func: Option<String>,
    /// Beat the victim plays on arriving at its new tile. The board has already
    /// placed it there, so this beat slides it *in from* where it used to be:
    /// the same directional keyframes as a stagger, run the other way.
    pub push_beat: String,
    /// Condition applied to the victim on a hit (`prone` for a trip). The name
    /// is system vocabulary; the substrate stores it blind and the mechanical
    /// numbers travel alongside as recomputed mobility.
    pub condition_on_hit: Option<String>,
    /// Whether a hit wins the target over to the actor's side (`convince`). Like
    /// a push, the rules only say *that* it happened; the host rules the rest,
    /// because allegiance lives on the token's owner (which the system never
    /// sees) and the party cap is table policy. The resolver reports the win;
    /// the host decides the new owner and whether the party has room.
    pub recruit_on_hit: bool,
}

/// Why an intent was refused. Every one of these is checked before any die is
/// rolled, so a rejected intent changes nothing at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    UnknownAction(String),
    NotTargeted(String),
    SelfTarget,
    OutOfRange { range: u32, distance: u32 },
    /// The victim is already out of play. Hitting a corpse is not a move.
    AlreadyDefeated,
    ScriptFailed(String),
    BadDice(String),
}

/// A fully resolved action: the single fact that crosses the wire.
///
/// It carries its own evidence (the public dice), its verdict, its consequences
/// (typed deltas), and its representation (beats). Peers *apply* this. They never
/// rerun the script and never reroll, so one machine's Lua is the only Lua that
/// runs and the convergence hash stays meaningful.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolution {
    pub attack: RollRecord,
    pub hit: bool,
    pub damage: Option<RollRecord>,
    pub deltas: Vec<SheetDelta>,
    pub beats: Vec<Beat>,
    /// Tokens this action put out of play. The system decides (its `defeat_func`
    /// reading the sheet *after* the deltas land); the substrate merely obeys.
    pub defeated: Vec<TokenId>,
    /// Forced movement the rules demand of the victim: `(unit step, tiles)`.
    ///
    /// The rules say *how hard and which way*. They do not say where it lands,
    /// because the system does not know the board: a wall, a map edge, or another
    /// token can stop a shove short, and that is the substrate's ruling. The host
    /// walks it with [`isometry_core::push_path`].
    pub push: Option<((i32, i32), u32)>,
    /// Conditions this action applied: `(token, name, on)`.
    pub conditions: Vec<(TokenId, String, bool)>,
    /// The recomputed `(move budget, sight radius)` for each token whose
    /// conditions changed; `None` clears back to sheet base.
    pub mobility: Vec<(TokenId, Option<(u32, u32)>)>,
    /// The target was won over: the host should hand it to the actor's side (if
    /// the party has room). `None` for every action that is not a recruit, and
    /// for a recruit that missed.
    pub recruited: Option<TokenId>,
}

/// Copy `sheet` with each active condition added as a boolean field, so Lua
/// reads `c.prone` with no new marshalling: conditions ride the existing
/// character table. A sheet field with the same name would be shadowed, which is
/// why condition names are validated against the schema's field keys by packs
/// rather than guarded here.
pub fn sheet_with_conditions<'a>(
    sheet: &SheetData,
    conditions: impl IntoIterator<Item = &'a String>,
) -> SheetData {
    let mut out = sheet.clone();
    for name in conditions {
        out.fields.insert(name.clone(), FieldValue::Bool(true));
    }
    out
}

/// A loaded game system: schema + a live sandboxed Lua interpreter with
/// the system's script defining its functions.
pub struct System {
    pub id: String,
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub derived: Vec<DerivedDef>,
    pub actions: Vec<ActionDef>,
    /// Lua `f(c) -> 1|0`: is this character out of play? `None` for a system
    /// with no such concept (a pure worldbuilding pack). The substrate never
    /// asks *why*; it only acts on the answer.
    pub defeat_func: Option<String>,
    /// Lua `f(c) -> tiles`, the movement and sight projections. The character
    /// table carries condition booleans, so `prone` halving speed is one line of
    /// script. `None` means the sheet's base numbers stand unmodified.
    pub speed_func: Option<String>,
    pub sight_func: Option<String>,
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
/// recorded. The result may be a tagged Lua table or a legacy JSON string;
/// both decode to [`GenValue`]. This runtime only makes proposals. It has no
/// campaign, network, filesystem, or commit capability.
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

/// Loaded pack set for one host. Discovery accepts either pack directories or
/// roots whose immediate child directories are packs; failures remain visible
/// diagnostics instead of hiding the usable packs beside them.
pub struct GeneratorCatalog {
    packs: Vec<GeneratorPack>,
    diagnostics: Vec<String>,
}

/// One beat as the table will actually play it: the name the rules speak, the
/// label a player sees if they may throw it, and the stylesheet that draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedBeat {
    pub name: String,
    /// `Some` when a player may throw this on their own token.
    pub emote: Option<String>,
    pub css: String,
}

impl GeneratorCatalog {
    pub fn discover<I, P>(roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut candidates = Vec::new();
        let mut diagnostics = Vec::new();
        for root in roots {
            let root = root.as_ref();
            if root.join(GeneratorPack::MANIFEST_FILE).is_file() {
                candidates.push(root.to_path_buf());
                continue;
            }
            match std::fs::read_dir(root) {
                Ok(entries) => {
                    let mut children: Vec<PathBuf> = entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .filter(|path| path.join(GeneratorPack::MANIFEST_FILE).is_file())
                        .collect();
                    children.sort();
                    candidates.extend(children);
                }
                Err(error) => diagnostics.push(format!(
                    "read generator-pack root {}: {error}",
                    root.display()
                )),
            }
        }
        let mut packs = Vec::new();
        let mut ids = BTreeSet::new();
        for candidate in candidates {
            match GeneratorPack::load(&candidate) {
                Ok(pack) if ids.insert(pack.manifest().id.clone()) => packs.push(pack),
                Ok(pack) => diagnostics.push(format!(
                    "duplicate content-pack id {} at {}",
                    pack.manifest().id,
                    candidate.display()
                )),
                Err(error) => diagnostics.push(error),
            }
        }
        Self { packs, diagnostics }
    }

    pub fn choices(&self) -> Vec<GeneratorChoice> {
        self.packs
            .iter()
            .flat_map(|pack| pack.manifest().generator_choices())
            .collect()
    }

    /// The table's whole beat vocabulary, gathered from every discovered pack:
    /// `(name, emote label, stylesheet)`.
    ///
    /// A later pack declaring a beat name an earlier one already used wins, so a
    /// campaign restyles the swing simply by shipping its own `strike` -- no app
    /// change, no recompile, which is the point of putting it here.
    ///
    /// A pack whose stylesheet will not open is *skipped with a diagnostic*
    /// rather than failing the table: a missing beat costs an animation, and no
    /// rule may read a beat, so a board with no choreography still plays a
    /// correct game. That is only safe because beats are representation.
    pub fn choreography(&self) -> (Vec<LoadedBeat>, Vec<String>) {
        let mut beats: Vec<LoadedBeat> = Vec::new();
        let mut diagnostics = Vec::new();
        for pack in &self.packs {
            for entry in &pack.manifest().choreography {
                let path = pack.root.join(&entry.style);
                let css = match std::fs::read_to_string(&path) {
                    Ok(css) => css,
                    Err(error) => {
                        diagnostics.push(format!(
                            "beat '{}' in pack '{}': read {}: {error}",
                            entry.name,
                            pack.manifest().id,
                            path.display()
                        ));
                        continue;
                    }
                };
                let loaded = LoadedBeat {
                    name: entry.name.clone(),
                    emote: entry.emote.clone(),
                    css,
                };
                match beats.iter_mut().find(|b| b.name == loaded.name) {
                    Some(existing) => *existing = loaded,
                    None => beats.push(loaded),
                }
            }
        }
        (beats, diagnostics)
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub fn generate(
        &self,
        record_id: impl Into<String>,
        request: &GeneratorRequest,
        tape: &mut EntropyTape,
        limits: GeneratorLimits,
    ) -> Result<GenerationRecord, String> {
        let pack = self
            .packs
            .iter()
            .find(|pack| pack.manifest().generator(&request.generator).is_some())
            .ok_or_else(|| format!("no loaded pack declares generator {}", request.generator))?;
        pack.generate(record_id, request, tape, limits)
    }
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
        let entry = self.manifest.generator(&request.generator).ok_or_else(|| {
            format!(
                "generator is not declared by this pack: {}",
                request.generator
            )
        })?;
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
        let value = execute_bounded_gen_value(
            &mut self.lua,
            &ex,
            self.limits.fuel,
            self.limits.max_value_depth,
        )?;
        let output_bytes = serde_json::to_vec(&value)
            .map_err(|e| format!("serialize generated value for size check: {e}"))?;
        if output_bytes.len() > self.limits.max_output_bytes {
            return Err(format!(
                "generator output exceeds {} byte limit",
                self.limits.max_output_bytes
            ));
        }
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

/// Run a generator to completion and decode its arena-bound Lua result before
/// leaving the Piccolo context. String results preserve the W2 JSON ABI;
/// tables are the native authoring path.
fn execute_bounded_gen_value(
    lua: &mut Lua,
    executor: &StashedExecutor,
    total_fuel: i32,
    max_depth: usize,
) -> Result<GenValue, String> {
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
    lua.try_enter(|ctx| {
        let value = ctx.fetch(executor).take_result::<Value>(ctx)??;
        lua_value_to_gen(ctx, value, 0, max_depth).map_err(|error| error.into_value(ctx).into())
    })
    .map_err(|e| format!("run generator: {e}"))
}

fn lua_value_to_gen<'gc>(
    ctx: piccolo::Context<'gc>,
    value: Value<'gc>,
    depth: usize,
    max_depth: usize,
) -> Result<GenValue, String> {
    if depth > max_depth {
        return Err(format!(
            "generated value exceeds maximum depth of {max_depth}"
        ));
    }
    let table = match value {
        Value::String(json) => {
            return serde_json::from_slice(json.as_bytes())
                .map_err(|e| format!("generator returned invalid GenValue JSON: {e}"));
        }
        Value::Table(table) => table,
        other => {
            return Err(format!(
                "generator must return a tagged table or JSON string, found {}",
                other.type_name()
            ));
        }
    };
    let kind = lua_table_string(ctx, table, "type")?;
    match kind.as_str() {
        "text" => Ok(GenValue::Text {
            value: lua_table_string(ctx, table, "value")?,
        }),
        "object" => {
            let fields = lua_table_table(ctx, table, "fields")?;
            let mut out = BTreeMap::new();
            for (key, value) in fields.iter() {
                let Value::String(key) = key else {
                    return Err("generated object keys must be strings".to_owned());
                };
                let key = String::from_utf8(key.as_bytes().to_vec())
                    .map_err(|_| "generated object key is not UTF-8".to_owned())?;
                out.insert(key, lua_value_to_gen(ctx, value, depth + 1, max_depth)?);
            }
            Ok(GenValue::Object { fields: out })
        }
        "list" => Ok(GenValue::List {
            values: lua_gen_list(
                ctx,
                lua_table_table(ctx, table, "values")?,
                depth + 1,
                max_depth,
            )?,
        }),
        "item" => {
            let item = lua_table_table(ctx, table, "item")?;
            Ok(GenValue::Item {
                item: ItemProposal {
                    template: lua_table_string(ctx, item, "template")?,
                    name: lua_table_string(ctx, item, "name")?,
                    tags: lua_string_list(ctx, lua_table_table(ctx, item, "tags")?)?,
                },
            })
        }
        "npc" => {
            let npc = lua_table_table(ctx, table, "npc")?;
            Ok(GenValue::Npc {
                npc: NpcProposal {
                    key: lua_table_string(ctx, npc, "key")?,
                    name: lua_table_string(ctx, npc, "name")?,
                    tags: lua_string_list(ctx, lua_table_table(ctx, npc, "tags")?)?,
                },
            })
        }
        "map_patch" => {
            let patch = lua_table_table(ctx, table, "patch")?;
            Ok(GenValue::MapPatch {
                patch: MapPatchProposal {
                    target: lua_table_string(ctx, patch, "target")?,
                    operations: lua_gen_list(
                        ctx,
                        lua_table_table(ctx, patch, "operations")?,
                        depth + 1,
                        max_depth,
                    )?,
                },
            })
        }
        "world_fact" => {
            let fact = lua_table_table(ctx, table, "fact")?;
            Ok(GenValue::WorldFact {
                fact: WorldFact {
                    id: lua_table_string(ctx, fact, "id")?,
                    kind: lua_table_string(ctx, fact, "kind")?,
                    text: lua_table_string(ctx, fact, "text")?,
                    tags: lua_string_list(ctx, lua_table_table(ctx, fact, "tags")?)?,
                },
            })
        }
        "storylet" => {
            let storylet = lua_table_table(ctx, table, "storylet")?;
            Ok(GenValue::Storylet {
                storylet: StoryletProposal {
                    key: lua_table_string(ctx, storylet, "key")?,
                    entry: lua_table_string(ctx, storylet, "entry")?,
                    tags: lua_string_list(ctx, lua_table_table(ctx, storylet, "tags")?)?,
                    requirements: Default::default(),
                    roles: Vec::new(),
                    effects: Vec::new(),
                },
            })
        }
        "local_map" => {
            let map = lua_table_table(ctx, table, "map")?;
            Ok(GenValue::LocalMap {
                map: LocalMapProposal {
                    id: lua_table_string(ctx, map, "id")?,
                    name: lua_table_string(ctx, map, "name")?,
                    width: lua_table_u32(ctx, map, "width")?,
                    height: lua_table_u32(ctx, map, "height")?,
                    default_ground: lua_table_string(ctx, map, "default_ground")?,
                    cells: lua_map_cells(ctx, lua_table_table(ctx, map, "cells")?)?,
                    spawn_zones: lua_spawn_zones(ctx, lua_table_table(ctx, map, "spawn_zones")?)?,
                    transitions: lua_transitions(ctx, lua_table_table(ctx, map, "transitions")?)?,
                    encounter_anchors: lua_encounter_anchors(
                        ctx,
                        lua_table_table(ctx, map, "encounter_anchors")?,
                    )?,
                },
            })
        }
        "campaign" => {
            let json = lua_table_string(ctx, table, "campaign_json")?;
            let campaign: CampaignDraft = serde_json::from_str(&json)
                .map_err(|error| format!("generator returned invalid campaign draft: {error}"))?;
            campaign
                .validate()
                .map_err(|error| format!("generator returned invalid campaign draft: {error:?}"))?;
            Ok(GenValue::Campaign { campaign })
        }
        other => Err(format!("unknown generated value type: {other}")),
    }
}

fn lua_table_string<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
    key: &'static str,
) -> Result<String, String> {
    match table.get(ctx, key) {
        Value::String(value) => String::from_utf8(value.as_bytes().to_vec())
            .map_err(|_| format!("generated field {key} is not UTF-8")),
        value => Err(format!(
            "generated field {key} must be a string, found {}",
            value.type_name()
        )),
    }
}

fn lua_table_table<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
    key: &'static str,
) -> Result<Table<'gc>, String> {
    match table.get(ctx, key) {
        Value::Table(value) => Ok(value),
        value => Err(format!(
            "generated field {key} must be a table, found {}",
            value.type_name()
        )),
    }
}

fn lua_table_u32<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
    key: &'static str,
) -> Result<u32, String> {
    match table.get(ctx, key) {
        Value::Integer(value) => {
            u32::try_from(value).map_err(|_| format!("generated field {key} must fit u32"))
        }
        value => Err(format!(
            "generated field {key} must be an integer, found {}",
            value.type_name()
        )),
    }
}

fn lua_optional_string<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
    key: &'static str,
) -> Result<Option<String>, String> {
    match table.get(ctx, key) {
        Value::Nil => Ok(None),
        Value::String(value) => String::from_utf8(value.as_bytes().to_vec())
            .map(Some)
            .map_err(|_| format!("generated field {key} is not UTF-8")),
        value => Err(format!(
            "generated field {key} must be a string or nil, found {}",
            value.type_name()
        )),
    }
}

fn lua_optional_u8<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
    key: &'static str,
) -> Result<Option<u8>, String> {
    match table.get(ctx, key) {
        Value::Nil => Ok(None),
        Value::Integer(value) => u8::try_from(value)
            .map(Some)
            .map_err(|_| format!("generated field {key} must fit u8")),
        value => Err(format!(
            "generated field {key} must be an integer or nil, found {}",
            value.type_name()
        )),
    }
}

fn lua_map_point<'gc>(ctx: piccolo::Context<'gc>, table: Table<'gc>) -> Result<MapPoint, String> {
    Ok(MapPoint {
        col: lua_table_u32(ctx, table, "col")?,
        row: lua_table_u32(ctx, table, "row")?,
    })
}

fn lua_map_points<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
) -> Result<Vec<MapPoint>, String> {
    let length = usize::try_from(table.length())
        .map_err(|_| "generated point-list length is invalid".to_owned())?;
    (1..=length)
        .map(|index| match table.get(ctx, index as i64) {
            Value::Table(point) => lua_map_point(ctx, point),
            value => Err(format!(
                "generated point must be a table, found {}",
                value.type_name()
            )),
        })
        .collect()
}

fn lua_map_cells<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
) -> Result<Vec<MapCellProposal>, String> {
    let length = usize::try_from(table.length())
        .map_err(|_| "generated cell-list length is invalid".to_owned())?;
    (1..=length)
        .map(|index| {
            let Value::Table(cell) = table.get(ctx, index as i64) else {
                return Err("generated map cell must be a table".to_owned());
            };
            Ok(MapCellProposal {
                col: lua_table_u32(ctx, cell, "col")?,
                row: lua_table_u32(ctx, cell, "row")?,
                ground: lua_optional_string(ctx, cell, "ground")?,
                prop: lua_optional_string(ctx, cell, "prop")?,
                elevation: lua_optional_u8(ctx, cell, "elevation")?,
            })
        })
        .collect()
}

fn lua_spawn_zones<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
) -> Result<Vec<SpawnZone>, String> {
    let length = usize::try_from(table.length())
        .map_err(|_| "generated spawn-zone list length is invalid".to_owned())?;
    (1..=length)
        .map(|index| {
            let Value::Table(zone) = table.get(ctx, index as i64) else {
                return Err("generated spawn zone must be a table".to_owned());
            };
            Ok(SpawnZone {
                id: lua_table_string(ctx, zone, "id")?,
                cells: lua_map_points(ctx, lua_table_table(ctx, zone, "cells")?)?,
            })
        })
        .collect()
}

fn lua_transitions<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
) -> Result<Vec<MapTransition>, String> {
    let length = usize::try_from(table.length())
        .map_err(|_| "generated transition list length is invalid".to_owned())?;
    (1..=length)
        .map(|index| {
            let Value::Table(transition) = table.get(ctx, index as i64) else {
                return Err("generated transition must be a table".to_owned());
            };
            Ok(MapTransition {
                id: lua_table_string(ctx, transition, "id")?,
                at: lua_map_point(ctx, lua_table_table(ctx, transition, "at")?)?,
                target_map: lua_table_string(ctx, transition, "target_map")?,
                target_entry: lua_optional_string(ctx, transition, "target_entry")?,
            })
        })
        .collect()
}

fn lua_encounter_anchors<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
) -> Result<Vec<EncounterAnchor>, String> {
    let length = usize::try_from(table.length())
        .map_err(|_| "generated encounter-anchor list length is invalid".to_owned())?;
    (1..=length)
        .map(|index| {
            let Value::Table(anchor) = table.get(ctx, index as i64) else {
                return Err("generated encounter anchor must be a table".to_owned());
            };
            Ok(EncounterAnchor {
                id: lua_table_string(ctx, anchor, "id")?,
                at: lua_map_point(ctx, lua_table_table(ctx, anchor, "at")?)?,
                tags: lua_string_list(ctx, lua_table_table(ctx, anchor, "tags")?)?,
            })
        })
        .collect()
}

fn lua_gen_list<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
    depth: usize,
    max_depth: usize,
) -> Result<Vec<GenValue>, String> {
    let length = usize::try_from(table.length())
        .map_err(|_| "generated list length is invalid".to_owned())?;
    (1..=length)
        .map(|index| lua_value_to_gen(ctx, table.get(ctx, index as i64), depth, max_depth))
        .collect()
}

fn lua_string_list<'gc>(
    ctx: piccolo::Context<'gc>,
    table: Table<'gc>,
) -> Result<Vec<String>, String> {
    let length = usize::try_from(table.length())
        .map_err(|_| "generated string-list length is invalid".to_owned())?;
    (1..=length)
        .map(|index| match table.get(ctx, index as i64) {
            Value::String(value) => String::from_utf8(value.as_bytes().to_vec())
                .map_err(|_| "generated list entry is not UTF-8".to_owned()),
            value => Err(format!(
                "generated list entry must be a string, found {}",
                value.type_name()
            )),
        })
        .collect()
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
            item_table
                .set(ctx, "tags", lua_strings(ctx, &item.tags))
                .unwrap();
            table.set(ctx, "item", item_table).unwrap();
        }
        GenValue::Npc { npc } => {
            table.set(ctx, "type", "npc").unwrap();
            let npc_table = Table::new(&ctx);
            set_lua_string(npc_table, ctx, "key", &npc.key);
            set_lua_string(npc_table, ctx, "name", &npc.name);
            npc_table
                .set(ctx, "tags", lua_strings(ctx, &npc.tags))
                .unwrap();
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
            fact_table
                .set(ctx, "tags", lua_strings(ctx, &fact.tags))
                .unwrap();
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
            set_lua_string(
                storylet_table,
                ctx,
                "storylet_json",
                &serde_json::to_string(storylet).expect("storylet is serializable"),
            );
            table.set(ctx, "storylet", storylet_table).unwrap();
        }
        GenValue::LocalMap { map } => {
            table.set(ctx, "type", "local_map").unwrap();
            let map_table = Table::new(&ctx);
            set_lua_string(map_table, ctx, "id", &map.id);
            set_lua_string(map_table, ctx, "name", &map.name);
            map_table.set(ctx, "width", map.width).unwrap();
            map_table.set(ctx, "height", map.height).unwrap();
            set_lua_string(map_table, ctx, "default_ground", &map.default_ground);
            map_table
                .set(ctx, "cells", map_cells_table(ctx, &map.cells))
                .unwrap();
            map_table
                .set(ctx, "spawn_zones", spawn_zones_table(ctx, &map.spawn_zones))
                .unwrap();
            map_table
                .set(ctx, "transitions", transitions_table(ctx, &map.transitions))
                .unwrap();
            map_table
                .set(
                    ctx,
                    "encounter_anchors",
                    encounter_anchors_table(ctx, &map.encounter_anchors),
                )
                .unwrap();
            table.set(ctx, "map", map_table).unwrap();
        }
        GenValue::Campaign { campaign } => {
            table.set(ctx, "type", "campaign").unwrap();
            set_lua_string(
                table,
                ctx,
                "campaign_json",
                &serde_json::to_string(campaign).expect("campaign draft is serializable"),
            );
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

fn map_point_table<'gc>(ctx: piccolo::Context<'gc>, point: MapPoint) -> Table<'gc> {
    let table = Table::new(&ctx);
    table.set(ctx, "col", point.col).unwrap();
    table.set(ctx, "row", point.row).unwrap();
    table
}

fn map_cells_table<'gc>(ctx: piccolo::Context<'gc>, cells: &[MapCellProposal]) -> Table<'gc> {
    let table = Table::new(&ctx);
    for (index, cell) in cells.iter().enumerate() {
        let value = Table::new(&ctx);
        value.set(ctx, "col", cell.col).unwrap();
        value.set(ctx, "row", cell.row).unwrap();
        if let Some(ground) = &cell.ground {
            set_lua_string(value, ctx, "ground", ground);
        }
        if let Some(prop) = &cell.prop {
            set_lua_string(value, ctx, "prop", prop);
        }
        if let Some(elevation) = cell.elevation {
            value.set(ctx, "elevation", elevation).unwrap();
        }
        table.set(ctx, index as i64 + 1, value).unwrap();
    }
    table
}

fn spawn_zones_table<'gc>(ctx: piccolo::Context<'gc>, zones: &[SpawnZone]) -> Table<'gc> {
    let table = Table::new(&ctx);
    for (index, zone) in zones.iter().enumerate() {
        let value = Table::new(&ctx);
        set_lua_string(value, ctx, "id", &zone.id);
        let cells = Table::new(&ctx);
        for (cell_index, point) in zone.cells.iter().enumerate() {
            cells
                .set(ctx, cell_index as i64 + 1, map_point_table(ctx, *point))
                .unwrap();
        }
        value.set(ctx, "cells", cells).unwrap();
        table.set(ctx, index as i64 + 1, value).unwrap();
    }
    table
}

fn transitions_table<'gc>(ctx: piccolo::Context<'gc>, transitions: &[MapTransition]) -> Table<'gc> {
    let table = Table::new(&ctx);
    for (index, transition) in transitions.iter().enumerate() {
        let value = Table::new(&ctx);
        set_lua_string(value, ctx, "id", &transition.id);
        value
            .set(ctx, "at", map_point_table(ctx, transition.at))
            .unwrap();
        set_lua_string(value, ctx, "target_map", &transition.target_map);
        if let Some(target_entry) = &transition.target_entry {
            set_lua_string(value, ctx, "target_entry", target_entry);
        }
        table.set(ctx, index as i64 + 1, value).unwrap();
    }
    table
}

fn encounter_anchors_table<'gc>(
    ctx: piccolo::Context<'gc>,
    anchors: &[EncounterAnchor],
) -> Table<'gc> {
    let table = Table::new(&ctx);
    for (index, anchor) in anchors.iter().enumerate() {
        let value = Table::new(&ctx);
        set_lua_string(value, ctx, "id", &anchor.id);
        value
            .set(ctx, "at", map_point_table(ctx, anchor.at))
            .unwrap();
        value
            .set(ctx, "tags", lua_strings(ctx, &anchor.tags))
            .unwrap();
        table.set(ctx, index as i64 + 1, value).unwrap();
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
            defeat_func: None,
            speed_func: None,
            sight_func: None,
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
    fn call_int(&mut self, func: &str, sheet: &SheetData) -> Option<i64> {
        self.call_int_ctx(func, sheet, None, None)
    }

    /// Call `func(c, t, n) -> int`, where `t` is an optional target sheet and
    /// `n` an optional scalar (the actor's total roll).
    ///
    /// Lua discards arguments a function does not declare, so the existing
    /// one-argument scripts (`m_str(c)`, `a_attack(c)`) are unaffected by the
    /// extra parameters, while a targeted script can read `t.ac`. That is the
    /// whole ABI widening: no tagged returns, no new marshalling, one call path.
    fn call_int_ctx(
        &mut self,
        func: &str,
        sheet: &SheetData,
        target: Option<&SheetData>,
        extra: Option<i64>,
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
                let fname = piccolo::String::from_slice(&ctx, func.as_bytes());
                let f = ctx.globals().get(ctx, fname);
                let Value::Function(f) = f else {
                    return Err("not a function".into_value(ctx).into());
                };
                Ok(ctx.stash(Executor::start(ctx, f, (table, t, n))))
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
            recruit_on_hit: spec.recruit_on_hit,
        };
        let by = actor_sheet.text("name").unwrap_or("?").to_owned();

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
        let hit = self
            .call_int_ctx(
                &spec.hit_func,
                actor_sheet,
                Some(target_sheet),
                Some(total as i64),
            )
            .ok_or_else(|| ActionError::ScriptFailed(spec.hit_func.clone()))?
            != 0;

        // 3. The consequence.
        let mut damage = None;
        let mut deltas = Vec::new();
        if hit {
            let dmg_bonus = self
                .call_int_ctx(&spec.damage_func, actor_sheet, Some(target_sheet), None)
                .ok_or_else(|| ActionError::ScriptFailed(spec.damage_func.clone()))?;
            let (dmg_raw, dmg_dice) = roll(&spec.damage, rng)
                .ok_or_else(|| ActionError::BadDice(spec.damage.clone()))?;
            // Damage never heals: a big negative modifier floors at zero rather
            // than restoring the victim.
            let dmg_total = (dmg_raw + dmg_bonus as i32).max(0);
            damage = Some(RollRecord {
                by,
                expr: format!("{}{dmg_bonus:+}", spec.damage),
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
            if let Some(name) = spec.condition_on_hit.as_ref() {
                let already = matches!(
                    target_sheet.fields.get(name),
                    Some(FieldValue::Bool(true))
                );
                if !already {
                    conditions.push((target, name.clone(), true));
                    // The projection, computed against the sheet as it will be:
                    // with the new condition set.
                    let conditioned =
                        sheet_with_conditions(target_sheet, std::iter::once(name));
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

        Ok(Resolution {
            attack,
            hit,
            damage,
            deltas,
            beats,
            defeated,
            push: (push_tiles > 0).then_some((step, push_tiles)),
            conditions,
            mobility,
            recruited,
        })
    }
}

/// Build a 5e sheet from a compendium stat block, so a spawned monster arrives
/// on the board already statted.
///
/// Without this the goblin is a sprite: its 7 hit points and AC 15 sit in the
/// compendium and never reach a [`SheetData`], so nothing can be done to it.
pub fn monster_sheet(m: &Monster) -> SheetData {
    let mut sheet = SheetData::new("5e-srd");
    sheet.set_text("name", m.name.clone());
    for (key, score) in ["str", "dex", "con", "int", "wis", "cha"]
        .iter()
        .zip(m.abilities)
    {
        sheet.set_int(*key, score as i64);
    }
    // Proficiency by CR, the SRD's own table flattened to its low end; the
    // compendium does not carry it as a field.
    let prof = if m.challenge_rating >= 5.0 { 3 } else { 2 };
    sheet.set_int("prof", prof);
    sheet.set_int("level", 1);
    sheet.set_int("hp_max", m.hit_points as i64);
    sheet.set_int("hp_current", m.hit_points as i64);
    sheet.set_int("ac", m.armor_class as i64);
    sheet.set_int("attack_bonus", 0);
    // 5 ft to a tile, floored: a 30 ft goblin walks 6 tiles.
    sheet.set_int("speed", (m.speed_ft / 5).max(1) as i64);
    sheet.set_int("sight", 6);
    // Resolve DC to convince it: 8 + WIS mod, so a wise creature is harder to
    // talk around. Wisdom is ability index 4.
    sheet.set_int("will", 8 + (m.abilities[4] - 10).div_euclid(2) as i64);
    sheet
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
            key: "hp_current".to_owned(),
            label: "HP".to_owned(),
            default: FieldValue::Int(10),
        },
        FieldDef {
            key: "hp_max".to_owned(),
            label: "HP max".to_owned(),
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
        // The retired MOVE_BUDGET / SIGHT_RADIUS constants, as data. Base
        // values; conditions project them through s_speed / s_sight.
        FieldDef {
            key: "speed".to_owned(),
            label: "Speed".to_owned(),
            default: FieldValue::Int(5),
        },
        FieldDef {
            key: "sight".to_owned(),
            label: "Sight".to_owned(),
            default: FieldValue::Int(6),
        },
        // The DC to win this creature over (`convince`). A wary monster is
        // higher; a wavering one lower. The DM sets it per creature.
        FieldDef {
            key: "will".to_owned(),
            label: "Resolve".to_owned(),
            default: FieldValue::Int(12),
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
        // A check is a number for the table to read; it names no victim and
        // changes nothing.
        target: None,
    };
    let actions = vec![
        ActionDef {
            key: "attack".to_owned(),
            label: "Attack".to_owned(),
            base: "1d20".to_owned(),
            func: "a_attack".to_owned(),
            target: Some(TargetSpec {
                // Adjacent melee. Reach weapons and ranged attacks are the same
                // spec with a larger number.
                range: 1,
                hit_func: "a_attack_hit".to_owned(),
                damage: "1d8".to_owned(),
                damage_func: "a_attack_dmg".to_owned(),
                damage_field: "hp_current".to_owned(),
                actor_beat: "strike".to_owned(),
                hit_beat: "recoil".to_owned(),
                miss_beat: "dodge".to_owned(),
                fall_beat: "fall".to_owned(),
                // A solid blow rocks you back. Purely a flourish: 5e melee does
                // not move anybody, and nor does this.
                stagger_func: Some("a_attack_stagger".to_owned()),
                stagger_beat: "staggered".to_owned(),
                push_func: None,
                push_beat: "shoved".to_owned(),
                condition_on_hit: None,
                recruit_on_hit: false,
            }),
        },
        // The other half of the contrast, and the reason both exist: a shove
        // *actually* moves you. No damage, no stagger, a real tile change that
        // every peer applies and that changes what you can reach and see.
        ActionDef {
            key: "shove".to_owned(),
            label: "Shove".to_owned(),
            base: "1d20".to_owned(),
            func: "a_attack".to_owned(),
            target: Some(TargetSpec {
                range: 1,
                hit_func: "a_attack_hit".to_owned(),
                damage: "0".to_owned(),
                damage_func: "a_zero".to_owned(),
                damage_field: "hp_current".to_owned(),
                actor_beat: "strike".to_owned(),
                hit_beat: "recoil".to_owned(),
                miss_beat: "dodge".to_owned(),
                fall_beat: "fall".to_owned(),
                stagger_func: None,
                stagger_beat: "staggered".to_owned(),
                push_func: Some("a_shove_push".to_owned()),
                push_beat: "shoved".to_owned(),
                condition_on_hit: None,
                recruit_on_hit: false,
            }),
        },
        // Trip: the first condition-inflicting action. No damage, no shove: a
        // hit knocks the target prone, and prone is what changes the game (half
        // speed until it stands).
        ActionDef {
            key: "trip".to_owned(),
            label: "Trip".to_owned(),
            base: "1d20".to_owned(),
            func: "a_attack".to_owned(),
            target: Some(TargetSpec {
                range: 1,
                hit_func: "a_attack_hit".to_owned(),
                damage: "0".to_owned(),
                damage_func: "a_zero".to_owned(),
                damage_field: "hp_current".to_owned(),
                actor_beat: "strike".to_owned(),
                hit_beat: "recoil".to_owned(),
                miss_beat: "dodge".to_owned(),
                fall_beat: "fall".to_owned(),
                stagger_func: None,
                stagger_beat: "staggered".to_owned(),
                push_func: None,
                push_beat: "shoved".to_owned(),
                condition_on_hit: Some("prone".to_owned()),
                recruit_on_hit: false,
            }),
        },
        // Convince: win a creature over to your side. A social action shaped
        // exactly like an attack -- a roll against a resolve DC -- but its
        // consequence is allegiance, not damage. The rules only say the pitch
        // landed; the host changes the owner and enforces the party cap, because
        // both are the map's business, not the sheet's. Ranged: you can talk
        // from a few tiles away.
        ActionDef {
            key: "convince".to_owned(),
            label: "Convince".to_owned(),
            base: "1d20".to_owned(),
            func: "a_convince".to_owned(),
            target: Some(TargetSpec {
                range: 4,
                hit_func: "a_convince_hit".to_owned(),
                damage: "0".to_owned(),
                damage_func: "a_zero".to_owned(),
                damage_field: "hp_current".to_owned(),
                actor_beat: "cheer".to_owned(),
                hit_beat: "cheer".to_owned(),
                miss_beat: "shrug".to_owned(),
                fall_beat: "fall".to_owned(),
                stagger_func: None,
                stagger_beat: "staggered".to_owned(),
                push_func: None,
                push_beat: "shoved".to_owned(),
                condition_on_hit: None,
                recruit_on_hit: true,
            }),
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

        -- The hit rule. This is the line that makes Isometry adjudicate rather
        -- than merely roll, and it lives in the system, not the substrate: the
        -- core never learns what AC is. `roll` is the actor's total (die +
        -- a_attack), `t` is the defender's sheet. Crits and fumbles need the
        -- raw die, which the ABI does not pass yet.
        function a_attack_hit(c, t, roll)
            if roll >= (t.ac or 12) then return 1 else return 0 end
        end
        function a_attack_dmg(c) return ab_mod(c.str) end

        -- Convince: a Charisma pitch against the target's resolve DC. This is
        -- the persuasion twin of a_attack/a_attack_hit -- same shape, social
        -- stat, and no damage. Winning is the host's to apply (an owner change).
        function a_convince(c) return ab_mod(c.cha) + c.prof end
        function a_convince_hit(c, t, roll)
            -- `or` the schema default, so a sheet saved before `will` existed
            -- (a pre-C5 campaign) resolves against 12 rather than erroring on nil.
            if roll >= (t.will or 12) then return 1 else return 0 end
        end

        function a_zero(c) return 0 end

        -- Force. Two different questions that happen to share a direction.
        --
        -- A stagger is a *flourish*: a solid blow rocks the victim off its feet
        -- and it recovers. It moves nobody, so it is free to differ between one
        -- table's screen and another's.
        function a_attack_stagger(c, t, dmg)
            if dmg >= 5 then return 1 else return 0 end
        end
        -- A shove is *truth*: the victim ends up on a different tile and stays
        -- there. The board decides where that is (a wall can stop it); the rules
        -- only say how far and which way.
        function a_shove_push(c, t, dmg) return 1 end

        -- Movement and senses, as rules. The substrate retired its hardcoded
        -- MOVE_BUDGET/SIGHT_RADIUS constants; these own the numbers now, and a
        -- condition is one more input. Conditions arrive as booleans on the
        -- character table (c.prone), so the projection is plain arithmetic.
        function s_speed(c)
            local v = c.speed
            if c.prone then
                local r = ((v % 2) + 2) % 2
                v = (v - r) // 2
            end
            if c.immobilized then v = 0 end
            return v
        end
        function s_sight(c)
            local v = c.sight
            if c.blinded then v = 0 end
            return v
        end

        -- Out of play. In 5e a creature at 0 hit points drops; a PC would roll
        -- death saves, which is a rule this system can grow without the
        -- substrate learning anything new (it only ever sees the verdict).
        function s_defeated(c)
            if c.hp_current <= 0 then return 1 else return 0 end
        end
    "#;
    System::load("5e-srd", "5e SRD", fields, derived, actions, script)
        .expect("builtin 5e system loads")
        .with_defeat("s_defeated")
        .with_mobility("s_speed", "s_sight")
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
    fn generator_returns_nested_tagged_lua_tables() {
        let script = r#"
            function call_gen(request_json, entropy, request)
                return {
                    type = "object",
                    fields = {
                        title = { type = "text", value = "river cache" },
                        contents = {
                            type = "list",
                            values = {
                                {
                                    type = "item",
                                    item = {
                                        template = "demo:river-blade",
                                        name = "River Blade",
                                        tags = { "weapon", "river" }
                                    }
                                }
                            }
                        }
                    }
                }
            end
        "#;
        let mut runtime = GeneratorRuntime::load(script, GeneratorLimits::default()).unwrap();
        let mut tape = EntropyTape::from_seed(4);
        let value = runtime.call(&generator_request(), &mut tape).unwrap().value;
        let GenValue::Object { fields } = value else {
            panic!("expected object proposal");
        };
        assert_eq!(
            fields.get("title"),
            Some(&GenValue::Text {
                value: "river cache".to_owned()
            })
        );
        assert!(matches!(
            fields.get("contents"),
            Some(GenValue::List { values }) if matches!(values.as_slice(), [GenValue::Item { .. }])
        ));
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
        assert_eq!(
            record.proposal,
            GenValue::Text {
                value: "forge".to_owned()
            }
        );
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

        let catalog = GeneratorCatalog::discover([&root]);
        assert!(catalog.diagnostics().is_empty());
        assert_eq!(catalog.choices()[0].id, "demo:forge_item");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_core_pack_supplies_the_beat_vocabulary() {
        // The app ships no choreography of its own: the default beats come from
        // the `core` pack on disk, exactly like a campaign's would.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/core");
        let catalog = GeneratorCatalog::discover([root]);
        let (beats, diagnostics) = catalog.choreography();
        assert!(diagnostics.is_empty(), "core pack stylesheets all open");

        // A rules-produced beat carries real CSS and no emote label...
        let strike = beats.iter().find(|b| b.name == "strike").expect("strike");
        assert!(strike.css.contains("@keyframes"));
        assert!(strike.emote.is_none(), "no one performs a strike on demand");

        // ...while a social beat is emotable, which is what puts it in the menu.
        let cheer = beats.iter().find(|b| b.name == "cheer").expect("cheer");
        assert_eq!(cheer.emote.as_deref(), Some("Cheer"));

        // The emote vocabulary is exactly the emotable beats.
        let emotes: Vec<&str> = beats
            .iter()
            .filter(|b| b.emote.is_some())
            .map(|b| b.name.as_str())
            .collect();
        assert_eq!(emotes, ["cheer", "shrug", "taunt"]);
    }

    #[test]
    fn a_later_pack_overrides_a_beat_by_name() {
        // A campaign restyles the swing by shipping its own `strike`; the last
        // pack to declare a name wins, so nothing in the app has to change.
        let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/core");
        let core_only = GeneratorCatalog::discover([core.clone()]);
        let (base, _) = core_only.choreography();
        let base_strike = &base.iter().find(|b| b.name == "strike").unwrap().css;
        assert!(base_strike.contains("iso-strike"));
        // (An override test needs a second on-disk pack; the override *rule* is
        // covered by the catalog unit below, which does not touch the disk.)
        let names: Vec<&str> = base.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"fall"));
    }

    #[test]
    fn demo_pack_composes_an_inspectable_campaign_draft() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/demo");
        let pack = GeneratorPack::load(root).unwrap();
        let request = GeneratorRequest {
            generator: "demo:campaign".to_owned(),
            args: GenValue::Text {
                value: "river".to_owned(),
            },
            locks: BTreeMap::new(),
        };
        let mut tape = EntropyTape::from_seed(17);
        let record = pack
            .generate(
                "generated.demo.campaign.1",
                &request,
                &mut tape,
                GeneratorLimits::default(),
            )
            .unwrap();
        let GenValue::Campaign { campaign } = record.proposal else {
            panic!("expected campaign draft");
        };
        campaign.validate().unwrap();
        assert_eq!(campaign.maps.len(), 3);
        assert_eq!(campaign.world.factions.len(), 2);
        assert_eq!(campaign.secrets.len(), 1);
        assert!(campaign.world.laws.contains_key("iron-remembers"));
        assert!(campaign
            .world
            .storylets
            .contains_key(&campaign.final_storylet));
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

    /// A knight who reliably hits, and a victim whose AC is the only variable.
    fn duel(target_ac: i64, target_hp: i64) -> (System, SheetData, SheetData) {
        let sys = srd_5e();
        let mut knight = sys.default_sheet();
        knight.set_text("name", "Knight");
        knight.set_int("str", 16); // +3, plus prof 2 => 1d20+5
        let mut goblin = sys.default_sheet();
        goblin.set_text("name", "Goblin");
        goblin.set_int("ac", target_ac);
        goblin.set_int("hp_current", target_hp);
        goblin.set_int("hp_max", target_hp);
        (sys, knight, goblin)
    }

    const KNIGHT: TokenId = TokenId(1);
    const GOBLIN: TokenId = TokenId(2);

    #[test]
    fn a_hit_subtracts_from_the_target_and_nothing_else() {
        // AC 1: the attack cannot fail, so this isolates the consequence.
        let (mut sys, knight, goblin) = duel(1, 7);
        let mut rng = Rng::new(42);
        let r = sys
            .resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut rng)
            .expect("resolves");

        assert!(r.hit);
        assert_eq!(r.attack.expr, "1d20+5");
        let dmg = r.damage.as_ref().expect("a hit rolls damage");
        assert!(dmg.total > 0, "damage never heals");
        // Exactly one consequence, and it lands on the victim's hit points.
        assert_eq!(r.deltas.len(), 1);
        assert_eq!(r.deltas[0].token, GOBLIN);
        assert_eq!(r.deltas[0].key, "hp_current");
        assert_eq!(r.deltas[0].add, -(dmg.total as i64));
        // And it represents itself. A solid blow (5+) rocks the victim off its
        // feet rather than merely flinching; either way nothing has moved.
        assert_eq!(r.beats.len(), 2);
        assert_eq!(r.beats[0], Beat::new(KNIGHT, "strike"));
        let expected = if dmg.total >= 5 { "staggered-e" } else { "recoil" };
        assert_eq!(r.beats[1], Beat::new(GOBLIN, expected));
        assert!(r.push.is_none(), "a plain attack moves nobody");
    }

    #[test]
    fn a_miss_changes_nothing() {
        // AC 100 is unreachable by 1d20+5.
        let (mut sys, knight, goblin) = duel(100, 7);
        let mut rng = Rng::new(42);
        let r = sys
            .resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut rng)
            .expect("resolves");

        assert!(!r.hit);
        assert!(r.damage.is_none());
        assert!(r.deltas.is_empty(), "a miss must not touch game state");
        assert_eq!(r.beats[1], Beat::new(GOBLIN, "dodge"));
    }

    #[test]
    fn a_fixed_entropy_tape_yields_an_identical_resolution() {
        // The property the whole replication model rests on: one machine
        // resolves, every other machine applies, and they agree.
        let (mut a, knight, goblin) = duel(12, 7);
        let (mut b, _, _) = duel(12, 7);
        let first = a
            .resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut Rng::new(7))
            .expect("resolves");
        let second = b
            .resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut Rng::new(7))
            .expect("resolves");
        assert_eq!(first, second);
    }

    #[test]
    fn an_invalid_intent_is_refused_before_any_die_is_rolled() {
        let (mut sys, knight, goblin) = duel(1, 7);
        let mut rng = Rng::new(42);

        // Out of reach: melee has range 1.
        assert_eq!(
            sys.resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (3, 0), &mut rng),
            Err(ActionError::OutOfRange {
                range: 1,
                distance: 3
            })
        );
        // No hitting yourself.
        assert_eq!(
            sys.resolve_action("attack", KNIGHT, &knight, (0, 0), KNIGHT, &knight, (0, 0), &mut rng),
            Err(ActionError::SelfTarget)
        );
        // An ability check names no victim, so it cannot be resolved at one.
        assert_eq!(
            sys.resolve_action("str_check", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut rng),
            Err(ActionError::NotTargeted("str_check".to_owned()))
        );
        assert!(sys.is_targeted("attack"));
        assert!(!sys.is_targeted("str_check"));

        // The rng was never drawn from, so a refused intent is truly inert.
        let mut fresh = Rng::new(42);
        let a = sys
            .resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut rng)
            .expect("resolves");
        let b = sys
            .resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut fresh)
            .expect("resolves");
        assert_eq!(a, b);
    }

    #[test]
    fn a_killing_blow_puts_the_target_out_of_play_and_it_falls() {
        // AC 1 so it always lands; 1 hit point so any damage is lethal.
        let (mut sys, knight, goblin) = duel(1, 1);
        let mut rng = Rng::new(42);
        let r = sys
            .resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut rng)
            .expect("resolves");

        assert!(r.hit);
        assert_eq!(r.defeated, vec![GOBLIN]);
        // It falls rather than flinching: the beat follows the outcome.
        assert_eq!(r.beats[1], Beat::new(GOBLIN, "fall"));
    }

    #[test]
    fn a_survivable_hit_does_not_defeat() {
        // 50 hit points: a longsword is not going to do it.
        let (mut sys, knight, goblin) = duel(1, 50);
        let r = sys
            .resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut Rng::new(42))
            .expect("resolves");
        assert!(r.hit);
        assert!(r.defeated.is_empty());
        let dmg = r.damage.as_ref().expect("a hit rolls damage").total;
        let expected = if dmg >= 5 { "staggered-e" } else { "recoil" };
        assert_eq!(r.beats[1], Beat::new(GOBLIN, expected), "still standing");
    }

    #[test]
    fn a_corpse_is_not_a_target() {
        // Already at zero: the system says it is out of play.
        let (mut sys, knight, goblin) = duel(15, 0);
        assert!(sys.is_defeated(&goblin));
        let mut rng = Rng::new(42);
        assert_eq!(
            sys.resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut rng),
            Err(ActionError::AlreadyDefeated)
        );
        // Refused before any die is rolled, so the swing costs nothing.
        let a = sys
            .resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &sys_sheet_alive(), (1, 0), &mut rng)
            .expect("a living target still resolves");
        let b = sys
            .resolve_action(
                "attack",
                KNIGHT,
                &knight,
                (0, 0), GOBLIN,
                &sys_sheet_alive(),
                (1, 0),
                &mut Rng::new(42),
            )
            .expect("resolves");
        assert_eq!(a, b, "the refused swing must not have drawn from the rng");
    }

    /// A living stand-in victim (AC 1, plenty of hit points).
    fn sys_sheet_alive() -> SheetData {
        let mut s = srd_5e().default_sheet();
        s.set_text("name", "Goblin");
        s.set_int("ac", 1);
        s.set_int("hp_current", 50);
        s.set_int("hp_max", 50);
        s
    }

    /// The distinction the whole force design rests on. Both come out of one
    /// resolution, and only one of them is allowed to touch the game.
    #[test]
    fn a_stagger_is_a_flourish_and_a_shove_is_the_truth() {
        // A solid hit staggers: the victim is rocked off its feet, in the
        // direction the blow came from, and *nothing moves*.
        let (mut sys, knight, goblin) = duel(1, 50);
        let hit = sys
            .resolve_action(
                "attack",
                KNIGHT,
                &knight,
                (4, 4),
                GOBLIN,
                &goblin,
                (5, 4), // due east of the knight
                &mut Rng::new(3),
            )
            .expect("resolves");
        assert!(hit.hit);
        assert_eq!(hit.beats[1], Beat::new(GOBLIN, "staggered-e"), "shoved east");
        assert!(
            hit.push.is_none(),
            "a stagger must not move anybody: it is representation, and state that \
             came out of a flourish could not be agreed on"
        );

        // A shove is the other thing entirely: real forced movement, one tile,
        // and the rules only say how far and which way.
        let (mut sys, knight, goblin) = duel(1, 50);
        let shove = sys
            .resolve_action(
                "shove",
                KNIGHT,
                &knight,
                (4, 4),
                GOBLIN,
                &goblin,
                (5, 4),
                &mut Rng::new(3),
            )
            .expect("resolves");
        assert!(shove.hit);
        assert_eq!(shove.push, Some(((1, 0), 1)), "one tile, due east");
        assert_eq!(shove.beats[1], Beat::new(GOBLIN, "shoved-e"));
        // And it does no damage, so it changes position and nothing else.
        assert!(shove.deltas.iter().all(|d| d.add == 0));
    }

    #[test]
    fn the_board_rules_on_where_a_shove_lands() {
        // The system says "one tile east". The substrate is what knows there is
        // a wall there, so `push_path` is where the shove actually stops.
        let blocked = isometry_core::push_path((5, 4), (1, 0), 1, |_| false);
        assert_eq!(blocked, None, "shoved into a wall: nobody moves");
        let clear = isometry_core::push_path((5, 4), (1, 0), 2, |_| true);
        assert_eq!(clear, Some((7, 4)), "two clear tiles east");
        // Stopped short by an obstacle on the second tile.
        let short = isometry_core::push_path((5, 4), (1, 0), 2, |at| at == (6, 4));
        assert_eq!(short, Some((6, 4)));
    }

    #[test]
    fn a_trip_inflicts_prone_and_the_rules_recompute_mobility() {
        // AC 1: the trip cannot miss, so this isolates the consequence.
        let (mut sys, knight, goblin) = duel(1, 50);
        let r = sys
            .resolve_action("trip", KNIGHT, &knight, (4, 4), GOBLIN, &goblin, (5, 4), &mut Rng::new(3))
            .expect("resolves");
        assert!(r.hit);
        // No damage: prone IS the consequence.
        assert!(r.deltas.iter().all(|d| d.add == 0));
        assert_eq!(r.conditions, vec![(GOBLIN, "prone".to_owned(), true)]);
        // The projection travels with the change: base speed 5 halves to 2,
        // sight untouched. Rules ran once, on the resolver.
        assert_eq!(r.mobility, vec![(GOBLIN, Some((2, 6)))]);
    }

    #[test]
    fn tripping_the_already_prone_is_not_a_new_condition() {
        let (mut sys, knight, goblin) = duel(1, 50);
        // The caller passes the target sheet with its condition booleans on it,
        // which is how the resolver can tell "apply" from "already there".
        let prone = sheet_with_conditions(&goblin, std::iter::once(&"prone".to_owned()));
        let r = sys
            .resolve_action("trip", KNIGHT, &knight, (4, 4), GOBLIN, &prone, (5, 4), &mut Rng::new(3))
            .expect("resolves");
        assert!(r.hit);
        assert!(r.conditions.is_empty(), "already prone: nothing new to apply");
        assert!(r.mobility.is_empty());
    }

    #[test]
    fn the_projection_is_lua_not_rust() {
        // Blinded is nowhere in the Rust: the system script owns what a
        // condition does to the numbers.
        let mut sys = srd_5e();
        let sheet = sys.default_sheet();
        let blinded = sheet_with_conditions(&sheet, std::iter::once(&"blinded".to_owned()));
        assert_eq!(sys.mobility_for(&blinded, true), Some((5, 0)), "dark, not slow");
        let immobilized =
            sheet_with_conditions(&sheet, std::iter::once(&"immobilized".to_owned()));
        assert_eq!(sys.mobility_for(&immobilized, true), Some((0, 6)), "slow, not dark");
        // No conditions: no override at all; the sheet's base numbers stand.
        assert_eq!(sys.mobility_for(&sheet, false), None);
    }

    #[test]
    fn the_npc_generator_yields_a_bestiary_backed_creature_that_can_be_statted() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/demo");
        let pack = GeneratorPack::load(root).unwrap();
        let request = GeneratorRequest {
            generator: "demo:npc".to_owned(),
            args: GenValue::Text { value: "wilds".to_owned() },
            locks: BTreeMap::new(),
        };
        let gen = |seed: u64| {
            let mut tape = EntropyTape::from_seed(seed);
            let record = pack
                .generate("generated.demo.npc.1", &request, &mut tape, GeneratorLimits::default())
                .unwrap();
            match record.proposal {
                GenValue::Npc { npc } => npc,
                other => panic!("expected an npc proposal, got {other:?}"),
            }
        };

        // The proposal's key is a real bestiary slug, so it lowers to a stat
        // block: this is the bridge `>gen npc` relies on.
        let npc = gen(1);
        let bestiary: Vec<_> = srd_bestiary().into_iter().map(|m| m.key).collect();
        assert!(
            bestiary.contains(&npc.key),
            "generated key {:?} is not a bestiary creature",
            npc.key
        );
        assert!(!npc.name.is_empty(), "an NPC needs a name");
        // And that creature really does stat up.
        let monster = srd_bestiary().into_iter().find(|m| m.key == npc.key).unwrap();
        let mut sheet = monster_sheet(&monster);
        sheet.set_text("name", npc.name.clone());
        assert!(sheet.int("hp_current").unwrap() > 0);
        assert_eq!(sheet.text("name"), Some(npc.name.as_str()));

        // Deterministic per seed; a different draw (reroll) can change the pick.
        assert_eq!(gen(1), gen(1), "same seed, same NPC");
        let differs = (1..8).any(|s| gen(s) != npc);
        assert!(differs, "reroll should be able to produce a different NPC");
    }

    #[test]
    fn convince_wins_a_creature_over_when_the_pitch_beats_its_resolve() {
        // The recruit is the system's to *report*, not to apply: it names the
        // target won over and leaves the owner change (and the cap) to the host.
        let mut sys = srd_5e();
        let mut bard = sys.default_sheet();
        bard.set_text("name", "Bard");
        bard.set_int("cha", 18); // +4, plus prof 2 => 1d20+6 to persuade
        bard.set_int("prof", 2);
        let mut goblin = sys.default_sheet();
        goblin.set_text("name", "Goblin");
        goblin.set_int("will", 1); // a pushover: the pitch cannot fail

        let r = sys
            .resolve_action("convince", KNIGHT, &bard, (4, 4), GOBLIN, &goblin, (6, 4), &mut Rng::new(9))
            .expect("resolves");
        assert!(r.hit);
        assert_eq!(r.recruited, Some(GOBLIN), "won over");
        // A social action does no harm.
        assert!(r.deltas.iter().all(|d| d.add == 0));
        assert!(r.defeated.is_empty());

        // A resolute creature (will 99) cannot be talked around.
        let mut wall = goblin.clone();
        wall.set_int("will", 99);
        let miss = sys
            .resolve_action("convince", KNIGHT, &bard, (4, 4), GOBLIN, &wall, (6, 4), &mut Rng::new(9))
            .expect("resolves");
        assert!(!miss.hit);
        assert!(miss.recruited.is_none(), "a failed pitch wins no one");
    }

    #[test]
    fn convince_falls_back_to_the_default_resolve_when_the_sheet_predates_will() {
        // A sheet saved before `will` existed (a pre-C5 campaign) has no such
        // field. The hit rule must resolve against the schema default (12), not
        // error on nil -- otherwise convince silently fails against every legacy
        // token.
        let mut sys = srd_5e();
        let mut bard = sys.default_sheet();
        bard.set_int("cha", 20); // +5, plus prof 2 => 1d20+7
        bard.set_int("prof", 2);
        // A bare sheet with only a name: no `will`, as an old save would be.
        let mut legacy = SheetData::new("5e-srd");
        legacy.set_text("name", "Old Goblin");
        assert!(legacy.int("will").is_none(), "the legacy sheet has no will");

        // Must resolve (not ScriptFailed) and behave as DC 12.
        let r = sys
            .resolve_action("convince", KNIGHT, &bard, (4, 4), GOBLIN, &legacy, (5, 4), &mut Rng::new(1))
            .expect("resolves against the default DC, not an error");
        // The roll landed or missed against 12; either way, no script failure.
        assert_eq!(r.recruited.is_some(), r.hit);
    }

    #[test]
    fn only_a_recruit_action_reports_a_recruit() {
        // A plain attack must never set `recruited`, or the host would change
        // ownership on every hit.
        let (mut sys, knight, goblin) = duel(1, 50);
        let r = sys
            .resolve_action("attack", KNIGHT, &knight, (4, 4), GOBLIN, &goblin, (5, 4), &mut Rng::new(3))
            .expect("resolves");
        assert!(r.hit);
        assert!(r.recruited.is_none());
    }

    #[test]
    fn a_spawned_goblin_arrives_statted() {
        let goblin = srd_bestiary()
            .into_iter()
            .find(|m| m.name == "Goblin")
            .expect("goblin in the SRD bestiary");
        let sheet = monster_sheet(&goblin);
        // The stat block reaches the sheet, which is what makes it attackable.
        assert_eq!(sheet.int("hp_current"), Some(7));
        assert_eq!(sheet.int("hp_max"), Some(7));
        assert_eq!(sheet.int("ac"), Some(15));
        assert_eq!(sheet.text("name"), Some("Goblin"));
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
