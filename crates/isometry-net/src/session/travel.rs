//! Doorway crossings: ruled once, applied everywhere.
//!
//! Two halves of one law. [`resolve_transition`] is the authority's: it reads
//! the door the traveler stands on and decides every consequence of walking
//! through it. [`apply_transition`] is everybody's: it takes those decisions
//! verbatim. Nothing below the resolver looks at a transition table, a spawn
//! zone, an occupancy scan, or another map's ids, which is what makes a peer's
//! copy of a crossing the host's crossing rather than a re-enactment of it.
//!
//! Split out of `apply.rs` at H1 (2026-08-08), where the derivation used to run
//! inside `apply_game` on every peer.

use super::apply::{require_token, sync_active_map};
use super::*;
use crate::protocol::TransitionResolved;

/// Rule one doorway crossing: what walking through the door the traveler stands
/// on actually does.
///
/// The authority's half, and the only place any of this is decided. The host's
/// door sweep calls it, and so does solo play (which *is* the authority); a
/// joined client never does, and its `TransitionResolved` intent is refused.
pub fn resolve_transition(
    state: &GameSnapshot,
    token: TokenId,
    request: RequestId,
) -> Result<TransitionResolved, GameError> {
    require_token(state, token)?;
    let at = state.map.token(token).map(|t| t.at).unwrap_or_default();
    let from_map = state
        .active_map
        .clone()
        .ok_or_else(|| GameError::UnknownMap("<no active map>".to_owned()))?;
    // The door is the tile the traveler stands on.
    let transition = state
        .maps
        .get(&from_map)
        .and_then(|m| {
            m.transitions
                .iter()
                .find(|t| (t.at.col as i32, t.at.row as i32) == at)
        })
        .cloned()
        .ok_or(GameError::NotOnTransition(token))?;
    let to_map = transition.target_map.clone();
    let target = state
        .maps
        .get(&to_map)
        .ok_or_else(|| GameError::UnknownMap(to_map.clone()))?;

    // Landing: the target's named entry door, else its first spawn zone, else
    // the origin corner; then the first free tile scanning outward, the same
    // deterministic walk spawning already uses.
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

    // Identity: ids are per-map, so an arrival can collide with a resident.
    // Mint the next id above every token on every map (inventories key on
    // TokenId globally, so global uniqueness is what keeps them sound).
    let arrival = if target.document.tokens.iter().any(|t| t.id == token) {
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
    // A replaced identity strands whatever it was carrying, so the crossing
    // names the re-key. Nothing to move means nothing to name.
    let inventory_remaps = if arrival != token && state.inventories.contains_key(&token) {
        vec![(token, arrival)]
    } else {
        Vec::new()
    };

    // Clocks: nobody arrives before they left, so the destination catches up to
    // the traveler's time. This is the whole of split-party reconciliation.
    // While parties are apart their locations' clocks drift freely (simultaneity
    // is presentation); the moment anyone crosses, the two timelines agree.
    let source_time = state.clocks.get(&from_map).copied().unwrap_or(0);
    let destination_clock = state
        .clocks
        .get(&to_map)
        .copied()
        .unwrap_or(0)
        .max(source_time);

    // The board follows the last player out. Read here, with the traveler still
    // standing on the near side, so "is anyone left" means anyone but it.
    let others_remain = state
        .map
        .tokens
        .iter()
        .any(|t| t.id != token && t.owner.is_some());
    let activated = (!others_remain).then(|| to_map.clone());

    Ok(TransitionResolved {
        request,
        token,
        from_map,
        to_map,
        landing,
        arrival,
        inventory_remaps,
        destination_clock,
        activated,
    })
}

/// Apply a ruled crossing. Every peer's half: no door is looked up, no landing
/// searched for, no id minted, no clock compared, no activation inferred.
pub(crate) fn apply_transition(
    state: &mut GameSnapshot,
    res: &TransitionResolved,
) -> Result<(), GameError> {
    // The idempotency rule, by the identity the authority stamped: a
    // retransmit, or a log replayed over a state that already holds the
    // crossing, changes nothing.
    if state.applied_actions.contains(&res.request) {
        return Ok(());
    }
    require_token(state, res.token)?;
    if state.active_map.as_deref() != Some(res.from_map.as_str()) {
        return Err(GameError::UnknownMap(res.from_map.clone()));
    }
    if !state.maps.contains_key(&res.to_map) {
        return Err(GameError::UnknownMap(res.to_map.clone()));
    }

    // Depart: the traveler and everything it is leaves the active map.
    let Some(pos) = state.map.tokens.iter().position(|t| t.id == res.token) else {
        return Err(GameError::Core(EventError::UnknownToken(res.token)));
    };
    let mut traveler = state.map.tokens.remove(pos);
    let sheet = state.map.sheets.remove(&res.token);
    let conditions = state.map.conditions.remove(&res.token);
    let mobility = state.map.mobility.remove(&res.token);
    let was_defeated = state.map.defeated.remove(&res.token);
    state.turns.remove(res.token);
    sync_active_map(state);

    // Arrive, under the id and on the tile the authority named.
    traveler.id = res.arrival;
    traveler.at = res.landing;
    let target = state
        .maps
        .get_mut(&res.to_map)
        .expect("destination map was checked above");
    target.document.tokens.push(traveler);
    if let Some(sheet) = sheet {
        target.document.sheets.insert(res.arrival, sheet);
    }
    if let Some(conditions) = conditions {
        target.document.conditions.insert(res.arrival, conditions);
    }
    if let Some(mobility) = mobility {
        target.document.mobility.insert(res.arrival, mobility);
    }
    if was_defeated {
        target.document.defeated.insert(res.arrival);
    }
    for (from, to) in &res.inventory_remaps {
        if let Some(inventory) = state.inventories.remove(from) {
            state.inventories.insert(*to, inventory);
        }
    }

    state.clocks.insert(res.to_map.clone(), res.destination_clock);

    if let Some(id) = &res.activated {
        let document = state
            .maps
            .get(id)
            .ok_or_else(|| GameError::UnknownMap(id.clone()))?
            .document
            .clone();
        state.map = document;
        state.active_map = Some(id.clone());
        state.turns = isometry_core::TurnList::new();
        for token in &state.map.tokens {
            state.turns.add(token.id);
        }
    }
    state.applied_actions.insert(res.request);
    Ok(())
}
