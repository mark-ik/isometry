//! Shared chrome widgets, extracted on second use. The stat shapes the
//! compendium's monster page and the character sheet both render live here so
//! there is one implementation. View compositions, host-agnostic; promote to
//! the cross-repo catalog only when another repo needs them.

use xilem_serval::{el, text};

use crate::board::UiChild;
use crate::state::UiState;

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
