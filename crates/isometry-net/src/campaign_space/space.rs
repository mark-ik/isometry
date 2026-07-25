//! The campaign space itself: the store-backed collaboration surface.
//!
//! Split out of `campaign_space.rs` on 2026-07-24; behavior unchanged.

use super::*;

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
    seq_num: u32,
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
        // p2panda 0.7 dropped Header.timestamp. Isometry orders a branch by
        // seq_num + backlink + parents (a DAG), never the header clock, and the
        // event body still carries at_ms(), so nothing here is lost. Fresh-data
        // prototype: no stored operations to keep hash-stable across the bump.
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

pub(crate) fn decode_event(
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

