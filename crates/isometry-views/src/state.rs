use isometry_core::{IsoGeometry, MapDocument, TileCoord};

/// Runner state for the board screen: the substrate document plus
/// view-layer concerns (camera, selection).
pub struct UiState {
    pub map: MapDocument,
    pub geo: IsoGeometry,
    /// Board-origin offset within the pane, logical px. Snap-scrolled by
    /// whole tile steps (the tactics references scroll in steps; the
    /// smooth-pan lane waits on the netrender camera-offset composite).
    pub camera: (f32, f32),
    pub selected: Option<TileCoord>,
}

impl UiState {
    pub fn new(map: MapDocument) -> Self {
        Self {
            map,
            geo: IsoGeometry::default(),
            camera: (0.0, 0.0),
            selected: None,
        }
    }

    /// Pan by whole tiles: one step is half a tile footprint on each
    /// axis, the diamond lattice spacing.
    pub fn pan_tiles(&mut self, dc: f32, dr: f32) {
        self.camera.0 -= dc * self.geo.tile_w / 2.0;
        self.camera.1 -= dr * self.geo.tile_h / 2.0;
    }
}
