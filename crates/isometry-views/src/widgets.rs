//! Shared chrome widgets, extracted on second use. The stat shapes the
//! compendium's monster page and the character sheet both render live here so
//! there is one implementation. View compositions, host-agnostic; promote to
//! the cross-repo catalog only when another repo needs them.

use xilem_serval::{clickable, el, text};

use crate::board::UiChild;
use crate::state::UiState;

/// A segmented namespace nav: one clickable tab per `(label, active)`, firing
/// `on_select(index)`. The compendium's Monsters/Spells/Items nav; a second
/// consumer (roster tabs) can share it later.
pub fn tab_strip(
    tabs: Vec<(String, bool)>,
    on_select: impl Fn(&mut UiState, usize) + Clone + 'static,
) -> UiChild {
    let items: Vec<UiChild> = tabs
        .into_iter()
        .enumerate()
        .map(|(i, (label, active))| {
            let class = if active { "tab tab-active" } else { "tab" };
            let sel = on_select.clone();
            Box::new(clickable(
                el::<_, UiState, ()>("div", text(label)).attr("class", class),
                move |ui: &mut UiState, _| sel(ui, i),
            )) as UiChild
        })
        .collect();
    Box::new(el::<_, UiState, ()>("div", items).attr("class", "tab-strip"))
}

/// A filter box showing the current `query` (or a hint) with a clear button.
/// Keys route to the query at the host, so this only displays; the compendium
/// is the first consumer, a list-heavy chrome surface the second.
pub fn search_field(query: &str) -> UiChild {
    let mut kids: Vec<UiChild> = Vec::new();
    if query.is_empty() {
        kids.push(Box::new(
            el::<_, UiState, ()>("span", text("type to filter...")).attr("class", "search-hint"),
        ));
    } else {
        kids.push(Box::new(
            el::<_, UiState, ()>("span", text(query.to_owned())).attr("class", "search-text"),
        ));
        kids.push(Box::new(clickable(
            el::<_, UiState, ()>("span", text("clear")).attr("class", "search-clear"),
            |ui: &mut UiState, _| ui.clear_compendium_search(),
        )));
    }
    Box::new(el::<_, UiState, ()>("div", kids).attr("class", "search-field"))
}

/// One read-only labeled value: a muted label beside an emphasised value
/// ("AC 13", "Reflex +2").
pub fn stat_row(label: &str, value: impl Into<String>) -> UiChild {
    Box::new(
        el::<_, UiState, ()>(
            "div",
            (
                el("span", text(label.to_owned())).attr("class", "stat-label"),
                el("span", text(value.into())).attr("class", "stat-val"),
            ),
        )
        .attr("class", "stat-row"),
    )
}

/// A container of labeled values under `container_class`. Shared by the monster
/// page (AC/HP/Speed/CR) and the sheet's derived modifiers.
pub fn stat_list(
    pairs: impl IntoIterator<Item = (String, String)>,
    container_class: &'static str,
) -> UiChild {
    let rows: Vec<UiChild> = pairs.into_iter().map(|(l, v)| stat_row(&l, v)).collect();
    Box::new(el::<_, UiState, ()>("div", rows).attr("class", container_class))
}
