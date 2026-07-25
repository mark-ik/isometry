//! The 5e SRD content pack (CC-BY-4.0).
//!
//! First-party content expressed through the same `System` seam a third-party
//! pack would use, so the SRD gets no privileged path into the substrate.
//!
//! Split out of `lib.rs` on 2026-07-24; behavior unchanged.

use super::*;

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
                damage_mult_func: Some("a_attack_mult".to_owned()),
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
                condition_value_func: None,
                recruit_on_hit: false,
                afford_func: None,
                turn_effect: Vec::new(),
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
                damage_mult_func: None,
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
                condition_value_func: None,
                recruit_on_hit: false,
                afford_func: None,
                turn_effect: Vec::new(),
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
                damage_mult_func: None,
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
                condition_value_func: None,
                recruit_on_hit: false,
                afford_func: None,
                turn_effect: Vec::new(),
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
                damage_mult_func: None,
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
                condition_value_func: None,
                recruit_on_hit: true,
                afford_func: None,
                turn_effect: Vec::new(),
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
        -- a_attack), `die` the natural roll, `t` the defender's sheet.
        -- 5e is a two-rung system: hit or miss. A natural 20 is the one
        -- exception -- it always hits, and crits (degree 2, which doubles the
        -- damage via a_attack_mult); a natural 1 always misses.
        function a_attack_hit(c, t, roll, die)
            if die == 20 then return 2 end
            if die == 1 then return 0 end
            if roll >= (t.ac or 12) then return 1 else return 0 end
        end
        -- 5e doubles the dice on a crit. (Strictly it doubles only the dice and
        -- not the modifier; the percent seam scales the whole effect, so this is
        -- the honest approximation until the seam can split them.)
        function a_attack_mult(c, t, degree)
            if degree >= 2 then return 200 else return 100 end
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

