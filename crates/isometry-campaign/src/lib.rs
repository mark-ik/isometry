//! Isometry's campaign-state layer (worldbuilding plan W0).
//!
//! Two stores, by design: the **host-private campaign store** holds the
//! GM layer (secret facts, hidden modifiers, unrevealed history), and the
//! **shared log** carries only public projections. Session convergence is
//! a rolling hash over byte-identical event logs, so a secret must never
//! be serialized into a `GameSnapshot` or `GameEvent`; a reveal is an
//! ordinary event that *publishes* a fact. This crate is pure data:
//! no I/O, no net, no substrate geometry. The substrate stores and
//! displays these objects; system plugins interpret them.

mod fact;
mod store;

pub use fact::{RevealCondition, SecretFact, Visibility, WorldFact};
pub use store::CampaignStore;
