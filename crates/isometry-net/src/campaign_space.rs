//! Multi-writer campaign collaboration over signed p2panda author logs.
//!
//! Campaign collaboration and live tactical play have different ordering
//! needs. This module stores proposals, endorsements, and recognition as a
//! convergent set of signed records. The existing session sequencer remains the
//! authority for timing-sensitive combat events.

use std::collections::{BTreeMap, BTreeSet};

use isometry_campaign::CampaignProposal;
// Replication lives in `murm-replication` (Murm's peer-exchange foundation);
// `mooting` keeps only the governance policy. They were one crate until Murm's
// peer-runtime split, and mooting's compatibility re-export has since been
// removed, so the two must be imported from their real homes.
use mooting::{
    RecognitionContext, RecognitionDecision, RecognitionPolicy, RecognitionPolicyError,
};
use murm_replication::MunimentStore;
use muniment::{Backend, StoreError};
use p2panda_core::cbor::{decode_cbor, encode_cbor};
use p2panda_core::operation::validate_operation;
use p2panda_core::{Body, Hash, Header, Operation, SigningKey, Topic, VerifyingKey};
use p2panda_store::logs::LogStore;
use p2panda_store::topics::TopicStore;
use personae::Ed25519Keypair;
use serde::{Deserialize, Serialize};

mod space;
mod types;
mod view;

#[cfg(test)]
mod tests;

// The 2026-07-24 split moved the bodies into the modules above; this file keeps
// the shared imports they read through `use super::*` and re-publishes the same
// surface as before.
pub use space::*;
pub use types::*;
