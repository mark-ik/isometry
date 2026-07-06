use serde::{Deserialize, Serialize};

/// Integer tile coordinate: `(col, row)` in grid space.
pub type TileCoord = (i32, i32);

/// A point in the board's logical pixel space (pre integer-scaling).
pub type ScreenPoint = (f32, f32);

/// The 2:1 diamond projection, Knight-of-Lodis-shaped.
///
/// The camera is fixed by doctrine (see CLAUDE.md), so this is a pure
/// coordinate transform, not a camera model. `tile_w : tile_h` is 2:1 by
/// default (32x16 footprint); `elev_step` is the logical-pixel rise per
/// height unit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IsoGeometry {
    pub tile_w: f32,
    pub tile_h: f32,
    pub elev_step: f32,
}

impl Default for IsoGeometry {
    fn default() -> Self {
        Self {
            tile_w: 32.0,
            tile_h: 16.0,
            elev_step: 8.0,
        }
    }
}

impl IsoGeometry {
    /// Screen position of the tile's diamond center, at `elevation`
    /// height units. Origin: tile (0, 0) at elevation 0 maps to (0, 0).
    pub fn tile_to_screen(&self, (col, row): TileCoord, elevation: i32) -> ScreenPoint {
        let x = (col - row) as f32 * (self.tile_w / 2.0);
        let y = (col + row) as f32 * (self.tile_h / 2.0) - elevation as f32 * self.elev_step;
        (x, y)
    }

    /// Inverse of [`Self::tile_to_screen`] at elevation 0: which tile's
    /// diamond contains this screen point. Elevation-aware picking (a
    /// click on a raised tile's top face) resolves in the view layer,
    /// which knows the height field; this is the flat-ground primitive.
    pub fn screen_to_tile(&self, (x, y): ScreenPoint) -> TileCoord {
        let a = x / (self.tile_w / 2.0);
        let b = y / (self.tile_h / 2.0);
        (
            ((a + b) / 2.0).round() as i32,
            ((b - a) / 2.0).round() as i32,
        )
    }
}

/// Painter-order key: draw lower keys first. Rows deeper into the scene
/// draw later; within a diagonal, higher elevation draws later so raised
/// terrain and the sprites standing on it cover what is behind them.
/// Sized for direct use as a CSS z-index.
pub fn depth_key((col, row): TileCoord, elevation: i32) -> i32 {
    (col + row) * 64 + elevation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_round_trips_at_elevation_zero() {
        let geo = IsoGeometry::default();
        for col in -3..8 {
            for row in -3..8 {
                let p = geo.tile_to_screen((col, row), 0);
                assert_eq!(geo.screen_to_tile(p), (col, row), "at ({col}, {row})");
            }
        }
    }

    #[test]
    fn neighbors_project_to_expected_screen_offsets() {
        let geo = IsoGeometry::default();
        let (x0, y0) = geo.tile_to_screen((0, 0), 0);
        // col+1 steps toward lower-right, row+1 toward lower-left.
        assert_eq!(geo.tile_to_screen((1, 0), 0), (x0 + 16.0, y0 + 8.0));
        assert_eq!(geo.tile_to_screen((0, 1), 0), (x0 - 16.0, y0 + 8.0));
    }

    #[test]
    fn elevation_raises_straight_up() {
        let geo = IsoGeometry::default();
        let (x0, y0) = geo.tile_to_screen((2, 2), 0);
        let (x1, y1) = geo.tile_to_screen((2, 2), 3);
        assert_eq!(x1, x0);
        assert_eq!(y1, y0 - 3.0 * geo.elev_step);
    }

    #[test]
    fn depth_orders_back_to_front_and_low_to_high() {
        assert!(depth_key((0, 0), 0) < depth_key((1, 0), 0));
        assert!(depth_key((1, 0), 0) < depth_key((1, 1), 0));
        assert!(depth_key((1, 1), 0) < depth_key((1, 1), 2));
        // A full elevation column never outranks the next diagonal.
        assert!(depth_key((1, 1), 63) < depth_key((2, 1), 0));
    }
}
