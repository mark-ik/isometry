//! Generated map data and its lowering into the pure map substrate.

use isometry_core::MapDocument;
use serde::{Deserialize, Serialize};

pub const MAX_GENERATED_MAP_EDGE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapScale {
    Local,
    Region,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapPoint {
    pub col: u32,
    pub row: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapCellProposal {
    pub col: u32,
    pub row: u32,
    #[serde(default)]
    pub ground: Option<String>,
    #[serde(default)]
    pub prop: Option<String>,
    #[serde(default)]
    pub elevation: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnZone {
    pub id: String,
    pub cells: Vec<MapPoint>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapTransition {
    pub id: String,
    pub at: MapPoint,
    pub target_map: String,
    #[serde(default)]
    pub target_entry: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncounterAnchor {
    pub id: String,
    pub at: MapPoint,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Portable pack output. Sparse cells override `default_ground`; lowering
/// interns the authored string vocabulary into a `MapDocument`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalMapProposal {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub default_ground: String,
    #[serde(default)]
    pub cells: Vec<MapCellProposal>,
    #[serde(default)]
    pub spawn_zones: Vec<SpawnZone>,
    #[serde(default)]
    pub transitions: Vec<MapTransition>,
    #[serde(default)]
    pub encounter_anchors: Vec<EncounterAnchor>,
}

/// A generated or authored map retained in the campaign registry. The active
/// board remains `GameSnapshot::map`; this record carries scale and traversal
/// metadata that the substrate itself does not interpret.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CampaignMap {
    pub id: String,
    pub scale: MapScale,
    pub document: MapDocument,
    #[serde(default)]
    pub spawn_zones: Vec<SpawnZone>,
    #[serde(default)]
    pub transitions: Vec<MapTransition>,
    #[serde(default)]
    pub encounter_anchors: Vec<EncounterAnchor>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapProposalError {
    MissingId,
    InvalidDimensions { width: u32, height: u32 },
    MissingDefaultGround,
    OutOfBounds(MapPoint),
    MissingMetadataId(&'static str),
}

impl LocalMapProposal {
    pub fn lower(&self, scale: MapScale) -> Result<CampaignMap, MapProposalError> {
        if self.id.trim().is_empty() {
            return Err(MapProposalError::MissingId);
        }
        if self.width == 0
            || self.height == 0
            || self.width > MAX_GENERATED_MAP_EDGE
            || self.height > MAX_GENERATED_MAP_EDGE
        {
            return Err(MapProposalError::InvalidDimensions {
                width: self.width,
                height: self.height,
            });
        }
        if self.default_ground.trim().is_empty() {
            return Err(MapProposalError::MissingDefaultGround);
        }
        let mut document = MapDocument::new(&self.name, self.width, self.height);
        let default_ground = document.intern_tile_kind(&self.default_ground);
        for row in 0..self.height {
            for col in 0..self.width {
                document.ground.set(col, row, default_ground);
            }
        }
        for cell in &self.cells {
            let point = MapPoint {
                col: cell.col,
                row: cell.row,
            };
            require_point(self.width, self.height, point)?;
            if let Some(ground) = &cell.ground {
                let kind = document.intern_tile_kind(ground);
                document.ground.set(cell.col, cell.row, kind);
            }
            if let Some(prop) = &cell.prop {
                let kind = document.intern_tile_kind(prop);
                document.props.set(cell.col, cell.row, kind);
            }
            if let Some(elevation) = cell.elevation {
                document.elevation.set(cell.col, cell.row, elevation);
            }
        }
        for zone in &self.spawn_zones {
            if zone.id.trim().is_empty() {
                return Err(MapProposalError::MissingMetadataId("spawn zone"));
            }
            for point in &zone.cells {
                require_point(self.width, self.height, *point)?;
            }
        }
        for transition in &self.transitions {
            if transition.id.trim().is_empty() {
                return Err(MapProposalError::MissingMetadataId("transition"));
            }
            require_point(self.width, self.height, transition.at)?;
        }
        for anchor in &self.encounter_anchors {
            if anchor.id.trim().is_empty() {
                return Err(MapProposalError::MissingMetadataId("encounter anchor"));
            }
            require_point(self.width, self.height, anchor.at)?;
        }
        Ok(CampaignMap {
            id: self.id.clone(),
            scale,
            document,
            spawn_zones: self.spawn_zones.clone(),
            transitions: self.transitions.clone(),
            encounter_anchors: self.encounter_anchors.clone(),
        })
    }
}

fn require_point(width: u32, height: u32, point: MapPoint) -> Result<(), MapProposalError> {
    if point.col < width && point.row < height {
        Ok(())
    } else {
        Err(MapProposalError::OutOfBounds(point))
    }
}

impl std::fmt::Display for MapProposalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingId => write!(f, "generated map id is required"),
            Self::InvalidDimensions { width, height } => {
                write!(f, "generated map dimensions are invalid: {width}x{height}")
            }
            Self::MissingDefaultGround => write!(f, "generated map default ground is required"),
            Self::OutOfBounds(point) => {
                write!(
                    f,
                    "generated map point is out of bounds: {},{}",
                    point.col, point.row
                )
            }
            Self::MissingMetadataId(kind) => write!(f, "generated {kind} id is required"),
        }
    }
}

impl std::error::Error for MapProposalError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_proposal_lowers_to_playable_document_and_metadata() {
        let proposal = LocalMapProposal {
            id: "demo:river-cache".to_owned(),
            name: "River Cache".to_owned(),
            width: 4,
            height: 3,
            default_ground: "grass".to_owned(),
            cells: vec![MapCellProposal {
                col: 2,
                row: 1,
                ground: Some("stone".to_owned()),
                prop: Some("tree".to_owned()),
                elevation: Some(2),
            }],
            spawn_zones: vec![SpawnZone {
                id: "party".to_owned(),
                cells: vec![MapPoint { col: 0, row: 1 }],
            }],
            transitions: Vec::new(),
            encounter_anchors: vec![EncounterAnchor {
                id: "guardian".to_owned(),
                at: MapPoint { col: 3, row: 1 },
                tags: vec!["undead".to_owned()],
            }],
        };
        let map = proposal.lower(MapScale::Local).unwrap();
        assert_eq!(map.document.ground.width(), 4);
        assert_eq!(map.document.elevation.get(2, 1), Some(&2));
        assert_eq!(map.spawn_zones[0].id, "party");
        assert_eq!(map.encounter_anchors[0].tags, vec!["undead"]);
    }
}
