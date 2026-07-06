//! Measurement and area templates: grid distance and the tile sets an
//! area effect covers (burst, line, cone). Pure substrate geometry, like
//! [`visibility`](crate::visibility) and movement. The rules layer
//! decides what a template *does*; the substrate says which tiles it
//! touches and how far apart two tiles are.

use std::collections::HashSet;

use crate::iso::TileCoord;
use crate::map::MapDocument;

/// Grid distance in tiles, Chebyshev (a diagonal step counts as one, the
/// D&D-5e "every square is 5 ft" convention).
pub fn distance(a: TileCoord, b: TileCoord) -> u32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs()) as u32
}

/// The shape of an area template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateKind {
    /// A round area centered on the origin (fireball).
    Burst,
    /// A straight line from the origin toward a target (lightning bolt).
    Line,
    /// A wedge from the origin toward a target (dragon's breath).
    Cone,
}

impl TemplateKind {
    pub const ALL: [TemplateKind; 3] = [
        TemplateKind::Burst,
        TemplateKind::Line,
        TemplateKind::Cone,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TemplateKind::Burst => "burst",
            TemplateKind::Line => "line",
            TemplateKind::Cone => "cone",
        }
    }
}

fn bresenham(a: TileCoord, b: TileCoord) -> Vec<TileCoord> {
    let (mut x0, mut y0) = a;
    let (x1, y1) = b;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut pts = vec![(x0, y0)];
    while (x0, y0) != (x1, y1) {
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
        pts.push((x0, y0));
    }
    pts
}

/// The in-bounds tiles an area template covers. `origin` is the anchor;
/// `toward` aims line and cone (ignored by burst); `size` is the radius /
/// length in tiles.
pub fn template_tiles(
    map: &MapDocument,
    origin: TileCoord,
    kind: TemplateKind,
    size: u32,
    toward: TileCoord,
) -> HashSet<TileCoord> {
    let mut out = HashSet::new();
    let keep = |out: &mut HashSet<TileCoord>, t: TileCoord| {
        if map.ground.in_bounds(t.0, t.1) {
            out.insert(t);
        }
    };
    let r = size as i32;
    match kind {
        TemplateKind::Burst => {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx * dx + dy * dy <= r * r {
                        keep(&mut out, (origin.0 + dx, origin.1 + dy));
                    }
                }
            }
        }
        TemplateKind::Line => {
            keep(&mut out, origin);
            let (dx, dy) = (toward.0 - origin.0, toward.1 - origin.1);
            if dx != 0 || dy != 0 {
                let len = ((dx * dx + dy * dy) as f64).sqrt();
                let tx = origin.0 + (dx as f64 / len * size as f64).round() as i32;
                let ty = origin.1 + (dy as f64 / len * size as f64).round() as i32;
                for cell in bresenham(origin, (tx, ty)) {
                    keep(&mut out, cell);
                }
            }
        }
        TemplateKind::Cone => {
            keep(&mut out, origin);
            let (ax, ay) = (
                (toward.0 - origin.0) as f64,
                (toward.1 - origin.1) as f64,
            );
            let alen = (ax * ax + ay * ay).sqrt();
            if alen > 0.0 {
                let (ux, uy) = (ax / alen, ay / alen);
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        if dx * dx + dy * dy > r * r {
                            continue;
                        }
                        let dlen = ((dx * dx + dy * dy) as f64).sqrt();
                        // cos of the angle between this tile and the aim.
                        let cos = (dx as f64 * ux + dy as f64 * uy) / dlen;
                        if cos >= std::f64::consts::FRAC_1_SQRT_2 {
                            // within ~45 degrees of the aim => 90-degree cone
                            keep(&mut out, (origin.0 + dx, origin.1 + dy));
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(w: u32, h: u32) -> MapDocument {
        let mut m = MapDocument::new("tpl", w, h);
        let g = m.intern_tile_kind("grass");
        for r in 0..h {
            for c in 0..w {
                m.ground.set(c, r, g);
            }
        }
        m
    }

    #[test]
    fn chebyshev_distance() {
        assert_eq!(distance((0, 0), (3, 0)), 3);
        assert_eq!(distance((0, 0), (3, 2)), 3);
        assert_eq!(distance((1, 1), (4, 5)), 4);
    }

    #[test]
    fn burst_is_a_clipped_disc() {
        let m = field(20, 20);
        let t = template_tiles(&m, (10, 10), TemplateKind::Burst, 2, (0, 0));
        assert!(t.contains(&(10, 10)));
        assert!(t.contains(&(12, 10)));
        assert!(t.contains(&(10, 8)));
        assert!(!t.contains(&(13, 10))); // 3 out, radius 2
        assert!(!t.contains(&(12, 12))); // corner outside the disc
        // Clipping: a burst at the edge stays in bounds.
        let edge = template_tiles(&m, (0, 0), TemplateKind::Burst, 3, (0, 0));
        assert!(edge.iter().all(|&(x, y)| (0..20).contains(&x) && (0..20).contains(&y)));
    }

    #[test]
    fn line_runs_toward_the_target() {
        let m = field(20, 20);
        let t = template_tiles(&m, (2, 2), TemplateKind::Line, 4, (10, 2));
        assert!(t.contains(&(2, 2)));
        assert!(t.contains(&(6, 2))); // 4 east along the line
        assert!(!t.contains(&(2, 6))); // nothing off-axis
    }

    #[test]
    fn cone_opens_toward_the_target_only() {
        let m = field(20, 20);
        // Aim east.
        let t = template_tiles(&m, (10, 10), TemplateKind::Cone, 4, (14, 10));
        assert!(t.contains(&(12, 10))); // straight ahead
        assert!(t.contains(&(12, 11))); // within the wedge
        assert!(!t.contains(&(10, 14))); // behind/perpendicular, excluded
        assert!(!t.contains(&(8, 10))); // directly behind
    }
}
