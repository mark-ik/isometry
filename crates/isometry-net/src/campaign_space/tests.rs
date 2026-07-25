//! Tests for this module, split out on 2026-07-24; unchanged.

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
