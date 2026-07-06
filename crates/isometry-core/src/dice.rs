//! Dice: a tiny seedable PRNG and a modifier-expression roller
//! (`1d20+5`, `2d6`, `3d8-1d4+2`). Pure and dep-free so it stays
//! deterministic under a fixed seed for tests, and so the roll result is
//! plain data the session can replicate.
//!
//! Who rolls: the initiator rolls with their own [`Rng`] and the result
//! is what crosses the wire (a [`RollRecord`]), the same friendly-table
//! trust model as fog. The table sees the roll; it is not re-rolled per
//! peer.

use serde::{Deserialize, Serialize};

/// A small xorshift64* generator. Deterministic for a given seed.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// Seed the generator. Zero is remapped (xorshift can't leave 0).
    pub fn new(seed: u64) -> Self {
        Rng(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Roll one die with `sides` faces, in `1..=sides`. `sides` must be
    /// nonzero (the roller validates the expression first).
    pub fn die(&mut self, sides: u32) -> u32 {
        (self.next_u64() % sides as u64) as u32 + 1
    }
}

/// A resolved roll ready to share: who rolled, the expression, every die
/// face in order, and the total after modifiers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollRecord {
    pub by: String,
    pub expr: String,
    pub dice: Vec<u16>,
    pub total: i32,
}

/// Cap on dice per term / sides, so a malformed or hostile expression
/// can't allocate unbounded work.
const MAX_COUNT: u32 = 100;
const MAX_SIDES: u32 = 1000;

/// Roll a modifier expression (`NdS`, `dS`, integer constants, joined by
/// `+`/`-`, whitespace ignored). Returns the total and every die face
/// rolled, or `None` if the expression doesn't parse or exceeds the
/// caps. Uses `rng` for the dice, so a fixed seed is reproducible.
pub fn roll(expr: &str, rng: &mut Rng) -> Option<(i32, Vec<u16>)> {
    let s: String = expr
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    if s.is_empty() {
        return None;
    }
    let mut total = 0i32;
    let mut dice = Vec::new();
    // Turn every '-' into "+-" so a single split on '+' yields signed
    // terms; empty pieces (a leading sign) are skipped.
    for piece in s.replace('-', "+-").split('+') {
        if piece.is_empty() {
            continue;
        }
        let neg = piece.starts_with('-');
        let body = piece.trim_start_matches('-');
        if body.is_empty() {
            return None;
        }
        let value = if let Some((count, sides)) = body.split_once('d') {
            let count: u32 = if count.is_empty() { 1 } else { count.parse().ok()? };
            let sides: u32 = sides.parse().ok()?;
            if count == 0 || sides == 0 || count > MAX_COUNT || sides > MAX_SIDES {
                return None;
            }
            let mut sum = 0i32;
            for _ in 0..count {
                let face = rng.die(sides) as u16;
                dice.push(face);
                sum += face as i32;
            }
            sum
        } else {
            body.parse::<i32>().ok()?
        };
        total += if neg { -value } else { value };
    }
    Some((total, dice))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_seed_is_reproducible() {
        let (a, da) = roll("2d6+3", &mut Rng::new(42)).unwrap();
        let (b, db) = roll("2d6+3", &mut Rng::new(42)).unwrap();
        assert_eq!((a, &da), (b, &db));
    }

    #[test]
    fn dice_stay_in_range_and_modifier_applies() {
        let mut rng = Rng::new(7);
        for _ in 0..500 {
            let (total, dice) = roll("1d20+5", &mut rng).unwrap();
            assert_eq!(dice.len(), 1);
            assert!((1..=20).contains(&dice[0]));
            assert_eq!(total, dice[0] as i32 + 5);
        }
    }

    #[test]
    fn multiple_terms_and_signs() {
        let mut rng = Rng::new(1);
        let (total, dice) = roll("3d8-1d4+2", &mut rng).unwrap();
        assert_eq!(dice.len(), 4); // 3d8 then 1d4
        let sum3d8: i32 = dice[..3].iter().map(|&d| d as i32).sum();
        let d4 = dice[3] as i32;
        assert_eq!(total, sum3d8 - d4 + 2);
        assert!((1..=4).contains(&dice[3]));
    }

    #[test]
    fn bare_die_and_constant() {
        let mut rng = Rng::new(3);
        let (t, d) = roll("d20", &mut rng).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(t, d[0] as i32);
        assert_eq!(roll("7", &mut rng), Some((7, vec![])));
    }

    #[test]
    fn rejects_garbage_and_overflow() {
        let mut rng = Rng::new(1);
        assert_eq!(roll("", &mut rng), None);
        assert_eq!(roll("d", &mut rng), None);
        assert_eq!(roll("2d", &mut rng), None);
        assert_eq!(roll("hello", &mut rng), None);
        assert_eq!(roll("999d6", &mut rng), None); // over MAX_COUNT
        assert_eq!(roll("1d0", &mut rng), None);
    }
}
