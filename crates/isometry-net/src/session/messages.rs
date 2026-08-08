//! The host's wire half: peer messages, reveals, and whispers.
//!
//! Everything that arrives from a peer lands here and is checked before it
//! reaches game state. Ownership is verified per token, so a peer cannot move
//! what it does not command.
//!
//! Split out of `session.rs` on 2026-07-24; behavior unchanged.

use super::*;
use isometry_core::SessionEvent;

impl HostSession {
    pub fn reconcile_pending_reveals(&mut self) -> Result<Vec<Outbound>, String> {
        let pending: Vec<WorldFact> = self.campaign.pending_world_facts().collect();
        let mut out = Vec::new();
        for fact in pending {
            if let Some(existing) = self.state.journal.iter().find(|entry| entry.id == fact.id) {
                if existing != &fact {
                    return Err(format!("conflicting public campaign fact: {}", fact.id));
                }
            } else {
                out.extend(self.try_commit(GameEvent::Fact(fact.clone()))?);
            }
            self.campaign.finish_reveal(&fact.id);
        }
        let pending_modifiers: Vec<ItemModifierReveal> =
            self.campaign.pending_item_modifier_reveals().collect();
        for reveal in pending_modifiers {
            let item = self
                .state
                .inventories
                .values()
                .find_map(|inventory| inventory.items.get(&reveal.item))
                .ok_or_else(|| {
                    format!("missing public item for modifier reveal: {}", reveal.item.0)
                })?;
            if let Some(existing) = item
                .modifiers
                .iter()
                .find(|modifier| modifier.id == reveal.modifier.id)
            {
                if existing != &reveal.modifier {
                    return Err(format!(
                        "conflicting public item modifier: {}",
                        reveal.modifier.id
                    ));
                }
            } else {
                out.extend(self.try_commit(GameEvent::ItemModifierRevealed(reveal.clone()))?);
            }
            self.campaign.finish_item_modifier_reveal(&reveal.id);
        }
        Ok(out)
    }

    /// A message arrived from `from`: `Hello` announces the peer's protocol
    /// version and player name, `Intent` proposes an event, `Action` asks for a
    /// verdict; anything else is ignored (a misbehaving client cannot corrupt
    /// the authority).
    ///
    /// The version is the first thing checked, and a peer that fails it is
    /// refused for the session rather than partially served.
    pub fn on_message(&mut self, from: PeerId, msg: NetMessage) -> Vec<Outbound> {
        // A peer whose dialect this host cannot read is answered with the same
        // receipt for the rest of the session, and nothing it says is read. The
        // refusal is checked before the message is even matched, so there is no
        // arm through which a stranger's bytes can reach game state.
        if let Some(&offered) = self.refused.get(&from) {
            return vec![(Recipient::One(from), Self::version_receipt(offered))];
        }
        match msg {
            // The handshake. A version this host cannot speak is refused here,
            // before the peer is ever recorded as a player, so it owns no token
            // and can commit nothing.
            NetMessage::Hello { version, name } => {
                if version != PROTOCOL_VERSION {
                    self.refused.insert(from, version);
                    return vec![(Recipient::One(from), Self::version_receipt(version))];
                }
                self.peer_names.insert(from, name);
                Vec::new()
            }
            // The peer refused *us*: our version is one it cannot speak. Drop
            // it the same way, recording the dialect it does speak.
            NetMessage::VersionRefused { supported, .. } => {
                self.refused.insert(from, supported);
                self.peer_names.remove(&from);
                Vec::new()
            }
            // A player asking to act. Two things are checkable without any rules
            // at all, so they are checked here: the actor exists, and it is
            // yours. Everything else -- reach, turn, whether it hits, what it
            // costs -- is the rules system's, so the request is queued for the
            // host app to adjudicate and commit.
            NetMessage::Action(mut intent) => {
                if !self.peer_owns(from, intent.actor) {
                    return vec![(
                        Recipient::One(from),
                        NetMessage::Rejected {
                            reason: "you can only act with your own tokens".to_owned(),
                        },
                    )];
                }
                // Who asked is the authority's to say, not the asker's: the ask
                // is attributed to the connection it arrived on, so a peer
                // cannot number its request as somebody else's. The nonce stays
                // the asker's, so it can match this answer to its question.
                intent.request = RequestId {
                    peer: from,
                    nonce: intent.request.nonce,
                };
                self.pending_actions.push(intent);
                Vec::new()
            }
            NetMessage::Intent { event } => {
                if let Some(reason) = self.intent_refusal(from, &event) {
                    return vec![(Recipient::One(from), NetMessage::Rejected { reason })];
                }
                match self.try_commit(event) {
                    Ok(out) => out,
                    Err(reason) => vec![(Recipient::One(from), NetMessage::Rejected { reason })],
                }
            }
            _ => Vec::new(),
        }
    }

    /// The legible half of a refusal: what arrived, and what this host speaks.
    fn version_receipt(offered: u16) -> NetMessage {
        NetMessage::VersionRefused {
            offered,
            supported: PROTOCOL_VERSION,
        }
    }

    /// Why `from` may not commit `event`, or `None` if it may.
    ///
    /// The one place a client's authority is decided, and **exhaustive on
    /// purpose**: a new `GameEvent` variant fails to compile here until someone
    /// says who may send it. It used to fall through to a permissive catch-all,
    /// which is how `Map` and `SheetSet` came to be committable by any peer --
    /// the forged-verdict arms were written one at a time as each verdict was
    /// invented, and the events nobody thought about inherited "yes".
    ///
    /// Three rules, all checkable without running a single line of rules script,
    /// which is what keeps this crate rules-blind:
    ///
    /// - **A verdict is the host's.** Whether you hit, what `prone` costs you,
    ///   where the road took you. A peer pronouncing its own is forgery.
    /// - **Authoring is the DM's.** The world, the maps, the terrain, the
    ///   initiative order, and the sheets every rule reads its numbers from.
    /// - **A declaration about your own token is yours.** Moving, facing,
    ///   emoting, taking a stance. Ownership is the whole check; legality is not
    ///   (reach and turn order are the rules', and the host does not re-derive
    ///   them here).
    fn intent_refusal(&self, from: PeerId, event: &GameEvent) -> Option<String> {
        match event {
            // Verdicts. A peer cannot choose whether it hit and for how much,
            // what a condition means, or how its own journey went.
            GameEvent::ActionResolved(_) => Some("actions are adjudicated by the host".to_owned()),
            GameEvent::TravelResolved { .. } => {
                Some("travel is adjudicated by the host".to_owned())
            }
            GameEvent::ConditionSet { .. } => Some("conditions are ruled by the host".to_owned()),
            // Ruled by the host's own door sweep after each applied move, so a
            // client walks through a door by walking; it never asks in words.
            GameEvent::Traveled { .. } => Some("travel is ruled by the host".to_owned()),
            GameEvent::TimeAdvanced { .. } => Some("the DM keeps the clock".to_owned()),

            // Campaign authoring: what is true, what exists, and what it is worth.
            GameEvent::Fact(_)
            | GameEvent::InventorySet { .. }
            | GameEvent::ItemTransfer { .. }
            | GameEvent::ItemModifierRevealed(_)
            | GameEvent::Generation(_)
            | GameEvent::MapStored(_)
            | GameEvent::MapActivated { .. }
            | GameEvent::World(_) => Some("campaign authoring is committed by the DM".to_owned()),

            // A sheet holds every number the rules read: hit points, AC, the
            // ability scores. Replacing one outright is strictly stronger than
            // the forged verdict already refused above -- there is no need to
            // claim a 999-damage hit if you can simply set the boss to 0 hp. The
            // DM edits sheets (a joined player's UI never offers it, per
            // `can_edit_inventory`); damage reaches a sheet as an adjudicated
            // `ActionResolved` delta instead.
            GameEvent::SheetSet { .. } => {
                Some("sheets are edited by the DM; damage arrives as a resolution".to_owned())
            }

            // Initiative is the table's business, not a player's: reordering it
            // (or adding and dropping combatants) decides who acts and when.
            GameEvent::TurnAdd(_) | GameEvent::TurnRemove(_) | GameEvent::TurnSetOrder(_) => {
                Some("the turn order is set by the DM".to_owned())
            }

            // "I am done." Legitimate for whoever is up -- and only for them, or
            // a player could end someone else's turn, or skip an enemy's by
            // spamming it. Which token is active is replicated state, so this
            // asks nothing of the rules.
            GameEvent::TurnAdvance => match self.state.turns.active() {
                Some(active) if self.peer_owns(from, active) => None,
                _ => Some("you can only end your own turn".to_owned()),
            },

            // The substrate document. Moving and facing are declarations about a
            // token, so they need ownership and nothing more. Painting terrain,
            // spawning, and removing are authoring: a spawned token carries its
            // own `owner` field, so an unchecked `TokenPlaced` hands out
            // ownership of anything, and `TokenRemoved` deletes the boss.
            GameEvent::Map(map_event) => match map_event {
                SessionEvent::TokenMoved { id, .. } | SessionEvent::TokenFaced { id, .. } => {
                    (!self.peer_owns(from, *id))
                        .then(|| "you can only move your own tokens".to_owned())
                }
                SessionEvent::TilePlaced { .. }
                | SessionEvent::ElevationSet { .. }
                | SessionEvent::TokenPlaced(_)
                | SessionEvent::TokenRemoved { .. } => {
                    Some("the board is edited by the DM".to_owned())
                }
            },

            // A declaration about your own token, with no verdict to forge and
            // no state to change: the worst a liar can do is wave.
            GameEvent::Emoted { token, .. } => (!self.peer_owns(from, *token))
                .then(|| "you can only emote your own tokens".to_owned()),
            GameEvent::StanceSet { token, .. } => (!self.peer_owns(from, *token))
                .then(|| "you can only set the stance of your own tokens".to_owned()),

            // A die in the shared log. Accepted from anyone, deliberately: this
            // is the friendly-table trust model, where a forged total is no
            // worse than lying about a physical die, and the table can see it.
            // Attribution is *not* verified, because `by` legitimately carries a
            // character or side name ("Knight", "side A") rather than the
            // peer's, so it cannot be compared against the sender without a
            // schema change. Worth revisiting with the tier-3 commit-reveal
            // randomness the shared-authority plan describes.
            GameEvent::Rolled(_) => None,
        }
    }

    /// Whether the peer's announced player name may command `token`: either it
    /// owns the token directly, or the token is owned by a faction whose channel
    /// the player has been granted. A DM-controlled token (`owner: None`)
    /// belongs to nobody, so no client owns it.
    pub(crate) fn peer_owns(&self, peer: PeerId, token: TokenId) -> bool {
        let Some(name) = self.peer_names.get(&peer).map(String::as_str) else {
            return false;
        };
        let Some(owner) = self.state.map.token(token).and_then(|t| t.owner.as_deref()) else {
            return false;
        };
        // A faction is an owner name like any other; playing it means holding its
        // channel, so the grant extends command to the faction's tokens.
        owner == name || self.state.world.faction_controller(owner) == Some(name)
    }

    /// The DM whispers to the player named `to`. Returns a directed
    /// message to that peer (empty if nobody by that name is connected).
    pub fn whisper(&self, from: &str, to: &str, text: &str) -> Vec<Outbound> {
        self.peer_names
            .iter()
            .find(|(_, name)| name.as_str() == to)
            .map(|(&peer, _)| {
                vec![(
                    Recipient::One(peer),
                    NetMessage::Whisper {
                        from: from.to_owned(),
                        text: text.to_owned(),
                    },
                )]
            })
            .unwrap_or_default()
    }

    /// The player names currently connected (whisper targets).
    pub fn peer_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.peer_names.values().cloned().collect();
        names.sort();
        names.dedup();
        names
    }

    pub(crate) fn commit(&mut self, event: GameEvent) -> Vec<Outbound> {
        self.try_commit(event).unwrap_or_default()
    }

    pub(crate) fn try_commit(&mut self, event: GameEvent) -> Result<Vec<Outbound>, String> {
        apply_game(&mut self.state, &event).map_err(|e| format!("{e:?}"))?;
        let appended = self.history.append(event.clone());
        self.seq = appended.0 + 1;
        self.log_hash = fold_event(self.log_hash, self.seq, &event);
        Ok(vec![(
            Recipient::All,
            NetMessage::Applied {
                seq: self.seq,
                event,
            },
        )])
    }
}
