//! Reading a Mesocosm body profile (`mesocosm.body/v0`) into [`Voxels`] +
//! [`Palette`].
//!
//! A sibling of [`load_vox`](crate::load_vox), and the same kind of thing: an
//! importer for somebody else's format. `.vox` is the authoring input a person
//! makes in MagicaVoxel; a body profile is the artifact another *game* emits,
//! carried as a part of a `mere.pack/v1` bundle.
//!
//! # Why this crate owns no Mesocosm types
//!
//! The games wing ruled that its vessels **couple by data, not by types**.
//! Isometry is a general VTT; depending on one specific game's crates to read
//! its output would invert that, and would make every future writer a new
//! dependency instead of a new file.
//!
//! So the wire structs below are a **local mirror**, deliberately duplicated.
//! Every field is a primitive or a `Vec` of primitives, which is what makes
//! mirroring cheap enough to be the right call. Nothing here links Mesocosm.
//!
//! # Axes agree, and that was checked
//!
//! `.vox` needs an axis remap because MagicaVoxel is Z-up. Mesocosm is **Y-up**
//! like we are — its `Above` facing is axis 1, and its yaw rotates about axis 1
//! — and its grids use the same `x + y * dx + z * dx * dy` cell order. So this
//! importer copies straight across. That is verified against Mesocosm's body
//! and plan modules, not assumed from the array shapes lining up.
//!
//! # Refusal, not best effort
//!
//! Coupling by data trades a compile error for a runtime one, so the reader has
//! to be the one that notices. The header is a fixed 8-byte magic plus a
//! little-endian `u16` version, checked *before* any payload decoding, so a
//! profile from a newer writer is diagnosed as a version disagreement rather
//! than mis-decoded into plausible nonsense.

use serde::{Deserialize, Serialize};

use crate::recipe::Palette;
use crate::voxel::{Rgb, Voxels};

/// The schema this importer accepts.
pub const BODY_SCHEMA: &str = "mesocosm.body/v0";

const MAGIC: [u8; 8] = *b"MESOBODY";
const VERSION: u16 = 0;
const HEADER_LEN: usize = 10;

/// Why a body profile could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyError {
    /// Fewer bytes than a header. Not a body profile.
    TooShort { got: usize },
    /// The magic does not match. These bytes are something else.
    NotABody,
    /// This *is* a body profile, from a writer we do not agree with.
    UnknownVersion { found: u16, expected: u16 },
    /// Header good, payload not: truncated or corrupt.
    Malformed,
    /// The payload decoded but contradicts itself.
    Inconsistent,
}

/// Where one part came from. `from_species` is `None` when the part was there
/// at the lineage's founding, and `Some` when it was taken from somebody.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PartOrigin {
    pub from_species: Option<u32>,
    pub from_part: Option<u32>,
    pub epoch: u64,
}

impl PartOrigin {
    pub fn is_incorporated(&self) -> bool {
        self.from_species.is_some()
    }
}

/// A body profile as it arrives on the wire. Field order and names must match
/// the writer's; postcard is positional, so order is what actually binds.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BodyProfile {
    pub species: u32,
    pub size: [u32; 3],
    pub origin: [i32; 3],
    pub cells: Vec<u8>,
    /// Parallel to `cells`: part index + 1, or 0 for empty.
    pub attribution: Vec<u16>,
    pub parts: Vec<PartOrigin>,
}

impl BodyProfile {
    /// Reads a body profile, refusing anything it cannot vouch for.
    pub fn read(bytes: &[u8]) -> Result<Self, BodyError> {
        if bytes.len() < HEADER_LEN {
            return Err(BodyError::TooShort { got: bytes.len() });
        }
        if bytes[..8] != MAGIC {
            return Err(BodyError::NotABody);
        }
        let found = u16::from_le_bytes([bytes[8], bytes[9]]);
        if found != VERSION {
            return Err(BodyError::UnknownVersion { found, expected: VERSION });
        }

        let profile: Self =
            postcard::from_bytes(&bytes[HEADER_LEN..]).map_err(|_| BodyError::Malformed)?;

        let cells = profile.size.iter().map(|d| *d as usize).product::<usize>();
        let highest = profile.attribution.iter().copied().max().unwrap_or(0) as usize;
        if cells != profile.cells.len()
            || cells != profile.attribution.len()
            || highest > profile.parts.len()
        {
            return Err(BodyError::Inconsistent);
        }
        Ok(profile)
    }

    /// The body as a volume indexed by **material**, which is how the writing
    /// game coloured it.
    pub fn voxels(&self) -> Voxels {
        self.build(|index| self.cells[index])
    }

    /// The body as a volume indexed by **part**, so a caller can colour by
    /// where each piece came from rather than what it is made of.
    ///
    /// This is the half that matters for a body grown by incorporation: the
    /// world is colour-coded by role, and a creature is colour-coded by
    /// history. Palette index is the part slot + 1, matching
    /// [`origin_palette`](Self::origin_palette).
    ///
    /// Bodies with more than 255 parts saturate, because a palette index is a
    /// `u8`; nothing that grows a limb at a time approaches that.
    pub fn voxels_by_part(&self) -> Voxels {
        self.build(|index| self.attribution[index].min(u8::MAX as u16) as u8)
    }

    /// A palette matching [`voxels_by_part`](Self::voxels_by_part): founding
    /// parts take `own`, incorporated parts take `taken`.
    ///
    /// Deliberately two colours rather than a gradient. The question a viewer
    /// asks of a grown body is "which of this was always mine", and a two-way
    /// answer reads at sprite scale where a per-species hue would not.
    pub fn origin_palette(&self, own: Rgb, taken: Rgb) -> Palette {
        let mut colors = Vec::with_capacity(self.parts.len() + 1);
        colors.push([0, 0, 0]); // slot 0 is empty and never drawn
        for part in &self.parts {
            colors.push(if part.is_incorporated() { taken } else { own });
        }
        Palette::new(colors)
    }

    /// Which part occupies a cell, as an index into [`parts`](Self::parts).
    pub fn part_at(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let index = self.index(x, y, z)?;
        match self.attribution[index] {
            0 => None,
            slot => Some(slot as usize - 1),
        }
    }

    /// Where whatever occupies a cell came from.
    pub fn origin_at(&self, x: i32, y: i32, z: i32) -> Option<PartOrigin> {
        self.parts.get(self.part_at(x, y, z)?).copied()
    }

    /// How many of this body's parts were taken from other organisms.
    pub fn incorporated_parts(&self) -> usize {
        self.parts.iter().filter(|part| part.is_incorporated()).count()
    }

    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let [dx, dy, dz] = self.size.map(|d| d as i32);
        if x < 0 || y < 0 || z < 0 || x >= dx || y >= dy || z >= dz {
            return None;
        }
        Some((x + y * dx + z * dx * dy) as usize)
    }

    fn build(&self, pick: impl Fn(usize) -> u8) -> Voxels {
        let [dx, dy, dz] = self.size.map(|d| (d as i32).max(1));
        let mut voxels = Voxels::new(dx, dy, dz);
        for z in 0..dz {
            for y in 0..dy {
                for x in 0..dx {
                    let index = (x + y * dx + z * dx * dy) as usize;
                    if self.cells[index] != 0 {
                        voxels.set(x, y, z, pick(index));
                    }
                }
            }
        }
        voxels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The writer's side, mirrored for the test. If this and [`BodyProfile`]
    /// ever disagree, the round-trip below fails, which is exactly the drift
    /// this seam trades a compile error for.
    #[derive(Serialize)]
    struct Wire {
        species: u32,
        size: [u32; 3],
        origin: [i32; 3],
        cells: Vec<u8>,
        attribution: Vec<u16>,
        parts: Vec<PartOrigin>,
    }

    /// A 2x1x1 body: one founding cell, one taken from species 42.
    fn wire() -> Wire {
        Wire {
            species: 7,
            size: [2, 1, 1],
            origin: [-1, 0, 0],
            cells: vec![1, 2],
            attribution: vec![1, 2],
            parts: vec![
                PartOrigin { from_species: None, from_part: None, epoch: 0 },
                PartOrigin { from_species: Some(42), from_part: Some(3), epoch: 5 },
            ],
        }
    }

    fn framed(wire: &Wire) -> Vec<u8> {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&postcard::to_allocvec(wire).unwrap());
        bytes
    }

    #[test]
    fn a_body_profile_reads() {
        let body = BodyProfile::read(&framed(&wire())).unwrap();
        assert_eq!(body.species, 7);
        assert_eq!(body.size, [2, 1, 1]);
        assert_eq!(body.parts.len(), 2);
    }

    #[test]
    fn provenance_survives_the_crossing() {
        let body = BodyProfile::read(&framed(&wire())).unwrap();
        assert_eq!(body.incorporated_parts(), 1);
        assert_eq!(
            body.origin_at(1, 0, 0),
            Some(PartOrigin { from_species: Some(42), from_part: Some(3), epoch: 5 })
        );
        assert!(!body.origin_at(0, 0, 0).unwrap().is_incorporated());
    }

    #[test]
    fn a_body_bakes_to_a_sprite() {
        use crate::{BakeParams, bake_facing};

        let body = BodyProfile::read(&framed(&wire())).unwrap();
        let sheet = bake_facing(
            &body.voxels_by_part(),
            &body.origin_palette([90, 140, 60], [200, 120, 70]),
            0,
            &BakeParams::default(),
        );
        assert!(sheet.w > 0 && sheet.h > 0, "a body profile produces a sprite");
    }

    #[test]
    fn colouring_by_part_separates_taken_from_own() {
        let body = BodyProfile::read(&framed(&wire())).unwrap();
        let voxels = body.voxels_by_part();
        assert_eq!(voxels.get(0, 0, 0), Some(1));
        assert_eq!(voxels.get(1, 0, 0), Some(2));

        let palette = body.origin_palette([90, 140, 60], [200, 120, 70]);
        assert_eq!(palette.color(1), [90, 140, 60], "the founding part is its own");
        assert_eq!(palette.color(2), [200, 120, 70], "the taken part reads as taken");
    }

    #[test]
    fn material_colouring_is_still_available() {
        let body = BodyProfile::read(&framed(&wire())).unwrap();
        let voxels = body.voxels();
        assert_eq!(voxels.get(0, 0, 0), Some(1));
        assert_eq!(voxels.get(1, 0, 0), Some(2));
    }

    #[test]
    fn foreign_bytes_are_refused() {
        assert_eq!(
            BodyProfile::read(b"NOTABODY plus payload"),
            Err(BodyError::NotABody)
        );
        assert_eq!(BodyProfile::read(b"MESO"), Err(BodyError::TooShort { got: 4 }));
    }

    #[test]
    fn a_newer_writer_is_diagnosed_not_guessed_at() {
        let mut bytes = framed(&wire());
        bytes[8..10].copy_from_slice(&99u16.to_le_bytes());
        assert_eq!(
            BodyProfile::read(&bytes),
            Err(BodyError::UnknownVersion { found: 99, expected: VERSION })
        );
    }

    #[test]
    fn a_contradictory_profile_is_refused() {
        let mut bad = wire();
        bad.attribution.pop();
        assert_eq!(BodyProfile::read(&framed(&bad)), Err(BodyError::Inconsistent));

        let mut past_the_end = wire();
        past_the_end.attribution[0] = 99;
        assert_eq!(BodyProfile::read(&framed(&past_the_end)), Err(BodyError::Inconsistent));
    }
}
