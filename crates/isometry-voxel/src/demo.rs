//! Placeholder voxel models, drawn to the rig spec, deliberately temporary.
//!
//! These exist to prove the pipeline and to give the board-wiring step
//! something to bake before any artist opens MagicaVoxel. They are not the
//! art direction; they are scaffolding.

use crate::recipe::Palette;
use crate::voxel::Voxels;

/// A little humanoid and its palette. Facing 0's camera-front is `-z`.
pub fn hero() -> (Voxels, Palette) {
    // Palette indices used below.
    const SKIN: u8 = 0;
    const SHIRT: u8 = 1;
    const PANTS: u8 = 2;
    const BELT: u8 = 3;
    const HAIR: u8 = 4;
    const BOOT: u8 = 5;
    const EYE: u8 = 6;
    let palette = Palette::new(vec![
        [240, 195, 155], // skin
        [205, 65, 60],   // shirt
        [60, 72, 130],   // pants
        [40, 40, 52],    // belt
        [110, 72, 42],   // hair
        [52, 42, 40],    // boot
        [30, 28, 34],    // eye
    ]);

    let mut m = Voxels::new(10, 24, 8);
    // legs + boots
    m.fill(2, 4, 2, 9, 2, 6, PANTS);
    m.fill(6, 8, 2, 9, 2, 6, PANTS);
    m.fill(2, 4, 0, 2, 2, 6, BOOT);
    m.fill(6, 8, 0, 2, 2, 6, BOOT);
    // belt + torso
    m.fill(2, 8, 9, 10, 2, 6, BELT);
    m.fill(2, 8, 10, 17, 2, 6, SHIRT);
    // arms (shirt sleeves, skin hands)
    m.fill(0, 2, 10, 16, 3, 5, SHIRT);
    m.fill(8, 10, 10, 16, 3, 5, SHIRT);
    m.fill(0, 2, 10, 11, 3, 5, SKIN);
    m.fill(8, 10, 10, 11, 3, 5, SKIN);
    // head + hair + bangs + eyes (eyes on the -z front face)
    m.fill(3, 7, 17, 22, 2, 6, SKIN);
    m.fill(3, 7, 21, 24, 2, 6, HAIR);
    m.fill(3, 7, 21, 22, 2, 3, HAIR);
    m.set(4, 19, 2, EYE);
    m.set(6, 19, 2, EYE);
    (m, palette)
}

/// An 8x8 ground tile (grass over dirt) and its palette.
pub fn tile() -> (Voxels, Palette) {
    const GRASS: u8 = 0;
    const GRASS2: u8 = 1;
    const DIRT: u8 = 2;
    let palette = Palette::new(vec![[86, 150, 74], [66, 122, 58], [120, 92, 62]]);

    let (w, d) = (8, 8);
    let mut m = Voxels::new(w, 2, d);
    m.fill(0, w, 0, 1, 0, d, DIRT);
    m.fill(0, w, 1, 2, 0, d, GRASS);
    // deterministic darker specks on top for texture
    let mut n: u32 = 0x9e37_79b9;
    for x in 0..w {
        for z in 0..d {
            n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if (n >> 28) & 3 == 0 {
                m.set(x, 1, z, GRASS2);
            }
        }
    }
    (m, palette)
}
