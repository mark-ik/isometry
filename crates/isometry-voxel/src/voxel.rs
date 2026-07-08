//! Palette-indexed voxel volumes. `y` is up.
//!
//! Voxels store a palette *index*, not a colour, so recolouring a model is a
//! palette swap (the character-creator soul feature) rather than a repaint.
//! Colours resolve through [`crate::Palette`] at bake time.

use serde::{Deserialize, Serialize};

/// An 8-bit-per-channel colour.
pub type Rgb = [u8; 3];

/// A voxel volume: each cell is an optional palette index. `y` is up, matching
/// the substrate's elevation axis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Voxels {
    pub dx: i32,
    pub dy: i32,
    pub dz: i32,
    cells: Vec<Option<u8>>,
}

impl Voxels {
    pub fn new(dx: i32, dy: i32, dz: i32) -> Self {
        assert!(dx > 0 && dy > 0 && dz > 0, "voxel dims must be positive");
        Voxels { dx, dy, dz, cells: vec![None; (dx * dy * dz) as usize] }
    }

    #[inline]
    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if x >= 0 && x < self.dx && y >= 0 && y < self.dy && z >= 0 && z < self.dz {
            Some((x + y * self.dx + z * self.dx * self.dy) as usize)
        } else {
            None
        }
    }

    /// Set the voxel at `(x, y, z)` to palette index `pal`. Out-of-bounds is a
    /// no-op (so builders can overspill without bounds math at every call).
    pub fn set(&mut self, x: i32, y: i32, z: i32, pal: u8) {
        if let Some(i) = self.index(x, y, z) {
            self.cells[i] = Some(pal);
        }
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        self.index(x, y, z).and_then(|i| self.cells[i])
    }

    /// Fill a half-open box `[x0,x1) x [y0,y1) x [z0,z1)` with one index.
    pub fn fill(&mut self, x0: i32, x1: i32, y0: i32, y1: i32, z0: i32, z1: i32, pal: u8) {
        for z in z0..z1 {
            for y in y0..y1 {
                for x in x0..x1 {
                    self.set(x, y, z, pal);
                }
            }
        }
    }

    /// Stamp `src` into this volume at offset `(ox, oy, oz)`. This is how a
    /// paper-doll recipe stacks its layers into one bakeable volume; later
    /// layers overwrite earlier ones where they overlap.
    pub fn blit(&mut self, src: &Voxels, ox: i32, oy: i32, oz: i32) {
        for z in 0..src.dz {
            for y in 0..src.dy {
                for x in 0..src.dx {
                    if let Some(p) = src.get(x, y, z) {
                        self.set(x + ox, y + oy, z + oz, p);
                    }
                }
            }
        }
    }

    /// Count of filled voxels. Handy for tests and for the baker's cache key.
    pub fn filled(&self) -> usize {
        self.cells.iter().filter(|c| c.is_some()).count()
    }

    /// Iterate filled voxels as `(x, y, z, pal)`.
    pub fn iter(&self) -> impl Iterator<Item = (i32, i32, i32, u8)> + '_ {
        let (dx, dy) = (self.dx, self.dy);
        self.cells.iter().enumerate().filter_map(move |(i, c)| {
            c.map(|p| {
                let i = i as i32;
                (i % dx, (i / dx) % dy, i / (dx * dy), p)
            })
        })
    }
}
