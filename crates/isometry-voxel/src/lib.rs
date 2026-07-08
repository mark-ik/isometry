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
mod recipe;
mod voxel;

pub mod demo;

pub use bake::{BakeParams, Sheet, bake_facing, bake_strip};
pub use recipe::{Appearance, Clip, Palette, compose};
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
