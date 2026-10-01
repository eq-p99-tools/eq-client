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
                    health: None,
                    posture: None,
                    revision: *revision,
                },
            );
        }
    }

    /// A spawn leaves the zone.
    pub(super) fn vanish(&mut self, id: u16) {
        self.spawns.remove(&id);
        self.considered.remove(&id);
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

    /// A spawn died: it becomes a corpse, drawn anew. Titanium corpses keep
    /// the spawn ID.
    pub(super) fn corpse(&mut self, id: u16, revision: &mut u64) {
        if let Some(spawn) = self.spawns.get_mut(&id) {
            *revision = revision.wrapping_add(1);
            spawn.health = Some(0);
            spawn.state.kind = spawn.state.kind.corpse();
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
        if self.is_player(spawn_id) {
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
