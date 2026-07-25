//! Projecting the event log into a readable campaign-space view.
//!
//! Folding is pure, so every peer holding the same log renders the same view.
//!
//! Split out of `campaign_space.rs` on 2026-07-24; behavior unchanged.

use super::*;

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

    pub(crate) fn evaluate_governance(
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

    pub(crate) fn evaluate_governance_resolution(
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

