use serde::{Deserialize, Serialize};

use crate::map::TokenId;

/// The substrate's turn order: an ordered list of tokens with one
/// active slot. Which order tokens enter in (speed ticks, side-based
/// rounds) is a system plugin's business; the substrate only keeps the
/// list and the cursor. A token outside the list moves freely (the
/// "dragged out of initiative" casual mode).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnList {
    entries: Vec<TokenId>,
    active: usize,
}

impl TurnList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[TokenId] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, id: TokenId) -> bool {
        self.entries.contains(&id)
    }

    /// The token whose turn it is; `None` when the list is empty.
    pub fn active(&self) -> Option<TokenId> {
        self.entries.get(self.active).copied()
    }

    /// Append `id` (no-op if already listed).
    pub fn add(&mut self, id: TokenId) {
        if !self.contains(id) {
            self.entries.push(id);
        }
    }

    /// Replace the whole order (e.g. after rolling initiative) and start
    /// the round at the top.
    pub fn set_order(&mut self, entries: Vec<TokenId>) {
        self.entries = entries;
        self.active = 0;
    }

    /// Remove `id`, keeping the active cursor on the same token when
    /// possible (the turn does not skip because someone left the list).
    pub fn remove(&mut self, id: TokenId) {
        let Some(pos) = self.entries.iter().position(|&e| e == id) else {
            return;
        };
        self.entries.remove(pos);
        if self.entries.is_empty() {
            self.active = 0;
        } else if pos < self.active {
            self.active -= 1;
        } else if self.active >= self.entries.len() {
            self.active = 0;
        }
    }

    /// Advance to the next turn, wrapping to a new round.
    pub fn advance(&mut self) {
        if !self.entries.is_empty() {
            self.active = (self.active + 1) % self.entries.len();
        }
    }

    /// Advance to the next turn that `skip` does not reject: the corpse on the
    /// floor does not get to take its turn.
    ///
    /// The substrate does not know *why* a token is skipped; a system plugin
    /// decides that (hit points at zero, stunned, fled) and the caller passes the
    /// verdict in. If every remaining token is skippable the cursor still moves
    /// exactly one step, so a wiped-out order cannot spin forever.
    pub fn advance_skipping(&mut self, skip: impl Fn(TokenId) -> bool) {
        if self.entries.is_empty() {
            return;
        }
        for _ in 0..self.entries.len() {
            self.advance();
            if !self.active().is_some_and(&skip) {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_wraps_rounds() {
        let mut t = TurnList::new();
        for i in 1..=3 {
            t.add(TokenId(i));
        }
        assert_eq!(t.active(), Some(TokenId(1)));
        t.advance();
        t.advance();
        assert_eq!(t.active(), Some(TokenId(3)));
        t.advance();
        assert_eq!(t.active(), Some(TokenId(1)));
    }

    #[test]
    fn remove_keeps_the_active_token_stable() {
        let mut t = TurnList::new();
        for i in 1..=4 {
            t.add(TokenId(i));
        }
        t.advance(); // active = 2
        t.remove(TokenId(1)); // removal before the cursor
        assert_eq!(t.active(), Some(TokenId(2)));
        t.remove(TokenId(2)); // removing the active token
        assert_eq!(t.active(), Some(TokenId(3)));
        t.remove(TokenId(4)); // cursor past the end wraps
        assert_eq!(t.active(), Some(TokenId(3)));
        t.remove(TokenId(3));
        assert_eq!(t.active(), None);
        t.advance(); // empty list: no panic
    }

    #[test]
    fn set_order_replaces_and_resets_active() {
        let mut t = TurnList::new();
        for i in 1..=3 {
            t.add(TokenId(i));
        }
        t.advance(); // active = index 1
        t.set_order(vec![TokenId(9), TokenId(8), TokenId(7)]);
        assert_eq!(t.entries(), &[TokenId(9), TokenId(8), TokenId(7)]);
        assert_eq!(t.active(), Some(TokenId(9)));
    }

    #[test]
    fn advance_skips_the_fallen() {
        let mut t = TurnList::new();
        for i in 1..=4 {
            t.add(TokenId(i));
        }
        // 2 and 3 are down; the turn passes from 1 straight to 4.
        let down = |id: TokenId| id == TokenId(2) || id == TokenId(3);
        assert_eq!(t.active(), Some(TokenId(1)));
        t.advance_skipping(down);
        assert_eq!(t.active(), Some(TokenId(4)));
        // And wraps back past them.
        t.advance_skipping(down);
        assert_eq!(t.active(), Some(TokenId(1)));
    }

    #[test]
    fn a_wiped_out_order_still_advances_exactly_once() {
        let mut t = TurnList::new();
        for i in 1..=3 {
            t.add(TokenId(i));
        }
        // Everyone is down: the cursor must move rather than spin forever.
        t.advance_skipping(|_| true);
        assert_eq!(t.active(), Some(TokenId(1)), "one full lap, back to the top");
    }

    #[test]
    fn add_is_idempotent() {
        let mut t = TurnList::new();
        t.add(TokenId(7));
        t.add(TokenId(7));
        assert_eq!(t.entries().len(), 1);
    }
}
