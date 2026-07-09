//! The SRD compendium overlay: the Monsters namespace as a `data_grid`.
//!
//! This is the first real `data_grid` consumer
//! (design_docs/2026-07-08_campaign_packs_plan.md). A fixed-width overlay
//! panel gives the grid the known pixel dimensions that fluid chrome could
//! not, which is exactly where the widget fits. Sort is caller state (a
//! header click toggles it); the rows come from the host-supplied bestiary,
//! so the view still names no rules.

use std::rc::Rc;

use xilem_serval::{GridColumn, GridSpec, clickable, data_grid, el, text};

use crate::board::UiChild;
use crate::state::{MonsterRow, UiState};

/// The compendium as an overlay, or `None` when closed.
pub fn compendium_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.compendium_open {
        return None;
    }

    // Sort order is caller state; compute the displayed order here.
    let (col, desc) = ui.compendium_sort;
    let mut order: Vec<usize> = (0..ui.bestiary.len()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&ui.bestiary[a], &ui.bestiary[b]);
        let o = match col {
            0 => x.name.cmp(&y.name),
            1 => x.cr.partial_cmp(&y.cr).unwrap_or(std::cmp::Ordering::Equal),
            2 => x.kind.cmp(&y.kind),
            3 => x.hp.cmp(&y.hp),
            _ => x.ac.cmp(&y.ac),
        };
        if desc { o.reverse() } else { o }
    });
    let rows: Rc<Vec<MonsterRow>> =
        Rc::new(order.iter().map(|&i| ui.bestiary[i].clone()).collect());

    let spec = GridSpec {
        columns: vec![
            GridColumn::new("Name", 132.0),
            GridColumn::new("CR", 44.0),
            GridColumn::new("Type", 96.0),
            GridColumn::new("HP", 40.0),
            GridColumn::new("AC", 40.0),
        ],
        row_height: 22.0,
        header_height: 24.0,
        overscan: 4,
    };
    let total = rows.len();
    let cell_rows = rows.clone();
    let grid = data_grid::<UiState, ()>(
        &spec,
        total,
        320.0,
        ui.compendium_scroll,
        move |r, c| {
            let m = &cell_rows[r];
            let s = match c {
                0 => m.name.clone(),
                1 => m.cr_label.clone(),
                2 => m.kind.clone(),
                3 => m.hp.to_string(),
                _ => m.ac.to_string(),
            };
            Box::new(el::<_, UiState, ()>("span", text(s)).attr("class", "compendium-cell"))
        },
        |ui: &mut UiState, col| ui.sort_compendium(col),
        |_r| None,
    );

    Some(Box::new(
        el::<_, UiState, ()>(
            "div",
            (
                el::<_, UiState, ()>(
                    "div",
                    (
                        el("span", text(format!("Bestiary ({total})")))
                            .attr("class", "compendium-title"),
                        clickable(
                            el("span", text("close")).attr("class", "btn btn-mini"),
                            |ui: &mut UiState, _| ui.close_compendium(),
                        ),
                    ),
                )
                .attr("class", "compendium-header"),
                grid,
            ),
        )
        .attr("class", "compendium"),
    ))
}
