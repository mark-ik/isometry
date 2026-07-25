//! The collaboration and governance vocabulary: events, bindings, claims,
//! proposals, and the errors each can raise.
//!
//! Split out of `campaign_space.rs` on 2026-07-24; behavior unchanged.

use super::*;

/// Signed campaign and branch addressing plus causal proposal dependencies.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignExt {
    pub campaign_id: [u8; 32],
    pub branch_id: [u8; 32],
    #[serde(default)]
    pub parents: Vec<[u8; 32]>,
}

/// A social record in campaign history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CampaignCollaborationEvent {
    Proposed {
        proposal: CampaignProposal,
        at_ms: u64,
    },
    Endorsed {
        subject: [u8; 32],
        at_ms: u64,
    },
    /// Propose binding this campaign to one Moot and selecting the policy that
    /// Moot will use for later campaign decisions.
    GovernanceProposed {
        binding: CampaignGovernanceBinding,
        at_ms: u64,
    },
    /// Claim that the target Moot recognized a governance proposal under one
    /// frozen Moot policy context.
    GovernanceClaimed {
        proposal: [u8; 32],
        context_hash: [u8; 32],
        at_ms: u64,
    },
    /// Propose an explicit outcome for a set of competing, accepted campaign
    /// governance bindings.
    GovernanceResolutionProposed {
        resolution: CampaignGovernanceResolution,
        at_ms: u64,
    },
    /// Claim that the governing Moot recognized a resolution proposal under
    /// one frozen policy context.
    GovernanceResolutionClaimed {
        proposal: [u8; 32],
        context_hash: [u8; 32],
        at_ms: u64,
    },
    /// Claim the state head produced by applying a proposal under one frozen
    /// recognition context. Materialization verifies the context and policy.
    RecognitionClaimed {
        proposal: [u8; 32],
        resulting_head: [u8; 32],
        context_hash: [u8; 32],
        at_ms: u64,
    },
}

impl CampaignCollaborationEvent {
    pub(crate) fn at_ms(&self) -> u64 {
        match self {
            Self::Proposed { at_ms, .. }
            | Self::Endorsed { at_ms, .. }
            | Self::GovernanceProposed { at_ms, .. }
            | Self::GovernanceClaimed { at_ms, .. }
            | Self::GovernanceResolutionProposed { at_ms, .. }
            | Self::GovernanceResolutionClaimed { at_ms, .. }
            | Self::RecognitionClaimed { at_ms, .. } => *at_ms,
        }
    }
}

/// A proposed campaign-to-Moot association and the policy that association
/// installs for subsequent campaign decisions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignGovernanceBinding {
    pub moot_id: [u8; 32],
    pub campaign_policy: RecognitionPolicy,
}

impl CampaignGovernanceBinding {
    pub fn validate(&self) -> Result<(), CampaignGovernanceError> {
        if self.moot_id == [0; 32] {
            return Err(CampaignGovernanceError::MissingMoot);
        }
        self.campaign_policy
            .validate()
            .map_err(CampaignGovernanceError::Policy)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CampaignGovernanceError {
    #[error("campaign governance binding has no Moot id")]
    MissingMoot,
    #[error("campaign governance policy is invalid: {0}")]
    Policy(RecognitionPolicyError),
}

/// The durable result proposed for a competing set of governance bindings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GovernanceResolutionOutcome {
    /// Continue this campaign branch under exactly one candidate binding.
    Adopt { selected: [u8; 32] },
    /// Preserve every candidate as a separately addressable campaign branch.
    Branch {
        branches: BTreeMap<[u8; 32], [u8; 32]>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignGovernanceResolution {
    pub candidates: BTreeSet<[u8; 32]>,
    pub outcome: GovernanceResolutionOutcome,
}

impl CampaignGovernanceResolution {
    pub fn validate(&self) -> Result<(), CampaignGovernanceResolutionError> {
        if self.candidates.len() < 2 {
            return Err(CampaignGovernanceResolutionError::TooFewCandidates);
        }
        if self.candidates.contains(&[0; 32]) {
            return Err(CampaignGovernanceResolutionError::MissingCandidate);
        }
        match &self.outcome {
            GovernanceResolutionOutcome::Adopt { selected } => {
                if !self.candidates.contains(selected) {
                    return Err(CampaignGovernanceResolutionError::SelectedOutsideConflict);
                }
            }
            GovernanceResolutionOutcome::Branch { branches } => {
                if branches.keys().copied().collect::<BTreeSet<_>>() != self.candidates {
                    return Err(CampaignGovernanceResolutionError::IncompleteBranches);
                }
                let branch_ids = branches.values().copied().collect::<BTreeSet<_>>();
                if branch_ids.contains(&[0; 32]) {
                    return Err(CampaignGovernanceResolutionError::MissingBranch);
                }
                if branch_ids.len() != branches.len() {
                    return Err(CampaignGovernanceResolutionError::DuplicateBranch);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CampaignGovernanceResolutionError {
    #[error("a governance conflict needs at least two candidates")]
    TooFewCandidates,
    #[error("a governance resolution contains an empty candidate id")]
    MissingCandidate,
    #[error("the adopted binding is not one of the conflict candidates")]
    SelectedOutsideConflict,
    #[error("a branch resolution must assign every candidate exactly once")]
    IncompleteBranches,
    #[error("a branch resolution contains an empty branch id")]
    MissingBranch,
    #[error("a branch resolution assigns the same branch id more than once")]
    DuplicateBranch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalRecord {
    pub proposal: CampaignProposal,
    pub author: [u8; 32],
    pub parents: Vec<[u8; 32]>,
}

/// A deterministic projection. Concurrent records survive rather than being
/// collapsed by last-writer-wins rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CampaignSpaceView {
    pub proposals: BTreeMap<[u8; 32], ProposalRecord>,
    pub endorsements: BTreeMap<[u8; 32], BTreeSet<[u8; 32]>>,
    pub recognition_claims: BTreeMap<[u8; 32], BTreeSet<RecognitionClaim>>,
    pub governance_proposals: BTreeMap<[u8; 32], GovernanceProposalRecord>,
    pub governance_claims: BTreeMap<[u8; 32], BTreeSet<GovernanceClaim>>,
    pub governance_resolution_proposals: BTreeMap<[u8; 32], GovernanceResolutionProposalRecord>,
    pub governance_resolution_claims: BTreeMap<[u8; 32], BTreeSet<GovernanceResolutionClaim>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecognitionClaim {
    pub author: [u8; 32],
    pub resulting_head: [u8; 32],
    pub context_hash: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernanceProposalRecord {
    pub binding: CampaignGovernanceBinding,
    pub author: [u8; 32],
    pub parents: Vec<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GovernanceClaim {
    pub author: [u8; 32],
    pub context_hash: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernanceResolutionProposalRecord {
    pub resolution: CampaignGovernanceResolution,
    pub author: [u8; 32],
    pub parents: Vec<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GovernanceResolutionClaim {
    pub author: [u8; 32],
    pub context_hash: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignGovernanceStatus {
    pub proposal: GovernanceProposalRecord,
    pub decision: RecognitionDecision,
    pub context_hash: [u8; 32],
    pub matching_claims: BTreeSet<GovernanceClaim>,
    pub stale_context_claims: BTreeSet<GovernanceClaim>,
    pub is_bound: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignGovernanceResolutionStatus {
    pub proposal: GovernanceResolutionProposalRecord,
    pub decision: RecognitionDecision,
    pub context_hash: [u8; 32],
    pub matching_claims: BTreeSet<GovernanceResolutionClaim>,
    pub stale_context_claims: BTreeSet<GovernanceResolutionClaim>,
    pub is_resolved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignRecognitionStatus {
    pub decision: RecognitionDecision,
    pub context_hash: [u8; 32],
    pub matching_claims: BTreeSet<RecognitionClaim>,
    pub stale_context_claims: BTreeSet<RecognitionClaim>,
    /// Candidate heads are applicable only after policy acceptance. More than
    /// one is an explicit application conflict for the UI to resolve.
    pub applicable_heads: BTreeSet<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CampaignRecognitionError {
    #[error(transparent)]
    Policy(#[from] RecognitionPolicyError),
    #[error("recognition context belongs to another Moot")]
    WrongMoot,
    #[error("recognition context does not use the campaign's selected policy")]
    PolicyMismatch,
    #[error("governance resolution references an unknown binding: {0:?}")]
    UnknownGovernanceCandidate([u8; 32]),
    #[error("governance resolution candidate is not accepted under this context: {0:?}")]
    GovernanceCandidateNotBound([u8; 32]),
}

impl CampaignRecognitionStatus {
    pub fn has_head_conflict(&self) -> bool {
        self.applicable_heads.len() > 1
    }
}

