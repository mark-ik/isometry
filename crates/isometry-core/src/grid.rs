use serde::{Deserialize, Serialize};

/// A dense rectangular grid of tiles, row-major.
///
/// Coordinates are `(col, row)` with `(0, 0)` at the north corner of the
/// diamond; `col` runs toward the screen's lower-right, `row` toward the
/// lower-left (see [`crate::IsoGeometry`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileGrid<T> {
    width: u32,
    height: u32,
    cells: Vec<T>,
}

impl<T: Clone> TileGrid<T> {
    /// Build a grid filled with `fill`. Panics if either dimension is 0.
    pub fn new(width: u32, height: u32, fill: T) -> Self {
        assert!(width > 0 && height > 0, "grid dimensions must be nonzero");
        Self {
            width,
            height,
            cells: vec![fill; (width as usize) * (height as usize)],
        }
    }
}

impl<T> TileGrid<T> {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn in_bounds(&self, col: i32, row: i32) -> bool {
        col >= 0 && row >= 0 && (col as u32) < self.width && (row as u32) < self.height
    }

    pub fn get(&self, col: u32, row: u32) -> Option<&T> {
        if col < self.width && row < self.height {
            self.cells.get((row * self.width + col) as usize)
        } else {
            None
        }
    }

    /// Replace the cell at `(col, row)`, returning the previous value.
    /// Returns `None` (and changes nothing) when out of bounds.
    pub fn set(&mut self, col: u32, row: u32, value: T) -> Option<T> {
        if col < self.width && row < self.height {
            let idx = (row * self.width + col) as usize;
            Some(std::mem::replace(&mut self.cells[idx], value))
        } else {
            None
        }
    }

    /// Iterate cells with their coordinates, row-major.
    pub fn iter(&self) -> impl Iterator<Item = (u32, u32, &T)> {
        self.cells.iter().enumerate().map(move |(i, cell)| {
            let i = i as u32;
            (i % self.width, i / self.width, cell)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_round_trip() {
        let mut g = TileGrid::new(4, 3, 0u16);
        assert_eq!(g.set(3, 2, 7), Some(0));
        assert_eq!(g.get(3, 2), Some(&7));
        assert_eq!(g.get(0, 0), Some(&0));
    }

    #[test]
    fn out_of_bounds_is_none() {
        let mut g = TileGrid::new(4, 3, 0u16);
        assert_eq!(g.get(4, 0), None);
        assert_eq!(g.get(0, 3), None);
        assert_eq!(g.set(4, 0, 1), None);
        assert!(!g.in_bounds(-1, 0));
        assert!(g.in_bounds(3, 2));
    }

    #[test]
    fn iter_is_row_major_with_coords() {
        let mut g = TileGrid::new(2, 2, 0u16);
        g.set(1, 0, 10);
        g.set(0, 1, 20);
        let seen: Vec<_> = g.iter().map(|(c, r, v)| (c, r, *v)).collect();
        assert_eq!(seen, vec![(0, 0, 0), (1, 0, 10), (0, 1, 20), (1, 1, 0)]);
    }
}
