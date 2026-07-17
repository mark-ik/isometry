//! Host composition between a live Moot roster and campaign materialization.

use gemot::moot::{MootRoster, MootStore, MootStoreError};
use mooting::{RecognitionContext, RecognitionPolicy};

use crate::campaign_space::{
    CampaignGovernanceBinding, CampaignGovernanceStatus, CampaignRecognitionError,
    CampaignRecognitionStatus, CampaignSpaceView,
};

#[derive(Debug, thiserror::Error)]
pub enum CampaignMootError {
    #[error(transparent)]
    Moot(#[from] MootStoreError),
    #[error(transparent)]
    Recognition(#[from] CampaignRecognitionError),
}

/// One live Moot roster projected into a recognition context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignMootContext {
    pub moot_id: [u8; 32],
    pub roster: MootRoster,
    pub recognition: RecognitionContext,
}

impl CampaignMootContext {
    /// Load current membership from the Moot store and bind the supplied policy
    /// to the roster's deterministic signed-membership commitment.
    pub async fn load(
        store: &MootStore,
        moot_id: [u8; 32],
        policy: RecognitionPolicy,
    ) -> Result<Self, CampaignMootError> {
        let roster = store.roster(moot_id).await?;
        let recognition = roster.recognition_context(moot_id, policy);
        Ok(Self {
            moot_id,
            roster,
            recognition,
        })
    }

    pub fn campaign_recognition(
        &self,
        campaign: &CampaignSpaceView,
        proposal: [u8; 32],
        governance: &CampaignGovernanceBinding,
    ) -> Result<Option<CampaignRecognitionStatus>, CampaignMootError> {
        Ok(campaign.recognition_status(proposal, governance, &self.recognition)?)
    }

    pub fn governance_admission(
        &self,
        campaign: &CampaignSpaceView,
        proposal: [u8; 32],
    ) -> Result<Option<CampaignGovernanceStatus>, CampaignMootError> {
        Ok(campaign.governance_admission_status(proposal, &self.recognition)?)
    }

    pub fn governance_change(
        &self,
        campaign: &CampaignSpaceView,
        proposal: [u8; 32],
        current: &CampaignGovernanceBinding,
    ) -> Result<Option<CampaignGovernanceStatus>, CampaignMootError> {
        Ok(campaign.governance_change_status(proposal, current, &self.recognition)?)
    }
}

#[cfg(test)]
mod tests {
    use isometry_campaign::{CampaignProposal, CampaignProposalMode};
    use gemot::moot::MootEvent;
    use muniment::MemoryBackend;
    use personae::Ed25519Keypair;

    use super::*;
    use crate::campaign_space::{CampaignCollaborationEvent, CampaignSpace};

    const CAMPAIGN: [u8; 32] = [0xca; 32];
    const BRANCH: [u8; 32] = [0xba; 32];
    const MOOT: [u8; 32] = [0x6d; 32];

    #[tokio::test]
    async fn live_moot_membership_drives_campaign_recognition() {
        let moot = MootStore::in_memory();
        let alice = Ed25519Keypair::from_seed([51; 32]);
        let bob = Ed25519Keypair::from_seed([52; 32]);
        for (key, name, at_ms) in [(&alice, "alice", 1), (&bob, "bob", 2)] {
            moot.author_seed(
                key.to_seed(),
                MOOT,
                &MootEvent::Joined {
                    name: name.into(),
                    at_ms,
                },
            )
            .await
            .unwrap();
        }

        let governance = CampaignGovernanceBinding {
            moot_id: MOOT,
            campaign_policy: RecognitionPolicy::Threshold { required: 2 },
        };
        let context = CampaignMootContext::load(&moot, MOOT, governance.campaign_policy.clone())
            .await
            .unwrap();
        let context_hash = context.recognition.fingerprint().unwrap();

        let campaign = CampaignSpace::new(MemoryBackend::new(), CAMPAIGN, BRANCH);
        let proposed = campaign
            .author(
                &alice,
                &CampaignCollaborationEvent::Proposed {
                    proposal: CampaignProposal {
                        id: "shared-map".into(),
                        title: "Shared map".into(),
                        mode: CampaignProposalMode::Apply { base: [1; 32] },
                        content_hash: [2; 32],
                    },
                    at_ms: 3,
                },
                vec![],
            )
            .await
            .unwrap();
        let proposal = *proposed.hash.as_bytes();
        for (key, at_ms) in [(&alice, 4), (&bob, 5)] {
            campaign
                .author(
                    key,
                    &CampaignCollaborationEvent::Endorsed {
                        subject: proposal,
                        at_ms,
                    },
                    vec![proposal],
                )
                .await
                .unwrap();
        }
        campaign
            .author(
                &alice,
                &CampaignCollaborationEvent::RecognitionClaimed {
                    proposal,
                    resulting_head: [9; 32],
                    context_hash,
                    at_ms: 6,
                },
                vec![proposal],
            )
            .await
            .unwrap();

        let view = campaign.materialize().await.unwrap();
        let status = context
            .campaign_recognition(&view, proposal, &governance)
            .unwrap()
            .unwrap();
        assert!(status.decision.accepted);
        assert_eq!(status.applicable_heads.len(), 1);

        // Fauna does not alter the electorate or invalidate a policy context.
        moot.author_seed(
            alice.to_seed(),
            MOOT,
            &MootEvent::Shared {
                manifest_id: [7; 32],
                schema_id: "isometry.Campaign/v1".into(),
                title: "River Oath".into(),
                at_ms: 7,
            },
        )
        .await
        .unwrap();
        let after_fauna =
            CampaignMootContext::load(&moot, MOOT, governance.campaign_policy.clone())
                .await
                .unwrap();
        assert_eq!(
            context.recognition.fingerprint().unwrap(),
            after_fauna.recognition.fingerprint().unwrap()
        );

        // A real membership change creates a new context; the old claim stays
        // visible but cannot apply under the new electorate revision.
        let charlie = Ed25519Keypair::from_seed([53; 32]);
        moot.author_seed(
            charlie.to_seed(),
            MOOT,
            &MootEvent::Joined {
                name: "charlie".into(),
                at_ms: 8,
            },
        )
        .await
        .unwrap();
        let after_join = CampaignMootContext::load(&moot, MOOT, governance.campaign_policy.clone())
            .await
            .unwrap();
        let changed = after_join
            .campaign_recognition(&view, proposal, &governance)
            .unwrap()
            .unwrap();
        assert!(changed.decision.accepted);
        assert!(changed.applicable_heads.is_empty());
        assert_eq!(changed.stale_context_claims.len(), 1);
    }
}
