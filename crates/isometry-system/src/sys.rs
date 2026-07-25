//! The system-plugin internals, split out of `lib.rs` on 2026-07-24.
//!
//! `lib.rs` keeps the public vocabulary (the schema and resolution types) and
//! these carry the machinery: loading a system, projecting sheets, adjudicating
//! actions, the sandboxed generator runtime, and the two directions of the Lua
//! bridge. Behavior is unchanged by the split.

use super::*;

mod generator;
mod lua_read;
mod lua_write;
mod srd;
mod system_actions;
mod system_core;

pub use srd::*;

// The split modules call across each other (the generator runtime uses both
// directions of the Lua bridge), so each sibling's items are visible here.
pub(crate) use lua_read::*;
pub(crate) use lua_write::*;
