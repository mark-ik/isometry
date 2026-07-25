//! Rust to Lua: presenting substrate values to authored scripts.
//!
//! The mirror of `lua_read`. These build the tables a generator script sees as
//! its request, and they are the only shape packs may depend on.
//!
//! Split out of `lib.rs` on 2026-07-24; behavior unchanged.

use super::*;

/// Marshal one generator request into deterministic, tagged Lua data. Tables
/// are built from `BTreeMap` iteration and list order, so pack code sees the
/// same structure for a given request on every host.
pub(crate) fn generator_request_table<'gc>(
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

pub(crate) fn gen_value_table<'gc>(ctx: piccolo::Context<'gc>, value: &GenValue) -> Table<'gc> {
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

pub(crate) fn lua_strings<'gc>(ctx: piccolo::Context<'gc>, strings: &[String]) -> Table<'gc> {
    let table = Table::new(&ctx);
    for (index, string) in strings.iter().enumerate() {
        table
            .set(ctx, index as i64 + 1, lua_string(ctx, string))
            .unwrap();
    }
    table
}

pub(crate) fn map_point_table<'gc>(ctx: piccolo::Context<'gc>, point: MapPoint) -> Table<'gc> {
    let table = Table::new(&ctx);
    table.set(ctx, "col", point.col).unwrap();
    table.set(ctx, "row", point.row).unwrap();
    table
}

pub(crate) fn map_cells_table<'gc>(ctx: piccolo::Context<'gc>, cells: &[MapCellProposal]) -> Table<'gc> {
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

pub(crate) fn spawn_zones_table<'gc>(ctx: piccolo::Context<'gc>, zones: &[SpawnZone]) -> Table<'gc> {
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

pub(crate) fn transitions_table<'gc>(ctx: piccolo::Context<'gc>, transitions: &[MapTransition]) -> Table<'gc> {
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

pub(crate) fn encounter_anchors_table<'gc>(
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

pub(crate) fn set_lua_string<'gc>(
    table: Table<'gc>,
    ctx: piccolo::Context<'gc>,
    key: &'static str,
    value: &str,
) {
    table.set(ctx, key, lua_string(ctx, value)).unwrap();
}

pub(crate) fn lua_string<'gc>(ctx: piccolo::Context<'gc>, value: &str) -> piccolo::String<'gc> {
    piccolo::String::from_slice(&ctx, value.as_bytes())
}

/// Drive one executor with a finite total fuel budget. `Lua::execute` refuels
/// internally, which is appropriate for rules formulas but not untrusted pack
/// generators, so this path intentionally steps the executor itself.
pub(crate) fn execute_bounded<R: for<'gc> piccolo::FromMultiValue<'gc>>(
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

