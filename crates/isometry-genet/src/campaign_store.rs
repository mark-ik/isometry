//! Desktop persistence for Isometry's private campaign layer.
//!
//! The public map stays a readable JSON document. GM-only campaign state uses
//! Muniment's typed slot over its transactional redb backend, so secrets never
//! share that map file and a later web host can replace redb with OPFS at the
//! same `SlotStore` boundary.

use std::path::Path;

use codicil::Codicil;
use isometry_campaign::CampaignStore;
use isometry_net::GameSnapshot;
use muniment::{JsonSlots, RedbBackend};
use serde::{Deserialize, Serialize};

const PRIVATE_CAMPAIGN_SLOT: &str = "isometry/campaign/private";
const CAMPAIGN_CHECKPOINT_SLOT: &str = "isometry/campaign/checkpoint";

/// The durable campaign boundary: shared state plus GM-private state in one
/// store transaction. `format` gives future migrations an explicit switch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CampaignCheckpoint {
    pub format: u32,
    pub public: GameSnapshot,
    pub private: CampaignStore,
    pub history: Codicil<isometry_net::GameEvent>,
}

impl CampaignCheckpoint {
    pub const FORMAT: u32 = 1;

    pub fn new(
        public: GameSnapshot,
        private: CampaignStore,
        history: Codicil<isometry_net::GameEvent>,
    ) -> Self {
        Self {
            format: Self::FORMAT,
            public,
            private,
            history,
        }
    }
}

pub struct CampaignRepository {
    slots: JsonSlots<RedbBackend>,
}

impl CampaignRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let backend = RedbBackend::open(path).map_err(|error| error.to_string())?;
        Ok(Self {
            slots: JsonSlots::new(backend),
        })
    }

    /// Save the authoritative campaign checkpoint. One slot write commits the
    /// public snapshot and private store together in Muniment's backend.
    pub fn save_checkpoint(&self, checkpoint: &CampaignCheckpoint) -> Result<(), String> {
        pollster::block_on(self.slots.save(CAMPAIGN_CHECKPOINT_SLOT, checkpoint))
            .map_err(|error| error.to_string())
    }

    pub fn load_checkpoint(&self) -> Result<Option<CampaignCheckpoint>, String> {
        let checkpoint: Option<CampaignCheckpoint> =
            pollster::block_on(self.slots.load(CAMPAIGN_CHECKPOINT_SLOT))
                .map_err(|error| error.to_string())?;
        match checkpoint {
            Some(checkpoint) if checkpoint.format == CampaignCheckpoint::FORMAT => {
                Ok(Some(checkpoint))
            }
            Some(checkpoint) => Err(format!(
                "unsupported campaign checkpoint format: {}",
                checkpoint.format
            )),
            None => Ok(None),
        }
    }

    /// Legacy private-state slot. Existing W0a sidecars load through this while
    /// the public JSON map remains their source of truth.
    pub fn load_private(&self) -> Result<CampaignStore, String> {
        pollster::block_on(self.slots.load(PRIVATE_CAMPAIGN_SLOT))
            .map_err(|error| error.to_string())
            .map(|campaign| campaign.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isometry_campaign::{
        EntropyTape, EquipmentSlot, GenValue, GenerationRecord, GeneratorRequest, Inventory,
        ItemId, ItemInstance, ItemProposal, RevealCondition, SecretFact, WorldFact,
    };
    use isometry_core::{MapDocument, TokenId, TurnList};
    use isometry_net::GameEvent;

    #[test]
    fn campaign_checkpoint_survives_reopen() {
        let path = std::env::temp_dir().join(format!(
            "isometry-campaign-{}-{}.redb",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut campaign = CampaignStore::new();
        campaign.insert_secret(SecretFact {
            id: "sword-01.curse".to_owned(),
            text: "The sword is cursed.".to_owned(),
            tags: vec!["item:sword-01".to_owned()],
            reveal: RevealCondition::Manual,
        });

        let fact = WorldFact {
            id: "history.founding".to_owned(),
            kind: "history".to_owned(),
            text: "The city was founded on a ford.".to_owned(),
            tags: Vec::new(),
        };
        let mut history = Codicil::new();
        history.append(GameEvent::Fact(fact.clone()));
        let generation = GenerationRecord {
            id: "generated.river-blade.1".to_owned(),
            request: GeneratorRequest {
                generator: "demo:forge-item".to_owned(),
                args: GenValue::Text {
                    value: "river".to_owned(),
                },
                locks: Default::default(),
            },
            entropy: EntropyTape::from_seed(7).draw(),
            proposal: GenValue::Item {
                item: ItemProposal {
                    template: "demo:river-blade".to_owned(),
                    name: "River Blade".to_owned(),
                    tags: vec!["fixture".to_owned()],
                },
            },
        };
        history.append(GameEvent::Generation(generation.clone()));
        let mut inventory = Inventory::default();
        inventory
            .insert(ItemInstance {
                id: ItemId::new("checkpoint.sword"),
                template: "srd5e:longsword".to_owned(),
                name: "Checkpoint Sword".to_owned(),
                quantity: 1,
                tags: Vec::new(),
                modifiers: Vec::new(),
                appearance_layers: vec!["weapon:longsword".to_owned()],
            })
            .unwrap();
        inventory
            .equip(EquipmentSlot::MainHand, ItemId::new("checkpoint.sword"))
            .unwrap();
        let checkpoint = CampaignCheckpoint::new(
            GameSnapshot {
                map: MapDocument::new("checkpoint", 2, 2),
                turns: TurnList::new(),
                roll_log: Vec::new(),
                journal: vec![fact],
                inventories: std::collections::BTreeMap::from([(TokenId(1), inventory)]),
                generations: vec![generation],
                maps: Default::default(),
                active_map: None,
                world: Default::default(),
                clocks: Default::default(),

                party_cap: isometry_net::default_party_cap(),
                last_beats: Vec::new(),
                beat_seq: 0,
            },
            campaign,
            history,
        );

        CampaignRepository::open(&path)
            .unwrap()
            .save_checkpoint(&checkpoint)
            .unwrap();
        let restored = CampaignRepository::open(&path)
            .unwrap()
            .load_checkpoint()
            .unwrap();
        assert_eq!(restored, Some(checkpoint));

        std::fs::remove_file(path).unwrap();
    }
}
