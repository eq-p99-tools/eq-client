//! The client's picture of the game: what the server has said about the
//! connection, the admission, the player, their belongings and the zone
//! around them.
//!
//! Updates from the session are its only writer, applied by
//! [`ClientWorld::apply`], and each reset has one reason, which decides what
//! is forgotten. Nothing here draws, so any front end can host the world and
//! its rules are tested without one.

mod casting;
mod items;
mod target;
mod trade;
mod vitals;

pub use casting::{CastNews, Casting, Cooldowns, NoSpells, SpellCatalog, SpellTiming};
pub use items::ItemCache;
pub use target::Target;
pub use trade::{Loot, Merchant};
pub use vitals::{ReportedHp, Vitals};

use crate::{
    BookActionStatus, CampStatus, CharacterChoice, Coins, Death, PlayerState, PostureState,
    SpawnState, SpellBook, SpellUpdate, WorldEvent, WorldPosition, WorldUpdate, ZoneOffer,
    buffs::BuffTracker, combat::ConColor, doors::DoorTable, ground::Objects, inventory::Inventory,
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

/// Camping under way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Camp {
    /// When the player began camping.
    pub since: Instant,
    /// Whether the logout itself has begun.
    pub logging_out: bool,
}

/// What an update changed that a front end must redo.
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag names one independent thing to redo"
)]
pub struct Changes {
    /// The world was reset, and why.
    pub reset: Option<Reset>,
    /// A new admission began: present its zone and player.
    pub entered: bool,
    /// The server put the player here.
    pub placed: Option<WorldPosition>,
    /// The world server offered characters to play.
    pub characters: bool,
    /// What a spell notice did to the player's casting.
    pub cast: Option<CastNews>,
    /// The inventory changed.
    pub inventory: bool,
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
            WorldUpdate::Connection {
                connected,
                terminal,
                ..
            } => self.connection(*connected, *terminal),
            WorldUpdate::Game(event) => self.event(event, now, spells),
            WorldUpdate::Chat(_) | WorldUpdate::ServerMessage { .. } => Changes::default(),
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
            }
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
                ..
            } => {
                if self.accepts_reply(*session_id) && self.target.selected == *spawn_id {
                    self.target = Target::default();
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
            WorldEvent::Objects(update) => self.objects.apply(update),
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
            WorldEvent::Coins(coins) => self.coins = Some(*coins),
            WorldEvent::Loot(update) => changes.ignored = !self.trade.loot(update, &mut self.coins),
            WorldEvent::Merchant(update) => {
                changes.ignored = !self.trade.merchant(update, &mut self.coins);
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
            WorldEvent::CastRejected { session_id, .. } => {
                changes.ignored = !(self.accepts_reply(*session_id) && self.death.is_none());
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
            _ => (),
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
        // Whatever the reason, the player can no longer act on their target,
        // and only a death leaves camping under way.
        self.target = Target::default();
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

    /// The player's target.
    #[must_use]
    pub const fn target(&self) -> &Target {
        &self.target
    }

    /// Whether the target is gone: its spawn despawned, was replaced or
    /// turned invisible. The player choosing themselves never goes stale.
    #[must_use]
    pub fn target_stale(&self) -> bool {
        self.target.selected.is_some_and(|id| {
            !self.is_player(id)
                && self.spawns.get(&id).is_none_or(|spawn| {
                    spawn.state.invisible || Some(spawn.revision) != self.target.revision
                })
        })
    }

    /// The level color the server gave when the player last considered a
    /// spawn.
    #[must_use]
    pub fn considered(&self, id: u16) -> Option<ConColor> {
        self.considered.get(&id).copied()
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

    /// The player's mana, endurance and experience.
    #[must_use]
    pub const fn vitals(&self) -> &Vitals {
        &self.vitals
    }

    /// The player's HP to show, current and maximum, once reported: what the
    /// server reported, with what equipped items add when it leaves that out.
    #[must_use]
    pub fn hit_points(&self) -> Option<(u32, u32)> {
        self.vitals.hit_points(self.death.is_some())
    }

    /// The player's inventory, bank and cursor as the server reported them,
    /// with any local predictions the session marked.
    #[must_use]
    pub const fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// The corpse the player is looting.
    #[must_use]
    pub const fn loot(&self) -> Option<&Loot> {
        self.trade.loot.as_ref()
    }

    /// The merchant the player is trading with.
    #[must_use]
    pub const fn merchant(&self) -> Option<&Merchant> {
        self.trade.merchant.as_ref()
    }

    /// An item's definition, when the server sent it this admission.
    #[must_use]
    pub fn item(&self, id: u32) -> Option<&crate::ItemDetails> {
        self.items.get(id)
    }

    /// Camping under way.
    #[must_use]
    pub const fn camp(&self) -> Option<Camp> {
        self.camp
    }

    /// The coins the player carries, as last reported.
    #[must_use]
    pub const fn coins(&self) -> Option<&Coins> {
        self.coins.as_ref()
    }

    /// The player's own casting and gem timers.
    #[must_use]
    pub const fn casting(&self) -> &Casting {
        &self.casting
    }

    /// The player's buffs and the lasting effects not yet slotted.
    #[must_use]
    pub const fn buffs(&self) -> &BuffTracker {
        &self.buffs
    }

    /// The player's spellbook.
    #[must_use]
    pub const fn spell_book(&self) -> Option<&SpellBook> {
        self.spell_book.as_ref()
    }

    /// The spellbook change in flight, until confirmed.
    #[must_use]
    pub const fn book_action(&self) -> Option<&BookActionStatus> {
        self.book_action.as_ref()
    }

    /// Counts the session's spellbook replies, so that two alike still differ.
    #[must_use]
    pub const fn book_action_revision(&self) -> u64 {
        self.book_action_revision
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
