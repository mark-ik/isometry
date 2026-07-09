//! Loading MagicaVoxel `.vox` files into [`Voxels`] + [`Palette`].
//!
//! `.vox` is the authoring format for rig and parts packs: make it in
//! MagicaVoxel, ship the file. MagicaVoxel is Z-up while our volumes are Y-up
//! (elevation), so we remap `(x, y, z)_mv -> (x, z, y)_ours`: the file's Z
//! becomes our height. The model's 256-colour palette rides along, so a
//! recolour is still just a [`Palette`] swap.

use crate::recipe::Palette;
use crate::voxel::Voxels;

/// Load the first model of a `.vox` byte stream into a volume + palette.
/// Returns `None` if the bytes do not parse as `.vox` or carry no model.
///
/// Takes `models[0]` at its raw voxel coordinates; multi-model scene-graph
/// transforms (nTRN/nGRP/nSHP) are not applied. That covers single-model rig
/// and parts files, which is the authoring shape packs use.
pub fn load_vox(bytes: &[u8]) -> Option<(Voxels, Palette)> {
    let data = dot_vox::load_bytes(bytes).ok()?;
    let model = data.models.first()?;

    let mv_x = model.size.x as i32; // right
    let mv_y = model.size.y as i32; // depth
    let mv_z = model.size.z as i32; // up
    // Our axes: dx = right, dy = up (= mv Z), dz = depth (= mv Y).
    let mut vox = Voxels::new(mv_x.max(1), mv_z.max(1), mv_y.max(1));
    for v in &model.voxels {
        vox.set(v.x as i32, v.z as i32, v.y as i32, v.i);
    }

    // MagicaVoxel voxel index `i` selects `palette[i - 1]`; dot_vox exposes the
    // palette 1:1 with the file, so we rotate it right by one to make index `i`
    // resolve directly (our voxels store `i` verbatim).
    let mut colors: Vec<[u8; 3]> = data.palette.iter().map(|c| [c.r, c.g, c.b]).collect();
    if !colors.is_empty() {
        colors.rotate_right(1);
    }
    Some((vox, Palette::new(colors)))
}
