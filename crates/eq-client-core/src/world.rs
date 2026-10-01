//! The client's picture of the game: what the server has said about the
//! connection, the admission, the player and the zone around them.
//!
//! Updates from the session are its only writer, applied by
//! [`ClientWorld::apply`], and each reset has one reason, which decides what
//! is forgotten. Nothing here draws, so any front end can host the world and
//! its rules are tested without one.

use crate::{
    CampStatus, CharacterChoice, Death, PlayerState, PostureState, SpawnState, WorldEvent,
    WorldPosition, WorldUpdate, ZoneOffer, doors::DoorTable, ground::Objects,
};
use std::{collections::BTreeMap, time::Instant};

/// The characters the world server offered to play.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterList {
    /// Names this offer, which a choice must repeat.
    pub selection_id: u64,
    /// The characters, in their server slots.
    pub characters: Vec<CharacterChoice>,
}

/// One spawn in the zone and what is known about it.
#[derive(Clone, Debug)]
pub struct Spawn {
    /// The spawn as the server last described it.
    pub state: SpawnState,
    /// Its health in percent, once reported.
    pub health: Option<u8>,
    /// Its posture, once reported.
    pub posture: Option<PostureState>,
    /// Changes whenever the spawn must be drawn again: it appeared, was
    /// replaced or became a corpse.
    pub revision: u64,
}

/// Why the world was reset, which decides what it forgot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reset {
    /// A new admission replaced the old one and its zone.
    Entered,
    /// The server offered a transfer; the zone stays until the next admission.
    Zoning {
        /// Whether the transfer returns the player to their bind point.
        to_bind: bool,
    },
    /// The player died; the zone stays, with their corpse in it.
    Died,
    /// The player camped to the character list.
    Camped,
    /// The connection dropped.
    Lost {
        /// Whether the session is over and will not reconnect.
        ended: bool,
        /// Whether a transfer was under way when it dropped.
        transferring: bool,
    },
}

/// What an update changed that a front end must redo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    /// The world was reset, and why.
    pub reset: Option<Reset>,
    /// A new admission began: present its zone and player.
    pub entered: bool,
    /// The server put the player here.
    pub placed: Option<WorldPosition>,
    /// The world server offered characters to play.
    pub characters: bool,
    /// The update was for an earlier admission, or came when the world takes
    /// no such news, so nothing changed.
    pub ignored: bool,
}

/// What the server has told the client, kept by one writer.
#[derive(Default)]
pub struct ClientWorld {
    connected: bool,
    ended: bool,
    world_name: Option<String>,
    characters: Option<CharacterList>,
    session_id: Option<u64>,
    zone: String,
    far_clip: Option<f32>,
    death: Option<Death>,
    pending_transfer: Option<ZoneOffer>,
    player: Option<PlayerState>,
    /// The player's own posture, which the zone's spawns do not carry.
    player_posture: Option<PostureState>,
    spawns: BTreeMap<u16, Spawn>,
    doors: DoorTable,
    objects: Objects,
    /// The last revision given to a spawn.
    revision: u64,
}

impl ClientWorld {
    /// Applies one update from the session and says what a front end must redo.
    pub fn apply(&mut self, update: &WorldUpdate, now: Instant) -> Changes {
        match update {
            WorldUpdate::Connection {
                connected,
                terminal,
                ..
            } => self.connection(*connected, *terminal),
            WorldUpdate::Game(event) => self.event(event, now),
            WorldUpdate::Chat(_) | WorldUpdate::ServerMessage { .. } => Changes::default(),
        }
    }

    /// Closes the doors whose time is up, as the server's close action would;
    /// true when any closed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let due = self.doors.closes_due(now);
        if due {
            self.doors.close_due(now);
        }
        due
    }

    /// Notes the player's health as the HUD computes it from the last report.
    // Until the player's vitals move into the world, the HUD works it out.
    pub fn note_own_health(&mut self, percent: u8) {
        if let Some(player) = self.player.as_mut() {
            player.hp_percent = Some(percent);
        }
    }

    fn connection(&mut self, connected: bool, ended: bool) -> Changes {
        let transferring = self.pending_transfer.is_some();
        self.connected = connected;
        self.ended = ended;
        if connected {
            return Changes::default();
        }
        self.reset(Reset::Lost {
            ended,
            transferring,
        })
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one arm per kind of news, each a line or two"
    )]
    fn event(&mut self, event: &WorldEvent, now: Instant) -> Changes {
        let mut changes = Changes::default();
        match event {
            WorldEvent::WorldName { short_name } => self.world_name = Some(short_name.clone()),
            WorldEvent::CharacterSelection {
                selection_id,
                characters,
            } => {
                self.characters = Some(CharacterList {
                    selection_id: *selection_id,
                    characters: characters.clone(),
                });
                changes.characters = true;
            }
            WorldEvent::Entered {
                session_id,
                zone,
                player,
                far_clip,
            } => {
                changes = self.reset(Reset::Entered);
                changes.entered = true;
                self.session_id = Some(*session_id);
                self.zone.clone_from(zone);
                self.far_clip = *far_clip;
                self.player = Some((**player).clone());
            }
            WorldEvent::MotionSent {
                session_id,
                position,
                ..
            } => match self.player.as_mut() {
                Some(player)
                    if self.connected
                        && self.session_id == Some(*session_id)
                        && self.death.is_none() =>
                {
                    player.position = *position;
                }
                _ => changes.ignored = true,
            },
            WorldEvent::Death(death) => return self.died(death),
            WorldEvent::ZoneTransfer(offer) => {
                self.pending_transfer = Some(offer.clone());
                self.connected = false;
                return self.reset(Reset::Zoning {
                    to_bind: offer.to_bind,
                });
            }
            WorldEvent::ZoneTransferRejected { session_id, .. } => {
                if self.session_id == Some(*session_id) {
                    self.pending_transfer = None;
                    self.connected = self.death.is_none();
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::Spawns(spawns) => {
                for spawn in spawns {
                    self.revision = self.revision.wrapping_add(1);
                    self.spawns.insert(
                        spawn.spawn_id,
                        Spawn {
                            state: spawn.clone(),
                            health: None,
                            posture: None,
                            revision: self.revision,
                        },
                    );
                }
            }
            WorldEvent::Despawn(id) => {
                self.spawns.remove(id);
            }
            WorldEvent::Visibility {
                spawn_id,
                invisible,
            } => {
                if let Some(spawn) = self.spawns.get_mut(spawn_id) {
                    spawn.state.invisible = *invisible;
                }
            }
            WorldEvent::Posture { spawn_id, posture } => {
                if self.is_player(*spawn_id) {
                    self.player_posture = Some(*posture);
                }
                if let Some(spawn) = self.spawns.get_mut(spawn_id) {
                    spawn.posture = Some(*posture);
                }
            }
            WorldEvent::HealthPercent { spawn_id, percent } => {
                if self.is_player(*spawn_id) {
                    self.note_own_health(*percent);
                }
                if let Some(spawn) = self.spawns.get_mut(spawn_id) {
                    spawn.health = Some(*percent);
                }
            }
            WorldEvent::Position {
                spawn_id,
                position,
                velocity,
            } => {
                if let Some(spawn) = self.spawns.get_mut(spawn_id) {
                    spawn.state.position = *position;
                    spawn.state.velocity = *velocity;
                }
                if let Some(player) = self
                    .player
                    .as_mut()
                    .filter(|player| player.spawn_id == *spawn_id)
                {
                    player.position = *position;
                    changes.placed = Some(*position);
                }
            }
            WorldEvent::WearChange(change) => {
                if let Some(spawn) = self.spawns.get_mut(&change.spawn_id) {
                    spawn.state.appearance.apply(change);
                }
                if let Some(player) = self
                    .player
                    .as_mut()
                    .filter(|player| player.spawn_id == change.spawn_id)
                {
                    player.appearance.apply(change);
                }
            }
            WorldEvent::Doors(update) => self.doors.apply(update, now),
            WorldEvent::Objects(update) => self.objects.apply(update),
            WorldEvent::Level { current, .. } => match self.player.as_mut() {
                Some(player) if self.connected => player.level = *current,
                _ => changes.ignored = true,
            },
            WorldEvent::Skill { skill_id, value } => match self.player.as_mut() {
                Some(player) if self.connected => player.apply_skill(*skill_id, *value),
                _ => changes.ignored = true,
            },
            WorldEvent::Spell(update) => {
                if let Some(player) = self.player.as_mut() {
                    update.apply_gems(&mut player.memorized_spells);
                }
            }
            WorldEvent::Camp(CampStatus::Camped) => return self.reset(Reset::Camped),
            _ => (),
        }
        changes
    }

    /// A death: a spawn becomes a corpse, and the player's own death holds
    /// them until the server offers to return them home.
    fn died(&mut self, death: &Death) -> Changes {
        if let Ok(id) = u16::try_from(death.spawn_id)
            && let Some(spawn) = self.spawns.get_mut(&id)
        {
            self.revision = self.revision.wrapping_add(1);
            spawn.health = Some(0);
            // Titanium corpses keep the spawn ID.
            spawn.state.kind = spawn.state.kind.corpse();
            spawn.revision = self.revision;
        }
        if self
            .player
            .as_ref()
            .is_none_or(|player| u32::from(player.spawn_id) != death.spawn_id)
        {
            return Changes::default();
        }
        self.note_own_health(0);
        self.death = Some(death.clone());
        self.reset(Reset::Died)
    }

    /// Forgets what the reason makes stale, and says so.
    fn reset(&mut self, reason: Reset) -> Changes {
        match reason {
            Reset::Entered => self.forget_admission(),
            Reset::Camped => {
                self.forget_admission();
                self.session_id = None;
                self.player = None;
                self.connected = false;
            }
            Reset::Lost { ended: true, .. } => {
                self.characters = None;
                self.pending_transfer = None;
                self.forget_zone();
            }
            Reset::Lost { ended: false, .. } | Reset::Zoning { .. } | Reset::Died => (),
        }
        Changes {
            reset: Some(reason),
            ..Changes::default()
        }
    }

    /// Forgets the admission and its zone.
    fn forget_admission(&mut self) {
        self.characters = None;
        self.pending_transfer = None;
        self.death = None;
        self.forget_zone();
    }

    /// Forgets the zone's spawns, doors and objects.
    fn forget_zone(&mut self) {
        self.spawns.clear();
        self.player_posture = None;
        self.doors = DoorTable::default();
        self.objects = Objects::default();
    }

    /// Whether the session has admitted the player and is connected.
    #[must_use]
    pub const fn connected(&self) -> bool {
        self.connected
    }

    /// Whether the session is over and will not reconnect.
    #[must_use]
    pub const fn ended(&self) -> bool {
        self.ended
    }

    /// The world server's short name.
    #[must_use]
    pub fn world_name(&self) -> Option<&str> {
        self.world_name.as_deref()
    }

    /// The characters the world server offered, until one enters the world.
    #[must_use]
    pub const fn characters(&self) -> Option<&CharacterList> {
        self.characters.as_ref()
    }

    /// The current admission, which commands must name.
    #[must_use]
    pub const fn session_id(&self) -> Option<u64> {
        self.session_id
    }

    /// The zone's short name, as the server gave it.
    #[must_use]
    pub fn zone(&self) -> &str {
        &self.zone
    }

    /// The zone's far clip distance, when the server gave one.
    #[must_use]
    pub const fn far_clip(&self) -> Option<f32> {
        self.far_clip
    }

    /// The player's death, until the next admission.
    #[must_use]
    pub const fn death(&self) -> Option<&Death> {
        self.death.as_ref()
    }

    /// The transfer the server offered, until it is answered.
    #[must_use]
    pub const fn pending_transfer(&self) -> Option<&ZoneOffer> {
        self.pending_transfer.as_ref()
    }

    /// The admitted player.
    #[must_use]
    pub const fn player(&self) -> Option<&PlayerState> {
        self.player.as_ref()
    }

    /// The zone's spawns, by spawn ID.
    #[must_use]
    pub const fn spawns(&self) -> &BTreeMap<u16, Spawn> {
        &self.spawns
    }

    /// One spawn.
    #[must_use]
    pub fn spawn(&self, id: u16) -> Option<&Spawn> {
        self.spawns.get(&id)
    }

    /// Whether a spawn ID is the player's.
    #[must_use]
    pub fn is_player(&self, id: u16) -> bool {
        self.player
            .as_ref()
            .is_some_and(|player| player.spawn_id == id)
    }

    /// The posture last reported for the player or a spawn.
    #[must_use]
    pub fn posture(&self, id: u16) -> Option<PostureState> {
        if self.is_player(id) {
            return self.player_posture;
        }
        self.spawns.get(&id).and_then(|spawn| spawn.posture)
    }

    /// The health last reported for the player or a spawn, in percent.
    #[must_use]
    pub fn health(&self, id: u16) -> Option<u8> {
        match self.player.as_ref() {
            Some(player) if player.spawn_id == id => player.hp_percent,
            _ => self.spawns.get(&id).and_then(|spawn| spawn.health),
        }
    }

    /// The zone's doors.
    #[must_use]
    pub const fn doors(&self) -> &DoorTable {
        &self.doors
    }

    /// The zone's items on the ground and world containers.
    #[must_use]
    pub const fn objects(&self) -> &Objects {
        &self.objects
    }

    /// Whether the player can act now: admitted, connected, alive and not
    /// zoning.
    #[must_use]
    pub const fn in_world(&self) -> bool {
        self.connected
            && self.session_id.is_some()
            && self.death.is_none()
            && self.pending_transfer.is_none()
    }

    /// Whether a reply that names this admission still applies: it is the
    /// current one and connected.
    #[must_use]
    pub fn accepts_reply(&self, session_id: u64) -> bool {
        self.connected && self.session_id == Some(session_id)
    }
}

#[cfg(test)]
mod tests;
