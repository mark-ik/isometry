//! The character-sheet overlay. Rendered entirely from plain data: the
//! sheet's field values (core `SheetData`), the host-supplied schema
//! (labels), and the host-precomputed derived stats. The view names no
//! rules and holds no scripting engine; the system plugin lives in the
//! host.

use isometry_campaign::EquipmentSlot;
use isometry_core::FieldValue;
use cambium::{clickable, el, text};

use crate::board::UiChild;
use crate::state::UiState;

/// The open sheet as an overlay, or `None` when no sheet is open.
pub fn sheet_overlay(ui: &UiState) -> Option<UiChild> {
    let id = ui.open_sheet?;
    let stored_sheet = ui.map.sheet(id)?;
    let sheet = ui.sheet_effective.as_ref().unwrap_or(stored_sheet);
    let schema = &ui.sheet_schema;
    let name = sheet.text("name").unwrap_or("Character").to_owned();

    let mut body: Vec<UiChild> = Vec::new();

    // Editable fields (name shown in the panel title). Int fields get steppers.
    for (key, label, is_int) in &schema.fields {
        if key == "name" {
            continue;
        }
        let val = match sheet.fields.get(key) {
            Some(FieldValue::Int(n)) => n.to_string(),
            Some(FieldValue::Text(s)) => s.clone(),
            Some(FieldValue::Bool(b)) => b.to_string(),
            Some(FieldValue::Float(f)) => f.to_string(),
            // Nested fields get real editors with the item/inventory
            // views (worldbuilding W1); show a count until then.
            Some(FieldValue::List(items)) => format!("{} entries", items.len()),
            Some(FieldValue::Map(m)) => format!("{} entries", m.len()),
            None => "-".to_owned(),
        };
        let row: UiChild = if *is_int {
            let dec = key.clone();
            let inc = key.clone();
            Box::new(
                el(
                    "div",
                    (
                        el("span", text(format!("{label}: {val}"))).attr("class", "sheet-field"),
                        clickable(
                            el("span", text("-")).attr("class", "btn btn-mini"),
                            move |ui: &mut UiState, _| ui.request_sheet_edit(&dec, -1),
                        ),
                        clickable(
                            el("span", text("+")).attr("class", "btn btn-mini"),
                            move |ui: &mut UiState, _| ui.request_sheet_edit(&inc, 1),
                        ),
                    ),
                )
                .attr("class", "sheet-row"),
            )
        } else {
            Box::new(el("div", text(format!("{label}: {val}"))).attr("class", "sheet-row"))
        };
        body.push(row);
    }

    // Derived stats (system-agnostic: whatever the schema declares).
    if !schema.derived.is_empty() {
        body.push(Box::new(
            el("div", text("Modifiers")).attr("class", "sheet-heading"),
        ));
        let pairs: Vec<(String, String)> = schema
            .derived
            .iter()
            .filter_map(|(k, label)| {
                ui.sheet_derived
                    .get(k)
                    .map(|v| (label.clone(), format!("{v:+}")))
            })
            .collect();
        body.push(crate::widgets::stat_list(pairs, "sheet-mods"));
    }

    // Actions: each rolls through the system.
    if !schema.actions.is_empty() {
        body.push(Box::new(
            el("div", text("Actions")).attr("class", "sheet-heading"),
        ));
        let actions: Vec<UiChild> = schema
            .actions
            .iter()
            .map(|(k, label, targeted)| {
                let key = k.clone();
                // A targeted action reads as a verb aimed at someone, so mark it
                // rather than letting it look like another passive check.
                let class = if *targeted { "btn btn-attack" } else { "btn" };
                Box::new(clickable(
                    el("div", text(label.clone())).attr("class", class),
                    move |ui: &mut UiState, _| ui.request_action(&key),
                )) as UiChild
            })
            .collect();
        body.push(Box::new(el("div", actions).attr("class", "sheet-actions")));
    }

    if let Some(inventory) = ui.inventories.get(&id) {
        body.push(Box::new(
            el("div", text("Equipment")).attr("class", "sheet-heading"),
        ));
        let slots = [
            EquipmentSlot::MainHand,
            EquipmentSlot::OffHand,
            EquipmentSlot::Head,
            EquipmentSlot::Body,
            EquipmentSlot::Feet,
            EquipmentSlot::Accessory,
        ];
        let rows: Vec<UiChild> = slots
            .iter()
            .filter_map(|slot| {
                let item_id = inventory.equipped.get(slot)?;
                let item = inventory.items.get(item_id)?;
                let modifiers = item
                    .modifiers
                    .iter()
                    .map(|modifier| modifier.name.as_str())
                    .collect::<Vec<_>>();
                let suffix = if modifiers.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", modifiers.join(", "))
                };
                Some(Box::new(
                    el("div", text(format!("{slot:?}: {}{suffix}", item.name)))
                        .attr("class", "sheet-row"),
                ) as UiChild)
            })
            .collect();
        if rows.is_empty() {
            body.push(Box::new(
                el("div", text("Nothing equipped")).attr("class", "sheet-row"),
            ));
        } else {
            body.extend(rows);
        }
        if ui.can_edit_inventory {
            if inventory.equipped.contains_key(&EquipmentSlot::MainHand) {
                body.push(Box::new(clickable(
                    el("span", text("unequip main hand")).attr("class", "btn btn-mini"),
                    |ui: &mut UiState, _| ui.request_unequip_main_hand(),
                )));
            }
            let carried: Vec<UiChild> = inventory
                .items
                .values()
                .filter(|item| {
                    !inventory
                        .equipped
                        .values()
                        .any(|equipped| equipped == &item.id)
                })
                .map(|item| {
                    let id = item.id.clone();
                    Box::new(clickable(
                        el("span", text(format!("equip {}", item.name)))
                            .attr("class", "btn btn-mini"),
                        move |ui: &mut UiState, _| ui.request_equip(id.clone()),
                    )) as UiChild
                })
                .collect();
            body.extend(carried);
            let recipients: Vec<(isometry_core::TokenId, String)> = ui
                .map
                .tokens
                .iter()
                .filter(|token| token.id != id)
                .map(|token| (token.id, format!("{} {}", token.sprite, token.id.0)))
                .collect();
            for item in inventory.items.values() {
                for (target, label) in &recipients {
                    let item_id = item.id.clone();
                    let target = *target;
                    body.push(Box::new(clickable(
                        el("span", text(format!("give {} to {label}", item.name)))
                            .attr("class", "btn btn-mini"),
                        move |ui: &mut UiState, _| ui.request_transfer(target, item_id.clone()),
                    )));
                }
            }
        }
    }

    let close: Vec<UiChild> = vec![Box::new(clickable(
        el("span", text("close")).attr("class", "btn btn-mini"),
        |ui: &mut UiState, _| ui.close_sheet(),
    ))];
    Some(crate::widgets::overlay_panel("sheet", name, close, body))
}
