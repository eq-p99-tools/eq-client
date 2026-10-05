//! News about the session's course: the worlds and characters on offer,
//! admissions, the motion the session allows, transfers between zones, and
//! camping.
use super::{
    Camp, Changes, CharacterList, ClientWorld, MotionGrant, Moved, Notice, Reply, Reset,
    ServerList, Vitals,
};
use crate::{
    CampStatus, Capability, CharacterChoice, PlayerState, WorldPosition, ZoneOffer, ZoneRejection,
    servers::{ServerChoice, ServerRefusal},
};
use std::time::Instant;

impl ClientWorld {
    /// The login server listed these worlds for the player to choose from.
    pub(super) fn servers_offered(
        &mut self,
        selection_id: u64,
        servers: &[ServerChoice],
        changes: &mut Changes,
    ) {
        self.servers = Some(ServerList {
            selection_id,
            servers: servers.to_vec(),
            asked: None,
            refused: None,
        });
        changes.servers = true;
    }

    /// The login server refused the world chosen from its list, which stays
    /// up for another choice. A refusal for an older list changes nothing.
    pub(super) fn server_refused(
        &mut self,
        selection_id: u64,
        refusal: &ServerRefusal,
        changes: &mut Changes,
    ) {
        match self.servers.as_mut() {
            Some(list) if list.selection_id == selection_id => {
                list.asked = None;
                list.refused = Some(refusal.clone());
                changes.servers = true;
            }
            _ => changes.ignored = true,
        }
    }

    /// The world server offered these characters to play: the login
    /// server's list is done with.
    pub(super) fn offered(
        &mut self,
        selection_id: u64,
        characters: &[CharacterChoice],
        changes: &mut Changes,
    ) {
        self.servers = None;
        self.characters = Some(CharacterList {
            selection_id,
            characters: characters.to_vec(),
        });
        changes.characters = true;
    }

    /// A new admission replaces the old one and its zone.
    pub(super) fn entered(
        &mut self,
        (capabilities, choices, session_id): (&[Capability], &[Capability], u64),
        (zone, far_clip): (&str, Option<f32>),
        player: &PlayerState,
        now: Instant,
    ) -> Changes {
        let mut changes = self.reset(Reset::Entered);
        changes.entered = true;
        self.capabilities = capabilities.to_vec();
        self.choices = choices.to_vec();
        self.session_id = Some(session_id);
        zone.clone_into(&mut self.zone_name);
        self.far_clip = far_clip;
        self.vitals = Vitals {
            mana: Some(player.mana),
            endurance: player.endurance,
            ..Vitals::default()
        };
        self.casting.pending = None;
        self.casting
            .cooldowns
            .restore(&player.memorized_spells, player.spell_refresh_ms, now);
        self.player = Some(player.clone());
        changes
    }

    /// The session granted calibrated motion, or withdrew it, for this
    /// admission.
    pub(super) fn grant_motion(
        &mut self,
        session_id: u64,
        grant: MotionGrant,
        changes: &mut Changes,
    ) {
        if self.session_id == Some(session_id) {
            self.motion = Some(grant);
            changes.motion = true;
        } else {
            changes.ignored = true;
        }
    }

    /// The session sent the player's own move, or refused it.
    pub(super) fn motion_sent(
        &mut self,
        session_id: u64,
        position: WorldPosition,
        refused: Option<&String>,
        changes: &mut Changes,
    ) {
        match self.player.as_mut() {
            Some(player)
                if self.connected
                    && self.session_id == Some(session_id)
                    && self.death.is_none() =>
            {
                player.position = position;
                changes.moved = Some(Moved {
                    position,
                    refused: refused.cloned(),
                });
            }
            _ => changes.ignored = true,
        }
    }

    /// The server offered a transfer: the zone stays until the next
    /// admission.
    pub(super) fn transfer(&mut self, offer: &ZoneOffer) -> Changes {
        self.pending_transfer = Some(offer.clone());
        self.connected = false;
        self.reset(Reset::Zoning {
            to_bind: offer.to_bind,
        })
    }

    /// The server refused the transfer the player asked for.
    pub(super) fn transfer_refused(
        &mut self,
        session_id: u64,
        reason: ZoneRejection,
        changes: &mut Changes,
    ) {
        if self.session_id == Some(session_id) {
            self.pending_transfer = None;
            self.connected = self.death.is_none();
            changes.notices.push(Notice::TransferRefused(reason));
        } else {
            changes.ignored = true;
        }
    }

    /// A zone line could not be crossed.
    pub(super) fn zone_line_refused(&self, session_id: u64, reason: &str, changes: &mut Changes) {
        if self.session_id == Some(session_id) {
            changes
                .notices
                .push(Notice::ZoneLineRefused(reason.to_owned()));
            changes.replies.push(Reply::ZoneLineRefused);
        } else {
            changes.ignored = true;
        }
    }

    /// Camping began, went on, or stopped; once camped, the character is
    /// gone.
    pub(super) fn camp_news(&mut self, status: &CampStatus, now: Instant) -> Changes {
        if matches!(status, CampStatus::Camped) {
            return self.reset(Reset::Camped);
        }
        self.camp = match status {
            CampStatus::Preparing => Some(Camp {
                since: now,
                logging_out: false,
            }),
            CampStatus::LoggingOut => Some(Camp {
                since: self.camp.map_or(now, |camp| camp.since),
                logging_out: true,
            }),
            _ => None,
        };
        Changes {
            notices: vec![Notice::Camp(status.clone())],
            ..Changes::default()
        }
    }
}
