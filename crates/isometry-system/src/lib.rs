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
mod pf2e;
mod spells;
mod sys;

// The machinery moved into `sys` on 2026-07-24; its public vocabulary stays at
// the crate root, so consumers see the same API as before the split.
pub use sys::*;
pub use bestiary::{srd_bestiary, Monster, MonsterAction};
pub use items::{srd_items, Item};
pub use pf2e::pf2e_srd;
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
    /// Lua `f(c, t, roll) -> degree`: given the actor, the target, and the
    /// actor's total roll, how well did it land? This is where "beats AC" lives.
    ///
    /// The return is a **degree of success**, not a boolean: `2` critical
    /// success, `1` success, `0` failure, `-1` critical failure. Anything `>= 1`
    /// is a hit. A binary system simply returns 1 or 0 and never sees the
    /// difference (5e's `a_attack_hit` is unchanged); a four-tier system (PF2e,
    /// and the whole roll-and-compare family) returns the full range. This costs
    /// no ABI change, because the Lua boundary already returns an integer.
    pub hit_func: String,
    /// Dice rolled for effect on a hit.
    pub damage: String,
    /// Lua `f(c, t) -> int`: the effect's flat bonus.
    pub damage_func: String,
    /// Lua `f(c, t, degree) -> percent`: scale the effect by the degree. `200`
    /// doubles (a PF2e critical hit), `50` halves (5e's save-for-half), `100` is
    /// the default when no function is declared.
    ///
    /// A percent rather than a float because the Lua boundary is integers only,
    /// and a multiplier rather than a bonus because doubling *dice plus
    /// modifiers* is not expressible as an addend.
    pub damage_mult_func: Option<String>,
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
    /// Lua `f(c, t, degree) -> magnitude` for a condition that has one:
    /// PF2e's Demoralize inflicts `frightened 2` on a critical success and
    /// `frightened 1` on a success, so the number rides the degree ladder.
    /// `None` means the plain on/off condition of `condition_on_hit`, stored as
    /// magnitude 1. Opt-in, like the degree and damage-multiplier hooks: a
    /// system that never needed graded conditions writes nothing.
    pub condition_value_func: Option<String>,
    /// Whether a hit wins the target over to the actor's side (`convince`). Like
    /// a push, the rules only say *that* it happened; the host rules the rest,
    /// because allegiance lives on the token's owner (which the system never
    /// sees) and the party cap is table policy. The resolver reports the win;
    /// the host decides the new owner and whether the party has room.
    pub recruit_on_hit: bool,
    /// Lua `f(c) -> 1|0`: can the actor afford this action *right now*, given its
    /// per-turn counters (injected as `c.turn_<key>`) and its sheet? `None`
    /// means always affordable, so a system with no action economy (5e) never
    /// pays for one. This is where "you have an action left" lives -- entirely
    /// in the ruleset, over the substrate's blind counter store.
    pub afford_func: Option<String>,
    /// Counters the actor spends or counts by on a resolved action, as
    /// `(counter, delta)`: a PF2e Strike is `[("actions_spent", 1),
    /// ("strikes", 1)]`. The substrate applies these to the actor's per-turn
    /// ledger and resets them when its turn begins; what they cost and mean is
    /// the ruleset's.
    pub turn_effect: Vec<(String, i64)>,
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
    /// The actor's per-turn budget will not cover this action (out of actions).
    CannotAfford(String),
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
    /// How well it landed: `2` critical success, `1` success, `0` failure, `-1`
    /// critical failure. [`Self::hit`] is simply `degree >= 1`, so a binary
    /// system never has to think about this and a four-tier one is expressible
    /// without a second resolver.
    pub degree: i64,
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
    /// Conditions this action applied: `(token, name, magnitude)`. A magnitude
    /// of 0 clears the condition; on/off conditions apply as 1.
    pub conditions: Vec<(TokenId, String, i64)>,
    /// The recomputed `(move budget, sight radius)` for each token whose
    /// conditions changed; `None` clears back to sheet base.
    pub mobility: Vec<(TokenId, Option<(u32, u32)>)>,
    /// The target was won over: the host should hand it to the actor's side (if
    /// the party has room). `None` for every action that is not a recruit, and
    /// for a recruit that missed.
    pub recruited: Option<TokenId>,
    /// Per-turn counters this action spent or counted, as `(token, counter,
    /// delta)` on the actor. The host applies them to the substrate's per-turn
    /// ledger; they reset at turn start.
    pub turn_counters: Vec<(TokenId, String, i64)>,
}

/// The outcome of one leg of overmap travel, as the system rules it: the travel
/// analogue of [`Resolution`]. The substrate applies it (advance the clock by
/// `ticks`, move the party) and no peer reruns the Lua. E0/E1 primitives feed it
/// (the route weight, the party's pace); the system decides how well the party
/// found its way. Attrition and encounters (E4/E5) will add fields; E2 is the
/// navigation outcome and the time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TravelResolution {
    /// The navigation roll, shown to the table like an attack roll.
    pub roll: RollRecord,
    /// The travel time in ticks the system ruled: the base cost, plus a penalty
    /// when the party loses its way.
    pub ticks: u64,
    /// Whether the party navigated poorly (took longer than the smooth cost).
    /// Legible for narration, and the reason the trip cost more.
    pub lost: bool,
    /// The exhaustion the march tolled: a graded condition (the same primitive as
    /// `frightened 2`) the host applies to every party member, worsening what
    /// they already carry. Zero for a trip short enough to shrug off. What counts
    /// as a long march is the system's `toll_func`, not the substrate's.
    pub exhaustion: i64,
    /// Whether the road threw an encounter: if so, the host drops the party onto
    /// the destination's tactical map to fight rather than arriving in peace. The
    /// system's `encounter_func` decides; the substrate only obeys.
    pub encounter: bool,
    /// Food the party foraged on the road (a Forage stance and a good roll), for
    /// the host to add to the party's stores. Zero when nobody foraged or the
    /// roll came up empty. What "food" is and what it is worth is the system's.
    pub forage: i64,
}

/// Copy `sheet` with each active condition added as a boolean field, so Lua
/// reads `c.prone` with no new marshalling: conditions ride the existing
/// character table. A sheet field with the same name would be shadowed, which is
/// why condition names are validated against the schema's field keys by packs
/// rather than guarded here.
pub fn sheet_with_conditions<'a>(
    sheet: &SheetData,
    conditions: impl IntoIterator<Item = (&'a String, &'a i64)>,
) -> SheetData {
    let mut out = sheet.clone();
    for (name, value) in conditions {
        // The magnitude, injected as an integer, so `frightened 2` reads
        // `c.frightened == 2` and a plain on/off `prone` reads 1. Only present
        // conditions arrive here (zero is never stored), so `if c.frightened`
        // stays a clean truthy/nil test -- no truthy Int(0) to trip a script.
        out.fields.insert(name.clone(), FieldValue::Int(*value));
    }
    out
}

/// Copy `sheet` with each per-turn counter injected as a `turn_<key>` integer
/// field, so a script reads `c.turn_actions_spent` or `c.turn_strikes` through
/// the existing flat character table -- no new marshalling, the same trick
/// conditions use. An absent counter is simply not injected and reads as nil,
/// which a script `or`s to zero.
pub fn sheet_with_turn_counters<'a>(
    sheet: &SheetData,
    counters: impl IntoIterator<Item = (&'a String, &'a i64)>,
) -> SheetData {
    let mut out = sheet.clone();
    for (key, value) in counters {
        out.fields.insert(format!("turn_{key}"), FieldValue::Int(*value));
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
    /// Lua `f(c, t, roll, weight) -> percent` for overmap travel: how well the
    /// party navigates a route. 100 is a smooth trip (the base time stands); more
    /// is losing the way, which costs that percent of the base. `None` means the
    /// party always finds its way (a system with no wilderness rules). The `t`
    /// slot is nil (travel has no target); `roll` is a d20 and `weight` the
    /// route's difficulty, so the DC can rise with the road.
    pub nav_func: Option<String>,
    /// Lua `f(c, t, ticks) -> exhaustion` for overmap travel: how tired a march
    /// of `ticks` leaves the party (a graded condition the host applies to every
    /// member). `None` means travel never tires (a system with no attrition).
    pub toll_func: Option<String>,
    /// Lua `f(c, t, roll, ticks) -> 1|0` for overmap travel: did the road throw
    /// an encounter? A fresh d20 makes it a chance the trip's length shifts.
    /// `None` means a safe road (a system with no wandering perils).
    pub encounter_func: Option<String>,
    /// Lua `f(c, t, roll) -> 1|0`: can this reader make sense of a map? The
    /// literacy/skill check behind "a dumb character cannot read a map" -- a low
    /// enough reader fails and learns nothing. `None` means anyone can read one.
    pub map_read_func: Option<String>,
    /// Lua `f(c, t, roll) -> food` for overmap travel: what the navigator forages
    /// on the road (reading its own `c.stance`, so it yields only when Foraging).
    /// `None` means travel gathers no food.
    pub forage_func: Option<String>,
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

#[cfg(test)]
mod tests;
