//! The appearance recipe: a token is a recipe, not an image.
//!
//! An [`Appearance`] names a stack of paper-doll layers, a [`Palette`] to
//! recolour them, and the clip vocabulary its animations expose. It is tiny
//! and serde-shippable, so peers sync the recipe and bake the pixels locally
//! (design_docs/2026-07-08_campaign_packs_plan.md, decision 4).

use serde::{Deserialize, Serialize};

use crate::voxel::{Rgb, Voxels};

/// Maps a palette index to a colour. Recolouring a token is swapping entries;
/// the silhouette is unchanged.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Palette(pub Vec<Rgb>);

impl Palette {
    pub fn new(colors: Vec<Rgb>) -> Self {
        Palette(colors)
    }

    /// Colour for an index. Unknown indices resolve to magenta so a missing
    /// palette entry is loud rather than silently invisible.
    pub fn color(&self, i: u8) -> Rgb {
        self.0.get(i as usize).copied().unwrap_or([255, 0, 255])
    }
}

/// A named animation clip over frame indices. Frames are a later concern
/// (P3, the creator); the vocabulary is fixed now so rig packs and the Lua
/// emote/action lane can agree on names.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    pub frames: Vec<u32>,
}

/// A token's full appearance: which layers stack, how they are coloured, and
/// what clips they can play. Layers are named into a host-provided library so
/// the recipe stays data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Appearance {
    pub layers: Vec<String>,
    pub palette: Palette,
    pub clips: Vec<Clip>,
}

/// Stack resolved layer volumes into one bakeable volume, sized to contain
/// them all. Later layers win on overlap (paper-doll order). Layers are
/// assumed to share an origin (the rig spec's job); a real library resolves
/// [`Appearance::layers`] to these slices.
pub fn compose(layers: &[&Voxels]) -> Voxels {
    let dx = layers.iter().map(|l| l.dx).max().unwrap_or(1);
    let dy = layers.iter().map(|l| l.dy).max().unwrap_or(1);
    let dz = layers.iter().map(|l| l.dz).max().unwrap_or(1);
    let mut out = Voxels::new(dx, dy, dz);
    for layer in layers {
        out.blit(layer, 0, 0, 0);
    }
    out
}
