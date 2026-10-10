//! What a zone holds around the player: its spawns and what is known about
//! them, its doors and ground objects, and the corpse or merchant the player
//! is dealing with. All of it is the zone's, so leaving the zone forgets it
//! in one step.
use super::{Notice, Spawn, trade::Trade};
use crate::{
    PostureState, SpawnState, WorldPosition,
    combat::{ConColor, Consideration},
    doors::DoorTable,
    ground::Objects,
    outfit::WearChange,
};
use std::collections::BTreeMap;

/// The zone's contents, as the server described them.
#[derive(Default)]
pub(super) struct Zone {
    pub(super) spawns: BTreeMap<u16, Spawn>,
    /// Level colors the server reported for spawns the player considered.
    pub(super) considered: BTreeMap<u16, ConColor>,
    /// The player's own posture, which the zone's spawns do not carry.
    pub(super) player_posture: Option<PostureState>,
    pub(super) doors: DoorTable,
    pub(super) objects: Objects,
    /// The corpse and the merchant are the zone's.
    pub(super) trade: Trade,
    /// How the zone's sky and fog look, once the zone says.
    pub(super) sky: Option<crate::clock::ZoneSky>,
    /// The player's pet's buffs, as the server last said.
    pub(super) pet_buffs: Option<crate::pets::PetBuffs>,
    /// The guildmaster the player is training with, and what they teach.
    pub(super) training: Option<crate::training::TrainingOffer>,
    /// The world container the player asked to open, until the server
    /// answers.
    pub(super) asked_container: Option<u32>,
    /// The world container open for the player, such as a forge.
    pub(super) container: Option<crate::ground::ContainerView>,
    /// Each spawn's latest one-shot motion, the player's own included.
    pub(super) motions: BTreeMap<u16, super::Motion>,
    /// The cast each spawn is under, the player's own included.
    pub(super) casts: BTreeMap<u16, super::motions::Cast>,
}

impl Zone {
    /// Spawns appear, or replace what had their IDs; each is drawn anew.
    pub(super) fn appear(&mut self, spawns: &[SpawnState], revision: &mut u64) {
        for spawn in spawns {
            // A new spawn has not been considered, whatever had its ID.
            self.considered.remove(&spawn.spawn_id);
            *revision = revision.wrapping_add(1);
            self.spawns.insert(
                spawn.spawn_id,
                Spawn {
                    state: spawn.clone(),
                    health: spawn.hp_percent,
                    posture: None,
                    revision: *revision,
                },
            );
        }
    }

    /// A spawn leaves the zone, and its motion and cast with it.
    pub(super) fn vanish(&mut self, id: u16) {
        self.spawns.remove(&id);
        self.considered.remove(&id);
        self.motions.remove(&id);
        self.casts.remove(&id);
    }

    /// The server's word on a spawn the player considered.
    pub(super) fn consider(&mut self, consideration: &Consideration) -> Notice {
        self.considered
            .insert(consideration.target_id, consideration.color);
        Notice::Consideration {
            consideration: *consideration,
            name: self.name(consideration.target_id),
        }
    }

    /// A spawn shows or hides.
    pub(super) fn visibility(&mut self, id: u16, invisible: bool) {
        if let Some(spawn) = self.spawns.get_mut(&id) {
            spawn.state.invisible = invisible;
        }
    }

    /// A spawn, or the player, sits, stands or ducks.
    pub(super) fn posture(&mut self, id: u16, posture: PostureState, player: bool) {
        if player {
            self.player_posture = Some(posture);
        }
        if let Some(spawn) = self.spawns.get_mut(&id) {
            spawn.posture = Some(posture);
        }
    }

    /// A spawn's health, in percent.
    pub(super) fn health(&mut self, id: u16, percent: u8) {
        if let Some(spawn) = self.spawns.get_mut(&id) {
            spawn.health = Some(percent);
        }
    }

    /// A spawn moved.
    pub(super) fn moved(&mut self, id: u16, position: WorldPosition, velocity: [f32; 3]) {
        if let Some(spawn) = self.spawns.get_mut(&id) {
            spawn.state.position = position;
            spawn.state.velocity = velocity;
        }
    }

    /// A spawn put on or took off something visible.
    pub(super) fn wear(&mut self, change: &WearChange) {
        if let Some(spawn) = self.spawns.get_mut(&change.spawn_id) {
            spawn.state.appearance.apply(change);
        }
    }

    /// A spawn died: it becomes a corpse, drawn anew, under the name the
    /// death gives it (the session's, by the client generation's rule) or
    /// its living name where the death gives none. Titanium corpses keep the
    /// spawn ID.
    pub(super) fn corpse(&mut self, id: u16, name: Option<&str>, revision: &mut u64) {
        if let Some(spawn) = self.spawns.get_mut(&id) {
            *revision = revision.wrapping_add(1);
            spawn.health = Some(0);
            let kind = spawn.state.kind.corpse();
            if kind != spawn.state.kind
                && let Some(name) = name
            {
                spawn.state.name = name.into();
            }
            spawn.state.kind = kind;
            spawn.revision = *revision;
        }
    }

    /// A spawn's server name, when it is known.
    pub(super) fn name(&self, id: u16) -> Option<String> {
        self.spawns.get(&id).map(|spawn| spawn.state.name.clone())
    }
}

impl super::ClientWorld {
    /// Damage the player dealt or took; only that is theirs to read.
    pub(super) fn damage(&self, damage: &crate::combat::Damage, changes: &mut super::Changes) {
        match self.player.as_ref() {
            Some(player)
                if player.spawn_id == damage.source_id || player.spawn_id == damage.target_id =>
            {
                changes.notices.push(Notice::Damage {
                    damage: *damage,
                    own_id: player.spawn_id,
                    source: self.zone.name(damage.source_id),
                    target: self.zone.name(damage.target_id),
                });
            }
            _ => changes.ignored = true,
        }
    }

    /// The session told the server the player's target.
    pub(super) fn target_sent(&mut self, id: Option<u16>, changes: &mut super::Changes) {
        if self.target.selected == id {
            self.target.sent = true;
        } else {
            changes.ignored = true;
        }
    }

    /// The server refused the player's target.
    pub(super) fn target_refused(
        &mut self,
        (session_id, spawn_id): (u64, Option<u16>),
        reason: &str,
        changes: &mut super::Changes,
    ) {
        if self.accepts_reply(session_id) && self.target.selected == spawn_id {
            self.target = super::Target::default();
            changes
                .notices
                .push(Notice::TargetRefused(reason.to_owned()));
        } else {
            changes.ignored = true;
        }
    }

    /// A spawn's health; the player's own too, when it is theirs.
    pub(super) fn health_percent(&mut self, spawn_id: u16, percent: u8) {
        // The player's health follows their HP report once there is one:
        // `EQMac`'s percent for the player leaves out what items add.
        if self.is_player(spawn_id) && self.vitals.reported_hp.is_none() {
            self.own_health(percent);
        }
        self.zone.health(spawn_id, percent);
    }

    /// A spawn moved; when it is the player, the server put them here.
    pub(super) fn position(
        &mut self,
        spawn_id: u16,
        (position, velocity): (WorldPosition, [f32; 3]),
        changes: &mut super::Changes,
    ) {
        self.zone.moved(spawn_id, position, velocity);
        if let Some(player) = self
            .player
            .as_mut()
            .filter(|player| player.spawn_id == spawn_id)
        {
            player.position = position;
            changes.placed = Some(position);
        }
    }

    /// A spawn, or the player, put on or took off something visible.
    pub(super) fn wear(&mut self, change: &WearChange) {
        self.zone.wear(change);
        if let Some(player) = self
            .player
            .as_mut()
            .filter(|player| player.spawn_id == change.spawn_id)
        {
            player.appearance.apply(change);
        }
    }

    /// The server's word on a door the player used.
    pub(super) fn door_used(
        &self,
        (session_id, door_id): (u64, u8),
        error: Option<&String>,
        changes: &mut super::Changes,
    ) {
        if self.accepts_reply(session_id) {
            changes.notices.push(Notice::Door {
                door_id,
                error: error.cloned(),
            });
        } else {
            changes.ignored = true;
        }
    }

    /// The server's word on the zone's objects: a container the player asked
    /// for opens, or is in use by someone else; one opened unasked the
    /// session closes again, and it never shows.
    pub(super) fn objects_news(
        &mut self,
        update: &crate::ground::ObjectUpdate,
        changes: &mut super::Changes,
    ) {
        self.zone.objects.apply(update);
        let own = self.player.as_ref().map(|player| player.spawn_id);
        if let crate::ground::ObjectUpdate::Container(view) = update
            && own.is_some_and(|own| u32::from(own) == view.player_id)
            && self.zone.asked_container == Some(view.drop_id)
        {
            self.zone.asked_container = None;
            if view.open {
                self.zone.container = Some(view.clone());
            } else {
                changes.notices.push(Notice::ContainerInUse);
            }
        }
    }

    /// The server refused to hand over a ground object; one handed over
    /// shows by leaving the ground.
    pub(super) fn object_refused(
        &self,
        session_id: u64,
        error: Option<&String>,
        changes: &mut super::Changes,
    ) {
        match error {
            Some(error) if self.accepts_reply(session_id) => {
                changes.notices.push(Notice::GroundRefused(error.clone()));
            }
            _ => changes.ignored = true,
        }
    }
}
