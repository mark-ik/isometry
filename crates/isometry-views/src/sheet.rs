//! The character-sheet overlay. Rendered entirely from plain data: the
//! sheet's field values (core `SheetData`), the host-supplied schema
//! (labels), and the host-precomputed derived stats. The view names no
//! rules and holds no scripting engine; the system plugin lives in the
//! host.

use isometry_core::FieldValue;
use xilem_serval::{clickable, el, text};

use crate::board::UiChild;
use crate::state::UiState;

/// The open sheet as an overlay, or `None` when no sheet is open.
pub fn sheet_overlay(ui: &UiState) -> Option<UiChild> {
    let id = ui.open_sheet?;
    let sheet = ui.map.sheet(id)?;
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
                        el("span", text(format!("{label}: {val}")))
                            .attr("class", "sheet-field"),
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
                ui.sheet_derived.get(k).map(|v| (label.clone(), format!("{v:+}")))
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
            .map(|(k, label)| {
                let key = k.clone();
                Box::new(clickable(
                    el("div", text(label.clone())).attr("class", "btn"),
                    move |ui: &mut UiState, _| ui.request_action(&key),
                )) as UiChild
            })
            .collect();
        body.push(Box::new(el("div", actions).attr("class", "sheet-actions")));
    }

    let close: Vec<UiChild> = vec![Box::new(clickable(
        el("span", text("close")).attr("class", "btn btn-mini"),
        |ui: &mut UiState, _| ui.close_sheet(),
    ))];
    Some(crate::widgets::overlay_panel("sheet", name, close, body))
}
