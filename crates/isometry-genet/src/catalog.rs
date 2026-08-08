//! System content projected into view-side compendium rows.
//!
//! Free functions, not host state: they translate whatever the loaded system
//! plugin offers (bestiary, spells, items, sheet schema) into the row types the
//! compendium renders. The substrate hard-codes no game system, so everything
//! here reads from the plugin rather than from a table in this crate.
//!
//! Split out of `main.rs` on 2026-07-24; behavior unchanged.

use super::*;

/// The view-facing schema (plain labels) for a loaded system, so the
/// board renders a sheet without depending on isometry-system.
/// Translate the system's bestiary into the view-side compendium rows.
pub(crate) fn bestiary_of() -> Vec<MonsterRow> {
    srd_bestiary()
        .into_iter()
        .map(|m| {
            let cr_label = m.cr_label();
            MonsterRow {
                key: m.key,
                name: m.name,
                cr: m.challenge_rating,
                cr_label,
                kind: m.kind,
                size: m.size,
                alignment: m.alignment,
                hp: m.hit_points,
                hit_dice: m.hit_dice,
                ac: m.armor_class,
                speed_ft: m.speed_ft,
                xp: m.xp,
                abilities: m.abilities,
                actions: m
                    .actions
                    .into_iter()
                    .map(|a| ActionRow {
                        name: a.name,
                        to_hit: a.to_hit,
                        damage: a.damage,
                        desc: a.desc,
                    })
                    .collect(),
                sprite: m.sprite,
            }
        })
        .collect()
}

pub(crate) fn spells_of() -> Vec<SpellRow> {
    srd_spells()
        .into_iter()
        .map(|s| {
            let level_label = s.level_label();
            SpellRow {
                key: s.key,
                name: s.name,
                level: s.level,
                level_label,
                school: s.school,
                casting_time: s.casting_time,
                range: s.range,
                components: s.components,
                duration: s.duration,
                desc: s.desc,
            }
        })
        .collect()
}

pub(crate) fn items_of() -> Vec<ItemRow> {
    srd_items()
        .into_iter()
        .map(|i| ItemRow {
            key: i.key,
            name: i.name,
            category: i.category,
            cost: i.cost,
            weight: i.weight,
            detail: i.detail,
            desc: i.desc,
        })
        .collect()
}

pub(crate) fn schema_of(system: &System) -> SheetSchema {
    SheetSchema {
        fields: system
            .fields
            .iter()
            .map(|f| {
                (
                    f.key.clone(),
                    f.label.clone(),
                    matches!(f.default, FieldValue::Int(_)),
                )
            })
            .collect(),
        derived: system
            .derived
            .iter()
            .map(|d| (d.key.clone(), d.label.clone()))
            .collect(),
        actions: system
            .actions
            .iter()
            .map(|a| (a.key.clone(), a.label.clone(), a.target.is_some()))
            .collect(),
    }
}
