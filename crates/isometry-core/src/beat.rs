//! **Beats**: the representation half of a resolved action.
//!
//! A beat is a named, timed visual state on a token: `strike`, `recoil`,
//! `dodge`, `cheer`. The substrate stores the *name* and nothing else. What a
//! beat looks like is a stylesheet's business (a tileset supplies the
//! `@keyframes`, exactly as it supplies tile appearance), and how long it runs
//! is the engine's animation clock. The core never interpolates anything.
//!
//! Beats ride on a resolved action but are not *part* of its truth: a peer that
//! drops every frame still converges on the same log hash, because the beat is a
//! consequence of the event rather than a member of its state. That is the same
//! friendly-table split fog already uses (the host sends state; each viewer
//! renders what it can see).
//!
//! Combat and emotes are the same primitive. An emote is simply a beat with no
//! resolved action behind it.

use serde::{Deserialize, Serialize};

use crate::map::TokenId;

/// One token playing one named beat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    pub token: TokenId,
    /// Beat name in the pack's vocabulary (`strike`, `recoil`, ...). The view
    /// lowers it to a CSS class; the pack decides what that class looks like.
    pub name: String,
}

impl Beat {
    pub fn new(token: TokenId, name: impl Into<String>) -> Self {
        Self {
            token,
            name: name.into(),
        }
    }
}
