//! The host's wire half: peer messages, reveals, and whispers.
//!
//! Everything that arrives from a peer lands here and is checked before it
//! reaches game state. Ownership is verified per token, so a peer cannot move
//! what it does not command.
//!
//! Split out of `session.rs` on 2026-07-24; behavior unchanged.

use super::*;

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

    /// A message arrived from `from`: `Intent` proposes an event,
    /// `Hello` announces the player's name; anything else is ignored (a
    /// misbehaving client cannot corrupt the authority).
    pub fn on_message(&mut self, from: PeerId, msg: NetMessage) -> Vec<Outbound> {
        match msg {
            // Campaign reveals are DM-committed only (`local_event`); a
            // client cannot make a hidden record public by proposing it.
            // A resolution is a *verdict*, and a peer cannot pronounce its own.
            // Accepting this as an intent would let a client choose whether it
            // hit and how much damage it dealt. The rules run on the sequencer;
            // a client asks, it does not decide. (The ask itself, an action
            // intent a client can send, is the next step: it needs a message the
            // host app can drain and resolve with its system plugin, since this
            // crate is deliberately rules-blind.)
            NetMessage::Intent {
                event: GameEvent::ActionResolved(_),
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "actions are adjudicated by the host".to_owned(),
                },
            )],
            // Travel is a verdict too: a client cannot pronounce where its party
            // arrived, how long it took, or whether it got lost.
            NetMessage::Intent {
                event: GameEvent::TravelResolved { .. },
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "travel is adjudicated by the host".to_owned(),
                },
            )],
            // The DM keeps the clock: a player does not declare hours passing.
            NetMessage::Intent {
                event: GameEvent::TimeAdvanced { .. },
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "the DM keeps the clock".to_owned(),
                },
            )],
            // Travel is ruled by the host's own sweep (it watches for tokens
            // standing on doors after every applied move), so a client walks
            // through a door by walking; it never asks in words.
            NetMessage::Intent {
                event: GameEvent::Traveled { .. },
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "travel is ruled by the host".to_owned(),
                },
            )],
            // A condition is a rules ruling with numbers attached; a client
            // proposing one would be pronouncing what `prone` means. Standing up
            // travels as an action intent instead, so the host's rules answer.
            NetMessage::Intent {
                event: GameEvent::ConditionSet { .. },
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "conditions are ruled by the host".to_owned(),
                },
            )],
            NetMessage::Intent {
                event:
                    GameEvent::Fact(_)
                    | GameEvent::InventorySet { .. }
                    | GameEvent::ItemTransfer { .. }
                    | GameEvent::ItemModifierRevealed(_)
                    | GameEvent::Generation(_)
                    | GameEvent::MapStored(_)
                    | GameEvent::MapActivated { .. }
                    | GameEvent::World(_),
            } => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "campaign authoring is committed by the DM".to_owned(),
                },
            )],
            // An emote needs no adjudication (there is no verdict to forge), but
            // it does need ownership: waving is harmless, and puppeteering the
            // DM's monsters is not. A player emotes their own tokens.
            NetMessage::Intent {
                event: GameEvent::Emoted { token, .. },
            } if !self.peer_owns(from, token) => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "you can only emote your own tokens".to_owned(),
                },
            )],
            // A stance is a declaration, not a verdict, so a player sets it on its
            // own tokens (and only its own), exactly like an emote.
            NetMessage::Intent {
                event: GameEvent::StanceSet { token, .. },
            } if !self.peer_owns(from, token) => vec![(
                Recipient::One(from),
                NetMessage::Rejected {
                    reason: "you can only set the stance of your own tokens".to_owned(),
                },
            )],
            // A player asking to act. Two things are checkable without any rules
            // at all, so they are checked here: the actor exists, and it is
            // yours. Everything else -- reach, turn, whether it hits, what it
            // costs -- is the rules system's, so the request is queued for the
            // host app to adjudicate and commit.
            NetMessage::Action(intent) => {
                if !self.peer_owns(from, intent.actor) {
                    return vec![(
                        Recipient::One(from),
                        NetMessage::Rejected {
                            reason: "you can only act with your own tokens".to_owned(),
                        },
                    )];
                }
                self.pending_actions.push(intent);
                Vec::new()
            }
            NetMessage::Intent { event } => match self.try_commit(event) {
                Ok(out) => out,
                Err(reason) => vec![(Recipient::One(from), NetMessage::Rejected { reason })],
            },
            NetMessage::Hello { name } => {
                self.peer_names.insert(from, name);
                Vec::new()
            }
            _ => Vec::new(),
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
