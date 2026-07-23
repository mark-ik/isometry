//! Isometry's Graphshell endpoint.
//!
//! Campaign selection, player visibility, cards, and intent policy stay here.
//! Graphshell receives only disclosed Scenograph scenes and presentation
//! resources.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use graphshell_endpoint::{IntentSink, PresentationSource, ProjectionCatalog, ProjectionSource};
use graphshell_protocol::{
    AdvertisedAction, BoundsRelationship, CachePolicy, CardValueV1, ContentHash,
    EndpointDescriptor, IntentEffect, IntentInvocation, IntentReference, IntentResult,
    NativeGlyphV1, PortableCardV1, PresentationBinding, PresentationCapability, PresentationCodec,
    PresentationKey, PresentationManifest, PresentationOffer, PresentationSemantics,
    ProjectionOffer, ProjectionRequest, ProjectionSession, ProjectionSnapshot, ProtocolVersion,
    ResourceRequest, ResourceResponse, SemanticRole,
};
use graphshell_protocol::{Revision, SceneEpoch, SceneSnapshot};
use isometry_campaign::{CampaignWorld, WorldPlace, WorldRoute};
use isometry_core::{MapDocument, Overmap};
use isometry_views::{overmap_score, tile_board_score};
use sceno::{InstanceId, RoutedRelation, Scene};

const OVERMAP_SESSION: &str = "loopback:isometry:overmap";
const BOARD_SESSION: &str = "loopback:isometry:tile-board";
const FRAME_INTENT: &str = "isometry.frame-projection";
const INSPECT_INTENT: &str = "isometry.inspect-tile";
const TRAVEL_INTENT: &str = "isometry.travel";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectionKind {
    Overmap,
    TileBoard,
}

/// A read-only player projection over live Isometry campaign and map state.
pub struct IsometryEndpoint {
    world: CampaignWorld,
    map: MapDocument,
    party: String,
    snapshots: BTreeMap<ProjectionSession, ProjectionSnapshot>,
    resources: BTreeMap<(ProjectionSession, ContentHash), Vec<u8>>,
    accepted_curation_intents: u64,
}

impl IsometryEndpoint {
    pub fn new(world: CampaignWorld, map: MapDocument, party: impl Into<String>) -> Self {
        Self {
            world,
            map,
            party: party.into(),
            snapshots: BTreeMap::new(),
            resources: BTreeMap::new(),
            accepted_curation_intents: 0,
        }
    }

    pub fn fixture() -> Self {
        let mut world = CampaignWorld::default();
        for (id, name, position) in [
            ("harbour", "Harbour", (0, 2)),
            ("moor", "Glass Moor", (5, 0)),
            ("observatory", "Old Observatory", (10, 3)),
        ] {
            world.places.insert(
                id.into(),
                WorldPlace {
                    id: id.into(),
                    name: name.into(),
                    tags: Vec::new(),
                    map: (id == "moor").then(|| "moor-crossing".into()),
                    position: Some(position),
                },
            );
        }
        for (id, from, to, weight) in [
            ("road-west", "harbour", "moor", 2),
            ("ridge-path", "moor", "observatory", 3),
        ] {
            world.routes.insert(
                id.into(),
                WorldRoute {
                    id: id.into(),
                    from: from.into(),
                    to: to.into(),
                    tags: Vec::new(),
                    weight,
                },
            );
        }
        world.party_node.insert("players".into(), "harbour".into());
        world.party_known.insert(
            "players".into(),
            BTreeSet::from(["harbour".into(), "moor".into(), "observatory".into()]),
        );

        let mut map = MapDocument::new("Moor crossing", 4, 3);
        let grass = map.intern_tile_kind("grass");
        let stone = map.intern_tile_kind("stone");
        for (column, row, kind) in [(0, 0, grass), (1, 0, grass), (2, 1, stone), (3, 2, grass)] {
            map.ground.set(column, row, kind);
        }
        Self::new(world, map, "players")
    }

    pub fn accepted_curation_intents(&self) -> u64 {
        self.accepted_curation_intents
    }

    fn kind(session: &ProjectionSession) -> Option<ProjectionKind> {
        match session.0.as_str() {
            OVERMAP_SESSION => Some(ProjectionKind::Overmap),
            BOARD_SESSION => Some(ProjectionKind::TileBoard),
            _ => None,
        }
    }

    fn request_for(&self, kind: ProjectionKind) -> ProjectionRequest {
        let score = match kind {
            ProjectionKind::Overmap => {
                let arrangement = overmap_score(&self.world.overmap_for(&self.party)).arrangement;
                sceno::Score::new(arrangement)
            }
            ProjectionKind::TileBoard => {
                let arrangement = tile_board_score(&self.map).arrangement;
                sceno::Score::new(arrangement)
            }
        };
        ProjectionRequest {
            version: ProtocolVersion::V1,
            session: ProjectionSession(
                match kind {
                    ProjectionKind::Overmap => OVERMAP_SESSION,
                    ProjectionKind::TileBoard => BOARD_SESSION,
                }
                .into(),
            ),
            score,
        }
    }

    fn scene(&self, kind: ProjectionKind) -> (Scene, Option<Overmap>) {
        match kind {
            ProjectionKind::Overmap => {
                let overmap = self.world.overmap_for(&self.party);
                let mut scene = scenomise::solve(&overmap_score(&overmap));
                add_overmap_routes(&mut scene, &overmap);
                (scene, Some(overmap))
            }
            ProjectionKind::TileBoard => (scenomise::solve(&tile_board_score(&self.map)), None),
        }
    }

    fn presentations(
        &mut self,
        session: &ProjectionSession,
        kind: ProjectionKind,
        scene: &SceneSnapshot,
    ) -> Result<PresentationManifest, String> {
        let mut manifest = PresentationManifest::default();
        for (instance, item) in scene.active_items_in_order() {
            let source = scene
                .tables
                .sources
                .get(item.source.0 as usize)
                .and_then(Option::as_ref)
                .ok_or_else(|| format!("scene item {} has no source", instance.0))?;
            let (title, values, badges, icon, actions) = match kind {
                ProjectionKind::Overmap => {
                    let place = self
                        .world
                        .places
                        .get(&source.id)
                        .ok_or_else(|| format!("unknown projected place {}", source.id))?;
                    let here = self.world.party_at(&self.party) == Some(place.id.as_str());
                    (
                        place.name.clone(),
                        vec![
                            CardValueV1 {
                                label: "Place".into(),
                                value: place.id.clone(),
                            },
                            CardValueV1 {
                                label: "Party".into(),
                                value: if here { "Here" } else { "Discovered" }.into(),
                            },
                        ],
                        vec!["overmap".into(), "player-visible".into()],
                        if here { "◆" } else { "◇" },
                        overmap_actions(),
                    )
                }
                ProjectionKind::TileBoard => {
                    let (column, row) = parse_tile(&source.id)?;
                    let kind = self
                        .map
                        .ground
                        .get(column as u32, row as u32)
                        .ok_or_else(|| format!("projected tile {} vanished", source.id))?;
                    let kind_name = self
                        .map
                        .tile_kinds
                        .get(kind.0 as usize)
                        .cloned()
                        .unwrap_or_else(|| format!("tile-kind-{}", kind.0));
                    (
                        format!("{kind_name} tile"),
                        vec![
                            CardValueV1 {
                                label: "Cell".into(),
                                value: format!("{column}, {row}"),
                            },
                            CardValueV1 {
                                label: "Map".into(),
                                value: self.map.name.clone(),
                            },
                        ],
                        vec!["tile-board".into(), "player-visible".into()],
                        "▱",
                        tile_actions(),
                    )
                }
            };
            let semantics = PresentationSemantics {
                label: title.clone(),
                role: SemanticRole::Article,
                bounds: BoundsRelationship::FillFootprint,
                actions,
            };
            let card = PortableCardV1 {
                title: title.clone(),
                values,
                badges,
                media: Vec::new(),
            };
            let glyph = NativeGlyphV1 {
                label: title,
                icon: Some(icon.into()),
                color: Some("#d8a657".into()),
            };
            let card_bytes = serde_json::to_vec(&card)
                .map_err(|error| format!("could not encode Isometry card: {error}"))?;
            let glyph_bytes = serde_json::to_vec(&glyph)
                .map_err(|error| format!("could not encode Isometry glyph: {error}"))?;
            let card_hash = ContentHash::of(&card_bytes);
            let glyph_hash = ContentHash::of(&glyph_bytes);
            self.resources
                .insert((session.clone(), card_hash), card_bytes.clone());
            self.resources
                .insert((session.clone(), glyph_hash), glyph_bytes.clone());
            let key = PresentationKey(source.id.clone());
            manifest.bindings.push(PresentationBinding {
                instance,
                key: key.clone(),
            });
            manifest.offers.insert(
                key,
                vec![
                    PresentationOffer {
                        codec: PresentationCodec::PortableCardV1,
                        resource: card_hash,
                        byte_size: card_bytes.len() as u64,
                        requires: PresentationCapability::PortableCard,
                        semantics: semantics.clone(),
                    },
                    PresentationOffer {
                        codec: PresentationCodec::NativeGlyphV1,
                        resource: glyph_hash,
                        byte_size: glyph_bytes.len() as u64,
                        requires: PresentationCapability::NativeGlyph,
                        semantics,
                    },
                ],
            );
        }
        Ok(manifest)
    }

    fn intent_was_advertised(&self, intent: &IntentInvocation) -> bool {
        self.snapshots.get(&intent.session).is_some_and(|snapshot| {
            snapshot.scene.active_item(intent.target).is_some()
                && snapshot
                    .presentation
                    .offers_for(intent.target)
                    .is_some_and(|offers| {
                        offers.iter().any(|offer| {
                            offer
                                .semantics
                                .actions
                                .iter()
                                .any(|action| action.intent.0 == intent.intent)
                        })
                    })
        })
    }
}

impl ProjectionCatalog for IsometryEndpoint {
    fn describe(&self) -> EndpointDescriptor {
        EndpointDescriptor {
            label: "Isometry".into(),
            projections: vec![
                ProjectionOffer {
                    label: "Player overmap".into(),
                    request: self.request_for(ProjectionKind::Overmap),
                },
                ProjectionOffer {
                    label: "Moor crossing".into(),
                    request: self.request_for(ProjectionKind::TileBoard),
                },
            ],
        }
    }
}

impl ProjectionSource for IsometryEndpoint {
    type Error = String;

    fn snapshot(&mut self, request: ProjectionRequest) -> Result<ProjectionSnapshot, Self::Error> {
        if request.version.major != ProtocolVersion::V1.major {
            return Err("projection request uses an unsupported protocol".into());
        }
        if request.score.version != sceno::SCORE_VERSION {
            return Err("projection request uses an unsupported score".into());
        }
        let kind = Self::kind(&request.session)
            .ok_or_else(|| "projection request names an unknown Isometry session".to_string())?;
        let expected = self.request_for(kind);
        if std::mem::discriminant(&request.score.arrangement)
            != std::mem::discriminant(&expected.score.arrangement)
        {
            return Err("projection request uses the wrong arrangement for this session".into());
        }
        let (scene, _) = self.scene(kind);
        let scene = SceneSnapshot::from_dense(SceneEpoch(1), Revision(1), scene)
            .map_err(|error| format!("invalid Isometry scene: {error:?}"))?;
        let presentation = self.presentations(&request.session, kind, &scene)?;
        let snapshot = ProjectionSnapshot {
            version: ProtocolVersion::V1,
            session: request.session.clone(),
            scene,
            presentation,
            cache_policy: CachePolicy::default(),
        };
        self.snapshots.insert(request.session, snapshot.clone());
        Ok(snapshot)
    }
}

impl PresentationSource for IsometryEndpoint {
    type Error = String;

    fn resource(&mut self, request: ResourceRequest) -> Result<ResourceResponse, Self::Error> {
        let bytes = self
            .resources
            .get(&(request.session.clone(), request.resource))
            .cloned()
            .ok_or_else(|| "resource was not disclosed by this Isometry session".to_string())?;
        Ok(ResourceResponse {
            session: request.session,
            resource: request.resource,
            bytes,
        })
    }
}

impl IntentSink for IsometryEndpoint {
    type Error = String;

    fn invoke(&mut self, intent: IntentInvocation) -> Result<IntentResult, Self::Error> {
        let Some(snapshot) = self.snapshots.get(&intent.session) else {
            return Err("intent arrived before an Isometry snapshot".into());
        };
        if intent.observed_epoch != snapshot.scene.epoch
            || intent.observed_revision != snapshot.scene.revision
        {
            return Ok(IntentResult::Stale {
                current_epoch: snapshot.scene.epoch,
                current_revision: snapshot.scene.revision,
            });
        }
        if !self.intent_was_advertised(&intent) {
            return Ok(IntentResult::Rejected {
                reason: "target or intent was not disclosed by this Isometry snapshot".into(),
            });
        }
        match intent.intent.as_str() {
            FRAME_INTENT | INSPECT_INTENT => {
                self.accepted_curation_intents += 1;
                Ok(IntentResult::Accepted)
            }
            TRAVEL_INTENT => Ok(IntentResult::Rejected {
                reason: "the player projection grant is read-only for campaign travel".into(),
            }),
            _ => Ok(IntentResult::Rejected {
                reason: "unknown Isometry endpoint intent".into(),
            }),
        }
    }
}

fn overmap_actions() -> Vec<AdvertisedAction> {
    vec![
        advertised(
            FRAME_INTENT,
            "Frame map",
            "Frame the disclosed player overmap.",
            IntentEffect::Curation,
        ),
        advertised(
            TRAVEL_INTENT,
            "Travel here",
            "Ask the campaign authority to move the party.",
            IntentEffect::DomainTruth,
        ),
    ]
}

fn tile_actions() -> Vec<AdvertisedAction> {
    vec![advertised(
        INSPECT_INTENT,
        "Inspect tile",
        "Focus the disclosed tile without changing the campaign.",
        IntentEffect::Curation,
    )]
}

fn advertised(
    intent: &str,
    label: &str,
    explanation: &str,
    effect: IntentEffect,
) -> AdvertisedAction {
    AdvertisedAction {
        intent: IntentReference(intent.into()),
        label: label.into(),
        explanation: explanation.into(),
        payload_schema: r#"{"type":"null"}"#.into(),
        effect,
    }
}

fn parse_tile(id: &str) -> Result<(i32, i32), String> {
    let (column, row) = id
        .split_once(':')
        .ok_or_else(|| format!("invalid projected tile id {id}"))?;
    Ok((
        column
            .parse()
            .map_err(|error| format!("invalid tile column {column}: {error}"))?,
        row.parse()
            .map_err(|error| format!("invalid tile row {row}: {error}"))?,
    ))
}

fn add_overmap_routes(scene: &mut Scene, overmap: &Overmap) {
    let instances: HashMap<_, _> = scene
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            scene
                .sources
                .get(item.source.0 as usize)
                .map(|source| (source.id.as_str(), InstanceId(index as u32)))
        })
        .collect();
    for edge in &overmap.edges {
        let (Some(&from), Some(&to)) = (
            instances.get(edge.from.as_str()),
            instances.get(edge.to.as_str()),
        ) else {
            continue;
        };
        let from_point = scene.items[from.0 as usize].transform.translate;
        let to_point = scene.items[to.0 as usize].transform.translate;
        scene.relations.push(RoutedRelation {
            from,
            to,
            space: Scene::WORLD,
            points: vec![from_point, to_point],
            kind: Some(
                if edge.directed {
                    "directed-route"
                } else {
                    "route"
                }
                .into(),
            ),
            weight: Some((edge.weight as f32 / 10.0).clamp(0.1, 1.0)),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_and_snapshots_disclose_both_player_surfaces() {
        let mut endpoint = IsometryEndpoint::fixture();
        let descriptor = endpoint.describe();
        assert_eq!(descriptor.projections.len(), 2);
        let overmap = endpoint
            .snapshot(descriptor.projections[0].request.clone())
            .unwrap();
        assert_eq!(overmap.scene.active_item_count(), 3);
        assert_eq!(overmap.scene.tables.relations.iter().flatten().count(), 2);
        assert!(overmap
            .scene
            .tables
            .sources
            .iter()
            .flatten()
            .all(|source| source.adapter == isometry_views::ISOMETRY_OVERMAP_ADAPTER));
        let board = endpoint
            .snapshot(descriptor.projections[1].request.clone())
            .unwrap();
        assert_eq!(board.scene.active_item_count(), 4);
        assert!(board
            .scene
            .tables
            .sources
            .iter()
            .flatten()
            .all(|source| source.adapter == isometry_views::ISOMETRY_TILE_BOARD_ADAPTER));
    }

    #[test]
    fn player_curation_is_accepted_while_travel_is_refused() {
        let mut endpoint = IsometryEndpoint::fixture();
        let request = endpoint.describe().projections.remove(0).request;
        let snapshot = endpoint.snapshot(request).unwrap();
        let target = snapshot.scene.active_items_in_order()[0].0;
        let invoke = |intent: &str| IntentInvocation {
            session: snapshot.session.clone(),
            target,
            observed_epoch: snapshot.scene.epoch,
            observed_revision: snapshot.scene.revision,
            intent: intent.into(),
            payload: Vec::new(),
        };
        assert_eq!(
            endpoint.invoke(invoke(FRAME_INTENT)).unwrap(),
            IntentResult::Accepted
        );
        assert!(matches!(
            endpoint.invoke(invoke(TRAVEL_INTENT)).unwrap(),
            IntentResult::Rejected { .. }
        ));
        assert_eq!(endpoint.accepted_curation_intents(), 1);
    }
}
