//! Campaign proposal lifecycle shared by local review and p2p replication.

use serde::{Deserialize, Serialize};

/// How an accepted proposal relates to campaign history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CampaignProposalMode {
    /// Establish a new campaign root.
    Create { campaign_id: String },
    /// Apply content against one known campaign revision.
    Apply { base: [u8; 32] },
    /// Fork one known revision under a new branch name.
    Branch { base: [u8; 32], branch: String },
}

/// An inspectable proposal envelope. The full draft or patch is immutable
/// content addressed by `content_hash`; replication can move it through any
/// blob carrier without reinterpreting campaign state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignProposal {
    pub id: String,
    pub title: String,
    pub mode: CampaignProposalMode,
    pub content_hash: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CampaignProposalError {
    MissingId,
    MissingTitle,
    MissingCampaignId,
    MissingBranch,
    EmptyContent,
}

impl CampaignProposal {
    pub fn validate(&self) -> Result<(), CampaignProposalError> {
        if self.id.trim().is_empty() {
            return Err(CampaignProposalError::MissingId);
        }
        if self.title.trim().is_empty() {
            return Err(CampaignProposalError::MissingTitle);
        }
        if self.content_hash == [0; 32] {
            return Err(CampaignProposalError::EmptyContent);
        }
        match &self.mode {
            CampaignProposalMode::Create { campaign_id } if campaign_id.trim().is_empty() => {
                Err(CampaignProposalError::MissingCampaignId)
            }
            CampaignProposalMode::Branch { branch, .. } if branch.trim().is_empty() => {
                Err(CampaignProposalError::MissingBranch)
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_modes_round_trip_without_conflating_their_history_semantics() {
        let modes = [
            CampaignProposalMode::Create {
                campaign_id: "river-oath".into(),
            },
            CampaignProposalMode::Apply { base: [1; 32] },
            CampaignProposalMode::Branch {
                base: [2; 32],
                branch: "ash-ending".into(),
            },
        ];
        for mode in modes {
            let proposal = CampaignProposal {
                id: "proposal-1".into(),
                title: "River Oath".into(),
                mode,
                content_hash: [3; 32],
            };
            proposal.validate().unwrap();
            let json = serde_json::to_string(&proposal).unwrap();
            assert_eq!(
                serde_json::from_str::<CampaignProposal>(&json).unwrap(),
                proposal
            );
        }
    }
}
