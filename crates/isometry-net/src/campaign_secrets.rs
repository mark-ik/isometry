//! Campaign secret audiences bound to Murm channel membership.
//!
//! Secret bodies stay out of the public campaign and tactical logs. This
//! module records which Murm cabal/channel audience may receive them, freezing
//! the signed Join/Leave projection by its membership revision.

use std::collections::BTreeSet;

use murm::CabalMembership;
use serde::{Deserialize, Serialize};

/// A frozen Murm audience for one campaign-private channel.
///
/// This record is private capability metadata. Do not publish it into the
/// public campaign space: the cabal id is its gossip routing topic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignSecretMembership {
    pub campaign_id: [u8; 32],
    pub cabal_id: [u8; 32],
    pub channel: String,
    /// Per-cabal author keys currently admitted to this secret channel.
    pub members: BTreeSet<[u8; 32]>,
    /// Murm's commitment to the latest signed membership state per author.
    pub membership_revision: [u8; 32],
}

impl CampaignSecretMembership {
    /// Freeze a live Murm projection for campaign use.
    pub fn capture(
        campaign_id: [u8; 32],
        cabal_id: [u8; 32],
        membership: &CabalMembership,
    ) -> Result<Self, CampaignSecretMembershipError> {
        let binding = Self {
            campaign_id,
            cabal_id,
            channel: membership.channel.clone(),
            members: membership.members.clone(),
            membership_revision: membership.revision,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), CampaignSecretMembershipError> {
        if self.campaign_id == [0; 32] {
            return Err(CampaignSecretMembershipError::MissingCampaign);
        }
        if self.cabal_id == [0; 32] {
            return Err(CampaignSecretMembershipError::MissingCabal);
        }
        if self.channel.is_empty() {
            return Err(CampaignSecretMembershipError::MissingChannel);
        }
        if self.members.is_empty() {
            return Err(CampaignSecretMembershipError::EmptyAudience);
        }
        if self.members.contains(&[0; 32]) {
            return Err(CampaignSecretMembershipError::MissingMember);
        }
        Ok(())
    }

    /// Check that a newly read Murm projection is exactly the frozen audience.
    pub fn verify_current(
        &self,
        membership: &CabalMembership,
    ) -> Result<(), CampaignSecretMembershipError> {
        self.validate()?;
        if membership.channel != self.channel
            || membership.revision != self.membership_revision
            || membership.members != self.members
        {
            return Err(CampaignSecretMembershipError::StaleAudience);
        }
        Ok(())
    }

    pub fn contains(&self, author: &[u8; 32]) -> bool {
        self.members.contains(author)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CampaignSecretMembershipError {
    #[error("campaign secret membership has no campaign id")]
    MissingCampaign,
    #[error("campaign secret membership has no Murm cabal id")]
    MissingCabal,
    #[error("campaign secret membership has no Murm channel")]
    MissingChannel,
    #[error("campaign secret membership has no members")]
    EmptyAudience,
    #[error("campaign secret membership contains an empty author key")]
    MissingMember,
    #[error("Murm secret membership has changed since this audience was frozen")]
    StaleAudience,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campaign_secret_audience_freezes_murm_membership_revision() {
        let membership = CabalMembership {
            channel: "campaign-secrets".to_owned(),
            members: BTreeSet::from([[1; 32], [2; 32]]),
            revision: [9; 32],
        };
        let binding =
            CampaignSecretMembership::capture([0xca; 32], [0xcb; 32], &membership).unwrap();
        assert!(binding.contains(&[1; 32]));
        binding.verify_current(&membership).unwrap();

        let changed = CabalMembership {
            members: BTreeSet::from([[2; 32]]),
            revision: [10; 32],
            ..membership
        };
        assert_eq!(
            binding.verify_current(&changed),
            Err(CampaignSecretMembershipError::StaleAudience)
        );
    }

    #[test]
    fn campaign_secret_audience_cannot_be_empty() {
        let membership = CabalMembership {
            channel: "campaign-secrets".to_owned(),
            members: BTreeSet::new(),
            revision: [9; 32],
        };
        assert_eq!(
            CampaignSecretMembership::capture([0xca; 32], [0xcb; 32], &membership),
            Err(CampaignSecretMembershipError::EmptyAudience)
        );
    }
}
