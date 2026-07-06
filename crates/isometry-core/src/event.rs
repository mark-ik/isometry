use serde::{Deserialize, Serialize};

use crate::iso::TileCoord;
use crate::map::{Facing, Layer, MapDocument, TileKindId, Token, TokenId};

/// One entry in the session's ordered event log.
///
/// This is the replication unit for both editing and play: the host
/// validates, applies, and rebroadcasts events; every peer applies the
/// same log in the same order and stays convergent. Editor undo is the
/// inverse event, derived from the `Option` returns of [`apply`]'s
/// mutations (wired in I2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SessionEvent {
    TilePlaced {
        layer: Layer,
        at: TileCoord,
        kind: TileKindId,
    },
    ElevationSet {
        at: TileCoord,
        height: u8,
    },
    TokenPlaced(Token),
    TokenMoved {
        id: TokenId,
        to: TileCoord,
    },
    TokenFaced {
        id: TokenId,
        facing: Facing,
    },
    TokenRemoved {
        id: TokenId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventError {
    OutOfBounds(TileCoord),
    UnknownToken(TokenId),
    DuplicateToken(TokenId),
    UnknownTileKind(TileKindId),
}

/// Apply one event to the map, or reject it unchanged. The host runs
/// this as validation before broadcast; peers run it on receipt.
pub fn apply(map: &mut MapDocument, event: &SessionEvent) -> Result<(), EventError> {
    match event {
        SessionEvent::TilePlaced { layer, at, kind } => {
            if kind.0 as usize >= map.tile_kinds.len() {
                return Err(EventError::UnknownTileKind(*kind));
            }
            let (col, row) = in_bounds(map, *at)?;
            map.layer_mut(*layer).set(col, row, *kind);
            Ok(())
        }
        SessionEvent::ElevationSet { at, height } => {
            let (col, row) = in_bounds(map, *at)?;
            map.elevation.set(col, row, *height);
            Ok(())
        }
        SessionEvent::TokenPlaced(token) => {
            if map.token(token.id).is_some() {
                return Err(EventError::DuplicateToken(token.id));
            }
            in_bounds(map, token.at)?;
            map.tokens.push(token.clone());
            Ok(())
        }
        SessionEvent::TokenMoved { id, to } => {
            let to = *to;
            in_bounds(map, to)?;
            let token = map.token_mut(*id).ok_or(EventError::UnknownToken(*id))?;
            token.at = to;
            Ok(())
        }
        SessionEvent::TokenFaced { id, facing } => {
            let token = map.token_mut(*id).ok_or(EventError::UnknownToken(*id))?;
            token.facing = *facing;
            Ok(())
        }
        SessionEvent::TokenRemoved { id } => {
            let before = map.tokens.len();
            map.tokens.retain(|t| t.id != *id);
            if map.tokens.len() == before {
                return Err(EventError::UnknownToken(*id));
            }
            Ok(())
        }
    }
}

fn in_bounds(map: &MapDocument, at: TileCoord) -> Result<(u32, u32), EventError> {
    if map.ground.in_bounds(at.0, at.1) {
        Ok((at.0 as u32, at.1 as u32))
    } else {
        Err(EventError::OutOfBounds(at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> MapDocument {
        let mut m = MapDocument::new("t", 4, 4);
        m.intern_tile_kind("grass");
        m
    }

    fn knight(id: u32, at: TileCoord) -> Token {
        Token {
            id: TokenId(id),
            at,
            facing: Facing::South,
            sprite: "knight".to_owned(),
            owner: None,
        }
    }

    #[test]
    fn same_log_same_state() {
        let log = vec![
            SessionEvent::TilePlaced {
                layer: Layer::Ground,
                at: (1, 1),
                kind: TileKindId(1),
            },
            SessionEvent::ElevationSet { at: (1, 1), height: 2 },
            SessionEvent::TokenPlaced(knight(1, (0, 0))),
            SessionEvent::TokenMoved { id: TokenId(1), to: (1, 1) },
            SessionEvent::TokenFaced { id: TokenId(1), facing: Facing::East },
        ];
        let mut a = board();
        let mut b = board();
        for e in &log {
            apply(&mut a, e).unwrap();
            apply(&mut b, e).unwrap();
        }
        assert_eq!(a, b);
        assert_eq!(a.token(TokenId(1)).unwrap().at, (1, 1));
        assert_eq!(a.token(TokenId(1)).unwrap().facing, Facing::East);
    }

    #[test]
    fn rejected_events_change_nothing() {
        let mut m = board();
        apply(&mut m, &SessionEvent::TokenPlaced(knight(1, (0, 0)))).unwrap();
        let snapshot = m.clone();

        let bad: Vec<(SessionEvent, EventError)> = vec![
            (
                SessionEvent::TokenMoved { id: TokenId(1), to: (9, 0) },
                EventError::OutOfBounds((9, 0)),
            ),
            (
                SessionEvent::TokenMoved { id: TokenId(2), to: (1, 1) },
                EventError::UnknownToken(TokenId(2)),
            ),
            (
                SessionEvent::TokenPlaced(knight(1, (2, 2))),
                EventError::DuplicateToken(TokenId(1)),
            ),
            (
                SessionEvent::TilePlaced {
                    layer: Layer::Ground,
                    at: (0, 0),
                    kind: TileKindId(9),
                },
                EventError::UnknownTileKind(TileKindId(9)),
            ),
        ];
        for (event, expected) in bad {
            assert_eq!(apply(&mut m, &event), Err(expected));
            assert_eq!(m, snapshot);
        }
    }

    #[test]
    fn token_remove_round_trip() {
        let mut m = board();
        apply(&mut m, &SessionEvent::TokenPlaced(knight(1, (0, 0)))).unwrap();
        apply(&mut m, &SessionEvent::TokenRemoved { id: TokenId(1) }).unwrap();
        assert!(m.token(TokenId(1)).is_none());
        assert_eq!(
            apply(&mut m, &SessionEvent::TokenRemoved { id: TokenId(1) }),
            Err(EventError::UnknownToken(TokenId(1)))
        );
    }
}
