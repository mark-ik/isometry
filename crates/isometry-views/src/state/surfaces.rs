//! Opening, closing, and cycling the overlay surfaces.
//!
//! Sheets, storylets, downtime, the overmap, the generator, governance, and the
//! compendium. These only ever set view state and arm request flags; the host
//! pumps notice the flag and do the adjudication, so a surface never decides an
//! outcome itself.
//!
//! Split out of `state.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl UiState {
    /// Open the selected token's sheet, requesting the host bind a fresh
    /// one first if it has none.
    pub fn open_or_bind_sheet(&mut self) {
        let Some(id) = self.selected_token.or_else(|| self.turns.active()) else {
            self.status = "select a token first".to_owned();
            return;
        };
        // Minting a sheet is DM work: the authority refuses a joined player's
        // `SheetSet`, and binding one locally anyway would leave this peer
        // holding a sheet the host never logged, until the next mirror wiped it
        // (the orphan-sheet shape the `>spawn` review found). A player opening
        // an unsheeted token simply sees nothing to show.
        if self.map.sheet(id).is_none() {
            if !self.can_edit_inventory {
                self.status = "no sheet yet; the DM binds one".to_owned();
                return;
            }
            self.bind_sheet_request = Some(id);
        }
        self.open_sheet = Some(id);
    }

    pub fn close_sheet(&mut self) {
        self.open_sheet = None;
        self.sheet_effective = None;
    }

    /// Queue a GM-side item instance from an SRD/content-pack entry for the
    /// currently open sheet. The host assigns the item id and commits it.
    pub fn request_compendium_item(&mut self, item: &ItemRow) {
        let Some(token) = self.open_sheet else {
            self.status = "open a character sheet first".to_owned();
            return;
        };
        if !self.can_edit_inventory {
            self.status = "inventory changes require the host".to_owned();
            return;
        }
        self.inventory_request = Some(InventoryRequest::AddCompendiumItem {
            token,
            template: item.key.clone(),
            name: item.name.clone(),
            category: item.category.clone(),
        });
    }

    pub fn request_equip(&mut self, item: ItemId) {
        let Some(token) = self.open_sheet else {
            return;
        };
        if self.can_edit_inventory {
            self.inventory_request = Some(InventoryRequest::Equip {
                token,
                slot: EquipmentSlot::MainHand,
                item,
            });
        }
    }

    pub fn request_unequip_main_hand(&mut self) {
        if self.can_edit_inventory {
            if let Some(token) = self.open_sheet {
                self.inventory_request = Some(InventoryRequest::Unequip {
                    token,
                    slot: EquipmentSlot::MainHand,
                });
            }
        }
    }

    pub fn request_transfer(&mut self, to: TokenId, item: ItemId) {
        if self.can_edit_inventory {
            if let Some(from) = self.open_sheet {
                if from != to {
                    self.inventory_request = Some(InventoryRequest::Transfer { from, to, item });
                }
            }
        }
    }

    /// Open the W2 host-only generator preview. The initial bundled pack has
    /// one item generator; later pack discovery will populate this selector.
    /// Open the storylet surface (the DM's dialogue/scene menu). Host-only, like
    /// generation: matching a storylet reads secret facts a client never holds.
    pub fn open_storylets(&mut self) {
        if !self.can_edit_inventory {
            self.status = "storylets are the DM's".to_owned();
        } else {
            self.storylet_open = true;
            self.storylet_selected = 0;
        }
    }

    pub fn close_storylets(&mut self) {
        self.storylet_open = false;
    }

    pub fn cycle_storylet(&mut self) {
        if !self.storylets.is_empty() {
            self.storylet_selected = (self.storylet_selected + 1) % self.storylets.len();
        }
    }

    pub fn selected_storylet(&self) -> Option<&StoryletRow> {
        self.storylets.get(self.storylet_selected)
    }

    /// Ask the host to play the selected storylet: commit its effects (facts,
    /// history, items, maps), which replicate. Only a playable one can be run.
    pub fn play_storylet(&mut self) {
        if !self.can_edit_inventory {
            self.status = "storylets are the DM's".to_owned();
            return;
        }
        let picked = self.selected_storylet().map(|row| {
            (
                row.available,
                row.key.clone(),
                row.entry.clone(),
                row.status.clone(),
            )
        });
        match picked {
            Some((true, key, entry, _)) => {
                self.storylet_request = Some(key);
                self.status = format!("playing: {entry}");
            }
            Some((false, _, _, status)) => self.status = format!("not yet: {status}"),
            None => self.status = "no storylet selected".to_owned(),
        }
    }

    /// Open the downtime surface and ask the host to roll a faction tick.
    /// Host-only, like storylets: the roll reads the world and spends entropy
    /// the host owns, so a joined client never sees it.
    pub fn open_downtime(&mut self) {
        if !self.can_edit_inventory {
            self.status = "downtime is the DM's".to_owned();
            return;
        }
        self.downtime_open = true;
        self.downtime_selected = 0;
        self.downtime_roll_request = true;
    }

    pub fn close_downtime(&mut self) {
        self.downtime_open = false;
    }

    pub fn cycle_downtime(&mut self) {
        if !self.faction_moves.is_empty() {
            self.downtime_selected = (self.downtime_selected + 1) % self.faction_moves.len();
        }
    }

    /// Roll a fresh tick, discarding the current batch and its edits.
    pub fn reroll_downtime(&mut self) {
        if !self.can_edit_inventory {
            return;
        }
        self.downtime_selected = 0;
        self.downtime_roll_request = true;
    }

    pub fn selected_downtime_move(&self) -> Option<&FactionMoveRow> {
        self.faction_moves.get(self.downtime_selected)
    }

    /// Strike or keep the selected move: the DM's edit before commit. A struck
    /// move is dropped from the tick; a kept one commits.
    pub fn toggle_strike_downtime(&mut self) {
        if let Some(row) = self.faction_moves.get_mut(self.downtime_selected) {
            row.struck = !row.struck;
        }
    }

    /// Commit the kept moves. Arms a one-shot the host drains and commits; a
    /// batch with everything struck commits nothing.
    pub fn commit_downtime(&mut self) {
        if !self.can_edit_inventory {
            self.status = "downtime is the DM's".to_owned();
            return;
        }
        let kept = self.faction_moves.iter().filter(|m| !m.struck).count();
        if kept == 0 {
            self.status = "no moves kept to commit".to_owned();
            return;
        }
        self.downtime_commit_request = true;
        self.status = format!("committing {kept} downtime move(s)");
    }

    /// Open the overmap: the party's pointcrawl of known places and routes.
    /// Anyone at the table may look at where the party is; travelling is gated
    /// downstream (the host adjudicates, and only a controller's request lands).
    pub fn open_overmap(&mut self) {
        self.overmap_open = true;
    }

    pub fn close_overmap(&mut self) {
        self.overmap_open = false;
    }

    /// Ask to travel to `node`. Arms a one-shot the host drains: it rolls the
    /// navigation, spends the time, and moves the party, or refuses if there is
    /// no route. The view never decides the outcome.
    pub fn request_travel(&mut self, node: String) {
        self.overmap_travel_request = Some(node);
    }

    /// Activate an overmap node as a travel request unless the current pointer
    /// gesture pulled that node. The drag callback arrives before the browser's
    /// click activation, so this consumes exactly the release-click that would
    /// otherwise make repositioning a site also travel there.
    pub fn activate_overmap_node(&mut self, node: String) {
        if let Some(dragged) = self.overmap_dragged_node.take() {
            if dragged == node {
                return;
            }
        }
        self.request_travel(node);
    }

    /// Respond to a captured Swatch node gesture with a local position override.
    /// The pointcrawl's source graph stays immutable: this is projection curation
    /// for the currently open UI only. A small normalized slop keeps a press and
    /// release available as the existing travel click.
    pub fn drag_overmap_node(&mut self, event: cambium::GraphCanvasNodeDrag<String>) {
        const SLOP: f32 = 0.015;
        match event.phase {
            cambium::PointerPhase::Down => {
                self.overmap_drag_start = Some((event.id, event.position));
                self.overmap_dragged_node = None;
            }
            cambium::PointerPhase::Move | cambium::PointerPhase::Up => {
                let Some((started_id, start)) = self.overmap_drag_start.as_ref() else {
                    return;
                };
                if started_id != &event.id {
                    return;
                }
                let dx = event.position.0 - start.0;
                let dy = event.position.1 - start.1;
                if self.overmap_dragged_node.is_some() || dx.hypot(dy) >= SLOP {
                    self.overmap_dragged_node = Some(event.id.clone());
                    self.overmap_position_overrides
                        .insert(event.id, event.position);
                }
                if matches!(event.phase, cambium::PointerPhase::Up) {
                    self.overmap_drag_start = None;
                }
            }
        }
    }

    /// Choose the party's travel pace (a percent of normal time). The host
    /// records it; the next trip travels at it.
    pub fn request_pace(&mut self, pace: i64) {
        self.overmap_pace_request = Some(pace);
    }

    /// Choose the navigator's exploration stance (empty clears it). The host sets
    /// it on the lead token, and the travel rule reads it.
    pub fn request_stance(&mut self, stance: &str) {
        self.overmap_stance_request = Some(stance.to_owned());
    }

    /// Study the party's maps. Arms a one-shot the host resolves: a literacy
    /// check that, on a pass, reveals the places just beyond the known ones. A
    /// party that cannot read a map learns nothing.
    pub fn request_map_read(&mut self) {
        self.overmap_read_request = true;
    }

    /// The pointer entered or left an overmap node (`None` clears). Drives the
    /// painted graph's hover emphasis; no game state changes.
    pub fn hover_overmap(&mut self, node: Option<String>) {
        self.overmap_hover = node;
    }

    /// Inspect one relation cell in the local overmap projection. Campaign
    /// routes remain source truth; this merely selects the Link card target.
    pub fn select_overmap_relation(&mut self, relation: String) {
        self.overmap_selected_relation = Some(relation);
    }

    /// The pointer entered or left an overmap relation cell.
    pub fn hover_overmap_relation(&mut self, relation: Option<String>) {
        self.overmap_hovered_relation = relation;
    }

    /// Toggle one relation cell's local visibility. This must not delete or
    /// mutate the campaign route it projects.
    pub fn toggle_overmap_relation_visibility(&mut self, relation: String) {
        if !self.overmap_hidden_relations.insert(relation.clone()) {
            self.overmap_hidden_relations.remove(&relation);
        }
    }

    pub fn open_generator(&mut self) {
        if !self.can_edit_inventory {
            self.status = "generation requires the host".to_owned();
        } else if self.generator_choices.is_empty() {
            self.status = "no generator packs loaded".to_owned();
        } else {
            self.generator_open = true;
        }
    }

    pub fn close_generator(&mut self) {
        self.generator_open = false;
        self.generator_preview = None;
        self.generation_request = None;
    }

    pub fn request_generation(&mut self) {
        if self.can_edit_inventory {
            self.generation_request = Some(GenerationRequest::Generate);
        } else {
            self.status = "generation requires the host".to_owned();
        }
    }

    pub fn cycle_generator(&mut self) {
        if !self.generator_choices.is_empty() {
            self.generator_selected = (self.generator_selected + 1) % self.generator_choices.len();
            self.generator_preview = None;
            self.generator_locks.clear();
            self.generation_request = None;
        }
    }

    pub fn selected_generator(&self) -> Option<&GeneratorChoice> {
        self.generator_choices.get(self.generator_selected)
    }

    /// Toggle the selected generator's first declared lock preset. A lock is
    /// a visible value passed to each reroll, never entropy replay.
    pub fn toggle_generator_lock(&mut self) {
        if !self.can_edit_inventory {
            self.status = "generation requires the host".to_owned();
            return;
        }
        let Some(preset) = self
            .selected_generator()
            .and_then(|choice| choice.lock_presets.first())
            .cloned()
        else {
            self.status = "selected generator has no lock presets".to_owned();
            return;
        };
        if self.generator_locks.remove(&preset.key).is_some() {
            self.status = format!("unlocked {}", preset.label);
        } else {
            self.generator_locks.insert(preset.key, preset.value);
            self.status = format!("locked {}", preset.label);
        }
    }

    pub fn commit_generation_preview(&mut self) {
        if self.can_edit_inventory && self.generator_preview.is_some() {
            self.generation_request = Some(GenerationRequest::Commit);
        }
    }

    pub fn discard_generation_preview(&mut self) {
        self.generator_preview = None;
        self.generation_request = None;
        self.status = "discarded generation preview".to_owned();
    }

    pub fn open_governance_conflict(&mut self) {
        if self
            .governance_conflict
            .as_ref()
            .is_some_and(|conflict| conflict.candidates.len() >= 2)
        {
            self.governance_selected = self.governance_selected.min(
                self.governance_conflict
                    .as_ref()
                    .map_or(0, |conflict| conflict.candidates.len() - 1),
            );
            self.governance_conflict_open = true;
        } else {
            self.status = "no competing campaign bindings".to_owned();
        }
    }

    pub fn close_governance_conflict(&mut self) {
        self.governance_conflict_open = false;
    }

    pub fn select_governance_candidate(&mut self, index: usize) {
        if self
            .governance_conflict
            .as_ref()
            .is_some_and(|conflict| index < conflict.candidates.len())
        {
            self.governance_selected = index;
        }
    }

    pub fn request_governance_adopt(&mut self) {
        let Some(conflict) = &self.governance_conflict else {
            return;
        };
        if !conflict.can_adopt {
            self.status = conflict
                .restriction
                .clone()
                .unwrap_or_else(|| "this conflict cannot be adopted".to_owned());
            return;
        }
        let Some(candidate) = conflict.candidates.get(self.governance_selected) else {
            return;
        };
        self.governance_resolution_request = Some(GovernanceResolutionRequest::Adopt {
            selected: candidate.proposal,
        });
        self.governance_conflict_open = false;
    }

    pub fn request_governance_branch(&mut self) {
        let Some(conflict) = &self.governance_conflict else {
            return;
        };
        if !conflict.can_branch {
            self.status = conflict
                .restriction
                .clone()
                .unwrap_or_else(|| "this conflict cannot be branched".to_owned());
            return;
        }
        self.governance_resolution_request = Some(GovernanceResolutionRequest::Branch {
            candidates: conflict
                .candidates
                .iter()
                .map(|candidate| candidate.proposal)
                .collect(),
        });
        self.governance_conflict_open = false;
    }

    /// Open the SRD compendium overlay.
    pub fn open_compendium(&mut self) {
        self.compendium_open = true;
    }

    pub fn close_compendium(&mut self) {
        self.compendium_open = false;
        self.compendium_search.clear();
    }

    /// Append a character to the compendium filter.
    pub fn search_char(&mut self, c: char) {
        self.compendium_search.push(c);
        self.compendium_scroll = 0.0;
    }

    /// Delete the last filter character.
    pub fn search_backspace(&mut self) {
        self.compendium_search.pop();
        self.compendium_scroll = 0.0;
    }

    /// Clear the compendium filter.
    pub fn clear_compendium_search(&mut self) {
        self.compendium_search.clear();
        self.compendium_scroll = 0.0;
    }

    /// Escape in the compendium: from a page back to the index, else close.
    pub fn compendium_escape(&mut self) {
        if self.compendium_selected.is_some() {
            self.back_to_index();
        } else {
            self.close_compendium();
        }
    }

    /// Sort the compendium by a column: the same column toggles direction, a
    /// new column starts ascending. Resets scroll.
    pub fn sort_compendium(&mut self, col: usize) {
        if self.compendium_sort.0 == col {
            self.compendium_sort.1 = !self.compendium_sort.1;
        } else {
            self.compendium_sort = (col, false);
        }
        self.compendium_scroll = 0.0;
    }

    /// Open an entry's page in the current compendium tab.
    pub fn open_entry(&mut self, key: String) {
        self.compendium_selected = Some(key);
    }

    /// Switch the compendium namespace, returning to that tab's index.
    pub fn set_compendium_tab(&mut self, tab: CompendiumTab) {
        self.compendium_tab = tab;
        self.compendium_selected = None;
        self.compendium_sort = (0, false);
        self.compendium_scroll = 0.0;
        self.compendium_search.clear();
    }

    /// Scroll the compendium grid by wheel `dy`, clamped to `max`.
    pub fn scroll_compendium(&mut self, dy: f32, max: f32) {
        self.compendium_scroll = (self.compendium_scroll + dy).clamp(0.0, max);
    }

    /// Back from a monster page to the index.
    pub fn back_to_index(&mut self) {
        self.compendium_selected = None;
    }

    /// Push the authoritative values into the catalog rows' selection state.
    ///
    /// Truth lives in `mode` and in the world, not in a `SelectionState`. This
    /// runs wherever truth can change (a snapshot from the authority, opening a
    /// surface), so afterwards the rows agree with it. Any *later* disagreement
    /// is the user having moved a control, which is exactly what the host pumps
    /// compare for. Without this one-way push the two would drift and the
    /// compare could not tell which side moved.
    pub fn sync_selection_rows(&mut self) {
        let mode = EditMode::ALL.iter().position(|m| *m == self.mode).unwrap_or(0);
        point_selection(&mut self.mode_selection, mode);

        let party = self.viewer.clone().unwrap_or_else(|| "dm".to_owned());
        let pace = self.world.pace(&party);
        point_selection(
            &mut self.pace_selection,
            PACE_PCTS.iter().position(|p| *p == pace).unwrap_or(1),
        );

        // Stance is per-token; the row speaks for the party's lead token.
        let stance = self
            .selected_token
            .or_else(|| self.map.tokens.first().map(|t| t.id))
            .and_then(|id| self.map.stances.get(&id).cloned())
            .unwrap_or_default();
        point_selection(
            &mut self.stance_selection,
            STANCE_KEYS
                .iter()
                .position(|k| *k == stance)
                .unwrap_or(STANCE_KEYS.len() - 1),
        );

        // The compendium's namespace nav is the same shape with the catalog's
        // tab state: truth is `compendium_tab`, the strip mirrors it.
        self.compendium_tabs.selected = CompendiumTab::ALL
            .iter()
            .position(|t| *t == self.compendium_tab)
            .unwrap_or(0);
    }
}
