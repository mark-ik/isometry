//! Multi-writer campaign collaboration over signed p2panda author logs.
//!
//! Campaign collaboration and live tactical play have different ordering
//! needs. This module stores proposals, endorsements, and recognition as a
//! convergent set of signed records. The existing session sequencer remains the
//! authority for timing-sensitive combat events.

use std::collections::{BTreeMap, BTreeSet};

use isometry_campaign::CampaignProposal;
use mooting::{
    MunimentStore, RecognitionContext, RecognitionDecision, RecognitionPolicy,
    RecognitionPolicyError,
};
use muniment::{Backend, StoreError};
use p2panda_core::cbor::{decode_cbor, encode_cbor};
use p2panda_core::operation::validate_operation;
use p2panda_core::{Body, Hash, Header, Operation, SigningKey, Timestamp, Topic, VerifyingKey};
use p2panda_store::logs::LogStore;
use p2panda_store::topics::TopicStore;
use personae::Ed25519Keypair;
use serde::{Deserialize, Serialize};

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
    fn at_ms(&self) -> u64 {
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

impl CampaignSpaceView {
    /// Evaluate one proposal against a policy bound to a frozen Moot
    /// electorate. Returns `None` when the proposal is unknown.
    pub fn recognition_status(
        &self,
        proposal: [u8; 32],
        governance: &CampaignGovernanceBinding,
        context: &RecognitionContext,
    ) -> Result<Option<CampaignRecognitionStatus>, CampaignRecognitionError> {
        if !self.proposals.contains_key(&proposal) {
            return Ok(None);
        }
        if context.electorate.group_id != governance.moot_id {
            return Err(CampaignRecognitionError::WrongMoot);
        }
        if context.policy != governance.campaign_policy {
            return Err(CampaignRecognitionError::PolicyMismatch);
        }
        let endorsements = self
            .endorsements
            .get(&proposal)
            .cloned()
            .unwrap_or_default();
        let decision = context.evaluate(&endorsements)?;
        let context_hash = context.fingerprint()?;
        let mut matching_claims = BTreeSet::new();
        let mut stale_context_claims = BTreeSet::new();
        for claim in self.recognition_claims.get(&proposal).into_iter().flatten() {
            if claim.context_hash == context_hash {
                matching_claims.insert(claim.clone());
            } else {
                stale_context_claims.insert(claim.clone());
            }
        }
        let applicable_heads = if decision.accepted {
            matching_claims
                .iter()
                .map(|claim| claim.resulting_head)
                .collect()
        } else {
            BTreeSet::new()
        };
        Ok(Some(CampaignRecognitionStatus {
            decision,
            context_hash,
            matching_claims,
            stale_context_claims,
            applicable_heads,
        }))
    }

    /// Evaluate an initial campaign association under the target Moot's own
    /// admission context.
    pub fn governance_admission_status(
        &self,
        proposal: [u8; 32],
        context: &RecognitionContext,
    ) -> Result<Option<CampaignGovernanceStatus>, CampaignRecognitionError> {
        let Some(record) = self.governance_proposals.get(&proposal) else {
            return Ok(None);
        };
        if context.electorate.group_id != record.binding.moot_id {
            return Err(CampaignRecognitionError::WrongMoot);
        }
        self.evaluate_governance(proposal, context)
    }

    /// Evaluate a policy change or Moot migration under the campaign's current
    /// binding. The candidate destination cannot authorize its own takeover.
    /// Competing accepted changes remain separate statuses for explicit UI.
    pub fn governance_change_status(
        &self,
        proposal: [u8; 32],
        current: &CampaignGovernanceBinding,
        context: &RecognitionContext,
    ) -> Result<Option<CampaignGovernanceStatus>, CampaignRecognitionError> {
        if context.electorate.group_id != current.moot_id {
            return Err(CampaignRecognitionError::WrongMoot);
        }
        if context.policy != current.campaign_policy {
            return Err(CampaignRecognitionError::PolicyMismatch);
        }
        self.evaluate_governance(proposal, context)
    }

    /// Evaluate an initial same-Moot binding conflict under that Moot's
    /// admission context. Initial bindings for unrelated Moots have no shared
    /// electorate and cannot authorize one another through this path.
    pub fn governance_resolution_admission_status(
        &self,
        proposal: [u8; 32],
        context: &RecognitionContext,
    ) -> Result<Option<CampaignGovernanceResolutionStatus>, CampaignRecognitionError> {
        let Some(record) = self.governance_resolution_proposals.get(&proposal) else {
            return Ok(None);
        };
        for candidate in &record.resolution.candidates {
            let binding = self.governance_proposals.get(candidate).ok_or(
                CampaignRecognitionError::UnknownGovernanceCandidate(*candidate),
            )?;
            if binding.binding.moot_id != context.electorate.group_id {
                return Err(CampaignRecognitionError::WrongMoot);
            }
        }
        self.evaluate_governance_resolution(proposal, context)
    }

    /// Evaluate a conflict among proposed policy changes or Moot migrations
    /// under the campaign's current binding.
    pub fn governance_resolution_change_status(
        &self,
        proposal: [u8; 32],
        current: &CampaignGovernanceBinding,
        context: &RecognitionContext,
    ) -> Result<Option<CampaignGovernanceResolutionStatus>, CampaignRecognitionError> {
        if context.electorate.group_id != current.moot_id {
            return Err(CampaignRecognitionError::WrongMoot);
        }
        if context.policy != current.campaign_policy {
            return Err(CampaignRecognitionError::PolicyMismatch);
        }
        self.evaluate_governance_resolution(proposal, context)
    }

    fn evaluate_governance(
        &self,
        proposal: [u8; 32],
        context: &RecognitionContext,
    ) -> Result<Option<CampaignGovernanceStatus>, CampaignRecognitionError> {
        let Some(record) = self.governance_proposals.get(&proposal) else {
            return Ok(None);
        };
        let endorsements = self
            .endorsements
            .get(&proposal)
            .cloned()
            .unwrap_or_default();
        let decision = context.evaluate(&endorsements)?;
        let context_hash = context.fingerprint()?;
        let mut matching_claims = BTreeSet::new();
        let mut stale_context_claims = BTreeSet::new();
        for claim in self.governance_claims.get(&proposal).into_iter().flatten() {
            if claim.context_hash == context_hash {
                matching_claims.insert(claim.clone());
            } else {
                stale_context_claims.insert(claim.clone());
            }
        }
        let is_bound = decision.accepted && !matching_claims.is_empty();
        Ok(Some(CampaignGovernanceStatus {
            proposal: record.clone(),
            decision,
            context_hash,
            matching_claims,
            stale_context_claims,
            is_bound,
        }))
    }

    fn evaluate_governance_resolution(
        &self,
        proposal: [u8; 32],
        context: &RecognitionContext,
    ) -> Result<Option<CampaignGovernanceResolutionStatus>, CampaignRecognitionError> {
        let Some(record) = self.governance_resolution_proposals.get(&proposal) else {
            return Ok(None);
        };
        for candidate in &record.resolution.candidates {
            let status = self.evaluate_governance(*candidate, context)?.ok_or(
                CampaignRecognitionError::UnknownGovernanceCandidate(*candidate),
            )?;
            if !status.is_bound {
                return Err(CampaignRecognitionError::GovernanceCandidateNotBound(
                    *candidate,
                ));
            }
        }
        let endorsements = self
            .endorsements
            .get(&proposal)
            .cloned()
            .unwrap_or_default();
        let decision = context.evaluate(&endorsements)?;
        let context_hash = context.fingerprint()?;
        let mut matching_claims = BTreeSet::new();
        let mut stale_context_claims = BTreeSet::new();
        for claim in self
            .governance_resolution_claims
            .get(&proposal)
            .into_iter()
            .flatten()
        {
            if claim.context_hash == context_hash {
                matching_claims.insert(claim.clone());
            } else {
                stale_context_claims.insert(claim.clone());
            }
        }
        let is_resolved = decision.accepted && !matching_claims.is_empty();
        Ok(Some(CampaignGovernanceResolutionStatus {
            proposal: record.clone(),
            decision,
            context_hash,
            matching_claims,
            stale_context_claims,
            is_resolved,
        }))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CampaignSpaceError {
    #[error("campaign operation is invalid: {0}")]
    InvalidOperation(String),
    #[error("campaign operation addresses another campaign or branch")]
    WrongSpace,
    #[error("campaign proposal is invalid: {0:?}")]
    InvalidProposal(isometry_campaign::CampaignProposalError),
    #[error("campaign governance proposal is invalid: {0}")]
    InvalidGovernance(CampaignGovernanceError),
    #[error("campaign governance resolution is invalid: {0}")]
    InvalidGovernanceResolution(CampaignGovernanceResolutionError),
    #[error("campaign operation has no body")]
    MissingBody,
    #[error("campaign operation body is malformed")]
    MalformedBody,
    #[error("campaign store: {0}")]
    Store(#[from] StoreError),
}

/// Sign one collaboration event at an author's per-branch log position.
pub fn to_operation(
    keypair: &Ed25519Keypair,
    campaign_id: [u8; 32],
    branch_id: [u8; 32],
    event: &CampaignCollaborationEvent,
    seq_num: u64,
    backlink: Option<[u8; 32]>,
    parents: Vec<[u8; 32]>,
) -> Operation<CampaignExt> {
    let signing_key = SigningKey::from_bytes(&keypair.to_seed());
    let body_bytes = encode_cbor(event).expect("campaign events always CBOR-encode");
    let body = Body::new(&body_bytes);
    let mut header = Header {
        version: 1,
        verifying_key: signing_key.verifying_key(),
        signature: None,
        payload_size: body.size(),
        payload_hash: Some(body.hash()),
        timestamp: Timestamp::from(event.at_ms()),
        seq_num,
        backlink: backlink.map(Hash::from),
        extensions: CampaignExt {
            campaign_id,
            branch_id,
            parents,
        },
    };
    header.sign(&signing_key);
    Operation {
        hash: header.hash(),
        header,
        body: Some(body),
    }
}

fn decode_event(
    operation: &Operation<CampaignExt>,
) -> Result<CampaignCollaborationEvent, CampaignSpaceError> {
    let body = operation
        .body
        .as_ref()
        .ok_or(CampaignSpaceError::MissingBody)?;
    decode_cbor(body.to_bytes().as_slice()).map_err(|_| CampaignSpaceError::MalformedBody)
}

/// Backend-neutral campaign store suitable for p2panda LogSync.
#[derive(Clone)]
pub struct CampaignSpace<B> {
    store: MunimentStore<B, CampaignExt>,
    campaign_id: [u8; 32],
    branch_id: [u8; 32],
}

impl<B> CampaignSpace<B>
where
    B: Backend,
{
    pub fn new(backend: B, campaign_id: [u8; 32], branch_id: [u8; 32]) -> Self {
        Self {
            store: MunimentStore::new(backend),
            campaign_id,
            branch_id,
        }
    }

    pub fn campaign_id(&self) -> [u8; 32] {
        self.campaign_id
    }

    pub fn branch_id(&self) -> [u8; 32] {
        self.branch_id
    }

    /// Clone the p2panda-compatible store handle for host-composed LogSync.
    pub fn sync_store(&self) -> MunimentStore<B, CampaignExt>
    where
        B: Clone,
    {
        self.store.clone()
    }

    pub async fn insert(
        &self,
        operation: &Operation<CampaignExt>,
    ) -> Result<bool, CampaignSpaceError> {
        validate_operation(operation)
            .map_err(|error| CampaignSpaceError::InvalidOperation(error.to_string()))?;
        if operation.header.extensions.campaign_id != self.campaign_id
            || operation.header.extensions.branch_id != self.branch_id
        {
            return Err(CampaignSpaceError::WrongSpace);
        }
        match decode_event(operation)? {
            CampaignCollaborationEvent::Proposed { proposal, .. } => proposal
                .validate()
                .map_err(CampaignSpaceError::InvalidProposal)?,
            CampaignCollaborationEvent::GovernanceProposed { binding, .. } => binding
                .validate()
                .map_err(CampaignSpaceError::InvalidGovernance)?,
            CampaignCollaborationEvent::GovernanceResolutionProposed { resolution, .. } => {
                resolution
                    .validate()
                    .map_err(CampaignSpaceError::InvalidGovernanceResolution)?
            }
            _ => {}
        }

        let fresh = self
            .store
            .insert_indexed_operation(&Topic::from(self.campaign_id), operation, &self.branch_id)
            .await?;
        Ok(fresh)
    }

    pub async fn latest(
        &self,
        author: &VerifyingKey,
    ) -> Result<Option<Operation<CampaignExt>>, CampaignSpaceError> {
        Ok(self.store.get_latest_entry(author, &self.branch_id).await?)
    }

    /// Sign and persist one event. A host publishes the returned operation;
    /// LogSync handles peers that were offline.
    pub async fn author(
        &self,
        keypair: &Ed25519Keypair,
        event: &CampaignCollaborationEvent,
        parents: Vec<[u8; 32]>,
    ) -> Result<Operation<CampaignExt>, CampaignSpaceError> {
        let author = SigningKey::from_bytes(&keypair.to_seed()).verifying_key();
        let (seq_num, backlink) = match self.latest(&author).await? {
            Some(previous) => (previous.header.seq_num + 1, Some(*previous.hash.as_bytes())),
            None => (0, None),
        };
        let operation = to_operation(
            keypair,
            self.campaign_id,
            self.branch_id,
            event,
            seq_num,
            backlink,
            parents,
        );
        self.insert(&operation).await?;
        Ok(operation)
    }

    pub async fn operations(&self) -> Result<Vec<Operation<CampaignExt>>, CampaignSpaceError> {
        let logs: BTreeMap<VerifyingKey, Vec<[u8; 32]>> =
            self.store.resolve(&Topic::from(self.campaign_id)).await?;
        let mut operations = Vec::new();
        for (author, branches) in logs {
            if !branches.contains(&self.branch_id) {
                continue;
            }
            if let Some(entries) = self
                .store
                .get_log_entries(&author, &self.branch_id, None, None)
                .await?
            {
                operations.extend(entries.into_iter().map(|(operation, _)| operation));
            }
        }
        Ok(operations)
    }

    pub async fn materialize(&self) -> Result<CampaignSpaceView, CampaignSpaceError> {
        let mut view = CampaignSpaceView::default();
        for operation in self.operations().await? {
            let operation_id = *operation.hash.as_bytes();
            let author = *operation.header.verifying_key.as_bytes();
            match decode_event(&operation)? {
                CampaignCollaborationEvent::Proposed { proposal, .. } => {
                    view.proposals.insert(
                        operation_id,
                        ProposalRecord {
                            proposal,
                            author,
                            parents: operation.header.extensions.parents,
                        },
                    );
                }
                CampaignCollaborationEvent::Endorsed { subject, .. } => {
                    view.endorsements.entry(subject).or_default().insert(author);
                }
                CampaignCollaborationEvent::GovernanceProposed { binding, .. } => {
                    view.governance_proposals.insert(
                        operation_id,
                        GovernanceProposalRecord {
                            binding,
                            author,
                            parents: operation.header.extensions.parents,
                        },
                    );
                }
                CampaignCollaborationEvent::GovernanceClaimed {
                    proposal,
                    context_hash,
                    ..
                } => {
                    view.governance_claims
                        .entry(proposal)
                        .or_default()
                        .insert(GovernanceClaim {
                            author,
                            context_hash,
                        });
                }
                CampaignCollaborationEvent::GovernanceResolutionProposed { resolution, .. } => {
                    view.governance_resolution_proposals.insert(
                        operation_id,
                        GovernanceResolutionProposalRecord {
                            resolution,
                            author,
                            parents: operation.header.extensions.parents,
                        },
                    );
                }
                CampaignCollaborationEvent::GovernanceResolutionClaimed {
                    proposal,
                    context_hash,
                    ..
                } => {
                    view.governance_resolution_claims
                        .entry(proposal)
                        .or_default()
                        .insert(GovernanceResolutionClaim {
                            author,
                            context_hash,
                        });
                }
                CampaignCollaborationEvent::RecognitionClaimed {
                    proposal,
                    resulting_head,
                    context_hash,
                    ..
                } => {
                    view.recognition_claims
                        .entry(proposal)
                        .or_default()
                        .insert(RecognitionClaim {
                            author,
                            resulting_head,
                            context_hash,
                        });
                }
            }
        }
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isometry_campaign::{CampaignProposal, CampaignProposalMode};
    use mooting::{ElectorateSnapshot, RecognitionPolicy};
    use muniment::MemoryBackend;

    const CAMPAIGN: [u8; 32] = [0xca; 32];
    const BRANCH: [u8; 32] = [0xba; 32];
    const MOOT: [u8; 32] = [0x6d; 32];

    fn proposal(id: &str) -> CampaignProposal {
        CampaignProposal {
            id: id.into(),
            title: format!("Proposal {id}"),
            mode: CampaignProposalMode::Apply { base: [1; 32] },
            content_hash: [2; 32],
        }
    }

    #[test]
    fn concurrent_authors_converge_independent_of_arrival_order() {
        pollster::block_on(async {
            let alice = Ed25519Keypair::from_seed([10; 32]);
            let bob = Ed25519Keypair::from_seed([11; 32]);
            let alice_op = to_operation(
                &alice,
                CAMPAIGN,
                BRANCH,
                &CampaignCollaborationEvent::Proposed {
                    proposal: proposal("alice"),
                    at_ms: 10,
                },
                0,
                None,
                vec![[3; 32]],
            );
            let bob_op = to_operation(
                &bob,
                CAMPAIGN,
                BRANCH,
                &CampaignCollaborationEvent::Proposed {
                    proposal: proposal("bob"),
                    at_ms: 11,
                },
                0,
                None,
                vec![[3; 32]],
            );

            let first = CampaignSpace::new(MemoryBackend::new(), CAMPAIGN, BRANCH);
            first.insert(&alice_op).await.unwrap();
            first.insert(&bob_op).await.unwrap();
            let second = CampaignSpace::new(MemoryBackend::new(), CAMPAIGN, BRANCH);
            second.insert(&bob_op).await.unwrap();
            second.insert(&alice_op).await.unwrap();

            assert_eq!(
                first.materialize().await.unwrap(),
                second.materialize().await.unwrap()
            );
            assert_eq!(first.materialize().await.unwrap().proposals.len(), 2);
        });
    }

    #[test]
    fn moot_policy_filters_outsiders_and_stale_recognition_claims() {
        pollster::block_on(async {
            let space = CampaignSpace::new(MemoryBackend::new(), CAMPAIGN, BRANCH);
            let alice = Ed25519Keypair::from_seed([20; 32]);
            let bob = Ed25519Keypair::from_seed([21; 32]);
            let outsider = Ed25519Keypair::from_seed([22; 32]);
            let governance = CampaignGovernanceBinding {
                moot_id: MOOT,
                campaign_policy: RecognitionPolicy::Threshold { required: 2 },
            };
            let context = RecognitionContext::new(
                governance.campaign_policy.clone(),
                ElectorateSnapshot::new(
                    MOOT,
                    [7; 32],
                    [alice.public_key().to_bytes(), bob.public_key().to_bytes()],
                ),
            );
            let context_hash = context.fingerprint().unwrap();
            let proposed = space
                .author(
                    &alice,
                    &CampaignCollaborationEvent::Proposed {
                        proposal: proposal("shared"),
                        at_ms: 1,
                    },
                    vec![],
                )
                .await
                .unwrap();
            let proposal_id = *proposed.hash.as_bytes();
            space
                .author(
                    &alice,
                    &CampaignCollaborationEvent::Endorsed {
                        subject: proposal_id,
                        at_ms: 2,
                    },
                    vec![proposal_id],
                )
                .await
                .unwrap();
            space
                .author(
                    &outsider,
                    &CampaignCollaborationEvent::Endorsed {
                        subject: proposal_id,
                        at_ms: 3,
                    },
                    vec![proposal_id],
                )
                .await
                .unwrap();
            space
                .author(
                    &alice,
                    &CampaignCollaborationEvent::RecognitionClaimed {
                        proposal: proposal_id,
                        resulting_head: [9; 32],
                        context_hash,
                        at_ms: 4,
                    },
                    vec![proposal_id],
                )
                .await
                .unwrap();

            let pending = space
                .materialize()
                .await
                .unwrap()
                .recognition_status(proposal_id, &governance, &context)
                .unwrap()
                .unwrap();
            assert!(!pending.decision.accepted);
            assert_eq!(pending.decision.ineligible_endorsements.len(), 1);
            assert!(pending.applicable_heads.is_empty());

            space
                .author(
                    &bob,
                    &CampaignCollaborationEvent::Endorsed {
                        subject: proposal_id,
                        at_ms: 3,
                    },
                    vec![proposal_id],
                )
                .await
                .unwrap();
            let stale_context = RecognitionContext::new(
                governance.campaign_policy.clone(),
                ElectorateSnapshot::new(
                    MOOT,
                    [8; 32],
                    [alice.public_key().to_bytes(), bob.public_key().to_bytes()],
                ),
            );
            space
                .author(
                    &bob,
                    &CampaignCollaborationEvent::RecognitionClaimed {
                        proposal: proposal_id,
                        resulting_head: [10; 32],
                        context_hash: stale_context.fingerprint().unwrap(),
                        at_ms: 5,
                    },
                    vec![proposal_id],
                )
                .await
                .unwrap();

            let view = space.materialize().await.unwrap();
            assert_eq!(view.endorsements[&proposal_id].len(), 3);
            let status = view
                .recognition_status(proposal_id, &governance, &context)
                .unwrap()
                .unwrap();
            assert!(status.decision.accepted);
            assert_eq!(status.decision.eligible_endorsements.len(), 2);
            assert_eq!(status.decision.ineligible_endorsements.len(), 1);
            assert_eq!(status.applicable_heads, BTreeSet::from([[9; 32]]));
            assert_eq!(status.stale_context_claims.len(), 1);
            assert!(!status.has_head_conflict());

            let wrong_moot = RecognitionContext::new(
                governance.campaign_policy.clone(),
                ElectorateSnapshot::new([0xee; 32], [7; 32], []),
            );
            assert!(matches!(
                view.recognition_status(proposal_id, &governance, &wrong_moot),
                Err(CampaignRecognitionError::WrongMoot)
            ));
            let wrong_policy = RecognitionContext::new(
                RecognitionPolicy::AnyEligible,
                ElectorateSnapshot::new(MOOT, [7; 32], []),
            );
            assert!(matches!(
                view.recognition_status(proposal_id, &governance, &wrong_policy),
                Err(CampaignRecognitionError::PolicyMismatch)
            ));
        });
    }

    #[test]
    fn signed_governance_binding_rejects_cross_moot_contexts_and_keeps_competitors() {
        pollster::block_on(async {
            let space = CampaignSpace::new(MemoryBackend::new(), CAMPAIGN, BRANCH);
            let alice = Ed25519Keypair::from_seed([40; 32]);
            let bob = Ed25519Keypair::from_seed([41; 32]);
            let electorate = [alice.public_key().to_bytes(), bob.public_key().to_bytes()];
            let admission = RecognitionContext::new(
                RecognitionPolicy::Unanimous,
                ElectorateSnapshot::new(MOOT, [11; 32], electorate),
            );
            let context_hash = admission.fingerprint().unwrap();

            let first = space
                .author(
                    &alice,
                    &CampaignCollaborationEvent::GovernanceProposed {
                        binding: CampaignGovernanceBinding {
                            moot_id: MOOT,
                            campaign_policy: RecognitionPolicy::Threshold { required: 2 },
                        },
                        at_ms: 1,
                    },
                    vec![],
                )
                .await
                .unwrap();
            let second = space
                .author(
                    &bob,
                    &CampaignCollaborationEvent::GovernanceProposed {
                        binding: CampaignGovernanceBinding {
                            moot_id: MOOT,
                            campaign_policy: RecognitionPolicy::Unanimous,
                        },
                        at_ms: 2,
                    },
                    vec![],
                )
                .await
                .unwrap();

            for proposal in [*first.hash.as_bytes(), *second.hash.as_bytes()] {
                for (author, at_ms) in [(&alice, 3), (&bob, 4)] {
                    space
                        .author(
                            author,
                            &CampaignCollaborationEvent::Endorsed {
                                subject: proposal,
                                at_ms,
                            },
                            vec![proposal],
                        )
                        .await
                        .unwrap();
                }
                space
                    .author(
                        &alice,
                        &CampaignCollaborationEvent::GovernanceClaimed {
                            proposal,
                            context_hash,
                            at_ms: 5,
                        },
                        vec![proposal],
                    )
                    .await
                    .unwrap();
            }

            let view = space.materialize().await.unwrap();
            let first_status = view
                .governance_admission_status(*first.hash.as_bytes(), &admission)
                .unwrap()
                .unwrap();
            let second_status = view
                .governance_admission_status(*second.hash.as_bytes(), &admission)
                .unwrap()
                .unwrap();
            assert!(first_status.is_bound);
            assert!(second_status.is_bound);
            assert_ne!(
                first_status.proposal.binding.campaign_policy,
                second_status.proposal.binding.campaign_policy
            );

            let foreign_context = RecognitionContext::new(
                RecognitionPolicy::Unanimous,
                ElectorateSnapshot::new([0xee; 32], [11; 32], electorate),
            );
            assert!(matches!(
                view.governance_admission_status(*first.hash.as_bytes(), &foreign_context),
                Err(CampaignRecognitionError::WrongMoot)
            ));

            let current = first_status.proposal.binding.clone();
            let current_context = RecognitionContext::new(
                current.campaign_policy.clone(),
                ElectorateSnapshot::new(MOOT, [12; 32], electorate),
            );
            space
                .author(
                    &bob,
                    &CampaignCollaborationEvent::GovernanceClaimed {
                        proposal: *second.hash.as_bytes(),
                        context_hash: current_context.fingerprint().unwrap(),
                        at_ms: 6,
                    },
                    vec![*second.hash.as_bytes()],
                )
                .await
                .unwrap();
            let changed_view = space.materialize().await.unwrap();
            assert!(
                changed_view
                    .governance_change_status(*second.hash.as_bytes(), &current, &current_context,)
                    .unwrap()
                    .unwrap()
                    .is_bound
            );
            assert!(matches!(
                changed_view.governance_change_status(
                    *second.hash.as_bytes(),
                    &current,
                    &foreign_context,
                ),
                Err(CampaignRecognitionError::WrongMoot)
            ));

            let candidates = BTreeSet::from([*first.hash.as_bytes(), *second.hash.as_bytes()]);
            let resolution = space
                .author(
                    &alice,
                    &CampaignCollaborationEvent::GovernanceResolutionProposed {
                        resolution: CampaignGovernanceResolution {
                            candidates: candidates.clone(),
                            outcome: GovernanceResolutionOutcome::Adopt {
                                selected: *first.hash.as_bytes(),
                            },
                        },
                        at_ms: 7,
                    },
                    candidates.iter().copied().collect(),
                )
                .await
                .unwrap();
            let resolution_id = *resolution.hash.as_bytes();
            for (author, at_ms) in [(&alice, 8), (&bob, 9)] {
                space
                    .author(
                        author,
                        &CampaignCollaborationEvent::Endorsed {
                            subject: resolution_id,
                            at_ms,
                        },
                        vec![resolution_id],
                    )
                    .await
                    .unwrap();
            }
            let accepted_without_claim = space
                .materialize()
                .await
                .unwrap()
                .governance_resolution_admission_status(resolution_id, &admission)
                .unwrap()
                .unwrap();
            assert!(accepted_without_claim.decision.accepted);
            assert!(!accepted_without_claim.is_resolved);

            space
                .author(
                    &bob,
                    &CampaignCollaborationEvent::GovernanceResolutionClaimed {
                        proposal: resolution_id,
                        context_hash,
                        at_ms: 10,
                    },
                    vec![resolution_id],
                )
                .await
                .unwrap();
            let resolved = space
                .materialize()
                .await
                .unwrap()
                .governance_resolution_admission_status(resolution_id, &admission)
                .unwrap()
                .unwrap();
            assert!(resolved.is_resolved);
            assert_eq!(resolved.proposal.resolution.candidates, candidates);
        });
    }

    #[test]
    fn branch_resolution_requires_one_unique_nonzero_branch_per_candidate() {
        let candidates = BTreeSet::from([[1; 32], [2; 32]]);
        let incomplete = CampaignGovernanceResolution {
            candidates: candidates.clone(),
            outcome: GovernanceResolutionOutcome::Branch {
                branches: BTreeMap::from([([1; 32], [10; 32])]),
            },
        };
        assert_eq!(
            incomplete.validate(),
            Err(CampaignGovernanceResolutionError::IncompleteBranches)
        );

        let duplicate = CampaignGovernanceResolution {
            candidates,
            outcome: GovernanceResolutionOutcome::Branch {
                branches: BTreeMap::from([([1; 32], [10; 32]), ([2; 32], [10; 32])]),
            },
        };
        assert_eq!(
            duplicate.validate(),
            Err(CampaignGovernanceResolutionError::DuplicateBranch)
        );
    }

    #[test]
    fn tampered_or_cross_campaign_operations_are_rejected() {
        pollster::block_on(async {
            let key = Ed25519Keypair::from_seed([30; 32]);
            let mut tampered = to_operation(
                &key,
                CAMPAIGN,
                BRANCH,
                &CampaignCollaborationEvent::Proposed {
                    proposal: proposal("one"),
                    at_ms: 1,
                },
                0,
                None,
                vec![],
            );
            tampered.header.extensions.campaign_id = [0xff; 32];
            let space = CampaignSpace::new(MemoryBackend::new(), CAMPAIGN, BRANCH);
            assert!(matches!(
                space.insert(&tampered).await,
                Err(CampaignSpaceError::InvalidOperation(_))
            ));

            let other = to_operation(
                &key,
                [0xdd; 32],
                BRANCH,
                &CampaignCollaborationEvent::Proposed {
                    proposal: proposal("two"),
                    at_ms: 2,
                },
                0,
                None,
                vec![],
            );
            assert!(matches!(
                space.insert(&other).await,
                Err(CampaignSpaceError::WrongSpace)
            ));

            let invalid_governance = to_operation(
                &key,
                CAMPAIGN,
                BRANCH,
                &CampaignCollaborationEvent::GovernanceProposed {
                    binding: CampaignGovernanceBinding {
                        moot_id: [0; 32],
                        campaign_policy: RecognitionPolicy::Threshold { required: 0 },
                    },
                    at_ms: 3,
                },
                0,
                None,
                vec![],
            );
            assert!(matches!(
                space.insert(&invalid_governance).await,
                Err(CampaignSpaceError::InvalidGovernance(
                    CampaignGovernanceError::MissingMoot
                ))
            ));
        });
    }
}
