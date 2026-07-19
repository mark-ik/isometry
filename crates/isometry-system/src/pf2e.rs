//! A Pathfinder 2e skeleton: the second ruleset, and the reason the action
//! spec has to generalize.
//!
//! This is deliberately a *skeleton*, not a port. Its job is to put weight on
//! the shape that five phases of 5e work hand-rolled, and to make the 5e-isms
//! fall out where they can be seen. What it proves, and what it does not, is
//! recorded in the roadmap's C9 section.
//!
//! What PF2e needs that 5e never asked for:
//!
//! - **Degrees of success.** A Strike is not hit-or-miss: beat the AC by 10 and
//!   it is a critical success, miss it by 10 and a critical failure. The old
//!   `hit_func -> 1|0` could not say this. It now returns a *degree*, and 5e's
//!   binary `1|0` is simply the two middle rungs of the same ladder, unchanged.
//! - **Critical damage.** A critical hit doubles dice *and* modifiers, which is
//!   not expressible as a bonus. `damage_mult_func` returns a percent, so a crit
//!   is 200 and 5e's own save-for-half (which it also could not express) is 50.
//! - **Level-scaled proficiency.** PF2e adds level + a rank bonus rather than a
//!   flat proficiency. That is plain arithmetic and the existing ABI took it
//!   without complaint.
//!
//! Content here is ORC-licensed Pathfinder 2e material, kept to the minimum the
//! skeleton needs. Isometry ships no copyrighted game content.

use isometry_core::FieldValue;

use crate::{ActionDef, DerivedDef, FieldDef, System, TargetSpec};

/// The PF2e skeleton: ability scores, level-scaled proficiency, and a Strike
/// resolved by degree of success.
pub fn pf2e_srd() -> System {
    let ability = |key: &str, label: &str| FieldDef {
        key: key.to_owned(),
        label: label.to_owned(),
        default: FieldValue::Int(10),
    };
    let fields = vec![
        FieldDef {
            key: "name".to_owned(),
            label: "Name".to_owned(),
            default: FieldValue::Text("Adventurer".to_owned()),
        },
        ability("str", "STR"),
        ability("dex", "DEX"),
        ability("con", "CON"),
        ability("int", "INT"),
        ability("wis", "WIS"),
        ability("cha", "CHA"),
        FieldDef {
            key: "level".to_owned(),
            label: "Level".to_owned(),
            default: FieldValue::Int(1),
        },
        // PF2e proficiency is level + a rank bonus, not a flat number: untrained
        // 0, trained 2, expert 4, master 6, legendary 8. Storing the rank keeps
        // the level scaling in the script where it belongs.
        FieldDef {
            key: "rank_attack".to_owned(),
            label: "Attack rank".to_owned(),
            default: FieldValue::Int(2),
        },
        // A second proficiency, so Demoralize scales on its own skill rather than
        // borrowing the sword's. The skeleton keeps two ranks; a full port would
        // carry one per skill.
        FieldDef {
            key: "rank_intimidation".to_owned(),
            label: "Intimidation rank".to_owned(),
            default: FieldValue::Int(2),
        },
        FieldDef {
            key: "ac".to_owned(),
            label: "AC".to_owned(),
            default: FieldValue::Int(16),
        },
        FieldDef {
            key: "hp_current".to_owned(),
            label: "HP".to_owned(),
            default: FieldValue::Int(20),
        },
        FieldDef {
            key: "hp_max".to_owned(),
            label: "HP max".to_owned(),
            default: FieldValue::Int(20),
        },
        // The substrate's movement and senses, as PF2e numbers (25 ft = 5 tiles).
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
        FieldDef {
            key: "will".to_owned(),
            label: "Will DC".to_owned(),
            default: FieldValue::Int(14),
        },
        // The three-action economy, as a sheet number rather than a baked
        // constant: a quickened creature could carry 4, a slowed one 2. The
        // substrate never sees it; only the afford rule reads it.
        FieldDef {
            key: "actions_per_turn".to_owned(),
            label: "Actions/turn".to_owned(),
            default: FieldValue::Int(3),
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
        DerivedDef {
            key: "attack_bonus".to_owned(),
            label: "Attack".to_owned(),
            func: "p_strike".to_owned(),
        },
    ];
    let actions = vec![ActionDef {
        key: "strike".to_owned(),
        label: "Strike".to_owned(),
        base: "1d20".to_owned(),
        func: "p_strike".to_owned(),
        target: Some(TargetSpec {
            range: 1,
            hit_func: "p_strike_degree".to_owned(),
            damage: "1d8".to_owned(),
            damage_func: "p_strike_dmg".to_owned(),
            // The whole point: a critical Strike doubles everything.
            damage_mult_func: Some("p_crit_mult".to_owned()),
            damage_field: "hp_current".to_owned(),
            actor_beat: "strike".to_owned(),
            hit_beat: "recoil".to_owned(),
            miss_beat: "dodge".to_owned(),
            fall_beat: "fall".to_owned(),
            stagger_func: None,
            stagger_beat: "staggered".to_owned(),
            push_func: None,
            push_beat: "shoved".to_owned(),
            condition_on_hit: None,
            condition_value_func: None,
            recruit_on_hit: false,
            // The action economy and the multiple-attack penalty, both riding
            // the substrate's one per-turn counter primitive: a Strike costs an
            // action, and each Strike this turn counts toward MAP (read by
            // `p_strike`). A missed Strike still spends both, as PF2e says.
            afford_func: Some("p_afford_strike".to_owned()),
            turn_effect: vec![
                ("actions_spent".to_owned(), 1),
                ("strikes".to_owned(), 1),
            ],
        }),
    },
    // Demoralize: the reason conditions had to carry a number. It deals no
    // damage; it frightens. Roll Intimidation against the target's Will DC on
    // the four-rung ladder, and the *degree* sets the magnitude -- a critical
    // success inflicts frightened 2, a plain success frightened 1. That value is
    // not baked anywhere: `p_frighten_value` reads the degree the resolver
    // already computed and returns the number, and the substrate stores it
    // blind. Frightened then reads back into the Strike bonus, so the loop
    // closes -- a rule sets a magnitude and another rule spends it. Costs one
    // action, like everything in the economy; a 30-ft (6-tile) reach.
    ActionDef {
        key: "demoralize".to_owned(),
        label: "Demoralize".to_owned(),
        base: "1d20".to_owned(),
        func: "p_intimidate".to_owned(),
        target: Some(TargetSpec {
            range: 6,
            hit_func: "p_demoralize_degree".to_owned(),
            damage: "0".to_owned(),
            damage_func: "p_zero".to_owned(),
            damage_mult_func: None,
            damage_field: "hp_current".to_owned(),
            actor_beat: "demoralize".to_owned(),
            hit_beat: "cower".to_owned(),
            miss_beat: "steady".to_owned(),
            fall_beat: "fall".to_owned(),
            stagger_func: None,
            stagger_beat: "staggered".to_owned(),
            push_func: None,
            push_beat: "shoved".to_owned(),
            condition_on_hit: Some("frightened".to_owned()),
            condition_value_func: Some("p_frighten_value".to_owned()),
            recruit_on_hit: false,
            afford_func: Some("p_afford_strike".to_owned()),
            turn_effect: vec![("actions_spent".to_owned(), 1)],
        }),
    }];

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

        -- PF2e proficiency: level + rank bonus, so a trained level-5 fighter is
        -- +7 before ability. Untrained (rank 0) adds nothing, not even level.
        function prof(level, rank)
            if rank <= 0 then return 0 end
            return level + rank
        end
        -- The Strike bonus, with the multiple-attack penalty folded in. Each
        -- Strike this turn (counted by the substrate as `turn_strikes`) is -5 on
        -- the next, capped at -10 -- so the first is unpenalized, the second -5,
        -- the third and beyond -10. The counter is the substrate's; the penalty
        -- rule is entirely here.
        function p_strike(c)
            local map = c.turn_strikes or 0
            if map > 2 then map = 2 end
            -- Frightened is a status penalty to everything the creature does, so
            -- a frightened striker swings at -N. This is the *read* side of the
            -- magnitude Demoralize writes; nothing here knows Demoralize exists,
            -- only that `c.frightened` is a number the substrate handed over.
            return ab_mod(c.str) + prof(c.level, c.rank_attack) - 5 * map - (c.frightened or 0)
        end
        function p_strike_dmg(c) return ab_mod(c.str) end
        -- Intimidation: the same level + rank shape as the attack, on its own
        -- proficiency, plus Charisma rather than Strength.
        function p_intimidate(c)
            return ab_mod(c.cha) + prof(c.level, c.rank_intimidation)
        end
        function p_zero(c) return 0 end

        -- Can you afford a Strike? Only if you have an action left this turn.
        -- `actions_per_turn` is the sheet's (3 by default); `turn_actions_spent`
        -- is the substrate's running count. Neither is baked into any Rust.
        function p_afford_strike(c)
            if (c.turn_actions_spent or 0) < c.actions_per_turn then
                return 1
            else
                return 0
            end
        end

        -- The four-rung ladder, and the reason hit_func had to stop being a
        -- boolean. Beat the DC by 10: critical success. Miss it by 10: critical
        -- failure. Then the natural die shifts the result one rung either way,
        -- which is why the ABI passes it alongside the total: a 20 promotes a
        -- success to a critical, a 1 demotes a failure to a fumble. One ladder,
        -- shared by every PF2e check -- a Strike beats AC, a Demoralize beats
        -- Will, and only the DC differs.
        function degree_vs(dc, roll, die)
            local d
            if roll >= dc + 10 then d = 2
            elseif roll >= dc then d = 1
            elseif roll <= dc - 10 then d = -1
            else d = 0 end
            if die == 20 then d = d + 1
            elseif die == 1 then d = d - 1 end
            if d > 2 then d = 2 end
            if d < -1 then d = -1 end
            return d
        end
        function p_strike_degree(c, t, roll, die)
            return degree_vs(t.ac or 16, roll, die)
        end
        function p_demoralize_degree(c, t, roll, die)
            return degree_vs(t.will or 14, roll, die)
        end

        -- A critical Strike doubles dice and modifiers together.
        function p_crit_mult(c, t, degree)
            if degree >= 2 then return 200 else return 100 end
        end

        -- The fear a Demoralize instills, straight off the degree ladder: a
        -- critical success frightens by 2, a plain success by 1, and anything
        -- less does nothing. This is the whole point of graded conditions --
        -- the magnitude is a return value, not a name, and not a Rust constant.
        function p_frighten_value(c, t, degree)
            if degree >= 2 then return 2
            elseif degree >= 1 then return 1
            else return 0 end
        end

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
        function s_defeated(c)
            if c.hp_current <= 0 then return 1 else return 0 end
        end

        -- Overmap navigation: a Wisdom (Survival) check against a DC that rises
        -- with the route's difficulty (its weight). Beat it and the party travels
        -- smoothly (100% of the base time); miss it and it loses the way, which
        -- costs half again as long (150%). The `t` slot is nil (travel has no
        -- target); wisdom finds the path. What "lost" then does to the party --
        -- exhaustion, a wandering encounter -- is later rungs; here it is time.
        function p_navigate(c, t, roll, weight)
            local dc = 12 + weight
            -- The navigator's exploration stance colours the check: Scouting
            -- ahead helps you find the way (+3), Searching every thicket slows
            -- and distracts you (-2). The stance is the substrate's `c.stance`
            -- string; what each one does is entirely here.
            local bonus = ab_mod(c.wis)
            if c.stance == "scout" then bonus = bonus + 3
            elseif c.stance == "search" then bonus = bonus - 2 end
            if roll + bonus >= dc then return 100 else return 150 end
        end

        -- The toll of a long march: travel of 20+ ticks leaves the party
        -- frightened-2's sibling, exhausted 2; 10+ exhausted 1; a short hop
        -- tires no one. A lost trip is longer, so it tires more, without this
        -- rule knowing the party got lost -- it reads only the time.
        function p_march_toll(c, t, ticks)
            if ticks >= 20 then return 2
            elseif ticks >= 10 then return 1
            else return 0 end
        end

        -- The perils of a long road: a journey of 15+ ticks runs into something,
        -- and the party is dropped onto the map to fight rather than arriving in
        -- peace. A short hop is safe. (A roll-based chance is the richer version;
        -- this keeps the peril legible and the length meaningful.)
        function p_road_peril(c, t, ticks)
            if ticks >= 15 then return 1 else return 0 end
        end
    "#;
    System::load("pf2e-srd", "Pathfinder 2e (skeleton)", fields, derived, actions, script)
        .expect("builtin pf2e skeleton loads")
        .with_defeat("s_defeated")
        .with_mobility("s_speed", "s_sight")
        .with_nav("p_navigate")
        .with_toll("p_march_toll")
        .with_encounters("p_road_peril")
}
