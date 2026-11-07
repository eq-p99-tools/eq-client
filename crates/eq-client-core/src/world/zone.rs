//! What a zone holds around the player: its spawns and what is known about
//! them, its doors and ground objects, and the corpse or merchant the player
//! is dealing with. All of it is the zone's, so leaving the zone forgets it
//! in one step.
use super::{Notice, Spawn, trade::Trade};
use crate::{
    PostureState, SpawnKind, SpawnState, WorldPosition,
    combat::{ConColor, Consideration},
    control::Charm,
    doors::DoorTable,
    ground::Objects,
    outfit::{Features, WearChange},
};
use std::{collections::BTreeMap, time::Instant};

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
    /// The last weather report in this admission.
    pub(super) weather: Option<crate::clock::Weather>,
    /// The player's pet's buffs, as the server last said.
    pub(super) pet_buffs: super::pet_buffs::PetWindow,
    /// The session's identity for this admission's currently owned pet.
    pub(super) own_pet: Option<crate::pets::PetIdentity>,
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
    /// The latest cast each spawn completed, the player's own included.
    pub(super) completions: BTreeMap<u16, super::CastDone>,
    /// The latest spell that landed on each spawn, the player included.
    pub(super) landings: BTreeMap<u16, super::Landing>,
    /// When each corpse in view died, for one seen dying rather than
    /// arriving dead.
    pub(super) deaths: BTreeMap<u16, Instant>,
}

impl Zone {
    /// Spawns appear, or replace what had their IDs; each is drawn anew.
    pub(super) fn appear(&mut self, spawns: &[SpawnState], revision: &mut u64, now: Instant) {
        for spawn in spawns {
            // A new incarnation cannot inherit the previous actor's motions,
            // casts, landings or assessment; an arriving corpse was not seen
            // dying either. The same cleanup applies without a Despawn first.
            if self.spawns.contains_key(&spawn.spawn_id)
                || self
                    .own_pet
                    .is_some_and(|pet| pet.spawn_id == spawn.spawn_id)
            {
                self.forget_pet(spawn.spawn_id);
            }
            self.clear_spawn(spawn.spawn_id);
            *revision = revision.wrapping_add(1);
            self.spawns.insert(
                spawn.spawn_id,
                Spawn {
                    state: {
                        let mut state = spawn.clone();
                        still_if_corpse(&mut state);
                        state
                    },
                    health: spawn.hp_percent,
                    posture: None,
                    revision: *revision,
                    reported: now,
                },
            );
        }
    }

    /// A spawn leaves the zone, and its motion and casts with it.
    pub(super) fn vanish(&mut self, id: u16) {
        self.forget_pet(id);
        self.clear_spawn(id);
    }

    fn clear_spawn(&mut self, id: u16) {
        self.spawns.remove(&id);
        self.considered.remove(&id);
        self.motions.remove(&id);
        self.casts.remove(&id);
        self.completions.remove(&id);
        self.landings.remove(&id);
        self.deaths.remove(&id);
    }

    /// Apply an explicit owner report. Duplicate positive reports preserve
    /// the already received pet buffs.
    pub(super) fn pet_owner(&mut self, id: u16, owner: Option<u16>, player: Option<u16>) {
        let Some(spawn) = self.spawns.get_mut(&id) else {
            if owner.is_none() || owner != player {
                self.forget_pet(id);
            }
            return;
        };
        if owner.is_some() && spawn.state.pet_owner == owner {
            return;
        }
        let had_owner = spawn.state.pet_owner.is_some();
        spawn.state.pet_owner = owner;
        if had_owner || owner.is_none() || owner != player {
            self.forget_pet(id);
        }
    }

    pub(super) fn pet_charm(&mut self, charm: Charm, player: Option<u16>) {
        let previous = self
            .spawns
            .get(&charm.pet_id)
            .and_then(|spawn| spawn.state.pet_owner);
        let owner = charm.pet_owner(previous);
        if owner != previous {
            self.pet_owner(charm.pet_id, owner, player);
        } else if previous.is_none()
            && !charm.charmed
            && player.is_some()
            && charm.owner_id == player
        {
            // A named release can invalidate early data even if the owner
            // never resolved. Ownerless control returns do not name a pet.
            self.forget_pet(charm.pet_id);
        }
    }

    fn forget_pet(&mut self, id: u16) {
        if self.own_pet.is_some_and(|pet| pet.spawn_id == id) {
            self.own_pet = None;
        }
        self.pet_buffs.retire(id);
    }

    /// The worker's projection changes after the spawn or ownership record.
    pub(super) fn set_own_pet(&mut self, pet: Option<crate::pets::PetIdentity>) {
        if self.own_pet == pet {
            return;
        }
        if let Some(old) = self.own_pet {
            self.pet_buffs.retire(old.spawn_id);
        }
        if let Some(pet) = pet {
            self.pet_buffs.keep_only(pet.spawn_id);
        }
        self.own_pet = pet;
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

    /// A spawn moved, as of `now`.
    pub(super) fn moved(
        &mut self,
        id: u16,
        (position, velocity, forward): (WorldPosition, [f32; 3], f32),
        now: Instant,
    ) {
        if let Some(spawn) = self.spawns.get_mut(&id) {
            spawn.state.position = position;
            spawn.state.velocity = velocity;
            spawn.state.forward = forward;
            still_if_corpse(&mut spawn.state);
            spawn.reported = now;
        }
    }

    /// A spawn put on or took off something visible.
    pub(super) fn wear(&mut self, change: &WearChange) {
        if let Some(spawn) = self.spawns.get_mut(&change.spawn_id) {
            spawn.state.appearance.apply(change);
        }
    }

    /// A named spawn's face or hair changed.
    pub(super) fn features(&mut self, id: Option<u16>, features: Features) {
        if let Some(spawn) = id.and_then(|id| self.spawns.get_mut(&id)) {
            spawn.state.appearance.set_features(features);
        }
    }

    /// A spawn the session named took a new name.
    pub(super) fn rename(&mut self, id: Option<u16>, name: &str) {
        if let Some(spawn) = id.and_then(|id| self.spawns.get_mut(&id)) {
            name.clone_into(&mut spawn.state.name);
        }
    }

    /// A corpse lies somewhere new, facing as it did.
    pub(super) fn corpse_moved(&mut self, id: u16, [x, y, z]: [f32; 3]) {
        if let Some(spawn) = self.spawns.get_mut(&id) {
            let position = &mut spawn.state.position;
            (position.x, position.y, position.z) = (x, y, z);
        }
    }

    /// A spawn died: it becomes a corpse, drawn anew, under the name the
    /// death gives it (the session's, by the client generation's rule) or
    /// its living name where the death gives none. Titanium corpses keep the
    /// spawn ID. A living spawn's death is when it fell; a repeated death
    /// changes nothing of that. The death says nothing of where, so the
    /// corpse lies where the client had carried the spawn to when the death
    /// came, and moves no more on its own. Inferred; recording item 6.275.
    pub(super) fn corpse(
        &mut self,
        id: u16,
        name: Option<&str>,
        (dialect, now): (crate::GameDialect, Instant),
        revision: &mut u64,
    ) {
        self.forget_pet(id);
        if let Some(spawn) = self.spawns.get_mut(&id) {
            *revision = revision.wrapping_add(1);
            spawn.health = Some(0);
            let kind = spawn.state.kind.corpse();
            if kind != spawn.state.kind {
                self.deaths.insert(id, now);
                if let Some(name) = name {
                    spawn.state.name = name.into();
                }
                spawn.state.position = spawn.carried(dialect, now);
                spawn.reported = now;
            }
            spawn.state.kind = kind;
            spawn.state.pet_owner = None;
            still_if_corpse(&mut spawn.state);
            spawn.revision = *revision;
        }
    }

    /// A spawn's server name, when it is known.
    pub(super) fn name(&self, id: u16) -> Option<String> {
        self.spawns.get(&id).map(|spawn| spawn.state.name.clone())
    }
}

/// Stills a corpse, which never moves on its own: no velocity and no
/// forward input; a drag places it anew. Inferred; recording item 6.275.
fn still_if_corpse(state: &mut SpawnState) {
    if matches!(state.kind, SpawnKind::PlayerCorpse | SpawnKind::NpcCorpse) {
        state.velocity = [0.0; 3];
        state.forward = 0.0;
    }
}

impl super::ClientWorld {
    /// Damage the player dealt or took; only that is theirs to read.
    pub(super) fn damage(&self, damage: &crate::combat::Damage, changes: &mut super::Changes) {
        changes.damage.push(*damage);
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

    /// Session authority, scoped to this admission and ordered independently
    /// of a newer local choice still waiting in the command queue.
    pub(super) fn target_updated(
        &mut self,
        session_id: u64,
        request_id: u64,
        state: eq_network_game::targets::TargetState,
        server_selected: bool,
        changes: &mut super::Changes,
    ) {
        if !self.target.accepts_updates
            || self.session_id != Some(session_id)
            || self.is_dead()
            || self.pending_transfer.is_some()
            || request_id > self.target_sequence
            || request_id < self.target.reported_request
            || self.target.authority.is_some_and(|old| {
                state.revision < old.revision
                    || (state.revision == old.revision
                        && request_id == self.target.reported_request)
            })
        {
            changes.ignored = true;
            return;
        }
        let stale_local_choice = !server_selected
            && !self.target.sent
            && request_id == self.target.request_id
            && state.spawn_id == self.target.selected
            && self.target_stale();
        self.target.authority = Some(state);
        self.target.reported_request = request_id;
        if request_id < self.target.request_id {
            // Record the authority, but do not acknowledge a newer pending
            // local choice or give it this different target's action token.
            return;
        }
        self.target.request_id = request_id;
        if stale_local_choice {
            // A same-ID replacement while this choice waited cannot turn
            // its receipt into a selection of the replacement incarnation.
            // Keep the original local revision stale; input can clear it,
            // and no ability may acquire an action token from this receipt.
            self.target.sent = true;
            return;
        }
        self.project_target(state, server_selected);
    }

    fn project_target(
        &mut self,
        state: eq_network_game::targets::TargetState,
        server_selected: bool,
    ) {
        self.target.selected = state.spawn_id;
        self.target.sent = true;
        self.target.server_selected = server_selected;
        self.target.revision = state
            .available
            .then_some(state.spawn_id)
            .flatten()
            .and_then(|id| self.zone.spawns.get(&id))
            .map(|spawn| spawn.revision);
    }

    /// Local refusal restores the session's target instead of leaving a
    /// cleared display against a still-selected session recipient.
    pub(super) fn target_refused(
        &mut self,
        (session_id, request_id, spawn_id): (u64, u64, Option<u16>),
        state: Option<eq_network_game::targets::TargetState>,
        reason: &str,
        changes: &mut super::Changes,
    ) {
        if !self.accepts_reply(session_id)
            || self.is_dead()
            || self.pending_transfer.is_some()
            || self.target.request_id != request_id
            || self.target.selected != spawn_id
            || self.target.sent
        {
            changes.ignored = true;
            return;
        }
        if let Some(state) = state {
            self.target_updated(session_id, request_id, state, true, changes);
        } else if let Some(state) = self.target.authority {
            // A shared gate refused before the target owner processed this
            // choice. Do not invent an advanced session request watermark.
            self.target.request_id = self.target.reported_request;
            self.project_target(state, true);
        } else {
            self.target = super::Target {
                accepts_updates: self.target.accepts_updates,
                ..super::Target::default()
            };
        }
        if !changes.ignored {
            changes
                .notices
                .push(Notice::TargetRefused(reason.to_owned()));
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
        (position, velocity, forward): (WorldPosition, [f32; 3], f32),
        now: Instant,
        changes: &mut super::Changes,
    ) {
        self.zone
            .moved(spawn_id, (position, velocity, forward), now);
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
            self.held_models.clear_hand(change.slot);
        }
    }

    /// The server's word on a door the player used.
    pub(super) fn door_used(
        &self,
        (session_id, door_id): (u64, u8),
        (error, string_id): (Option<&String>, Option<u32>),
        changes: &mut super::Changes,
    ) {
        if self.accepts_reply(session_id) {
            changes.notices.push(Notice::Door {
                door_id,
                error: error.cloned(),
                string_id,
            });
        } else {
            changes.ignored = true;
        }
    }

    /// The server's word on the zone's objects: a container the player asked
    /// for opens, or is in use by someone else; one opened unasked the
    /// session closes again, and it never shows. One the session closed
    /// because the player went out of reach closes its window.
    pub(super) fn objects_news(
        &mut self,
        update: &crate::ground::ObjectUpdate,
        changes: &mut super::Changes,
    ) {
        self.zone.objects.apply(update);
        if let crate::ground::ObjectUpdate::Closed { drop_id } = update {
            if self
                .zone
                .container
                .as_ref()
                .is_some_and(|view| view.drop_id == *drop_id)
            {
                self.zone.container = None;
            }
            return;
        }
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
