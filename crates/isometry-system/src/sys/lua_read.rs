//! Lua to Rust: reading authored tables into substrate types.
//!
//! Every reader is total. A pack may hand back anything, so a missing or
//! wrong-typed field becomes an `Err(String)` the host can report rather than a
//! panic inside someone else's script.
//!
//! Split out of `lib.rs` on 2026-07-24; behavior unchanged.

use super::*;

/// Run a generator to completion and decode its arena-bound Lua result before
/// leaving the Piccolo context. String results preserve the W2 JSON ABI;
/// tables are the native authoring path.
pub(crate) fn execute_bounded_gen_value(
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

pub(crate) fn lua_value_to_gen<'gc>(
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

pub(crate) fn lua_table_string<'gc>(
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

pub(crate) fn lua_table_table<'gc>(
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

pub(crate) fn lua_table_u32<'gc>(
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

pub(crate) fn lua_optional_string<'gc>(
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

pub(crate) fn lua_optional_u8<'gc>(
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

pub(crate) fn lua_map_point<'gc>(ctx: piccolo::Context<'gc>, table: Table<'gc>) -> Result<MapPoint, String> {
    Ok(MapPoint {
        col: lua_table_u32(ctx, table, "col")?,
        row: lua_table_u32(ctx, table, "row")?,
    })
}

pub(crate) fn lua_map_points<'gc>(
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

pub(crate) fn lua_map_cells<'gc>(
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

pub(crate) fn lua_spawn_zones<'gc>(
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

pub(crate) fn lua_transitions<'gc>(
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

pub(crate) fn lua_encounter_anchors<'gc>(
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

pub(crate) fn lua_gen_list<'gc>(
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

pub(crate) fn lua_string_list<'gc>(
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

