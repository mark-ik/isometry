//! The SRD compendium overlay: three namespaces (Monsters, Spells, Items) as
//! `data_grid` indexes with per-entry pages, switched by a `tab_strip`.
//!
//! The first real `data_grid` consumer
//! (design_docs/2026-07-08_campaign_packs_plan.md). A fixed-width overlay
//! gives the grid the known dimensions fluid chrome could not. Names open
//! pages (record card + `stat_list` + prose); a monster page can spawn onto
//! the board, where the compendium meets the voxel-appearance pipeline. Tab,
//! sort, and selection are caller state, so the view still names no rules.

use std::rc::Rc;

use xilem_serval::{GridColumn, GridSpec, clickable, data_grid, el, text};

use crate::board::UiChild;
use crate::state::{CompendiumTab, ItemRow, MonsterRow, SpellRow, UiState};
use crate::widgets::stat_list;

const ABIL: [&str; 6] = ["STR", "DEX", "CON", "INT", "WIS", "CHA"];

fn ability_mod(score: i32) -> i32 {
    (score - 10).div_euclid(2)
}

/// The compendium as an overlay, or `None` when closed.
pub fn compendium_overlay(ui: &UiState) -> Option<UiChild> {
    if !ui.compendium_open {
        return None;
    }
    let tab = ui.compendium_tab;
    let tabs: Vec<(String, bool)> = CompendiumTab::ALL
        .iter()
        .map(|t| (t.label().to_owned(), *t == tab))
        .collect();
    let nav = crate::widgets::tab_strip(tabs, |ui: &mut UiState, i| {
        ui.set_compendium_tab(CompendiumTab::ALL[i])
    });

    let body: UiChild = match (ui.compendium_selected.as_deref(), tab) {
        (Some(key), CompendiumTab::Monsters) => ui
            .bestiary
            .iter()
            .find(|m| m.key == key)
            .map(monster_page)
            .unwrap_or_else(|| monster_index(ui)),
        (Some(key), CompendiumTab::Spells) => ui
            .spells
            .iter()
            .find(|s| s.key == key)
            .map(spell_page)
            .unwrap_or_else(|| spell_index(ui)),
        (Some(key), CompendiumTab::Items) => ui
            .items
            .iter()
            .find(|it| it.key == key)
            .map(item_page)
            .unwrap_or_else(|| item_index(ui)),
        (None, CompendiumTab::Monsters) => monster_index(ui),
        (None, CompendiumTab::Spells) => spell_index(ui),
        (None, CompendiumTab::Items) => item_index(ui),
    };

    Some(Box::new(
        el::<_, UiState, ()>("div", (top_bar(ui), nav, body)).attr("class", "compendium"),
    ))
}

/// The overlay's title row: a back button when a page is open, always a close.
fn top_bar(ui: &UiState) -> UiChild {
    let mut actions: Vec<UiChild> = Vec::new();
    if ui.compendium_selected.is_some() {
        actions.push(Box::new(clickable(
            el::<_, UiState, ()>("span", text("back")).attr("class", "btn btn-mini"),
            |ui: &mut UiState, _| ui.back_to_index(),
        )));
    }
    actions.push(Box::new(clickable(
        el::<_, UiState, ()>("span", text("close")).attr("class", "btn btn-mini"),
        |ui: &mut UiState, _| ui.close_compendium(),
    )));
    Box::new(
        el::<_, UiState, ()>(
            "div",
            (
                el::<_, UiState, ()>("span", text("Compendium")).attr("class", "compendium-title"),
                el::<_, UiState, ()>("div", actions).attr("class", "compendium-actions"),
            ),
        )
        .attr("class", "compendium-header"),
    )
}

// ---------- shared cell + grid helpers ----------

fn name_cell(key: &str, name: &str) -> UiChild {
    let key = key.to_owned();
    Box::new(clickable(
        el::<_, UiState, ()>("span", text(name.to_owned())).attr("class", "compendium-link"),
        move |ui: &mut UiState, _| ui.open_entry(key.clone()),
    ))
}

fn text_cell(s: String) -> UiChild {
    Box::new(el::<_, UiState, ()>("span", text(s)).attr("class", "compendium-cell"))
}

fn grid(spec: &GridSpec, total: usize, scroll: f32, cell: impl Fn(usize, usize) -> UiChild) -> UiChild {
    data_grid::<UiState, ()>(
        spec,
        total,
        300.0,
        scroll,
        cell,
        |ui: &mut UiState, col| ui.sort_compendium(col),
        |_r| None,
    )
}

// ---------- indexes ----------

fn monster_index(ui: &UiState) -> UiChild {
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
    let rows: Rc<Vec<MonsterRow>> = Rc::new(order.iter().map(|&i| ui.bestiary[i].clone()).collect());
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
    grid(&spec, rows.len(), ui.compendium_scroll, move |r, c| {
        let m = &rows[r];
        if c == 0 {
            name_cell(&m.key, &m.name)
        } else {
            text_cell(match c {
                1 => m.cr_label.clone(),
                2 => m.kind.clone(),
                3 => m.hp.to_string(),
                _ => m.ac.to_string(),
            })
        }
    })
}

fn spell_index(ui: &UiState) -> UiChild {
    let (col, desc) = ui.compendium_sort;
    let mut order: Vec<usize> = (0..ui.spells.len()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&ui.spells[a], &ui.spells[b]);
        let o = match col {
            0 => x.name.cmp(&y.name),
            1 => x.level.cmp(&y.level),
            2 => x.school.cmp(&y.school),
            _ => x.range.cmp(&y.range),
        };
        if desc { o.reverse() } else { o }
    });
    let rows: Rc<Vec<SpellRow>> = Rc::new(order.iter().map(|&i| ui.spells[i].clone()).collect());
    let spec = GridSpec {
        columns: vec![
            GridColumn::new("Name", 140.0),
            GridColumn::new("Lvl", 44.0),
            GridColumn::new("School", 96.0),
            GridColumn::new("Range", 84.0),
        ],
        row_height: 22.0,
        header_height: 24.0,
        overscan: 4,
    };
    grid(&spec, rows.len(), ui.compendium_scroll, move |r, c| {
        let s = &rows[r];
        if c == 0 {
            name_cell(&s.key, &s.name)
        } else {
            text_cell(match c {
                1 => s.level_label.clone(),
                2 => s.school.clone(),
                _ => s.range.clone(),
            })
        }
    })
}

fn item_index(ui: &UiState) -> UiChild {
    let (col, desc) = ui.compendium_sort;
    let mut order: Vec<usize> = (0..ui.items.len()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&ui.items[a], &ui.items[b]);
        let o = match col {
            0 => x.name.cmp(&y.name),
            1 => x.category.cmp(&y.category),
            2 => x.cost.cmp(&y.cost),
            _ => x.weight.cmp(&y.weight),
        };
        if desc { o.reverse() } else { o }
    });
    let rows: Rc<Vec<ItemRow>> = Rc::new(order.iter().map(|&i| ui.items[i].clone()).collect());
    let spec = GridSpec {
        columns: vec![
            GridColumn::new("Name", 140.0),
            GridColumn::new("Category", 88.0),
            GridColumn::new("Cost", 60.0),
            GridColumn::new("Weight", 60.0),
        ],
        row_height: 22.0,
        header_height: 24.0,
        overscan: 4,
    };
    grid(&spec, rows.len(), ui.compendium_scroll, move |r, c| {
        let it = &rows[r];
        if c == 0 {
            name_cell(&it.key, &it.name)
        } else {
            text_cell(match c {
                1 => it.category.clone(),
                2 => it.cost.clone(),
                _ => it.weight.clone(),
            })
        }
    })
}

// ---------- pages ----------

fn entry_name(name: &str) -> UiChild {
    Box::new(el::<_, UiState, ()>("div", text(name.to_owned())).attr("class", "entry-name"))
}

fn subtitle(s: String) -> UiChild {
    Box::new(el::<_, UiState, ()>("div", text(s)).attr("class", "monster-sub"))
}

fn desc(s: String) -> UiChild {
    Box::new(el::<_, UiState, ()>("div", text(s)).attr("class", "compendium-desc"))
}

fn monster_page(m: &MonsterRow) -> UiChild {
    let stats = stat_list(
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
        (
            entry_name(&m.name),
            subtitle(format!("{} {}, {}", m.size, m.kind, m.alignment)),
            stats,
            ability_row,
            actions_block,
            spawn,
        ),
    ))
}

fn spell_page(s: &SpellRow) -> UiChild {
    let sub = if s.level == 0 {
        format!("{} cantrip", s.school)
    } else {
        format!("Level {} {}", s.level, s.school)
    };
    let stats = stat_list(
        [
            ("Casting".to_owned(), s.casting_time.clone()),
            ("Range".to_owned(), s.range.clone()),
            ("Components".to_owned(), s.components.clone()),
            ("Duration".to_owned(), s.duration.clone()),
        ],
        "monster-stats",
    );
    Box::new(el::<_, UiState, ()>(
        "div",
        (entry_name(&s.name), subtitle(sub), stats, desc(s.desc.clone())),
    ))
}

fn item_page(it: &ItemRow) -> UiChild {
    let mut pairs = vec![
        ("Cost".to_owned(), it.cost.clone()),
        ("Weight".to_owned(), it.weight.clone()),
    ];
    if !it.detail.is_empty() {
        pairs.push(("".to_owned(), it.detail.clone()));
    }
    let stats = stat_list(pairs, "monster-stats");
    Box::new(el::<_, UiState, ()>(
        "div",
        (
            entry_name(&it.name),
            subtitle(it.category.clone()),
            stats,
            desc(it.desc.clone()),
        ),
    ))
}
