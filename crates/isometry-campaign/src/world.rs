//! Durable world data, storylet matching, and editable campaign drafts.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use isometry_core::{Overmap, OvermapEdge, OvermapNode};

use crate::{ItemProposal, LocalMapProposal, MapScale, SecretFact, WorldFact};

mod draft;
mod ops;
mod types;

#[cfg(test)]
mod tests;

// The 2026-07-24 split moved the bodies into the modules above; this file keeps
// the shared imports they read through `use super::*` and re-publishes the same
// surface as before.
pub use draft::*;
pub use types::*;
