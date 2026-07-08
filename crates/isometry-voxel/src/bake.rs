//! Orthographic isometric bake: a voxel volume becomes a pixel sprite.
//!
//! This is the **2D mode** producer in the lens ladder: it renders the model
//! at the locked isometric angle for each token facing and emits a
//! transparent RGBA sheet, ready to bind through the CSS tileset vocabulary.
//! The 2.5D / 3D modes render the same voxel truth live at an adjustable
//! camera; that is a later, wgpu concern.
//!
//! Technique (verified soulful in the 2026-07-08 CPU spike): rotate the model
//! about vertical in 90-degree steps (the four diagonal facings), project each
//! voxel's top vertex into a 2:1 iso grid, and splat a precomputed cube stamp
//! (top / left / right faces, three-tone shaded) with a z-buffer.

use crate::recipe::Palette;
use crate::voxel::{Rgb, Voxels};

/// Bake geometry. Defaults match the spike (small, cute voxels).
#[derive(Clone, Copy, Debug)]
pub struct BakeParams {
    /// Half-width of a voxel's top rhombus, in px. The rhombus is `2*half_w`
    /// wide and `half_w` tall (the 2:1 iso ratio).
    pub half_w: i32,
    /// Height of a voxel's side faces, in px.
    pub cube_h: i32,
    /// Number of facings to bake. Only 4 (the diagonal 90-degree steps) is
    /// supported today; 8 needs 45-degree resampling (open fork: facing count).
    pub facings: u8,
    /// Transparent border around each facing, in px.
    pub margin: i32,
}

impl Default for BakeParams {
    fn default() -> Self {
        BakeParams { half_w: 5, cube_h: 5, facings: 4, margin: 6 }
    }
}

/// A baked sprite: RGBA8, row-major, top-down, background fully transparent.
#[derive(Clone, Debug)]
pub struct Sheet {
    pub w: i32,
    pub h: i32,
    pub rgba: Vec<u8>,
}

impl Sheet {
    fn transparent(w: i32, h: i32) -> Self {
        Sheet { w, h, rgba: vec![0; (w * h * 4) as usize] }
    }
    /// Count of non-transparent pixels (test/repro helper).
    pub fn opaque_pixels(&self) -> usize {
        self.rgba.chunks_exact(4).filter(|p| p[3] > 0).count()
    }
    /// The alpha silhouette as a bitmask (test helper: recolouring must leave
    /// this unchanged).
    pub fn alpha_mask(&self) -> Vec<bool> {
        self.rgba.chunks_exact(4).map(|p| p[3] > 0).collect()
    }
}

// One voxel's screen footprint, tagged by face: 0=top, 1=left, 2=right.
// Reference point (0,0) is the top vertex of the cube.
fn build_stamp(half_w: i32, cube_h: i32) -> Vec<(i32, i32, u8)> {
    let s = half_w as f32;
    let e = cube_h as f32;
    let t = (0.0, 0.0);
    let r = (s, s / 2.0);
    let b = (0.0, s);
    let l = (-s, s / 2.0);
    let bd = (0.0, s + e);
    let rd = (s, s / 2.0 + e);
    let ld = (-s, s / 2.0 + e);

    let minx = -half_w - 1;
    let maxx = half_w + 1;
    let miny = -1;
    let maxy = half_w + cube_h + 1;
    let gw = maxx - minx + 1;
    let gh = maxy - miny + 1;
    let mut grid: Vec<i16> = vec![-1; (gw * gh) as usize];

    let inside = |p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)| -> bool {
        let d1 = (p.0 - b.0) * (a.1 - b.1) - (a.0 - b.0) * (p.1 - b.1);
        let d2 = (p.0 - c.0) * (b.1 - c.1) - (b.0 - c.0) * (p.1 - c.1);
        let d3 = (p.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (p.1 - a.1);
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    };

    // Sides first, top last, so the up-face wins on shared edges.
    let quads: [([(f32, f32); 4], u8); 3] =
        [([l, b, bd, ld], 1), ([b, r, rd, bd], 2), ([t, r, b, l], 0)];
    for (q, face) in quads {
        for gy in 0..gh {
            for gx in 0..gw {
                let p = ((minx + gx) as f32, (miny + gy) as f32);
                if inside(p, q[0], q[1], q[2]) || inside(p, q[0], q[2], q[3]) {
                    grid[(gx + gy * gw) as usize] = face as i16;
                }
            }
        }
    }

    let mut cells = Vec::new();
    for gy in 0..gh {
        for gx in 0..gw {
            let f = grid[(gx + gy * gw) as usize];
            if f >= 0 {
                cells.push((minx + gx, miny + gy, f as u8));
            }
        }
    }
    cells
}

fn shade(c: Rgb, face: u8) -> Rgb {
    let f = match face {
        0 => 1.06, // top, slight lift
        1 => 0.74, // left
        _ => 0.55, // right
    };
    let m = |v: u8| ((v as f32 * f).round()).clamp(0.0, 255.0) as u8;
    [m(c[0]), m(c[1]), m(c[2])]
}

// Four diagonal facings = 90-degree yaw steps about the model centre, done as
// exact integer coordinate swaps so 90 degrees stays pixel-crisp.
fn rot(x: f32, z: f32, facing: u8) -> (f32, f32) {
    match facing & 3 {
        0 => (x, z),
        1 => (-z, x),
        2 => (-x, -z),
        _ => (z, -x),
    }
}

/// Bake one facing of `model` under `palette` at the locked iso angle.
pub fn bake_facing(model: &Voxels, palette: &Palette, facing: u8, p: &BakeParams) -> Sheet {
    let cx = (model.dx - 1) as f32 / 2.0;
    let cz = (model.dz - 1) as f32 / 2.0;
    let (hw, ch) = (p.half_w as f32, p.cube_h as f32);

    // Project every voxel; gather (sx, sy, depth, palette index) and bounds.
    let mut pts: Vec<(f32, f32, f32, u8)> = Vec::new();
    let (mut minx, mut maxx, mut miny, mut maxy) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for (x, y, z, idx) in model.iter() {
        let (xp, zp) = rot(x as f32 - cx, z as f32 - cz, facing);
        let sx = (xp - zp) * hw;
        let sy = (xp + zp) * (hw / 2.0) - y as f32 * ch;
        let depth = (xp + zp) + y as f32;
        pts.push((sx, sy, depth, idx));
        minx = minx.min(sx);
        maxx = maxx.max(sx);
        miny = miny.min(sy);
        maxy = maxy.max(sy);
    }
    if pts.is_empty() {
        return Sheet::transparent(1, 1);
    }

    let w = (maxx - minx).ceil() as i32 + 2 * p.half_w + 2 * p.margin;
    let h = (maxy - miny).ceil() as i32 + p.half_w + p.cube_h + 2 * p.margin;
    let ox = -minx + (p.half_w + p.margin) as f32;
    let oy = -miny + p.margin as f32;

    let mut sheet = Sheet::transparent(w, h);
    let mut zbuf = vec![f32::NEG_INFINITY; (w * h) as usize];
    let stamp = build_stamp(p.half_w, p.cube_h);

    for (sx, sy, depth, idx) in pts {
        let px = (sx + ox).round() as i32;
        let py = (sy + oy).round() as i32;
        let base = palette.color(idx);
        for &(dx, dy, face) in &stamp {
            let (x, y) = (px + dx, py + dy);
            if x < 0 || x >= w || y < 0 || y >= h {
                continue;
            }
            let zi = (x + y * w) as usize;
            if depth > zbuf[zi] {
                zbuf[zi] = depth;
                let c = shade(base, face);
                let pi = zi * 4;
                sheet.rgba[pi] = c[0];
                sheet.rgba[pi + 1] = c[1];
                sheet.rgba[pi + 2] = c[2];
                sheet.rgba[pi + 3] = 255;
            }
        }
    }
    sheet
}

/// Bake every facing side by side into one strip (a simple sprite sheet).
pub fn bake_strip(model: &Voxels, palette: &Palette, p: &BakeParams) -> Sheet {
    let faces: Vec<Sheet> = (0..p.facings).map(|f| bake_facing(model, palette, f, p)).collect();
    let gap = p.margin;
    let cell_w = faces.iter().map(|s| s.w).max().unwrap_or(1);
    let cell_h = faces.iter().map(|s| s.h).max().unwrap_or(1);
    let w = cell_w * p.facings as i32 + gap * (p.facings as i32 + 1);
    let h = cell_h + gap * 2;
    let mut out = Sheet::transparent(w, h);
    for (i, s) in faces.iter().enumerate() {
        let ox = gap + i as i32 * (cell_w + gap) + (cell_w - s.w) / 2;
        let oy = gap + (cell_h - s.h);
        for y in 0..s.h {
            for x in 0..s.w {
                let src = ((x + y * s.w) * 4) as usize;
                if s.rgba[src + 3] == 0 {
                    continue;
                }
                let (dx, dy) = (ox + x, oy + y);
                if dx >= 0 && dx < w && dy >= 0 && dy < h {
                    let dst = ((dx + dy * w) * 4) as usize;
                    out.rgba[dst..dst + 4].copy_from_slice(&s.rgba[src..src + 4]);
                }
            }
        }
    }
    out
}
