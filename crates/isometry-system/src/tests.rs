//! Tests for the system-plugin lane.
//!
//! They drive `System` end to end (schema, scripted rules, and the SRD pack
//! together) rather than any one split module, so they sit beside the crate
//! root instead of inside a piece of `sys`.
//!
//! Split out of `lib.rs` on 2026-07-24; unchanged.

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
    // Seed 3 rolls a 16: a plain hit, neither a natural 1 (which always
    // misses) nor a natural 20 (which crits). 50 hit points so the blow
    // cannot fell it -- this test is about the delta, not about defeat.
    let (mut sys, knight, goblin) = duel(1, 50);
    let mut rng = Rng::new(3);
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
    let mut rng = Rng::new(3);
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
        .resolve_action("attack", KNIGHT, &knight, (0, 0), GOBLIN, &goblin, (1, 0), &mut Rng::new(3))
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
    assert_eq!(r.conditions, vec![(GOBLIN, "prone".to_owned(), 1)]);
    // The projection travels with the change: base speed 5 halves to 2,
    // sight untouched. Rules ran once, on the resolver.
    assert_eq!(r.mobility, vec![(GOBLIN, Some((2, 6)))]);
}

#[test]
fn tripping_the_already_prone_is_not_a_new_condition() {
    let (mut sys, knight, goblin) = duel(1, 50);
    // The caller passes the target sheet with its condition booleans on it,
    // which is how the resolver can tell "apply" from "already there".
    let prone = sheet_with_conditions(&goblin, std::iter::once((&"prone".to_owned(), &1i64)));
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
    let blinded = sheet_with_conditions(&sheet, std::iter::once((&"blinded".to_owned(), &1i64)));
    assert_eq!(sys.mobility_for(&blinded, true), Some((5, 0)), "dark, not slow");
    let immobilized =
        sheet_with_conditions(&sheet, std::iter::once((&"immobilized".to_owned(), &1i64)));
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

/// A PF2e Strike against a known AC, with the d20 forced by choosing the
/// attacker's bonus: the skeleton's whole job is proving the four-rung
/// ladder and crit-doubling ride the *same* resolver 5e uses.
fn pf2e_strike(attack_bonus_str: i64, target_ac: i64, seed: u64) -> Resolution {
    let mut sys = pf2e_srd();
    let mut fighter = sys.default_sheet();
    fighter.set_text("name", "Fighter");
    fighter.set_int("str", attack_bonus_str);
    fighter.set_int("level", 1);
    fighter.set_int("rank_attack", 2);
    let mut foe = sys.default_sheet();
    foe.set_text("name", "Foe");
    foe.set_int("ac", target_ac);
    foe.set_int("hp_current", 200); // survives, so defeat never masks a degree
    foe.set_int("hp_max", 200);
    sys.resolve_action(
        "strike",
        KNIGHT,
        &fighter,
        (4, 4),
        GOBLIN,
        &foe,
        (5, 4),
        &mut Rng::new(seed),
    )
    .expect("resolves")
}

#[test]
fn pf2e_strike_reports_four_degrees_of_success() {
    // STR 30 (+10) and trained at level 1 (+3) is 1d20+13, so the lowest
    // possible total (14) still beats AC 1 by 10: always a critical.
    let crit = pf2e_strike(30, 1, 4);
    assert_eq!(crit.degree, 2, "beat the AC by 10 or more");
    assert!(crit.hit);

    // AC 100: even a 20 misses by 10+, so it always critically fails.
    let fumble = pf2e_strike(10, 100, 4);
    assert_eq!(fumble.degree, -1, "missed the AC by 10 or more");
    assert!(!fumble.hit);
    assert!(fumble.damage.is_none());

    // A plain success and a plain failure are the two middle rungs, and
    // `hit` reads them exactly as a binary system always did.
    let (mut saw_success, mut saw_failure) = (false, false);
    for seed in 1..40u64 {
        // STR 10 (+0), level 1, trained (+3) => 1d20+3 against AC 13:
        // rolls 10..19 succeed but never by 10; 1..9 fail but never by 10.
        let r = pf2e_strike(10, 13, seed);
        match r.degree {
            1 => {
                saw_success = true;
                assert!(r.hit);
            }
            0 => {
                saw_failure = true;
                assert!(!r.hit);
            }
            _ => {}
        }
    }
    assert!(saw_success && saw_failure, "the middle rungs are reachable");
}

#[test]
fn a_pf2e_critical_doubles_the_whole_effect() {
    // 1d20+13 against AC 1 always crits (the lowest total, 14, beats it by
    // 10). Re-run with the AC set to exactly the roll: the same seed rolls
    // the same die and the same damage, but the total now *meets* the AC
    // without beating it by 10, so it is a plain success. The only thing
    // that differs between the two is the degree, and therefore the
    // multiplier.
    let crit = pf2e_strike(30, 1, 11);
    assert_eq!(crit.degree, 2);
    let rolled = crit.attack.total as i64;
    let plain = pf2e_strike(30, rolled, 11);
    assert_eq!(plain.degree, 1);
    let (c, p) = (
        crit.damage.as_ref().expect("crit damages").total,
        plain.damage.as_ref().expect("hit damages").total,
    );
    assert_eq!(c, p * 2, "a critical doubles dice and modifiers together");
    // And the log says why, rather than silently reporting a bigger number.
    assert!(crit.damage.unwrap().expr.contains("200%"));
}

#[test]
fn pf2e_demoralize_frightens_by_degree() {
    let mut sys = pf2e_srd();
    let mut bully = sys.default_sheet();
    bully.set_text("name", "Bully");
    bully.set_int("cha", 30); // +10, trained (+3) at level 1 => 1d20+13
    let mut foe = sys.default_sheet();
    foe.set_text("name", "Foe");
    foe.set_int("hp_current", 200); // no HP change can mask the point
    foe.set_int("hp_max", 200);

    // Will 1: 1d20+13 always beats it by 10, so it always critically
    // succeeds -- and a critical Demoralize inflicts frightened *2*. The
    // magnitude is a number off the degree ladder, not a name and not a
    // constant, and the action deals no damage: fear is the whole effect.
    foe.set_int("will", 1);
    let crit = sys
        .resolve_action("demoralize", KNIGHT, &bully, (4, 4), GOBLIN, &foe, (5, 4), &mut Rng::new(4))
        .expect("resolves");
    assert_eq!(crit.degree, 2);
    assert_eq!(crit.conditions, vec![(GOBLIN, "frightened".to_owned(), 2)]);
    assert!(crit.deltas.iter().all(|d| d.add == 0), "Demoralize deals no damage");

    // Will 14: 1d20+13 still always beats it, but only a natural-ish high
    // roll beats it by 10, so a plain success is reachable -- and a plain
    // success frightens by only 1. Same ladder, a different rung, a
    // different number.
    foe.set_int("will", 14);
    let mut saw_one = false;
    for seed in 1..60u64 {
        let r = sys
            .resolve_action("demoralize", KNIGHT, &bully, (4, 4), GOBLIN, &foe, (5, 4), &mut Rng::new(seed))
            .expect("resolves");
        if r.degree == 1 {
            assert_eq!(r.conditions, vec![(GOBLIN, "frightened".to_owned(), 1)]);
            saw_one = true;
            break;
        }
    }
    assert!(saw_one, "a plain success frightens by 1, not 2");
}

#[test]
fn a_frightened_striker_swings_at_a_penalty() {
    // The read side of the same magnitude. Frightened N is a status penalty
    // to everything, so a frightened Strike is at -N -- and the resolver
    // learns N by reading the injected condition, exactly as it reads any
    // other field. Same seed both times, so the only difference is the fear.
    let mut sys = pf2e_srd();
    let mut fighter = sys.default_sheet();
    fighter.set_text("name", "Fighter");
    fighter.set_int("str", 10); // +0, so the bonus is proficiency alone
    let mut foe = sys.default_sheet();
    foe.set_int("ac", 13);
    foe.set_int("hp_current", 200);
    foe.set_int("hp_max", 200);

    let plain = sys
        .resolve_action("strike", KNIGHT, &fighter, (4, 4), GOBLIN, &foe, (5, 4), &mut Rng::new(7))
        .expect("resolves");
    let afraid_sheet =
        sheet_with_conditions(&fighter, std::iter::once((&"frightened".to_owned(), &2i64)));
    let afraid = sys
        .resolve_action("strike", KNIGHT, &afraid_sheet, (4, 4), GOBLIN, &foe, (5, 4), &mut Rng::new(7))
        .expect("resolves");
    assert_eq!(
        plain.attack.total - afraid.attack.total,
        2,
        "frightened 2 is a -2 status penalty to the Strike"
    );
}

#[test]
fn pf2e_travel_costs_more_when_the_party_loses_the_way() {
    let mut sys = pf2e_srd();
    // A keen navigator (WIS 40, +15) beats any DC on an easy route: smooth
    // travel at the base time, whatever the roll.
    let mut scout = sys.default_sheet();
    scout.set_text("name", "Scout");
    scout.set_int("wis", 40);
    let smooth = sys.resolve_travel(&scout, 2, 100, &mut Rng::new(1));
    assert!(!smooth.lost, "a great navigator does not lose the way");
    assert_eq!(smooth.ticks, 2, "smooth travel is the base (weight 2, normal pace)");

    // A hopeless navigator (WIS 1, -5) on a hard route (weight 20, DC 32)
    // loses the way on any roll, and pays 150% of the base.
    let mut greenhorn = sys.default_sheet();
    greenhorn.set_text("name", "Greenhorn");
    greenhorn.set_int("wis", 1);
    let lost = sys.resolve_travel(&greenhorn, 20, 100, &mut Rng::new(1));
    assert!(lost.lost, "a hopeless navigator on a hard road loses the way");
    assert_eq!(lost.ticks, 30, "lost is 150% of the base 20");
}

#[test]
fn the_navigator_stance_changes_the_travel_outcome() {
    // A borderline navigator (WIS 10, +0) on a weight-4 route (DC 16): the
    // exploration stance is what tips it. Scouting ahead (+3) finds the way
    // on a roll where Searching every thicket (-2) loses it. Find a roll in
    // that flip zone over a fixed seed range.
    let mut sys = pf2e_srd();
    let mut flipped = false;
    for seed in 0..64u64 {
        let mut scout = sys.default_sheet();
        scout.set_int("wis", 10);
        scout.set_text("stance", "scout");
        let scout_lost = sys.resolve_travel(&scout, 4, 100, &mut Rng::new(seed)).lost;

        let mut searcher = sys.default_sheet();
        searcher.set_int("wis", 10);
        searcher.set_text("stance", "search");
        let search_lost = sys.resolve_travel(&searcher, 4, 100, &mut Rng::new(seed)).lost;

        if !scout_lost && search_lost {
            flipped = true;
            break;
        }
    }
    assert!(
        flipped,
        "on some roll, Scouting finds the way where Searching loses it"
    );
}

#[test]
fn a_long_march_tolls_the_party_exhaustion() {
    let mut sys = pf2e_srd();
    let mut scout = sys.default_sheet();
    scout.set_int("wis", 100); // never loses even a hard road, so ticks == base
    // A 20-tick march exhausts the party (level 2), a graded condition; a
    // short hop tires no one.
    let long = sys.resolve_travel(&scout, 20, 100, &mut Rng::new(1));
    assert_eq!(long.ticks, 20);
    assert_eq!(long.exhaustion, 2, "a long march tires the party");
    let short = sys.resolve_travel(&scout, 4, 100, &mut Rng::new(1));
    assert_eq!(short.exhaustion, 0, "a short hop tires no one");

    // 5e declares no toll rule, so its travel never tires.
    let mut plain = srd_5e();
    let sheet = plain.default_sheet();
    assert_eq!(
        plain.resolve_travel(&sheet, 20, 100, &mut Rng::new(1)).exhaustion,
        0,
        "a system with no attrition never tires"
    );
}

#[test]
fn a_long_road_throws_encounters_by_chance() {
    let mut sys = pf2e_srd();
    let mut scout = sys.default_sheet();
    scout.set_int("wis", 100); // never lost, so ticks == base
    // A 30-tick road always runs into something (d20 + 30 clears 25 on any
    // roll); a 1-tick hop never does (it would need a 24 on a d20).
    assert!(
        sys.resolve_travel(&scout, 30, 100, &mut Rng::new(1)).encounter,
        "a very long road always has perils"
    );
    assert!(
        !sys.resolve_travel(&scout, 1, 100, &mut Rng::new(1)).encounter,
        "a short hop is safe"
    );
    // A middling road (15 ticks) is a chance, not a certainty: over seeds,
    // both a safe passage and a peril occur.
    let (mut safe, mut peril) = (false, false);
    for seed in 0..60u64 {
        if sys.resolve_travel(&scout, 15, 100, &mut Rng::new(seed)).encounter {
            peril = true;
        } else {
            safe = true;
        }
        if safe && peril {
            break;
        }
    }
    assert!(safe && peril, "a middling road throws perils by chance, not always");

    // 5e declares no encounter rule, so its roads are safe.
    let mut plain = srd_5e();
    let sheet = plain.default_sheet();
    assert!(!plain.resolve_travel(&sheet, 30, 100, &mut Rng::new(1)).encounter);
}

#[test]
fn a_dull_reader_cannot_read_a_map() {
    let mut sys = pf2e_srd();
    // A scholar (INT 40, +15) reads any map: roll + 15 clears DC 15 always.
    let mut scholar = sys.default_sheet();
    scholar.set_int("int", 40);
    assert!(
        sys.read_map(&scholar, &mut Rng::new(1)),
        "a lettered reader makes sense of it"
    );
    // A brute (INT 1, -5) cannot: only a natural 20 would clear the DC, so
    // over a fixed seed range it fails to read the map -- and holds a map it
    // cannot use.
    let mut brute = sys.default_sheet();
    brute.set_int("int", 1);
    let mut failed = false;
    for seed in 0..64u64 {
        if !sys.read_map(&brute, &mut Rng::new(seed)) {
            failed = true;
            break;
        }
    }
    assert!(failed, "a dull-witted reader fails to read a map");

    // 5e declares no reading rule, so anyone can read a map.
    let mut plain = srd_5e();
    let sheet = plain.default_sheet();
    assert!(plain.read_map(&sheet, &mut Rng::new(1)), "no rule means anyone reads it");
}

#[test]
fn foraging_yields_food_only_when_you_forage() {
    let mut sys = pf2e_srd();
    // A capable forager (WIS 40) who took the Forage stance gathers food.
    let mut forager = sys.default_sheet();
    forager.set_int("wis", 40);
    forager.set_text("stance", "forage");
    assert_eq!(
        sys.resolve_travel(&forager, 4, 100, &mut Rng::new(1)).forage,
        2,
        "foraging on the road gathers food"
    );
    // The same navigator just walking (no stance) gathers nothing.
    let mut walker = sys.default_sheet();
    walker.set_int("wis", 40);
    assert_eq!(
        sys.resolve_travel(&walker, 4, 100, &mut Rng::new(1)).forage,
        0,
        "you gather food only if you forage"
    );

    // 5e declares no foraging rule.
    let mut plain = srd_5e();
    let sheet = plain.default_sheet();
    assert_eq!(plain.resolve_travel(&sheet, 4, 100, &mut Rng::new(1)).forage, 0);
}

#[test]
fn pace_feeds_the_travel_base_and_no_nav_rule_never_loses_the_way() {
    // Pace scales the base the system rules against; a keen navigator travels
    // it smoothly, so the ticks track the pace-scaled base directly.
    let mut sys = pf2e_srd();
    let mut scout = sys.default_sheet();
    scout.set_int("wis", 40);
    assert_eq!(sys.resolve_travel(&scout, 4, 50, &mut Rng::new(2)).ticks, 2, "fast halves the base");
    assert_eq!(sys.resolve_travel(&scout, 4, 200, &mut Rng::new(2)).ticks, 8, "slow doubles it");

    // 5e declares no nav rule, so the party always finds its way at base cost.
    let mut plain = srd_5e();
    let sheet = plain.default_sheet();
    let calm = plain.resolve_travel(&sheet, 6, 100, &mut Rng::new(3));
    assert!(!calm.lost, "a system with no nav rule never loses the way");
    assert_eq!(calm.ticks, 6, "and always pays the base");
}

/// The action economy and the multiple-attack penalty, both proven against
/// the one per-turn counter primitive. The host would inject the running
/// counters into the sheet; here the test does it by hand, so a Strike sees
/// how many actions it has spent and how many times it has struck.
#[test]
fn pf2e_action_economy_and_map_ride_the_turn_counters() {
    let mut sys = pf2e_srd();
    // A quickened fighter (5 actions) so the multiple-attack penalty can be
    // watched past the point the three-action budget would cut it off --
    // proving the penalty and the economy are independent counters.
    let mut base = sys.default_sheet();
    base.set_int("actions_per_turn", 5);
    let foe = {
        let mut f = sys.default_sheet();
        f.set_int("ac", 10);
        f.set_int("hp_current", 500);
        f.set_int("hp_max", 500);
        f
    };
    // A per-turn counter ledger the test advances as the host would.
    let mut counters: std::collections::BTreeMap<String, i64> = Default::default();
    let strike = |sys: &mut System, counters: &std::collections::BTreeMap<String, i64>| {
        let sheet = sheet_with_turn_counters(&base, counters.iter());
        sys.resolve_action(
            "strike",
            KNIGHT,
            &sheet,
            (4, 4),
            GOBLIN,
            &foe,
            (5, 4),
            &mut Rng::new(7),
        )
    };

    // The multiple-attack penalty is in the *bonus*, so the same die gives a
    // lower total on each successive Strike: 0, -5, -10, then -10 (capped).
    let mut totals = Vec::new();
    for _ in 0..4 {
        let r = strike(&mut sys, &counters).expect("affordable while actions remain");
        totals.push(r.attack.total);
        // Apply this Strike's counter effect, as the host would.
        for (_, key, delta) in &r.turn_counters {
            *counters.entry(key.clone()).or_insert(0) += delta;
        }
    }
    assert_eq!(
        totals[0] - totals[1],
        5,
        "the second Strike takes -5 from the multiple-attack penalty"
    );
    assert_eq!(totals[1] - totals[2], 5, "the third takes -10");
    assert_eq!(totals[2], totals[3], "the penalty caps at -10");

    // The economy, now with a plain three-action fighter. Spend down from
    // an empty turn: three Strikes are affordable, the fourth is not.
    let plain = sys.default_sheet(); // actions_per_turn defaults to 3
    let strike3 = |sys: &mut System, spent: &std::collections::BTreeMap<String, i64>| {
        sys.resolve_action(
            "strike",
            KNIGHT,
            &sheet_with_turn_counters(&plain, spent.iter()),
            (4, 4),
            GOBLIN,
            &foe,
            (5, 4),
            &mut Rng::new(7),
        )
    };
    let mut spent: std::collections::BTreeMap<String, i64> = Default::default();
    for i in 0..3 {
        let r = strike3(&mut sys, &spent);
        assert!(r.is_ok(), "action {i} is within the three-action budget");
        for (_, key, delta) in &r.unwrap().turn_counters {
            *spent.entry(key.clone()).or_insert(0) += delta;
        }
    }
    // The fourth is refused before any die: out of actions.
    assert_eq!(
        strike3(&mut sys, &spent),
        Err(ActionError::CannotAfford("strike".to_owned())),
        "the fourth Strike has no action to pay for it"
    );

    // A fresh turn (counters cleared) affords a Strike again.
    assert!(strike3(&mut sys, &Default::default()).is_ok());
}

#[test]
fn a_system_with_no_action_economy_pays_nothing_for_one() {
    // 5e's attack declares no afford rule and no turn effect, so it is always
    // affordable and touches no counter -- the primitive is opt-in.
    let (mut sys, knight, goblin) = duel(1, 50);
    let r = sys
        .resolve_action("attack", KNIGHT, &knight, (4, 4), GOBLIN, &goblin, (5, 4), &mut Rng::new(3))
        .expect("resolves");
    assert!(r.turn_counters.is_empty(), "5e spends no per-turn counter");
}

#[test]
fn each_system_picks_its_own_rungs_on_the_ladder() {
    // The ladder is opt-in. 5e uses three rungs -- crit on a natural 20,
    // hit, miss -- and never a critical failure, because a natural 1 in 5e
    // simply misses rather than fumbling. PF2e uses all four. Neither pays
    // for the other's complexity, and both ride one resolver.
    let (mut sys, knight, goblin) = duel(1, 50);
    let hit = sys
        .resolve_action("attack", KNIGHT, &knight, (4, 4), GOBLIN, &goblin, (5, 4), &mut Rng::new(3))
        .expect("resolves"); // seed 3 rolls a 16
    assert_eq!(hit.degree, 1, "a plain 5e hit");
    assert!(hit.damage.unwrap().expr.contains("1d8"), "unscaled");

    // A natural 1 misses even against AC 1, and is a plain failure, never
    // the fourth rung.
    let (mut sys, knight, goblin) = duel(1, 50);
    let fumble = sys
        .resolve_action("attack", KNIGHT, &knight, (4, 4), GOBLIN, &goblin, (5, 4), &mut Rng::new(42))
        .expect("resolves"); // seed 42 rolls a natural 1
    assert_eq!(fumble.degree, 0, "5e has no critical-failure rung");
    assert!(!fumble.hit, "a natural 1 always misses, whatever the AC");

    // 5e never reaches -1; PF2e does.
    let pf2e_fumble = pf2e_strike(10, 100, 4);
    assert_eq!(pf2e_fumble.degree, -1, "the fourth rung is PF2e's");
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
