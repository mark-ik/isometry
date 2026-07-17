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
            recruit_on_hit: false,
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
        function p_strike(c) return ab_mod(c.str) + prof(c.level, c.rank_attack) end
        function p_strike_dmg(c) return ab_mod(c.str) end

        -- The four-rung ladder, and the reason hit_func had to stop being a
        -- boolean. Beat the DC by 10: critical success. Miss it by 10: critical
        -- failure. Then the natural die shifts the result one rung either way,
        -- which is why the ABI passes it alongside the total: a 20 promotes a
        -- success to a critical, a 1 demotes a failure to a fumble.
        function p_strike_degree(c, t, roll, die)
            local ac = t.ac or 16
            local d
            if roll >= ac + 10 then d = 2
            elseif roll >= ac then d = 1
            elseif roll <= ac - 10 then d = -1
            else d = 0 end
            if die == 20 then d = d + 1
            elseif die == 1 then d = d - 1 end
            if d > 2 then d = 2 end
            if d < -1 then d = -1 end
            return d
        end

        -- A critical Strike doubles dice and modifiers together.
        function p_crit_mult(c, t, degree)
            if degree >= 2 then return 200 else return 100 end
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
    "#;
    System::load("pf2e-srd", "Pathfinder 2e (skeleton)", fields, derived, actions, script)
        .expect("builtin pf2e skeleton loads")
        .with_defeat("s_defeated")
        .with_mobility("s_speed", "s_sight")
}
