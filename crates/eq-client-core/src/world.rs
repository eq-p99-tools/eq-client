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
mod control;
mod group;
mod inventory_moves;
mod items;
mod landings;
mod link;
mod motions;
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
pub use landings::Landing;
pub use link::Link;
pub use motions::{CastDone, Motion};
pub use notice::{ListingNotice, Notice, Party};
pub use raid::{Raid, RaidNotice, RaidRank};
pub use target::Target;
pub use trade::{Asker, Exchange, Loot, LootContinuation, Merchant};
pub use vitals::{ReportedHp, Vitals};
pub use who::ZonePlayer;

use crate::{
    BookActionStatus, CharacterChoice, Death, PlayerState, PostureState, SpawnState, SpellBook,
    WorldEvent, WorldUpdate, ZoneOffer, buffs::BuffTracker, inventory::Inventory,
};
use std::{collections::BTreeMap, time::Instant};

/// Own lifecycle knowledge; bind-only knowledge never manufactures packet details.
enum OwnDeath {
    Reported { details: Death, since: Instant },
    BindTransfer { since: Instant },
}

/// The characters the world server offered to play.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterList {
    /// Process-monotonic world-connection generation supplied by the session.
    pub connection_id: u64,
    /// Names this offer, which a choice must repeat.
    pub selection_id: u64,
    /// The characters, in their server slots.
    pub characters: Vec<CharacterChoice>,
    /// Creation policy supplied by the session with this list.
    pub creation: Option<crate::creation::Rules>,
    /// Native deletion capability, scoped to this list and connection.
    pub deletion: Option<crate::deletion::Rules>,
}

impl CharacterList {
    /// Exact scope required by a named deletion confirmation.
    #[must_use]
    pub const fn scope(&self) -> crate::deletion::Scope {
        crate::deletion::Scope {
            connection_id: self.connection_id,
            selection_id: self.selection_id,
        }
    }
}

/// The world's last answer to a character the player asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Creation {
    /// Counts the answers, so that two alike still differ.
    pub count: u64,
    /// The name asked for.
    pub name: String,
    /// How the world took it.
    pub outcome: crate::creation::CreationOutcome,
}

/// A generation request's answer, scoped to its character list and editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameSuggestion {
    /// Character list identity.
    pub selection_id: u64,
    /// Frontend request identity.
    pub request_id: u64,
    /// Valid suggestion, or no suggestion on failure.
    pub name: Option<String>,
}

/// The worlds the login server listed for the player to choose from.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerList {
    /// Names this list, which a choice must repeat.
    pub selection_id: u64,
    /// The worlds, in the login server's order.
    pub servers: Vec<crate::servers::ServerChoice>,
    /// The world the player asked to play on, by its place in the list,
    /// until the login server answers.
    pub asked: Option<usize>,
    /// Why the login server refused the world last asked for; the player
    /// may choose again.
    pub refused: Option<crate::servers::ServerRefusal>,
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
    /// When the server last said where the spawn is, from which the client
    /// carries it on.
    pub reported: Instant,
}

impl Spawn {
    /// Where the client generation has the spawn at `now`: carried on from
    /// its latest report as its program carries it
    /// ([`crate::entities::carried`]), from when that report came, however
    /// late the spawn is drawn.
    #[must_use]
    pub fn carried(&self, dialect: crate::GameDialect, now: Instant) -> crate::WorldPosition {
        crate::entities::carried(
            dialect,
            &self.state,
            now.saturating_duration_since(self.reported),
        )
    }
}

/// Camping under way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Camp {
    /// When the player began camping.
    pub since: Instant,
    /// Whether the logout itself has begun.
    pub logging_out: bool,
    /// Where it ends.
    pub to: crate::camp::CampTo,
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
    /// What the player turned on of what a session may leave to them, as
    /// their options say; it outlasts every connection.
    chosen: Vec<crate::Capability>,
    /// The installed client's generation, whose program carries spawns on
    /// between reports; it outlasts every connection.
    generation: crate::GameDialect,

    // The connection to the servers.
    connected: bool,
    ended: bool,
    world_name: Option<String>,
    /// The time in Norrath the server last gave, and when it came.
    time: Option<(crate::clock::GameTime, Instant)>,
    /// Guild names by number, from the world's guild list.
    guild_names: BTreeMap<u32, String>,
    /// The login server's worlds, until the chosen world lists characters.
    servers: Option<ServerList>,
    characters: Option<CharacterList>,
    /// The world's answers to the player's character creations, counted, and
    /// the last of them.
    creation: Option<Creation>,
    name_suggestion: Option<NameSuggestion>,
    name_generator: Option<bool>,
    /// The world's expansion mask, when it reported one.
    expansions: Option<crate::rules::Expansions>,
    /// The last revision given to a spawn, which never repeats.
    revision: u64,
    /// The last count given to a spawn's motion, which never repeats.
    motion_count: u64,
    /// Counts the session's spellbook replies, so that two alike still differ.
    book_action_revision: u64,
    /// Revisions of independently projected lists, retained across restarts.
    skills_revision: u64,
    training_revision: u64,
    merchant_revision: u64,

    // The admission: from entering a zone until the next admission.
    session_id: Option<u64>,
    /// The session's interpreted stun state and latest revision; None is
    /// unknown/unconfigured, not proof of clear control.
    own_stun: Option<(u64, crate::control::StunState)>,
    /// Latest local worker policy feedback, never a server movement ACK.
    movement_options_result: Option<(u64, Result<crate::movement::MovementOptions, String>)>,
    /// What the admission lets the player do.
    capabilities: Vec<crate::Capability>,
    /// What the admission leaves to the player, which they may do once they
    /// turn it on.
    choices: Vec<crate::Capability>,
    zone_name: String,
    far_clip: Option<f32>,
    /// The zone header's minimum clip, once this admission's header came.
    min_clip: Option<f32>,
    /// The world's player-versus-player ruleset, by the server's number, as
    /// its rules last said: 0 until they say. The Titanium program keeps it
    /// from its start, whatever zone or character follows (0x4a4cd5).
    pvp_rule: u32,
    /// The received world setting choosing language zero's installed name.
    /// Like the world's other rules it survives zone/character admission,
    /// and a new session's restart clears it to unknown.
    language_zero_selector: Option<crate::rules::LanguageZeroSelector>,
    /// The zone header's gravity, once this admission's header came.
    zone_gravity: Option<f32>,
    /// The zone header's top (`max_z`) and lowest height (its underworld),
    /// once this admission's header came.
    zone_heights: Option<crate::ZoneHeights>,
    /// The zone header's classic model scale flag, as it came; None until
    /// this admission's header comes, and where the generation has none.
    classic_model_scale_flag: Option<u32>,
    /// The spell action flag that says a spell landed on its target, as the
    /// admission says for its server type; None records no landings.
    spell_landing_flag: Option<u8>,
    death: Option<OwnDeath>,
    /// The HP at or below which the server takes the player as dead, where
    /// it leaves that death to the client's own report; the session names
    /// it only where it can count what the player's items add.
    death_threshold: Option<i32>,
    /// The player's HP, with what their items add, when the server's last
    /// report said they bled out, until a front end reports it.
    bled_out: Option<i64>,
    pending_transfer: Option<ZoneOffer>,
    /// The resurrection offered and not yet answered.
    resurrection: Option<crate::resurrection::ResurrectionOffer>,
    /// The translocate offered and not yet answered, and when it came.
    translocate: Option<(crate::confirmations::TranslocateOffer, Instant)>,
    /// The book or note open to read.
    reading: Option<crate::books::BookText>,
    /// The tradeskill container whose combine waits for the server.
    combining: Option<crate::inventory::InventorySlot>,
    /// The player's group, while they are in one. The server lists it again
    /// after each admission.
    group: Option<Group>,
    /// Who invited the player to their group, until the player answers or
    /// is in one; the session forgets it with the admission.
    group_invitation: Option<(String, crate::group::InvitationId)>,
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
    /// Host choices never reuse a correlation ID after a local reset.
    target_sequence: u64,
    motion: Option<MotionGrant>,
    speed_input_revision: Option<u64>,
    accepted_speed_inputs: Option<crate::SpeedInputAcceptance>,
    camp: Option<Camp>,
    /// Where the camp the player last asked for ends, until the session
    /// says it began, or it ends.
    camp_to: crate::camp::CampTo,
    /// Where the last camp ended, until a front end takes it: one to the
    /// desktop ends the program.
    camped: Option<crate::camp::CampTo>,

    // The character: from admission until camping.
    player: Option<PlayerState>,
    vitals: Vitals,
    inventory: Inventory,
    inventory_moves: inventory_moves::Moves,
    /// Where the player's coins are, as the session keeps and tells them.
    wallet: crate::money::Wallet,
    /// Cursor receipt is independent of the bank balance, which is not carried.
    known_cursor_coins: Option<crate::Coins>,
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
            WorldUpdate::Chat(_) | WorldUpdate::SpellTable(_) => Changes::default(),
        }
    }

    /// Runs the world's clocks: doors the server leaves open swing shut,
    /// refreshed gems start their timers once the spells' timing is known,
    /// and casts that nothing ended run out.
    pub fn tick(&mut self, now: Instant, spells: &dyn SpellCatalog) {
        self.zone.doors.close_due(now);
        self.casting.cooldowns.resolve(spells, now);
        self.expire_casts(now);
    }

    /// The player's choice, from their options, of what a session may leave
    /// to them to turn on.
    pub fn choose(&mut self, chosen: Vec<crate::Capability>) {
        self.chosen = chosen;
    }

    /// The installed client's generation, whose program carries spawns on
    /// between reports.
    pub const fn carry_as(&mut self, generation: crate::GameDialect) {
        self.generation = generation;
    }

    /// The generation whose program carries spawns on, as
    /// [`ClientWorld::carry_as`] set it.
    #[must_use]
    pub const fn generation(&self) -> crate::GameDialect {
        self.generation
    }

    /// The player asked to play on a world of the login server's list, by
    /// its place; the list waits for the login server's answer.
    pub fn ask_server(&mut self, index: usize) {
        if let Some(list) = self.servers.as_mut() {
            list.asked = Some(index);
            list.refused = None;
        }
    }

    /// Forgets everything the servers said, for a new session: only the
    /// player's own choices stay, and the revisions go on from where they
    /// were, so nothing drawn for the old session matches the new one's.
    pub fn restart(&mut self) {
        *self = Self {
            chosen: std::mem::take(&mut self.chosen),
            generation: self.generation,
            revision: self.revision,
            motion_count: self.motion_count,
            book_action_revision: self.book_action_revision,
            skills_revision: self.skills_revision.wrapping_add(1),
            training_revision: self.training_revision.wrapping_add(1),
            merchant_revision: self.merchant_revision.wrapping_add(1),
            ..Self::default()
        };
    }

    /// The player asked the session to camp, ending there. The camp the
    /// session then says began ends there; one already under way keeps
    /// its own end.
    pub fn camp_to(&mut self, to: crate::camp::CampTo) {
        self.camp_to = to;
    }

    /// Whether the player camped to the desktop, once: a front end ends the
    /// program when it is.
    pub fn take_leaving(&mut self) -> bool {
        self.camped.take() == Some(crate::camp::CampTo::Desktop)
    }

    /// Whether the player camped to the desktop and the program is to end.
    #[must_use]
    pub fn leaving(&self) -> bool {
        self.camped == Some(crate::camp::CampTo::Desktop)
    }

    /// The player's own choice of target, which the client tells the session
    /// separately; what the session makes of it arrives as news.
    pub fn select_target(&mut self, spawn: Option<u16>) {
        self.target_sequence = self.next_target_request();
        self.target = Target {
            selected: spawn,
            sent: false,
            revision: spawn
                .and_then(|id| self.zone.spawns.get(&id))
                .map(|spawn| spawn.revision),
            server_selected: false,
            request_id: self.target_sequence,
            reported_request: self.target.reported_request,
            authority: self.target.authority,
            accepts_updates: self.target.accepts_updates,
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

    /// The player's HP, with what their items add, when the server's last
    /// report said they bled out, taken once: a front end reports it to the
    /// session (`ClientCommand::BledOut`).
    pub fn take_bled_out(&mut self) -> Option<i64> {
        self.bled_out.take()
    }

    /// The player answered the resurrection offered; whatever comes of it
    /// arrives as news.
    pub fn answer_resurrection(&mut self) {
        self.resurrection = None;
    }

    /// The player, or the client for them, answered the translocate
    /// offered; whatever comes of it arrives as news.
    pub fn answer_translocate(&mut self) {
        self.translocate = None;
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

    /// The player is done trading.
    pub fn close_shop(&mut self) {
        if self.zone.trade.merchant.take().is_some() {
            self.merchant_revision = self.merchant_revision.wrapping_add(1);
        }
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
            WorldEvent::ZoneDescription(description) => self.described(description),
            WorldEvent::ServerSelection {
                selection_id,
                servers,
            } => self.servers_offered(*selection_id, servers, news),
            WorldEvent::ServerRefused {
                selection_id,
                refusal,
            } => self.server_refused(*selection_id, refusal, news),
            WorldEvent::CharacterSelection {
                connection_id,
                selection_id,
                characters,
                creation,
                deletion,
            } => self.offered((*connection_id, *selection_id), characters, (*creation, *deletion), news),
            WorldEvent::CharacterDeletion(update) => news.replies.push(Reply::CharacterDeletion(update.clone())),
            WorldEvent::Entered {
                capabilities,
                choices,
                session_id,
                zone,
                player,
                far_clip,
                spell_landing_flag,
            } => {
                return self.entered(
                    (capabilities, choices, *session_id),
                    (zone, *far_clip, *spell_landing_flag),
                    player,
                    now,
                );
            }
            WorldEvent::SpeedInputsRequired { session_id, revision } => {
                if self.session_id == Some(*session_id) {
                    self.speed_input_revision = Some(*revision);
                    self.accepted_speed_inputs = None;
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::MovementOptionsApplied { session_id, request_id, options } => {
                if self.accepts_reply(*session_id) {
                    self.movement_options_result = Some((*request_id, Ok(*options)));
                } else { news.ignored = true; }
            }
            WorldEvent::MovementOptionsRefused { session_id, request_id, reason } => {
                if self.accepts_reply(*session_id) {
                    self.movement_options_result = Some((*request_id, Err(reason.clone())));
                } else { news.ignored = true; }
            }
            WorldEvent::SpeedInputsAccepted(accepted) => {
                if self.in_world()
                    && self.session_id == Some(accepted.session_id)
                    && self.speed_input_revision == Some(accepted.revision)
                {
                    self.accepted_speed_inputs = Some(*accepted);
                } else {
                    news.ignored = true;
                }
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
            WorldEvent::OwnStun { session_id, revision, state } =>
                self.project_stun(*session_id, *revision, *state, news),
            WorldEvent::MotionSent {
                session_id,
                position,
                refused,
            } => self.motion_sent(*session_id, *position, refused.as_ref(), news),
            WorldEvent::Death(death) => return self.died(death, now),
            WorldEvent::DeathBind { session_id } => {
                if self.session_id != Some(*session_id) {
                    return Changes { ignored: true, ..Changes::default() };
                }
                if self.death.is_some() { return Changes::default(); }
                self.death = Some(OwnDeath::BindTransfer { since: now });
                self.own_health(0);
                self.bled_out = None;
                self.zone.trade.exchange = None;
                let mut changes = self.reset(Reset::Died);
                changes.notices.push(Notice::Slain { victim: Party::Player, killer: Party::Unseen });
                return changes;
            }
            WorldEvent::DeathThreshold(threshold) => self.death_threshold = Some(*threshold),
            WorldEvent::ZoneTransfer(offer) => return self.transfer(offer),
            WorldEvent::ZoneTransferRejected { session_id, reason } => {
                self.transfer_refused(*session_id, *reason, news);
            }
            WorldEvent::ZoneLineRejected { session_id, reason } => {
                self.zone_line_refused(*session_id, reason, news);
            }
            WorldEvent::Camp(status) => return self.camp_news(status, now),
            // A sound the server names, for the front end to play.
            WorldEvent::Effect(crate::effects::Effect::Sound(cue)) => news.sounds.push(cue.clone()),

            // The world's answer to a character the player asked for.
            WorldEvent::CharacterCreation { name, outcome } => {
                self.created(name, *outcome, news);
            }

            // The world's own rules stay with it across admissions.
            WorldEvent::Rules(rules) => {
                self.pvp_rule = rules.pvp;
                if let Some(selector) = rules.language_zero_selector
                    && self.language_zero_selector != Some(selector)
                {
                    self.language_zero_selector = Some(selector);
                    self.skills_revision = self.skills_revision.wrapping_add(1);
                }
                if let Some(enabled) = rules.name_generator { self.name_generator = Some(enabled); }
            }
            WorldEvent::NameSuggestion { selection_id, request_id, name } => {
                self.name_suggestion = Some(NameSuggestion { selection_id: *selection_id, request_id: *request_id, name: name.clone() });
            }
            WorldEvent::Expansions(expansions) => self.expansions = Some(*expansions),

            // Nothing here shows the world's other rules,
            // its chat server, a zone it says is down, the zone's weather or
            // its header beyond the sky and the clip yet, nor applies
            // charm or other server control, spins players or shows their
            // hiding's end. Berserk remains informational; raw stun durations
            // are interpreted by the session's OwnStun. The target
            // the server sets and whom it targets, a quest's reward and ding
            // (the session puts a reward's coins in the purse), a sacrifice
            // offer and a player becoming a corpse wait for their front ends
            // too; until one answers, an offer goes unanswered, as it did
            // unread. A translocate refused is only the session's log. A quest's change to the stats, a
            // skill's button ready again, the reuse timers of disciplines and
            // of spells that share one, and the discipline begun or ended
            // are not shown yet either, nor the effects the server plays
            // besides a sound: a spell's nimbus, particles, a camera shake and
            // a projectile. The official client's sound for a quest's ding is
            // not known, so it stays silent (inferred, item 6.273). A popup or
            // marquee, a yell for help, a voice macro, a request to inspect the
            // player and a duel wait for their front ends too; until one
            // answers, a popup, an inspection and a challenge go unanswered, as
            // they did unread. The guild's members and their changes, and an
            // invitation to join or rise in it, wait for a guild window; until
            // one answers, an invitation goes unanswered. Newly earned titles
            // wait for a window to choose one from, and what a game master did
            // to a player, or the petitions in the queue, for a front end that
            // shows them. The server's regeneration tick waits for its own.
            // Installed spell providers consume table facts before admission staging.
            WorldEvent::SpellTable(_)
            | WorldEvent::ChatServer(_)
            | WorldEvent::ZoneUnavailable { .. }
            | WorldEvent::Weather(_)
            | WorldEvent::ServerTick(_)
            | WorldEvent::Stun(_)
            | WorldEvent::Charm(_)
            | WorldEvent::Controlled(_)
            | WorldEvent::Berserk(_)
            | WorldEvent::Spun
            | WorldEvent::HideAndSneakEnded
            | WorldEvent::Target(_)
            | WorldEvent::TargetOfTarget(_)
            | WorldEvent::Reward(_)
            | WorldEvent::Ding
            | WorldEvent::SacrificeOffer(_)
            | WorldEvent::SacrificeRefused { .. }
            | WorldEvent::TranslocateRefused { .. }
            | WorldEvent::BecameCorpse { .. }
            | WorldEvent::StatsChanged(_)
            | WorldEvent::SkillReset(_)
            | WorldEvent::DisciplineReuse(_)
            | WorldEvent::LinkedReuse(_)
            | WorldEvent::Discipline(_)
            | WorldEvent::Effect(_)
            | WorldEvent::Popup(_)
            | WorldEvent::Marquee(_)
            | WorldEvent::YellForHelp { .. }
            | WorldEvent::VoiceMacro(_)
            | WorldEvent::InspectRequest { .. }
            | WorldEvent::Duel(_)
            | WorldEvent::Guild(_)
            | WorldEvent::NewTitles
            | WorldEvent::Gm(_)
            // The machine's weekly account left for the world; the session
            // host keeps the date it went (the installed client's own
            // `LastSysInfoDate` stamp). Nothing about the world changed.
            | WorldEvent::SystemInfoReported { .. } => (),

            // The zone and what is in it.
            WorldEvent::Spawns(spawns) => self.zone.appear(spawns, &mut self.revision, now),
            WorldEvent::Despawn(id) => self.zone.vanish(*id),
            WorldEvent::Consideration(consideration) => {
                news.notices.push(self.zone.consider(consideration));
            }
            WorldEvent::LootRequest {
                session_id,
                created,
                request,
                status,
            } => self.loot_request_result(*session_id, *created, *request, status, news),
            WorldEvent::CombatRequest {
                session_id, status, ..
            } => {
                if self.session_id != Some(*session_id) {
                    news.ignored = true;
                } else if let crate::combat::CombatRequestStatus::Refused(reason) = status {
                    news.notices.push(Notice::CombatRefused(reason.clone()));
                }
                // Submission does not change authoritative world state. The
                // renderer's toggle remains a local intention; server results
                // arrive independently as consideration, damage and animation.
            }
            WorldEvent::Damage(damage) => self.damage(damage, news),
            WorldEvent::Animation(animation) => self.motion_news(*animation, news),
            WorldEvent::TargetChanged { session_id, request_id, state } => {
                self.target_updated(*session_id, *request_id, *state, true, news);
            }
            WorldEvent::TargetSent { session_id, request_id, state } => {
                self.target_updated(*session_id, *request_id, *state, false, news);
            }
            WorldEvent::TargetRejected {
                session_id,
                request_id,
                spawn_id,
                state,
                reason,
            } => self.target_refused((*session_id, *request_id, *spawn_id), *state, reason, news),
            WorldEvent::Visibility {
                spawn_id,
                invisible,
            } => self.zone.visibility(*spawn_id, *invisible),
            // How a spawn shows, its name and where a corpse lies are kept
            // in the zone's table, as the session keeps them; a spawn already
            // drawn shows an illusion or a new size when it is drawn anew. A
            // face change that names no one changes no one.
            WorldEvent::Appearance { spawn_id, change } => {
                if let Some(spawn) = self.zone.spawns.get_mut(spawn_id) {
                    spawn.state.apply_appearance(change);
                }
                if self.is_player(*spawn_id)
                    && let Some(player) = self.player.as_mut()
                {
                    player.appearance.apply_update(change);
                }
            }
            WorldEvent::FaceChanged {
                spawn_id, features, ..
            } => self.zone.features(*spawn_id, *features),
            WorldEvent::Renamed {
                spawn_id, new_name, ..
            } => self.zone.rename(*spawn_id, new_name),
            WorldEvent::CorpseMoved { spawn_id, position } => {
                self.zone.corpse_moved(*spawn_id, *position);
            }
            WorldEvent::TitleChanged { spawn_id, change } => self.retitle(*spawn_id, change),
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
            // The client's line as the player goes over what they can carry
            // or comes back under it, once each way.
            WorldEvent::Encumbrance {
                session_id,
                string_id,
                ..
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::ServerString {
                        id: *string_id,
                        arguments: Vec::new(),
                        message_type: None,
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
                forward,
            } => self.position(*spawn_id, (*position, *velocity, *forward), now, news),
            WorldEvent::WearChange(change) => self.wear(change),
            WorldEvent::Doors(update) => self.zone.doors.apply_for(update, now, self.generation),
            WorldEvent::DoorAction {
                session_id,
                door_id,
                error,
                string_id,
            } => self.door_used((*session_id, *door_id), (error.as_ref(), *string_id), news),
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
            WorldEvent::Disciplines(list) => self.disciplines_news(list, news),
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
                items,
            } => self.hp_news(
                *spawn_id,
                ReportedHp {
                    current: *current,
                    maximum: *maximum,
                    items: *items,
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
                sequence,
                update,
                error,
            } => self.inventory_move_result(
                (*session_id, *revision, *sequence), update.as_ref(), error.as_ref(), news,
            ),
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
            WorldEvent::PurseUncertain { session_id } => {
                if self.accepts_reply(*session_id) {
                    self.wallet.purse = None;
                    news.trade = true;
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::CoinsElsewhere {
                cursor,
                bank,
                given,
                offered,
            } => {
                self.known_cursor_coins = Some(*cursor);
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
            WorldEvent::MerchantUnanswered { session_id, reason } => {
                self.merchant_unanswered(*session_id, reason, news);
            }
            WorldEvent::Exchange(update) => self.exchange_news(update, news),
            WorldEvent::ExchangeRefused { session_id, reason } => {
                self.exchange_refused(*session_id, reason, news);
            }

            // The player's abilities.
            WorldEvent::AbilitiesOffered(offered) => {
                self.offered_abilities = Some(offered.clone());
            }
            WorldEvent::SkillReady(skill) => self.skill_ready(*skill),
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
            WorldEvent::BookText(text) => {
                if self.in_world()
                    && (self.generation == crate::GameDialect::EqMac || text.kind <= 1)
                {
                    self.reading = Some(text.clone());
                } else {
                    news.ignored = true;
                }
            }
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
            // A translocate to answer, where the installed client asks.
            WorldEvent::TranslocateOffer(offer) => self.translocate = Some((offer.clone(), now)),
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
            WorldEvent::BuffSnapshot(buffs) => self.buff_snapshot(buffs, now),
            WorldEvent::Buff(update) => self.buff(update, now, news),
            WorldEvent::SpellEffect(effect) => self.spell_effect(effect, (now, spells), news),
            WorldEvent::SpellBook(book) => self.spell_book = Some(book.clone()),
            WorldEvent::BookAction(status) => self.book_action_news(status),
        }
        changes
    }

    /// A death: a spawn becomes a corpse, and the player's own death holds
    /// them until the server offers to return them home.
    fn died(&mut self, death: &Death, now: Instant) -> Changes {
        let killed = u16::try_from(death.spawn_id).ok().filter(|id| {
            self.player
                .as_ref()
                .is_some_and(|player| u32::from(player.spawn_id) == death.killer_id)
                && self
                    .zone
                    .spawns
                    .get(id)
                    .is_some_and(|spawn| spawn.state.kind == crate::SpawnKind::Npc)
        });
        let own = self
            .player
            .as_ref()
            .is_some_and(|player| u32::from(player.spawn_id) == death.spawn_id);
        let already_dead = own && self.death.is_some();
        // Who died and who killed them, named before the corpse takes its
        // name; a death of no one the player knows says nothing.
        let slain = if already_dead {
            None
        } else {
            match self.party(death.spawn_id) {
                Party::Unseen => None,
                victim => Some(Notice::Slain {
                    victim,
                    killer: self.party(death.killer_id),
                }),
            }
        };
        if let Ok(id) = u16::try_from(death.spawn_id) {
            self.zone.corpse(
                id,
                death.corpse_name.as_deref(),
                (self.generation, now),
                &mut self.revision,
            );
            self.zone.casts.remove(&id);
        }
        let mut changes = if self
            .player
            .as_ref()
            .is_none_or(|player| u32::from(player.spawn_id) != death.spawn_id)
        {
            Changes::default()
        } else {
            self.own_health(0);
            self.death = Some(OwnDeath::Reported {
                details: death.clone(),
                since: self.death_since().unwrap_or(now),
            });
            self.bled_out = None;
            // The server closes a window the player dies with.
            self.zone.trade.exchange = None;
            if already_dead {
                Changes::default()
            } else {
                self.reset(Reset::Died)
            }
        };
        changes.notices.splice(0..0, slain);
        changes.kills.extend(killed);
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
        self.movement_options_result = None;
        if matches!(
            reason,
            Reset::Entered | Reset::Camped | Reset::Lost { ended: true, .. }
        ) {
            self.inventory_moves.clear();
        } else {
            self.inventory_moves.interrupt();
        }
        self.accepted_speed_inputs = None;
        // A refused transfer resumes this same admission. Retain its revision
        // while suspended so later input changes can still be reported.
        if !matches!(
            reason,
            Reset::Zoning { .. }
                | Reset::Lost {
                    ended: false,
                    transferring: true
                }
        ) {
            self.speed_input_revision = None;
        }
        // Reset invalidates projections even when it retains part of the old world.
        self.skills_revision = self.skills_revision.wrapping_add(1);
        self.training_revision = self.training_revision.wrapping_add(1);
        self.merchant_revision = self.merchant_revision.wrapping_add(1);
        // Whatever the reason, the player can no longer act on their target
        // or move until the session says so again, and only a death leaves
        // camping under way.
        self.target = Target::default();
        self.motion = None;
        self.reading = None;
        if reason != Reset::Died {
            self.camp = None;
            self.camp_to = crate::camp::CampTo::default();
        }
        match reason {
            Reset::Entered => {
                self.forget_admission();
                self.wallet = crate::money::Wallet::default();
                self.known_cursor_coins = None;
                self.nourishment = None;
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
                self.known_cursor_coins = None;
                self.ability_timers.clear();
                self.offered_abilities = None;
                self.nourishment = None;
                self.spell_book = None;
                self.buffs.clear();
                self.session_id = None;
                self.capabilities.clear();
                self.choices.clear();
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
                    self.servers = None;
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
        self.translocate = None;
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
        self.own_stun = None;
        self.death_threshold = None;
        self.bled_out = None;
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
