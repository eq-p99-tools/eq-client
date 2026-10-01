//! The client's picture of the game: what the server has said about the
//! connection, the admission, the player, their belongings and the zone
//! around them.
//!
//! Updates from the session are its only writer, applied by
//! [`ClientWorld::apply`], and each reset has one reason, which decides what
//! is forgotten. Nothing here draws, so any front end can host the world and
//! its rules are tested without one.

mod casting;
mod changes;
mod items;
mod link;
mod notice;
mod read;
mod target;
mod trade;
mod vitals;

pub use casting::{CastNews, Casting, Cooldowns, NoSpells, SpellCatalog, SpellTiming};
pub use changes::{Changes, Moved, Reply, Reset};
pub use items::ItemCache;
pub use link::Link;
pub use notice::Notice;
pub use target::Target;
pub use trade::{Loot, Merchant};
pub use vitals::{ReportedHp, Vitals};

use crate::{
    BookActionStatus, CampStatus, CharacterChoice, Coins, Death, PlayerState, PostureState,
    SpawnState, SpellBook, SpellUpdate, WorldEvent, WorldUpdate, ZoneOffer, buffs::BuffTracker,
    combat::ConColor, doors::DoorTable, ground::Objects, inventory::Inventory, loot::LootUpdate,
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

/// Camping under way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Camp {
    /// When the player began camping.
    pub since: Instant,
    /// Whether the logout itself has begun.
    pub logging_out: bool,
}

/// How the session lets the player move: calibrated speeds, or none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionGrant {
    /// World units per second running forward, when moving is allowed.
    pub units_per_second: Option<f32>,
    /// Backing up, when it is allowed.
    pub backward_units_per_second: Option<f32>,
    /// Walking, when calibrated apart from running.
    pub walk_units_per_second: Option<f32>,
    /// Moving sideways, when calibrated apart.
    pub strafe_units_per_second: Option<f32>,
    /// Whether the session sends falling samples.
    pub falls: bool,
}

/// What the server has told the client, kept by one writer.
#[derive(Default)]
pub struct ClientWorld {
    connected: bool,
    ended: bool,
    world_name: Option<String>,
    characters: Option<CharacterList>,
    session_id: Option<u64>,
    /// What the admission lets the player do.
    capabilities: Vec<crate::Capability>,
    zone: String,
    far_clip: Option<f32>,
    death: Option<Death>,
    pending_transfer: Option<ZoneOffer>,
    player: Option<PlayerState>,
    /// The player's own posture, which the zone's spawns do not carry.
    player_posture: Option<PostureState>,
    spawns: BTreeMap<u16, Spawn>,
    /// Level colors the server reported for spawns the player considered.
    considered: BTreeMap<u16, ConColor>,
    target: Target,
    doors: DoorTable,
    objects: Objects,
    /// The last revision given to a spawn.
    revision: u64,
    vitals: Vitals,
    inventory: Inventory,
    /// Item definitions the server sent for inspection.
    items: ItemCache,
    coins: Option<Coins>,
    trade: trade::Trade,
    camp: Option<Camp>,
    motion: Option<MotionGrant>,
    casting: Casting,
    buffs: BuffTracker,
    spell_book: Option<SpellBook>,
    /// The spellbook change in flight, until confirmed.
    book_action: Option<BookActionStatus>,
    /// Counts the session's spellbook replies, so that two alike still differ.
    book_action_revision: u64,
}

impl ClientWorld {
    /// Applies one update from the session and says what a front end must redo.
    /// The installed client's spell data tells lasting effects from instant ones.
    pub fn apply(
        &mut self,
        update: &WorldUpdate,
        now: Instant,
        spells: &dyn SpellCatalog,
    ) -> Changes {
        match update {
            WorldUpdate::Connection(link) => {
                let mut changes = self.connection(link.connected(), link.ended());
                changes.notices.push(Notice::Connection {
                    link: *link,
                    dead: self.death.is_some(),
                });
                changes
            }
            WorldUpdate::Game(event) => self.event(event, now, spells),
            WorldUpdate::ServerMessage {
                string_id,
                arguments,
            } => Changes {
                notices: vec![Notice::ServerString {
                    id: *string_id,
                    arguments: arguments.clone(),
                }],
                ..Changes::default()
            },
            WorldUpdate::Chat(_) => Changes::default(),
        }
    }

    /// Runs the world's clocks: doors the server leaves open swing shut, and
    /// refreshed gems start their timers once the spells' timing is known.
    pub fn tick(&mut self, now: Instant, spells: &dyn SpellCatalog) {
        if self.doors.closes_due(now) {
            self.doors.close_due(now);
        }
        self.casting.cooldowns.resolve(spells, now);
    }

    /// The player's own choice of target, which the client tells the session
    /// separately; what the session makes of it arrives as news.
    pub fn select_target(&mut self, spawn: Option<u16>) {
        self.target = Target {
            selected: spawn,
            sent: false,
            revision: spawn
                .and_then(|id| self.spawns.get(&id))
                .map(|spawn| spawn.revision),
        };
    }

    /// The player opened a corpse; the server's word on it arrives as news.
    pub fn open_loot(&mut self, corpse_id: u16) {
        self.trade.loot = Some(Loot {
            corpse_id,
            items: BTreeMap::new(),
            listed: false,
        });
    }

    /// The player is done looting.
    pub fn close_loot(&mut self) {
        self.trade.loot = None;
    }

    /// The player asked a merchant to trade; the server's word on it arrives
    /// as news.
    pub fn open_shop(&mut self, merchant_id: u16) {
        self.trade.merchant = Some(Merchant {
            merchant_id,
            stock: BTreeMap::new(),
        });
    }

    /// The player is done trading.
    pub fn close_shop(&mut self) {
        self.trade.merchant = None;
    }

    /// Notes the player's health.
    fn own_health(&mut self, percent: u8) {
        if let Some(player) = self.player.as_mut() {
            player.hp_percent = Some(percent);
        }
    }

    /// The player's health follows the HP their last report and equipped
    /// items make.
    fn show_hp(&mut self) {
        if let Some(percent) = self.hit_points().and_then(vitals::percent) {
            self.own_health(percent);
        }
    }

    /// Works out the HP equipped items add again, after the inventory, the
    /// player or the report changed.
    fn refresh_item_hp(&mut self) {
        if let Some(player) = self.player.as_ref()
            && self.vitals.refresh_item_hp(player, &self.inventory)
        {
            self.show_hp();
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
    fn event(&mut self, event: &WorldEvent, now: Instant, spells: &dyn SpellCatalog) -> Changes {
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
                capabilities,
                session_id,
                zone,
                player,
                far_clip,
            } => {
                changes = self.reset(Reset::Entered);
                changes.entered = true;
                self.capabilities.clone_from(capabilities);
                self.session_id = Some(*session_id);
                self.zone.clone_from(zone);
                self.far_clip = *far_clip;
                self.vitals = Vitals {
                    mana: Some(player.mana),
                    endurance: player.endurance,
                    ..Vitals::default()
                };
                self.casting.pending = None;
                self.casting.cooldowns.restore(
                    &player.memorized_spells,
                    player.spell_refresh_ms,
                    now,
                );
                self.player = Some((**player).clone());
            }
            WorldEvent::MotionState {
                session_id,
                units_per_second,
                backward_units_per_second,
                walk_units_per_second,
                strafe_units_per_second,
                falls,
            } => {
                if self.session_id == Some(*session_id) {
                    self.motion = Some(MotionGrant {
                        units_per_second: *units_per_second,
                        backward_units_per_second: *backward_units_per_second,
                        walk_units_per_second: *walk_units_per_second,
                        strafe_units_per_second: *strafe_units_per_second,
                        falls: *falls,
                    });
                    changes.motion = true;
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::MotionSent {
                session_id,
                position,
                refused,
            } => match self.player.as_mut() {
                Some(player)
                    if self.connected
                        && self.session_id == Some(*session_id)
                        && self.death.is_none() =>
                {
                    player.position = *position;
                    changes.moved = Some(Moved {
                        position: *position,
                        refused: refused.clone(),
                    });
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
            WorldEvent::ZoneTransferRejected { session_id, reason } => {
                if self.session_id == Some(*session_id) {
                    self.pending_transfer = None;
                    self.connected = self.death.is_none();
                    changes.notices.push(Notice::TransferRefused(*reason));
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::ZoneLineRejected { session_id, reason } => {
                if self.session_id == Some(*session_id) {
                    changes
                        .notices
                        .push(Notice::ZoneLineRefused(reason.clone()));
                    changes.replies.push(Reply::ZoneLineRefused);
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::Spawns(spawns) => {
                for spawn in spawns {
                    // A new spawn has not been considered, whatever had its ID.
                    self.considered.remove(&spawn.spawn_id);
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
                self.considered.remove(id);
            }
            WorldEvent::Consideration(consideration) => {
                self.considered
                    .insert(consideration.target_id, consideration.color);
                changes.notices.push(Notice::Consideration {
                    consideration: *consideration,
                    name: self.name(consideration.target_id),
                });
            }
            WorldEvent::Damage(damage) => match self.player.as_ref() {
                // Only damage the player dealt or took is theirs to read.
                Some(player)
                    if player.spawn_id == damage.source_id
                        || player.spawn_id == damage.target_id =>
                {
                    changes.notices.push(Notice::Damage {
                        damage: *damage,
                        own_id: player.spawn_id,
                        source: self.name(damage.source_id),
                        target: self.name(damage.target_id),
                    });
                }
                _ => changes.ignored = true,
            },
            WorldEvent::TargetSent(id) => {
                if self.target.selected == *id {
                    self.target.sent = true;
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::TargetRejected {
                session_id,
                spawn_id,
                reason,
            } => {
                if self.accepts_reply(*session_id) && self.target.selected == *spawn_id {
                    self.target = Target::default();
                    changes.notices.push(Notice::TargetRefused(reason.clone()));
                } else {
                    changes.ignored = true;
                }
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
                    self.own_health(*percent);
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
            WorldEvent::DoorAction {
                session_id,
                door_id,
                error,
            } => {
                if self.accepts_reply(*session_id) {
                    changes.notices.push(Notice::Door {
                        door_id: *door_id,
                        error: error.clone(),
                    });
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::Objects(update) => self.objects.apply(update),
            WorldEvent::ObjectAction {
                session_id, error, ..
            } => match error {
                Some(error) if self.accepts_reply(*session_id) => {
                    changes.notices.push(Notice::GroundRefused(error.clone()));
                }
                _ => changes.ignored = true,
            },
            WorldEvent::Level {
                current,
                experience,
                ..
            } => match self.player.as_mut() {
                Some(player) if self.connected => {
                    player.level = *current;
                    self.vitals.experience = Some(*experience);
                    // What items give can depend on the level.
                    self.refresh_item_hp();
                }
                _ => changes.ignored = true,
            },
            WorldEvent::Skill { skill_id, value } => match self.player.as_mut() {
                Some(player) if self.connected => player.apply_skill(*skill_id, *value),
                _ => changes.ignored = true,
            },
            WorldEvent::Spell(update) => changes.cast = self.spell(update, now),
            WorldEvent::Mana(mana) => self.vitals.mana = Some(*mana),
            WorldEvent::Resources { mana, endurance } => {
                self.vitals.mana = Some(*mana);
                self.vitals.endurance = Some(*endurance);
            }
            WorldEvent::Experience(value) => self.vitals.experience = Some(*value),
            WorldEvent::HitPoints {
                spawn_id,
                current,
                maximum,
                without_items,
            } => {
                if self.is_player(*spawn_id) {
                    self.vitals.reported_hp = Some(ReportedHp {
                        current: *current,
                        maximum: *maximum,
                        without_items: *without_items,
                    });
                    self.refresh_item_hp();
                    self.show_hp();
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::Inventory(update) => {
                self.inventory.apply(update.clone());
                changes.inventory = true;
                self.refresh_item_hp();
            }
            WorldEvent::InventoryAction {
                session_id,
                revision,
                error,
            } => changes.replies.push(Reply::InventoryMove {
                session_id: *session_id,
                revision: *revision,
                error: error.clone(),
            }),
            WorldEvent::ItemUseAction {
                session_id,
                request_id,
                error,
            } => {
                if self.accepts_reply(*session_id) {
                    changes.replies.push(Reply::ItemUse {
                        session_id: *session_id,
                        request_id: *request_id,
                        error: error.clone(),
                    });
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::Coins(coins) => {
                self.coins = Some(*coins);
                changes.trade = true;
            }
            WorldEvent::Loot(update) => {
                if self.trade.loot(update, &mut self.coins) {
                    changes.trade = true;
                    changes.notices.extend(trade::loot_notice(update));
                    if let LootUpdate::Taken { slot, accepted } = update {
                        changes.replies.push(Reply::LootTaken {
                            slot: *slot,
                            accepted: *accepted,
                        });
                    }
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::Merchant(update) => {
                if self.trade.merchant(update, &mut self.coins) {
                    changes.trade = true;
                    changes.notices.extend(trade::merchant_notice(update));
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::MerchantRefused { session_id, reason } => {
                if self.session_id == Some(*session_id) {
                    changes.notices.push(Notice::TradeRefused(reason.clone()));
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::CastPending {
                session_id,
                spell_id,
            } => {
                if self.accepts_reply(*session_id) && self.death.is_none() {
                    self.casting.pending = *spell_id;
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::CastRejected {
                session_id,
                spell_id,
                reason,
            } => {
                if self.accepts_reply(*session_id) && self.death.is_none() {
                    changes.notices.push(Notice::CastRefused {
                        spell_id: *spell_id,
                        reason: reason.clone(),
                    });
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::BuffSnapshot(buffs) => self.buffs.replace_snapshot(
                buffs
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, buff)| Some((u32::try_from(slot).ok()?, buff.clone()?)))
                    .collect(),
            ),
            WorldEvent::Buff(update) => {
                if self
                    .player
                    .as_ref()
                    .is_some_and(|player| u32::from(player.spawn_id) == update.entity_id)
                {
                    self.buffs.apply(update.clone());
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::SpellEffect(effect) => {
                if self.is_player(effect.target_id) {
                    // Only a lasting effect leaves a buff the server has not slotted.
                    if effect.effect_flag == 4
                        && !matches!(effect.spell_id, 0 | u16::MAX)
                        && !spells.instant_effect(u32::from(effect.spell_id))
                    {
                        self.buffs.observe_effect(effect.clone());
                    }
                } else {
                    changes.ignored = true;
                }
            }
            WorldEvent::SpellBook(book) => self.spell_book = Some(book.clone()),
            WorldEvent::BookAction(status) => {
                self.book_action =
                    (!matches!(status, BookActionStatus::Confirmed)).then(|| status.clone());
                self.book_action_revision = self.book_action_revision.wrapping_add(1);
            }
            WorldEvent::Camp(CampStatus::Camped) => return self.reset(Reset::Camped),
            WorldEvent::Camp(status) => {
                changes.notices.push(Notice::Camp(status.clone()));
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
            }
            WorldEvent::ItemDetails(item) => self.items.insert(item.clone()),
            // No front end creates characters yet; when one does, the news
            // lands here, and a new kind of news is a compile error here.
            WorldEvent::CharacterCreation { .. } => (),
        }
        changes
    }

    /// A spell notice: the book and gems change, and the player's own casts
    /// move along while they are connected and alive.
    fn spell(&mut self, update: &SpellUpdate, now: Instant) -> Option<CastNews> {
        // Forgetting a gem is answered only by the gem emptying.
        if matches!(update, SpellUpdate::Slot { mode: 2, .. })
            && matches!(self.book_action, Some(BookActionStatus::Submitted))
        {
            self.book_action = None;
        }
        if let Some(book) = self.spell_book.as_mut() {
            book.apply(update);
        }
        let active = self.connected && self.death.is_none();
        let player = self.player.as_mut()?;
        update.apply_gems(&mut player.memorized_spells);
        if !active {
            return None;
        }
        self.casting
            .observe(player.spawn_id, &player.memorized_spells, update, now)
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
        self.own_health(0);
        self.death = Some(death.clone());
        self.reset(Reset::Died)
    }

    /// Forgets what the reason makes stale, and says so.
    fn reset(&mut self, reason: Reset) -> Changes {
        // Whatever the reason, the player can no longer act on their target
        // or move until the session says so again, and only a death leaves
        // camping under way.
        self.target = Target::default();
        self.motion = None;
        if reason != Reset::Died {
            self.camp = None;
        }
        match reason {
            Reset::Entered => {
                self.forget_admission();
                // The server sends the inventory again after each admission.
                self.inventory = Inventory::default();
                self.spell_book = None;
                self.buffs.clear();
                self.casting.interrupted = None;
            }
            Reset::Camped => {
                // The character is gone, and with them everything about them.
                self.forget_admission();
                self.book_action = None;
                self.casting = Casting::default();
                self.vitals = Vitals::default();
                self.inventory = Inventory::default();
                self.coins = None;
                self.spell_book = None;
                self.buffs.clear();
                self.session_id = None;
                self.capabilities.clear();
                self.player = None;
                self.connected = false;
            }
            Reset::Lost {
                ended,
                transferring,
            } => {
                self.drop_actions();
                if ended || !transferring {
                    self.casting.reset_cooldowns();
                    self.spell_book = None;
                    self.buffs.clear();
                }
                if ended {
                    self.characters = None;
                    self.pending_transfer = None;
                    self.inventory = Inventory::default();
                    self.forget_zone();
                }
            }
            Reset::Zoning { .. } => self.drop_actions(),
            Reset::Died => {
                self.casting.cast = None;
                self.casting.interrupted = None;
                self.casting.reset_cooldowns();
                self.book_action = None;
            }
        }
        Changes {
            reset: Some(reason),
            ..Changes::default()
        }
    }

    /// Drops the actions in flight: a cast, its request and interruption, and
    /// a spellbook change.
    fn drop_actions(&mut self) {
        self.casting.drop_actions();
        self.book_action = None;
    }

    /// Forgets the admission and its zone.
    fn forget_admission(&mut self) {
        self.characters = None;
        self.items = ItemCache::default();
        self.pending_transfer = None;
        self.death = None;
        self.forget_zone();
    }

    /// Forgets the zone's spawns, doors and objects.
    fn forget_zone(&mut self) {
        self.spawns.clear();
        self.considered.clear();
        // The corpse and the merchant were the zone's.
        self.trade = trade::Trade::default();
        self.player_posture = None;
        self.doors = DoorTable::default();
        self.objects = Objects::default();
    }

    /// A spawn's server name, when it is known.
    fn name(&self, id: u16) -> Option<String> {
        self.spawns.get(&id).map(|spawn| spawn.state.name.clone())
    }
}

#[cfg(test)]
mod tests;
