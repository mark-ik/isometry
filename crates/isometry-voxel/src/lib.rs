//! `isometry-voxel`: the voxel appearance pipeline.
//!
//! A token is a recipe, not an image (see
//! `design_docs/2026-07-08_campaign_packs_plan.md`). This crate turns
//! palette-indexed voxel volumes into isometric pixel sprites. In the lens
//! ladder it is the **2D mode** producer: it bakes the model at the locked
//! isometric angle for each token facing, honoring the pixel aesthetic
//! (nearest-neighbor, low internal resolution). The 2.5D / 3D modes render
//! the same voxel truth live at an adjustable camera; that is a later, wgpu
//! concern this crate's data types are meant to feed.
//!
//! It is asset production with no engine deps: the [`Sheet`]s it emits bind
//! through the CSS tileset vocabulary (bootstrap decision #4). Recolouring is
//! a [`Palette`] swap, so a character creator restyles a token without
//! touching its silhouette.

mod bake;
mod png;
mod recipe;
mod vox;
mod voxel;

pub mod demo;

pub use bake::{BakeParams, Sheet, bake_facing, bake_strip};
pub use recipe::{Appearance, Clip, Palette, compose};
pub use vox::load_vox;
pub use voxel::{Rgb, Voxels};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bakes_a_nonempty_facing() {
        let (hero, pal) = demo::hero();
        let sheet = bake_facing(&hero, &pal, 0, &BakeParams::default());
        assert!(sheet.w > 0 && sheet.h > 0);
        // A 10x24x8 figure at half_w 5 should cover a healthy pixel count.
        assert!(sheet.opaque_pixels() > 500, "got {}", sheet.opaque_pixels());
    }

    #[test]
    fn facings_differ() {
        let (hero, pal) = demo::hero();
        let p = BakeParams::default();
        let f0 = bake_facing(&hero, &pal, 0, &p);
        let f1 = bake_facing(&hero, &pal, 1, &p);
        // Same figure, different view: pixel data must differ.
        assert!(f0.rgba != f1.rgba || f0.w != f1.w, "facings 0 and 1 look identical");
    }

    #[test]
    fn palette_swap_keeps_silhouette_changes_color() {
        let (hero, base) = demo::hero();
        // A recolour: shift every entry toward blue.
        let recolor = Palette::new(base.0.iter().map(|c| [c[0] / 3, c[1] / 3, 255]).collect());
        let p = BakeParams::default();
        let a = bake_facing(&hero, &base, 0, &p);
        let b = bake_facing(&hero, &recolor, 0, &p);
        assert_eq!(a.alpha_mask(), b.alpha_mask(), "recolour must not move the silhouette");
        assert!(a.rgba != b.rgba, "recolour must change pixels");
    }

    #[test]
    fn png_data_uri_is_wellformed() {
        let (hero, pal) = demo::hero();
        let sheet = bake_facing(&hero, &pal, 0, &BakeParams::default());
        let png = sheet.to_png();
        // PNG signature.
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        // IHDR width (offset 16 = 8 sig + 4 len + 4 type) matches the sheet.
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        assert_eq!(w as i32, sheet.w);
        // Ends with IEND.
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
        let uri = sheet.to_png_data_uri();
        assert!(uri.starts_with("data:image/png;base64,"));
        assert!(uri.len() > 100);
    }

    // Build a spec-valid `.vox` in memory: MAIN { SIZE, XYZI, RGBA }.
    fn synth_vox() -> Vec<u8> {
        fn chunk(id: &[u8; 4], content: &[u8], children: &[u8]) -> Vec<u8> {
            let mut c = Vec::new();
            c.extend_from_slice(id);
            c.extend_from_slice(&(content.len() as u32).to_le_bytes());
            c.extend_from_slice(&(children.len() as u32).to_le_bytes());
            c.extend_from_slice(content);
            c.extend_from_slice(children);
            c
        }
        let mut size = Vec::new();
        for d in [2u32, 2, 3] {
            size.extend_from_slice(&d.to_le_bytes()); // mv x=2, y=2, z=3 (up)
        }
        let voxels = [(0u8, 0u8, 0u8, 1u8), (1, 1, 2, 2)];
        let mut xyzi = (voxels.len() as u32).to_le_bytes().to_vec();
        for (x, y, z, i) in voxels {
            xyzi.extend_from_slice(&[x, y, z, i]);
        }
        let mut rgba = vec![0u8; 256 * 4];
        rgba[0..4].copy_from_slice(&[220, 60, 60, 255]); // palette slot for index 1
        rgba[4..8].copy_from_slice(&[60, 200, 90, 255]); // palette slot for index 2
        let mut children = chunk(b"SIZE", &size, &[]);
        children.extend(chunk(b"XYZI", &xyzi, &[]));
        children.extend(chunk(b"RGBA", &rgba, &[]));
        let mut out = b"VOX ".to_vec();
        out.extend_from_slice(&150u32.to_le_bytes());
        out.extend(chunk(b"MAIN", &[], &children));
        out
    }

    #[test]
    fn loads_and_bakes_a_vox_model() {
        let (vox, pal) = load_vox(&synth_vox()).expect("parse .vox");
        // mv 2x2x3 (Z up) remaps to ours dx=2, dy=3 (height), dz=2 (depth).
        assert_eq!((vox.dx, vox.dy, vox.dz), (2, 3, 2));
        assert_eq!(vox.filled(), 2);
        // Index 1 resolves to the red we placed at file slot 0 (MagicaVoxel's
        // i -> palette[i-1], handled by the rotate in load_vox).
        assert_eq!(pal.color(1), [220, 60, 60]);
        assert_eq!(pal.color(2), [60, 200, 90]);
        let sheet = bake_facing(&vox, &pal, 0, &BakeParams::default());
        assert!(sheet.opaque_pixels() > 0, "a .vox bakes");
    }

    #[test]
    fn compose_stacks_layers() {
        // Two disjoint single-voxel layers compose into a two-voxel volume.
        let mut a = Voxels::new(2, 1, 1);
        a.set(0, 0, 0, 0);
        let mut b = Voxels::new(2, 1, 1);
        b.set(1, 0, 0, 1);
        let out = compose(&[&a, &b]);
        assert_eq!(out.filled(), 2);
        assert_eq!(out.get(0, 0, 0), Some(0));
        assert_eq!(out.get(1, 0, 0), Some(1));
    }

    #[test]
    fn appearance_round_trips_json() {
        let (_, pal) = demo::hero();
        let app = Appearance {
            layers: vec!["body".into(), "hair".into()],
            palette: pal,
            clips: vec![Clip { name: "idle".into(), frames: vec![0, 1, 2] }],
        };
        let json = serde_json::to_string(&app).unwrap();
        let back: Appearance = serde_json::from_str(&json).unwrap();
        assert_eq!(back.layers, app.layers);
        assert_eq!(back.clips[0].name, "idle");
    }
}
