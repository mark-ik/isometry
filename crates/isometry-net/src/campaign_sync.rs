//! Host composition of campaign storage with p2panda LogSync and gossip.

use mooting::MunimentStore;
use muniment::Backend;
use p2panda_core::{Operation, Topic};
use p2panda_net::sync::SyncHandle;
use p2panda_net::LogSync;
use p2panda_sync::protocols::TopicLogSyncEvent;
use personae::Ed25519Keypair;
use transport::{P2pandaTransport, SyncedSpace};

use crate::campaign_space::{
    CampaignCollaborationEvent, CampaignExt, CampaignSpace, CampaignSpaceError,
};

type CampaignHandle = SyncHandle<Operation<CampaignExt>, TopicLogSyncEvent<CampaignExt>>;

#[derive(Debug, thiserror::Error)]
pub enum CampaignSyncError {
    #[error("campaign sync: {0}")]
    Sync(String),
    #[error(transparent)]
    Campaign(#[from] CampaignSpaceError),
}

/// One joined campaign branch. The host owns this pump; campaign semantics
/// remain in [`CampaignSpace`], and endpoint mechanics remain in transport.
pub struct CampaignSyncSession<B>
where
    B: Backend + Clone + Send + Sync + 'static,
{
    pub space: CampaignSpace<B>,
    handle: CampaignHandle,
    synced: SyncedSpace,
    _log_sync: LogSync<MunimentStore<B, CampaignExt>, [u8; 32], CampaignExt>,
}

impl<B> CampaignSyncSession<B>
where
    B: Backend + Clone + Send + Sync + 'static,
{
    pub async fn join(
        transport: &P2pandaTransport,
        space: CampaignSpace<B>,
    ) -> Result<Self, CampaignSyncError> {
        let (endpoint, gossip) = transport
            .sync_parts()
            .ok_or_else(|| CampaignSyncError::Sync("transport gossip is disabled".into()))?;
        let log_sync = LogSync::builder(space.sync_store(), endpoint, gossip)
            .spawn()
            .await
            .map_err(|error| CampaignSyncError::Sync(error.to_string()))?;
        let handle = log_sync
            .stream(Topic::from(space.campaign_id()), true)
            .await
            .map_err(|error| CampaignSyncError::Sync(error.to_string()))?;
        let subscription = handle
            .subscribe()
            .await
            .map_err(|error| CampaignSyncError::Sync(error.to_string()))?;
        let receiving_space = space.clone();
        let synced = SyncedSpace::drive(subscription, move |operation: Operation<CampaignExt>| {
            let space = receiving_space.clone();
            async move { matches!(space.insert(&operation).await, Ok(true)) }
        });
        Ok(Self {
            space,
            handle,
            synced,
            _log_sync: log_sync,
        })
    }

    pub async fn author(
        &self,
        keypair: &Ed25519Keypair,
        event: &CampaignCollaborationEvent,
        parents: Vec<[u8; 32]>,
    ) -> Result<Operation<CampaignExt>, CampaignSyncError> {
        let operation = self.space.author(keypair, event, parents).await?;
        self.handle
            .publish(operation.clone())
            .await
            .map_err(|error| CampaignSyncError::Sync(error.to_string()))?;
        Ok(operation)
    }

    pub fn synced(&self) -> &SyncedSpace {
        &self.synced
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use isometry_campaign::{CampaignProposal, CampaignProposalMode};
    use muniment::MemoryBackend;
    use transport::{sync_overlay_topic, Transport};

    use super::*;

    const CAMPAIGN: [u8; 32] = [0xc1; 32];
    const BRANCH: [u8; 32] = [0xb1; 32];

    fn proposal(id: &str) -> CampaignCollaborationEvent {
        CampaignCollaborationEvent::Proposed {
            proposal: CampaignProposal {
                id: id.into(),
                title: format!("Proposal {id}"),
                mode: CampaignProposalMode::Create {
                    campaign_id: "river-oath".into(),
                },
                content_hash: [7; 32],
            },
            at_ms: 1,
        }
    }

    async fn transports() -> (P2pandaTransport, P2pandaTransport) {
        let alice = P2pandaTransport::builder_from_seed([61; 32])
            .gossip()
            .bind()
            .await
            .unwrap();
        let bob = P2pandaTransport::builder_from_seed([62; 32])
            .gossip()
            .bind()
            .await
            .unwrap();
        let alice_id = alice.local_peer_id();
        let bob_id = bob.local_peer_id();
        let overlay = sync_overlay_topic(CAMPAIGN);
        alice
            .add_peer(bob.endpoint_addr().await.unwrap())
            .await
            .unwrap();
        alice.set_topics(bob_id, &[overlay]).await.unwrap();
        bob.add_peer(alice.endpoint_addr().await.unwrap())
            .await
            .unwrap();
        bob.set_topics(alice_id, &[overlay]).await.unwrap();
        (alice, bob)
    }

    async fn wait_for_count(session: &CampaignSyncSession<MemoryBackend>, count: usize) {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if session.space.materialize().await.unwrap().proposals.len() == count {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("campaign peers converge");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn personae_authors_converge_live_and_after_offline_catch_up() {
        let (alice_transport, bob_transport) = transports().await;
        let alice = CampaignSyncSession::join(
            &alice_transport,
            CampaignSpace::new(MemoryBackend::new(), CAMPAIGN, BRANCH),
        )
        .await
        .unwrap();
        let alice_key = Ed25519Keypair::from_seed([71; 32]);
        alice
            .author(&alice_key, &proposal("offline"), vec![])
            .await
            .unwrap();

        let bob = CampaignSyncSession::join(
            &bob_transport,
            CampaignSpace::new(MemoryBackend::new(), CAMPAIGN, BRANCH),
        )
        .await
        .unwrap();
        wait_for_count(&bob, 1).await;

        let bob_key = Ed25519Keypair::from_seed([72; 32]);
        bob.author(&bob_key, &proposal("live"), vec![])
            .await
            .unwrap();
        wait_for_count(&alice, 2).await;
        wait_for_count(&bob, 2).await;
        assert_eq!(
            alice.space.materialize().await.unwrap(),
            bob.space.materialize().await.unwrap()
        );
        assert!(alice.synced().sync_status().ops_received >= 1);
        assert!(bob.synced().sync_status().ops_received >= 1);
    }
}
