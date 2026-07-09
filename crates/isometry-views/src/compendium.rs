//! The SRD compendium overlay: the Monsters namespace as a `data_grid` index,
//! and a per-monster page.
//!
//! The first real `data_grid` consumer
//! (design_docs/2026-07-08_campaign_packs_plan.md). A fixed-width overlay
//! panel gives the grid the known pixel dimensions that fluid chrome could
//! not. Clicking a name opens the monster page (a record card plus a stat
//! list, the first `record_card`/`stat_list` sibling shapes); the page can
//! spawn the monster onto the board, which is where the compendium meets the
//! voxel-appearance pipeline. Sort and selection are caller state, so the
//! view still names no rules.

use std::rc::Rc;

use xilem_serval::{GridColumn, GridSpec, clickable, data_grid, el, text};

use crate::board::UiChild;
use crate::state::{MonsterRow, UiState};

const ABIL: [&str; 6] = ["STR", "DEX", "CON", "INT", "WIS", "CHA"];

fn ability_mod(score: i32) -> i32 {
    (score - 10).div_euclid(2)
}

/// The compendium as an overlay, or `None` when closed. Shows a monster page
/// when one is selected, otherwise the Monsters index.
pub fn compendium_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.compendium_open {
        return None;
    }
    let content: UiChild = match &ui.compendium_selected {
        Some(key) => match ui.bestiary.iter().find(|m| &m.key == key) {
            Some(m) => monster_page(m),
            None => index_grid(ui),
        },
        None => index_grid(ui),
    };
    Some(Box::new(
        el::<_, UiState, ()>("div", content).attr("class", "compendium"),
    ))
}

/// The Monsters index: a sortable `data_grid` whose names open pages.
fn index_grid(ui: &UiState) -> UiChild {
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
            if c == 0 {
                let key = m.key.clone();
                Box::new(clickable(
                    el::<_, UiState, ()>("span", text(m.name.clone()))
                        .attr("class", "compendium-link"),
                    move |ui: &mut UiState, _| ui.open_monster(key.clone()),
                ))
            } else {
                let s = match c {
                    1 => m.cr_label.clone(),
                    2 => m.kind.clone(),
                    3 => m.hp.to_string(),
                    _ => m.ac.to_string(),
                };
                Box::new(el::<_, UiState, ()>("span", text(s)).attr("class", "compendium-cell"))
            }
        },
        |ui: &mut UiState, col| ui.sort_compendium(col),
        |_r| None,
    );

    Box::new(el::<_, UiState, ()>(
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
    ))
}

/// A monster's page: header, a stat list, the ability block, actions, and a
/// spawn button.
fn monster_page(m: &MonsterRow) -> UiChild {
    let header = el::<_, UiState, ()>(
        "div",
        (
            el("span", text(m.name.clone())).attr("class", "compendium-title"),
            el::<_, UiState, ()>(
                "div",
                (
                    clickable(
                        el("span", text("back")).attr("class", "btn btn-mini"),
                        |ui: &mut UiState, _| ui.back_to_index(),
                    ),
                    clickable(
                        el("span", text("close")).attr("class", "btn btn-mini"),
                        |ui: &mut UiState, _| ui.close_compendium(),
                    ),
                ),
            )
            .attr("class", "compendium-actions"),
        ),
    )
    .attr("class", "compendium-header");

    let subtitle = el::<_, UiState, ()>(
        "div",
        text(format!("{} {}, {}", m.size, m.kind, m.alignment)),
    )
    .attr("class", "monster-sub");

    let stats = crate::widgets::stat_list(
        [
            ("AC".to_owned(), m.ac.to_string()),
            ("HP".to_owned(), format!("{} ({})", m.hp, m.hit_dice)),
            ("Speed".to_owned(), format!("{} ft", m.speed_ft)),
            ("CR".to_owned(), format!("{} ({} XP)", m.cr_label, m.xp)),
        ],
        "monster-stats",
    );

    let abilities: Vec<UiChild> = (0..6)
        .map(|i| {
            let score = m.abilities[i];
            let md = ability_mod(score);
            let sign = if md >= 0 { "+" } else { "" };
            Box::new(
                el::<_, UiState, ()>(
                    "div",
                    (
                        el("div", text(ABIL[i])).attr("class", "abil-name"),
                        el("div", text(score.to_string())).attr("class", "abil-score"),
                        el("div", text(format!("{sign}{md}"))).attr("class", "abil-mod"),
                    ),
                )
                .attr("class", "abil"),
            ) as UiChild
        })
        .collect();
    let ability_row = el::<_, UiState, ()>("div", abilities).attr("class", "monster-abilities");

    let actions: Vec<UiChild> = m
        .actions
        .iter()
        .map(|a| {
            let line = match (a.to_hit, a.damage.as_ref()) {
                (Some(h), Some(d)) => format!("{} (+{h} to hit, {d})", a.name),
                _ => a.name.clone(),
            };
            Box::new(
                el::<_, UiState, ()>(
                    "div",
                    (
                        el("div", text(line)).attr("class", "action-name"),
                        el("div", text(a.desc.clone())).attr("class", "action-desc"),
                    ),
                )
                .attr("class", "monster-action"),
            ) as UiChild
        })
        .collect();
    let actions_block = el::<_, UiState, ()>("div", actions).attr("class", "monster-actions");

    let key = m.key.clone();
    let spawn = clickable(
        el::<_, UiState, ()>("div", text("Spawn onto board")).attr("class", "btn spawn-btn"),
        move |ui: &mut UiState, _| ui.spawn_monster(&key),
    );

    Box::new(el::<_, UiState, ()>(
        "div",
        (header, subtitle, stats, ability_row, actions_block, spawn),
    ))
}
