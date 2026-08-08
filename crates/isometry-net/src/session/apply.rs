//! Applying a `GameEvent` to a `GameSnapshot`.
//!
//! Pure and total: the same event applied to the same snapshot gives the same
//! result on every peer, which is what lets the ordered log stand in for
//! consensus.
//!
//! Split out of `session.rs` on 2026-07-24; behavior unchanged.

use super::*;

/// Why a [`GameEvent`] was rejected rather than applied. Turn ops that name a
/// token validate its existence, so a stale intent cannot desync the order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameError {
    Core(EventError),
    ConflictingFact(String),
    Inventory(InventoryError),
    UnknownItem(ItemId),
    DuplicateItem(ItemId),
    SameInventoryTransfer(TokenId),
    /// A resolution addressed a token with no sheet, so it could not be applied
    /// whole. Rejected rather than half-applied.
    UnsheetedTarget(TokenId),
    /// A travel event for a token not standing on a transition point.
    NotOnTransition(TokenId),
    ConflictingGeneration(String),
    InvalidGeneration(GenerationRecordError),
    UnknownMap(String),
    ConflictingMap(String),
    World(WorldError),
}

const MAX_GENERATION_VALUE_DEPTH: usize = 16;

/// Hand a fresh set of beats to every peer's board. Bumping the sequence is what
/// makes two identical consecutive strikes play twice rather than once.
pub(crate) fn play_beats(state: &mut GameSnapshot, beats: Vec<isometry_core::Beat>) {
    state.last_beats = beats;
    state.beat_seq = state.beat_seq.wrapping_add(1);
}

/// Append a roll to the shared log, dropping the oldest past the cap.
pub(crate) fn push_roll(state: &mut GameSnapshot, record: &isometry_core::RollRecord) {
    state.roll_log.push(record.clone());
    let overflow = state
        .roll_log
        .len()
        .saturating_sub(crate::protocol::ROLL_LOG_CAP);
    if overflow > 0 {
        state.roll_log.drain(0..overflow);
    }
}

pub fn apply_game(state: &mut GameSnapshot, event: &GameEvent) -> Result<(), GameError> {
    match event {
        GameEvent::Map(e) => {
            apply(&mut state.map, e).map_err(GameError::Core)?;
            sync_active_map(state);
            Ok(())
        }
        GameEvent::TurnAdd(id) => {
            require_token(state, *id)?;
            state.turns.add(*id);
            Ok(())
        }
        GameEvent::TurnRemove(id) => {
            state.turns.remove(*id);
            Ok(())
        }
        GameEvent::TurnAdvance => {
            // The fallen do not get a turn. Deterministic and replicated: the
            // skip is computed from state every peer already has, so nobody has
            // to be told about it separately.
            let before = state.turns.round();
            let map = &state.map;
            state.turns.advance_skipping(|id| map.is_defeated(id));
            // A turn beginning wipes the token's per-turn counters: its action
            // economy refills, its multiple-attack penalty resets. The substrate
            // clears the named ledger without knowing what any counter meant.
            if let Some(active) = state.turns.active() {
                state.map.clear_turn_counters(active);
            }
            // A completed round is elapsed time: tick the location's clock by
            // however many rounds the wrap crossed. Time is a campaign feature,
            // so a bare board with no stored map keeps no clock.
            let elapsed = state.turns.round().saturating_sub(before);
            if elapsed > 0 {
                if let Some(active) = state.active_map.clone() {
                    *state.clocks.entry(active).or_insert(0) += elapsed;
                }
            }
            Ok(())
        }
        GameEvent::TurnSetOrder(order) => {
            state.turns.set_order(order.clone());
            Ok(())
        }
        GameEvent::Rolled(record) => {
            push_roll(state, record);
            Ok(())
        }
        GameEvent::ActionResolved(res) => {
            // The idempotency rule. A verdict is taken once, named by the
            // request the authority stamped on it: a retransmit, a log replayed
            // over a state that already holds it, or an app that commits the
            // same answer twice all land here and change nothing. By identity,
            // not by content -- two identical strikes are two verdicts and both
            // must land, which is why the check is on the id and not on the
            // event.
            if state.applied_actions.contains(&res.request) {
                return Ok(());
            }
            require_token(state, res.actor)?;
            require_token(state, res.target)?;
            // Every delta must address a token that actually has a sheet. A
            // resolution that would half-apply is rejected whole, so a peer
            // either takes all of an action or none of it and the hashes cannot
            // drift apart.
            if res
                .deltas
                .iter()
                .any(|d| state.map.sheet(d.token).is_none())
            {
                return Err(GameError::UnsheetedTarget(res.target));
            }
            for delta in &res.deltas {
                state.map.apply_delta(delta);
            }
            // Forced movement is truth, so it lands here, in the ordered log,
            // where every peer applies the identical tile. A stagger beat never
            // reaches this function at all.
            for (token, to) in &res.displaced {
                if let Some(t) = state.map.tokens.iter_mut().find(|t| t.id == *token) {
                    t.at = *to;
                }
            }
            for token in &res.defeated {
                state.map.set_defeated(*token, true);
            }
            for (token, name, value) in &res.conditions {
                state.map.set_condition(*token, name, *value);
            }
            for (token, mobility) in &res.mobility {
                state.map.set_mobility(*token, *mobility);
            }
            // Allegiance: a convinced creature changes sides. The host already
            // ruled the owner and the cap; every peer applies the same change,
            // and each peer's fog recomputes from it (a new ally feeds your
            // sight, an ex-ally stops feeding it).
            for (token, owner) in &res.owner_changes {
                if let Some(t) = state.map.tokens.iter_mut().find(|t| t.id == *token) {
                    t.owner = owner.clone();
                }
            }
            // The action's per-turn spend: the acting peer's rules decided it,
            // and every peer folds the same integer deltas into the shared
            // ledger. Applied verbatim, like the sheet deltas -- the authority
            // never reruns the afford rule (that gate lives where the Lua ran).
            for (token, key, delta) in &res.turn_counters {
                state.map.bump_turn_counter(*token, key, *delta);
            }
            push_roll(state, &res.attack);
            if let Some(damage) = &res.damage {
                push_roll(state, damage);
            }
            play_beats(state, res.beats.clone());
            // Recorded only now: a resolution refused above (an unsheeted
            // target) never applied, so it must stay askable.
            state.applied_actions.insert(res.request);
            sync_active_map(state);
            Ok(())
        }
        GameEvent::Emoted { token, beat } => {
            require_token(state, *token)?;
            play_beats(state, vec![isometry_core::Beat::new(*token, beat.clone())]);
            Ok(())
        }
        GameEvent::StanceSet { token, stance } => {
            require_token(state, *token)?;
            state.map.set_stance(*token, stance);
            sync_active_map(state);
            Ok(())
        }
        GameEvent::ConditionSet {
            token,
            condition,
            value,
            mobility,
        } => {
            require_token(state, *token)?;
            state.map.set_condition(*token, condition, *value);
            state.map.set_mobility(*token, *mobility);
            sync_active_map(state);
            Ok(())
        }
        GameEvent::SheetSet { token, sheet } => {
            state.map.set_sheet(*token, sheet.clone());
            sync_active_map(state);
            Ok(())
        }
        GameEvent::Fact(fact) => {
            if !fact.id.is_empty() {
                if let Some(existing) = state.journal.iter().find(|entry| entry.id == fact.id) {
                    return if existing == fact {
                        Ok(())
                    } else {
                        Err(GameError::ConflictingFact(fact.id.clone()))
                    };
                }
            }
            state.journal.push(fact.clone());
            Ok(())
        }
        GameEvent::InventorySet { token, inventory } => {
            require_token(state, *token)?;
            inventory.validate().map_err(GameError::Inventory)?;
            for (owner, other) in &state.inventories {
                if owner != token {
                    if let Some(id) = inventory
                        .items
                        .keys()
                        .find(|id| other.items.contains_key(*id))
                    {
                        return Err(GameError::DuplicateItem(id.clone()));
                    }
                }
            }
            state.inventories.insert(*token, inventory.clone());
            Ok(())
        }
        GameEvent::ItemTransfer { from, to, item } => transfer_item(state, *from, *to, item),
        GameEvent::ItemModifierRevealed(reveal) => {
            apply_item_modifier_reveal(state, reveal)?;
            Ok(())
        }
        GameEvent::Generation(record) => {
            record
                .validate(MAX_GENERATION_VALUE_DEPTH)
                .map_err(GameError::InvalidGeneration)?;
            if let Some(existing) = state.generations.iter().find(|entry| entry.id == record.id) {
                return if existing == record {
                    Ok(())
                } else {
                    Err(GameError::ConflictingGeneration(record.id.clone()))
                };
            }
            state.generations.push(record.clone());
            Ok(())
        }
        GameEvent::MapStored(map) => {
            if map.id.trim().is_empty() {
                return Err(GameError::UnknownMap(map.id.clone()));
            }
            if let Some(existing) = state.maps.get(&map.id) {
                return if existing == map {
                    Ok(())
                } else {
                    Err(GameError::ConflictingMap(map.id.clone()))
                };
            }
            state.maps.insert(map.id.clone(), map.clone());
            Ok(())
        }
        GameEvent::MapActivated { id } => {
            let map = state
                .maps
                .get(id)
                .ok_or_else(|| GameError::UnknownMap(id.clone()))?;
            state.map = map.document.clone();
            state.active_map = Some(id.clone());
            state.turns = isometry_core::TurnList::new();
            for token in &state.map.tokens {
                state.turns.add(token.id);
            }
            Ok(())
        }
        GameEvent::World(event) => state.world.apply(event).map_err(GameError::World),
        GameEvent::Traveled { token } => travel(state, *token),
        GameEvent::TimeAdvanced { ticks } => {
            let active = state
                .active_map
                .clone()
                .ok_or_else(|| GameError::UnknownMap("<no active map>".to_owned()))?;
            *state.clocks.entry(active).or_insert(0) += ticks;
            Ok(())
        }
        GameEvent::TravelResolved {
            party,
            to,
            ticks,
            roll,
            lost: _,
            exhaustion,
            encounter,
            forage,
        } => {
            // The party arrives: its overmap position is world state, and
            // arriving discovers the place and what is one step on.
            state.world.party_node.insert(party.clone(), to.clone());
            state.world.discover_around(party, to);
            // Arriving advances the destination site's clock by the travel time,
            // so a place reached later is later there -- the C3 clock, reached
            // across the overmap instead of through a door. A bare waypoint (no
            // site) keeps no clock, so its leg is not banked anywhere yet.
            if let Some(map) = state.world.places.get(to).and_then(|p| p.map.clone()) {
                *state.clocks.entry(map).or_insert(0) += ticks;
            }
            // The march's toll: every party member gains exhaustion, a graded
            // condition, worsened to at least the level the march exacted (a
            // short leg after a long one does not refresh you). The party is the
            // tokens sharing its owner.
            if *exhaustion > 0 {
                let members: Vec<_> = state
                    .map
                    .tokens
                    .iter()
                    .filter(|t| t.owner.as_deref() == Some(party.as_str()))
                    .map(|t| t.id)
                    .collect();
                for id in members {
                    if state.map.condition_value(id, "exhaustion") < *exhaustion {
                        state.map.set_condition(id, "exhaustion", *exhaustion);
                    }
                }
            }
            // Food the party gathered on the road joins its stores.
            if *forage != 0 {
                state.world.add_party_resource(party, "food", *forage);
            }
            // A peril on the road drops the party onto the destination's tactical
            // map to fight, rather than arriving in peace: the same map switch a
            // door makes (C2). A bare waypoint with no site is a safe arrival.
            if *encounter {
                if let Some(map) = state.world.places.get(to).and_then(|p| p.map.clone()) {
                    if state.maps.contains_key(&map) {
                        state.active_map = Some(map);
                    }
                }
            }
            push_roll(state, roll);
            sync_active_map(state);
            Ok(())
        }
    }
}

/// Walk one token through the transition point it stands on. Everything is
/// derived from replicated state, so all peers land it identically.
pub(crate) fn travel(state: &mut GameSnapshot, token: TokenId) -> Result<(), GameError> {
    require_token(state, token)?;
    let at = state.map.token(token).map(|t| t.at).unwrap_or_default();
    let active_id = state
        .active_map
        .clone()
        .ok_or_else(|| GameError::UnknownMap("<no active map>".to_owned()))?;
    // The door is the tile the traveler stands on.
    let transition = state
        .maps
        .get(&active_id)
        .and_then(|m| {
            m.transitions
                .iter()
                .find(|t| (t.at.col as i32, t.at.row as i32) == at)
        })
        .cloned()
        .ok_or(GameError::NotOnTransition(token))?;
    let target = state
        .maps
        .get(&transition.target_map)
        .ok_or_else(|| GameError::UnknownMap(transition.target_map.clone()))?;

    // Destination: the target's named entry door, else its first spawn zone,
    // else the origin corner; then the first free tile scanning outward, the
    // same deterministic walk spawning already uses.
    let anchor: TileCoord = transition
        .target_entry
        .as_ref()
        .and_then(|entry| target.transitions.iter().find(|t| &t.id == entry))
        .map(|t| (t.at.col as i32, t.at.row as i32))
        .or_else(|| {
            target
                .spawn_zones
                .first()
                .and_then(|z| z.cells.first())
                .map(|c| (c.col as i32, c.row as i32))
        })
        .unwrap_or((1, 1));
    let (w, h) = (target.document.ground.width(), target.document.ground.height());
    let occupied: Vec<TileCoord> = target.document.tokens.iter().map(|t| t.at).collect();
    let mut landing = anchor;
    for d in 0..64 {
        let cand = (anchor.0 + (d % 8), anchor.1 + (d / 8));
        if cand.0 >= 0
            && cand.1 >= 0
            && (cand.0 as u32) < w
            && (cand.1 as u32) < h
            && !occupied.contains(&cand)
        {
            landing = cand;
            break;
        }
    }

    // Ids are per-map, so an arrival can collide with a resident. Mint the
    // next id above every token on every map (inventories key on TokenId
    // globally, so global uniqueness is what keeps them sound).
    let collides = target.document.tokens.iter().any(|t| t.id == token);
    let new_id = if collides {
        let max = state
            .maps
            .values()
            .flat_map(|m| m.document.tokens.iter())
            .chain(state.map.tokens.iter())
            .map(|t| t.id.0)
            .chain(state.inventories.keys().map(|id| id.0))
            .max()
            .unwrap_or(0);
        TokenId(max + 1)
    } else {
        token
    };

    // Depart: the traveler and everything it carries leaves the active map.
    let Some(pos) = state.map.tokens.iter().position(|t| t.id == token) else {
        return Err(GameError::Core(EventError::UnknownToken(token)));
    };
    let mut traveler = state.map.tokens.remove(pos);
    let sheet = state.map.sheets.remove(&token);
    let conditions = state.map.conditions.remove(&token);
    let mobility = state.map.mobility.remove(&token);
    let was_defeated = state.map.defeated.remove(&token);
    state.turns.remove(token);
    sync_active_map(state);

    // Arrive.
    traveler.id = new_id;
    traveler.at = landing;
    let target = state
        .maps
        .get_mut(&transition.target_map)
        .expect("target existed above");
    target.document.tokens.push(traveler);
    if let Some(sheet) = sheet {
        target.document.sheets.insert(new_id, sheet);
    }
    if let Some(conditions) = conditions {
        target.document.conditions.insert(new_id, conditions);
    }
    if let Some(mobility) = mobility {
        target.document.mobility.insert(new_id, mobility);
    }
    if was_defeated {
        target.document.defeated.insert(new_id);
    }
    if new_id != token {
        if let Some(inventory) = state.inventories.remove(&token) {
            state.inventories.insert(new_id, inventory);
        }
    }

    // Nobody arrives before they left: the destination's clock catches up to
    // the traveler's. This is the whole of split-party reconciliation: while
    // parties are apart their locations' clocks drift freely (simultaneity is
    // presentation), and the moment anyone crosses, the two timelines agree.
    let source_time = state.clocks.get(&active_id).copied().unwrap_or(0);
    let dest = state
        .clocks
        .entry(transition.target_map.clone())
        .or_insert(0);
    *dest = (*dest).max(source_time);

    // The board follows the last player out: when no player-owned token
    // remains on the active map, the target activates, exactly as a manual
    // `MapActivated` would (fresh board, fresh turn order).
    if !state.map.tokens.iter().any(|t| t.owner.is_some()) {
        let doc = state
            .maps
            .get(&transition.target_map)
            .expect("target existed above")
            .document
            .clone();
        state.map = doc;
        state.active_map = Some(transition.target_map.clone());
        state.turns = isometry_core::TurnList::new();
        for t in &state.map.tokens {
            state.turns.add(t.id);
        }
    }
    Ok(())
}

pub(crate) fn sync_active_map(state: &mut GameSnapshot) {
    if let Some(id) = &state.active_map {
        if let Some(map) = state.maps.get_mut(id) {
            map.document = state.map.clone();
        }
    }
}

pub(crate) fn transfer_item(
    state: &mut GameSnapshot,
    from: TokenId,
    to: TokenId,
    item: &ItemId,
) -> Result<(), GameError> {
    require_token(state, from)?;
    require_token(state, to)?;
    if from == to {
        return Err(GameError::SameInventoryTransfer(from));
    }
    let source = state
        .inventories
        .get(&from)
        .ok_or_else(|| GameError::UnknownItem(item.clone()))?;
    if !source.items.contains_key(item) {
        return Err(GameError::UnknownItem(item.clone()));
    }
    if state
        .inventories
        .get(&to)
        .is_some_and(|target| target.items.contains_key(item))
    {
        return Err(GameError::DuplicateItem(item.clone()));
    }
    let moved = state
        .inventories
        .get_mut(&from)
        .expect("source inventory was checked")
        .take(item)
        .map_err(GameError::Inventory)?;
    state
        .inventories
        .entry(to)
        .or_default()
        .insert(moved)
        .map_err(GameError::Inventory)
}

pub(crate) fn apply_item_modifier_reveal(
    state: &mut GameSnapshot,
    reveal: &ItemModifierReveal,
) -> Result<(), GameError> {
    let item = state
        .inventories
        .values_mut()
        .find_map(|inventory| inventory.item_mut(&reveal.item))
        .ok_or_else(|| GameError::UnknownItem(reveal.item.clone()))?;
    item.attach_modifier(reveal.modifier.clone())
        .map_err(GameError::Inventory)
}

pub(crate) fn require_token(state: &GameSnapshot, id: TokenId) -> Result<(), GameError> {
    if state.map.token(id).is_some() {
        Ok(())
    } else {
        Err(GameError::Core(EventError::UnknownToken(id)))
    }
}

