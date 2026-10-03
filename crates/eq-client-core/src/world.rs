//! The client's picture of the game: what the server has said about the
//! connection, the admission, the player, their belongings and the zone
//! around them.
//!
//! Updates from the session are its only writer, applied by
//! [`ClientWorld::apply`], and each reset has one reason, which decides what
//! is forgotten. Nothing here draws, so any front end can host the world and
//! its rules are tested without one.

mod abilities;
mod admission;
mod belongings;
mod casting;
mod changes;
mod character;
mod group;
mod items;
mod link;
mod notice;
mod raid;
mod read;
mod spells;
mod target;
mod trade;
mod vitals;
mod who;
mod zone;

pub use casting::{CastNews, Casting, Cooldowns, NoSpells, SpellCatalog, SpellTiming};
pub use changes::{Changes, Moved, Reply, Reset};
pub use group::{Group, GroupNotice};
pub use items::ItemCache;
pub use link::Link;
pub use notice::{ListingNotice, Notice, Party};
pub use raid::{Raid, RaidNotice, RaidRank};
pub use target::Target;
pub use trade::{Asker, Exchange, Loot, Merchant};
pub use vitals::{ReportedHp, Vitals};
pub use who::ZonePlayer;

use crate::{
    BookActionStatus, CharacterChoice, Death, PlayerState, PostureState, SpawnState, SpellBook,
    WorldEvent, WorldUpdate, ZoneOffer, buffs::BuffTracker, inventory::Inventory,
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

/// What the server has told the client, kept by one writer. Its fields go
/// in order of how long they last: the connection's, the admission's, the
/// character's, and the zone's, which leaving the zone forgets in one step.
#[derive(Default)]
pub struct ClientWorld {
    // The connection to the servers.
    connected: bool,
    ended: bool,
    world_name: Option<String>,
    /// The time in Norrath the server last gave, and when it came.
    time: Option<(crate::clock::GameTime, Instant)>,
    /// Guild names by number, from the world's guild list.
    guild_names: BTreeMap<u32, String>,
    characters: Option<CharacterList>,
    /// The last revision given to a spawn, which never repeats.
    revision: u64,
    /// Counts the session's spellbook replies, so that two alike still differ.
    book_action_revision: u64,

    // The admission: from entering a zone until the next admission.
    session_id: Option<u64>,
    /// What the admission lets the player do.
    capabilities: Vec<crate::Capability>,
    zone_name: String,
    far_clip: Option<f32>,
    death: Option<Death>,
    pending_transfer: Option<ZoneOffer>,
    /// The resurrection offered and not yet answered.
    resurrection: Option<crate::resurrection::ResurrectionOffer>,
    /// The book or note open to read.
    reading: Option<crate::books::BookText>,
    /// The tradeskill container whose combine waits for the server.
    combining: Option<crate::inventory::InventorySlot>,
    /// The player's group, while they are in one. The server lists it again
    /// after each admission.
    group: Option<Group>,
    /// Who invited the player to their group, until the player answers or
    /// is in one; the session forgets it with the admission.
    group_invitation: Option<String>,
    /// Whether the player agreed to join a group whose list has not come.
    joining_group: bool,
    /// The target the server's answer to an assist names, until the player
    /// takes it.
    assisted: Option<u16>,
    /// The player's raid, while they are in one. The server lists it again
    /// after each admission.
    raid: Option<Raid>,
    /// Who invited the player to their raid, until the player answers or is
    /// in one; the session forgets it with the admission.
    raid_invitation: Option<String>,
    /// Raid news on the way: an invitation sent or accepted, and the list
    /// the server is sending.
    raid_flow: raid::RaidFlow,
    /// Item definitions the server sent for inspection.
    items: ItemCache,
    // Until any reset: the target, the motion granted and camping.
    target: Target,
    motion: Option<MotionGrant>,
    camp: Option<Camp>,

    // The character: from admission until camping.
    player: Option<PlayerState>,
    vitals: Vitals,
    inventory: Inventory,
    /// Where the player's coins are, as the session keeps and tells them.
    wallet: crate::money::Wallet,
    /// How fed and watered the player is, as the server last said.
    nourishment: Option<crate::food::Nourishment>,
    casting: Casting,
    buffs: BuffTracker,
    spell_book: Option<SpellBook>,
    /// The spellbook change in flight, until confirmed.
    book_action: Option<BookActionStatus>,
    /// When each ability timer the session started runs out; servers keep
    /// them across zones.
    ability_timers: std::collections::BTreeMap<crate::abilities::Recovery, Instant>,
    /// The abilities the server type offers, once the session says.
    offered_abilities: Option<Vec<crate::abilities::Ability>>,

    // The zone's contents, the corpse and merchant among them.
    zone: zone::Zone,
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
                message_type,
            } => Changes {
                notices: vec![Notice::ServerString {
                    id: *string_id,
                    arguments: arguments.clone(),
                    message_type: *message_type,
                }],
                ..Changes::default()
            },
            WorldUpdate::Chat(_) => Changes::default(),
        }
    }

    /// Runs the world's clocks: doors the server leaves open swing shut, and
    /// refreshed gems start their timers once the spells' timing is known.
    pub fn tick(&mut self, now: Instant, spells: &dyn SpellCatalog) {
        if self.zone.doors.closes_due(now) {
            self.zone.doors.close_due(now);
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
                .and_then(|id| self.zone.spawns.get(&id))
                .map(|spawn| spawn.revision),
        };
    }

    /// The player opened a corpse; the server's word on it arrives as news.
    pub fn open_loot(&mut self, corpse_id: u16) {
        self.zone.trade.loot = Some(Loot {
            corpse_id,
            items: BTreeMap::new(),
            listed: false,
        });
    }

    /// The player is done looting.
    pub fn close_loot(&mut self) {
        self.zone.trade.loot = None;
    }

    /// The target the server's answer to the player's assist names, taken
    /// once: a front end targets it as the player would.
    pub fn take_assisted(&mut self) -> Option<u16> {
        self.assisted.take()
    }

    /// The player answered the resurrection offered; whatever comes of it
    /// arrives as news.
    pub fn answer_resurrection(&mut self) {
        self.resurrection = None;
    }

    /// The player closed the book or note they were reading.
    pub fn close_reading(&mut self) {
        self.reading = None;
    }

    /// The player asked to open a world container; the server's answer opens
    /// it.
    pub fn ask_container(&mut self, drop_id: u32) {
        self.zone.asked_container = Some(drop_id);
    }

    /// The player closed the world container open for them; the server puts
    /// what it still held back in the inventory.
    pub fn close_container(&mut self) {
        self.zone.asked_container = None;
        self.zone.container = None;
    }

    /// The player asked a merchant to trade; the server's word on it arrives
    /// as news.
    pub fn open_shop(&mut self, merchant_id: u16) {
        self.zone.trade.merchant = Some(Merchant {
            merchant_id,
            stock: BTreeMap::new(),
            rate: None,
        });
    }

    /// The player is done trading.
    pub fn close_shop(&mut self) {
        self.zone.trade.merchant = None;
    }

    /// The player asked a character to trade, holding something to hand
    /// over; their answer opens the window, the give window for an NPC.
    pub fn offer_trade(&mut self, with: u16) {
        let partner = match self.zone.spawns.get(&with).map(|spawn| spawn.state.kind) {
            Some(crate::SpawnKind::Player) => crate::exchange::Partner::Player,
            _ => crate::exchange::Partner::Npc,
        };
        self.zone.trade.exchange = Some(Exchange::asked(with, partner));
    }

    /// The player clicked Give; the server's word ends the window.
    pub fn give(&mut self) {
        if let Some(exchange) = self.zone.trade.exchange.as_mut() {
            exchange.given = true;
        }
    }

    /// The player closed the give window; what it held comes back as news.
    pub fn close_trade(&mut self) {
        self.zone.trade.exchange = None;
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

    /// Hands each kind of news to the part of the world it is about. Every
    /// kind has an arm, so a new kind of news is a compile error here.
    #[allow(
        clippy::too_many_lines,
        reason = "a dispatch table: one arm per kind of news, each a call"
    )]
    fn event(&mut self, event: &WorldEvent, now: Instant, spells: &dyn SpellCatalog) -> Changes {
        let mut changes = Changes::default();
        let news = &mut changes;
        match event {
            // The connection, the characters on offer and the admission.
            WorldEvent::WorldName { short_name } => self.world_name = Some(short_name.clone()),
            // The time of day, and how the zone's sky looks.
            WorldEvent::TimeOfDay(time) => self.time = Some((*time, now)),
            WorldEvent::Sky(sky) => self.zone.sky = Some(*sky),
            WorldEvent::CharacterSelection {
                selection_id,
                characters,
            } => self.offered(*selection_id, characters, news),
            WorldEvent::Entered {
                capabilities,
                session_id,
                zone,
                player,
                far_clip,
            } => {
                return self.entered((capabilities, *session_id), (zone, *far_clip), player, now);
            }
            WorldEvent::MotionState {
                session_id,
                units_per_second,
                backward_units_per_second,
                walk_units_per_second,
                strafe_units_per_second,
                falls,
            } => self.grant_motion(
                *session_id,
                MotionGrant {
                    units_per_second: *units_per_second,
                    backward_units_per_second: *backward_units_per_second,
                    walk_units_per_second: *walk_units_per_second,
                    strafe_units_per_second: *strafe_units_per_second,
                    falls: *falls,
                },
                news,
            ),
            WorldEvent::MotionSent {
                session_id,
                position,
                refused,
            } => self.motion_sent(*session_id, *position, refused.as_ref(), news),
            WorldEvent::Death(death) => return self.died(death),
            WorldEvent::ZoneTransfer(offer) => return self.transfer(offer),
            WorldEvent::ZoneTransferRejected { session_id, reason } => {
                self.transfer_refused(*session_id, *reason, news);
            }
            WorldEvent::ZoneLineRejected { session_id, reason } => {
                self.zone_line_refused(*session_id, reason, news);
            }
            WorldEvent::Camp(status) => return self.camp_news(status, now),
            // No front end creates characters yet; when one does, the news
            // lands here.
            WorldEvent::CharacterCreation { .. } => (),

            // The zone and what is in it.
            WorldEvent::Spawns(spawns) => self.zone.appear(spawns, &mut self.revision),
            WorldEvent::Despawn(id) => self.zone.vanish(*id),
            WorldEvent::Consideration(consideration) => {
                news.notices.push(self.zone.consider(consideration));
            }
            WorldEvent::Damage(damage) => self.damage(damage, news),
            WorldEvent::TargetSent(id) => self.target_sent(*id, news),
            WorldEvent::TargetRejected {
                session_id,
                spawn_id,
                reason,
            } => self.target_refused((*session_id, *spawn_id), reason, news),
            WorldEvent::Visibility {
                spawn_id,
                invisible,
            } => self.zone.visibility(*spawn_id, *invisible),
            // How /who lists a player, and the guilds it names.
            WorldEvent::Listing { spawn_id, change } => {
                if let Some(spawn) = self.zone.spawns.get_mut(spawn_id) {
                    let state = &mut spawn.state;
                    who::relist(*change, &mut state.level, &mut state.listing);
                }
                if self.is_player(*spawn_id)
                    && let Some(player) = self.player.as_mut()
                {
                    who::relist(*change, &mut player.level, &mut player.listing);
                }
            }
            WorldEvent::ListingSet { session_id, change } => {
                self.listing_set(*session_id, *change, news);
            }
            WorldEvent::ListingRefused {
                session_id,
                reason,
                string_id,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::ListingRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                    });
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::GuildNames(names) => self.guild_names = names.iter().cloned().collect(),
            WorldEvent::LastName { name, last_name } => self.last_name(name, last_name),
            WorldEvent::Posture { spawn_id, posture } => {
                let player = self.is_player(*spawn_id);
                self.zone.posture(*spawn_id, *posture, player);
            }
            WorldEvent::HealthPercent { spawn_id, percent } => {
                self.health_percent(*spawn_id, *percent);
            }
            WorldEvent::Position {
                spawn_id,
                position,
                velocity,
            } => self.position(*spawn_id, (*position, *velocity), news),
            WorldEvent::WearChange(change) => self.wear(change),
            WorldEvent::Doors(update) => self.zone.doors.apply(update, now),
            WorldEvent::DoorAction {
                session_id,
                door_id,
                error,
            } => self.door_used((*session_id, *door_id), error.as_ref(), news),
            WorldEvent::Objects(update) => self.objects_news(update, news),
            WorldEvent::ObjectAction {
                session_id, error, ..
            } => self.object_refused(*session_id, error.as_ref(), news),

            // The player character.
            WorldEvent::Level {
                current,
                experience,
                ..
            } => self.level_news(*current, *experience, news),
            WorldEvent::Skill { skill_id, value } => self.skill_news(*skill_id, *value, news),
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
            } => self.hp_news(
                *spawn_id,
                ReportedHp {
                    current: *current,
                    maximum: *maximum,
                    without_items: *without_items,
                },
                news,
            ),

            // What the player owns and trades.
            WorldEvent::Inventory(update) => {
                let before = self.offering();
                self.inventory_news(update, news);
                self.reconsider(&before);
            }
            WorldEvent::InventoryAction {
                session_id,
                revision,
                error,
            } => news.replies.push(Reply::InventoryMove {
                session_id: *session_id,
                revision: *revision,
                error: error.clone(),
            }),
            WorldEvent::ItemUseAction {
                session_id,
                request_id,
                error,
            } => self.item_used((*session_id, *request_id), error.as_ref(), news),
            WorldEvent::ItemDetails(item) => self.items.insert(item.clone()),
            // The session keeps the coins and says where they are.
            WorldEvent::Coins(coins) => {
                self.wallet.purse = Some(*coins);
                news.trade = true;
            }
            WorldEvent::CoinsElsewhere {
                cursor,
                bank,
                given,
                offered,
            } => {
                let before = self.offering();
                self.wallet.cursor = *cursor;
                self.wallet.bank = Some(*bank);
                self.wallet.given = *given;
                self.wallet.offered = *offered;
                self.reconsider(&before);
                news.trade = true;
            }
            WorldEvent::CoinsRefused { session_id, reason } => {
                self.coins_refused(*session_id, reason, news);
            }
            WorldEvent::Loot(update) => self.loot_news(update, news),
            WorldEvent::Merchant(update) => self.merchant_news(update, news),
            WorldEvent::MerchantRefused { session_id, reason } => {
                self.merchant_refused(*session_id, reason, news);
            }
            WorldEvent::Exchange(update) => self.exchange_news(update, news),
            WorldEvent::ExchangeRefused { session_id, reason } => {
                self.exchange_refused(*session_id, reason, news);
            }

            // The player's abilities.
            WorldEvent::AbilitiesOffered(offered) => {
                self.offered_abilities = Some(offered.clone());
            }
            WorldEvent::AbilityUsed {
                session_id,
                ability,
                ready_in,
            } => self.ability_used((*session_id, *ability, *ready_in), now, news),
            WorldEvent::AbilityRefused {
                session_id,
                reason,
                string_id,
                arguments,
            } => self.ability_refused(*session_id, (reason, *string_id, arguments), news),
            WorldEvent::BindWound(update) => self.bind_wound(update, news),
            WorldEvent::WhoList(list) => news.notices.push(Notice::WhoList(list.clone())),
            // News of raids, and a raid request the session would not send.
            WorldEvent::Raid(update) => self.raid_news(update, news),
            WorldEvent::RaidRefused {
                session_id,
                reason,
                string_id,
                arguments,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::RaidRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                        arguments: arguments.clone(),
                    });
                } else {
                    news.ignored = true;
                }
            }
            // Dice nearby, the server's answer to an assist, and a roll,
            // emote or assist the session would not send.
            WorldEvent::Roll(roll) => news.notices.push(Notice::Roll(roll.clone())),
            WorldEvent::Assisted(answer) => self.assisted = answer.target,
            WorldEvent::SocialRefused {
                session_id,
                reason,
                string_id,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::SocialRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                    });
                } else {
                    news.ignored = true;
                }
            }

            // Food and drink: the session eats and drinks for the player.
            WorldEvent::Nourishment(nourishment) => self.nourishment = Some(*nourishment),
            WorldEvent::NothingToEat { food, water } => news.notices.push(Notice::NothingToEat {
                food: *food,
                water: *water,
            }),
            // Players' corpses: the server's word on consents, and what the
            // session would not send.
            WorldEvent::Consent(consent) => {
                let own = self
                    .player
                    .as_ref()
                    .is_some_and(|player| player.name.eq_ignore_ascii_case(&consent.owner));
                news.notices.push(Notice::Consent {
                    consent: consent.clone(),
                    own,
                });
            }
            WorldEvent::CorpseRefused {
                session_id,
                reason,
                string_id,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::CorpseRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                    });
                } else {
                    news.ignored = true;
                }
            }
            // The player's pet: whose pet a spawn is, its buffs, and the
            // commands the session would not send.
            WorldEvent::PetOwner { spawn_id, owner } => {
                if let Some(spawn) = self.zone.spawns.get_mut(spawn_id) {
                    spawn.state.pet_owner = *owner;
                }
            }
            WorldEvent::PetBuffs(buffs) => self.zone.pet_buffs = Some(buffs.clone()),
            WorldEvent::PetRefused {
                session_id,
                reason,
                string_id,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::PetRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                    });
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::ConsumeRefused {
                session_id,
                reason,
                string_id,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::ConsumeRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                    });
                } else {
                    news.ignored = true;
                }
            }
            // A book or note to read, and a request the session would not send.
            WorldEvent::BookText(text) => self.reading = Some(text.clone()),
            WorldEvent::ReadRefused { session_id, reason } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::ReadRefused(reason.clone()));
                } else {
                    news.ignored = true;
                }
            }
            // A combine under way or judged, and one the session would not send.
            WorldEvent::Combine(update) => {
                self.combining = match update {
                    crate::tradeskills::CombineUpdate::Started(container) => Some(*container),
                    crate::tradeskills::CombineUpdate::Answered => None,
                };
            }
            WorldEvent::CombineRefused {
                session_id,
                reason,
                string_id,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::CombineRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                    });
                } else {
                    news.ignored = true;
                }
            }
            // News of groups, and a group request the session would not send.
            WorldEvent::Group(update) => self.group_news(update, news),
            WorldEvent::GroupRefused {
                session_id,
                reason,
                string_id,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::GroupRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                    });
                } else {
                    news.ignored = true;
                }
            }
            // A resurrection to answer, and an answer the session would not send.
            WorldEvent::Resurrection(offer) => self.resurrection = Some(offer.clone()),
            WorldEvent::ResurrectionRefused { session_id, reason } => {
                if self.session_id == Some(*session_id) {
                    news.notices
                        .push(Notice::ResurrectionRefused(reason.clone()));
                } else {
                    news.ignored = true;
                }
            }
            // Training at a guildmaster, and the practice points it spends.
            WorldEvent::Training(update) => self.training_news(update, news),
            WorldEvent::TrainingRefused { session_id, reason } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::TrainingRefused(reason.clone()));
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::PracticePoints(points) => {
                if let Some(player) = self.player.as_mut() {
                    player.practice_points = Some(*points);
                    news.trade = true;
                }
            }

            // The player's spells.
            WorldEvent::Spell(update) => self.spell(update, now, news),
            WorldEvent::CastPending {
                session_id,
                spell_id,
            } => self.cast_pending(*session_id, *spell_id, news),
            WorldEvent::CastRejected {
                session_id,
                spell_id,
                reason,
            } => self.cast_refused((*session_id, *spell_id), reason, news),
            WorldEvent::BuffSnapshot(buffs) => self.buff_snapshot(buffs),
            WorldEvent::Buff(update) => self.buff(update, news),
            WorldEvent::SpellEffect(effect) => self.spell_effect(effect, spells, news),
            WorldEvent::SpellBook(book) => self.spell_book = Some(book.clone()),
            WorldEvent::BookAction(status) => self.book_action_news(status),
        }
        changes
    }

    /// A death: a spawn becomes a corpse, and the player's own death holds
    /// them until the server offers to return them home.
    fn died(&mut self, death: &Death) -> Changes {
        // Who died and who killed them, named before the corpse takes its
        // name; a death of no one the player knows says nothing.
        let slain = match self.party(death.spawn_id) {
            Party::Unseen => None,
            victim => Some(Notice::Slain {
                victim,
                killer: self.party(death.killer_id),
            }),
        };
        if let Ok(id) = u16::try_from(death.spawn_id) {
            self.zone
                .corpse(id, death.corpse_name.as_deref(), &mut self.revision);
        }
        let mut changes = if self
            .player
            .as_ref()
            .is_none_or(|player| u32::from(player.spawn_id) != death.spawn_id)
        {
            Changes::default()
        } else {
            self.own_health(0);
            self.death = Some(death.clone());
            // The server closes a window the player dies with.
            self.zone.trade.exchange = None;
            self.reset(Reset::Died)
        };
        changes.notices.splice(0..0, slain);
        changes
    }

    /// Who a spawn ID names in a death: the player, a spawn by the name
    /// players see, or no one the player can name.
    fn party(&self, spawn_id: u32) -> Party {
        if self
            .player
            .as_ref()
            .is_some_and(|player| u32::from(player.spawn_id) == spawn_id)
        {
            return Party::Player;
        }
        u16::try_from(spawn_id)
            .ok()
            .filter(|id| *id != 0)
            .and_then(|id| self.zone.name(id))
            .map_or(Party::Unseen, |name| {
                Party::Named(crate::entities::display_name(&name))
            })
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
                self.wallet = crate::money::Wallet::default();
                self.ability_timers.clear();
                self.offered_abilities = None;
                self.nourishment = None;
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
        self.resurrection = None;
        self.reading = None;
        self.combining = None;
        self.group = None;
        self.group_invitation = None;
        self.joining_group = false;
        self.assisted = None;
        self.raid = None;
        self.raid_invitation = None;
        self.raid_flow = raid::RaidFlow::default();
        self.death = None;
        self.forget_zone();
    }

    /// Forgets the zone's contents: its spawns, doors and objects, and the
    /// corpse and merchant, which were the zone's.
    fn forget_zone(&mut self) {
        self.zone = zone::Zone::default();
    }
}

#[cfg(test)]
mod tests;
