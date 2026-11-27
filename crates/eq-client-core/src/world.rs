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
mod gem_timing;
mod group;
mod guild;
mod held_models;
mod inventory_moves;
mod items;
mod landings;
mod link;
mod melody;
mod motions;
mod notice;
mod pet_buffs;
mod raid;
mod read;
mod spells;
mod target;
mod tracking;
mod trade;
mod vitals;
mod who;
mod zone;

pub use casting::{CastNews, Casting, Cooldowns, NoSpells, SpellCatalog, SpellTiming};
pub use changes::{Changes, Moved, Reply, Reset};
pub use group::{Group, GroupNotice};
pub use guild::GuildInvitation;
pub use held_models::HeldModelEstimate;
pub use items::ItemCache;
pub use landings::Landing;
pub use link::Link;
pub use melody::{Melody, melody_gems};
pub use motions::{CastDone, Motion};
pub use notice::{
    ChannelLine, DuelNotice, InterruptPrint, ListingNotice, Notice, OtherCaster, Party, YellBearing,
};
pub use raid::{Raid, RaidNotice, RaidRank};
pub use target::Target;
pub use tracking::{TrackWindow, track_color};
pub use trade::{Asker, Exchange, Loot, LootContinuation, Merchant};
pub use vitals::{HpGauge, ReportedHp, Vitals};
pub use who::{GuildStatus, ZonePlayer};

use crate::{
    BookActionStatus, CharacterChoice, Death, PlayerState, PostureState, SpawnState, SpellBook,
    WorldEvent, WorldUpdate, ZoneOffer,
    buffs::BuffTracker,
    confirmations::{ConsentClosure, ConsentId, ConsentKind, ConsentOffer, TranslocateOffer},
    inventory::Inventory,
    resurrection::ResurrectionOffer,
};
use std::{collections::BTreeMap, time::Instant};

/// Own lifecycle knowledge; bind-only knowledge never manufactures packet details.
enum OwnDeath {
    Reported { details: Death, since: Instant },
    BindTransfer { since: Instant },
}

/// The posture owner's interpreted control and its optional accepted HP
/// basis. A new server report invalidates the basis without waking anyone.
#[derive(Clone, Copy, Debug)]
struct OwnConsciousness {
    revision: u64,
    unconscious: bool,
    basis: Option<crate::control::HitPointBasis>,
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
    /// Moving sideways, when calibrated apart; the rate running where
    /// [`Self::strafe_walk_units_per_second`] gives the walking one.
    pub strafe_units_per_second: Option<f32>,
    /// Moving sideways with walking selected, where it is slower: Titanium
    /// strafes on the selected base speed.
    pub strafe_walk_units_per_second: Option<f32>,
    /// Backing up in water, in the land's units per second before the
    /// water's share applies; 0 backs up not at all. None backs up in water
    /// as any swim stroke.
    pub swim_backward_units_per_second: Option<f32>,
    /// Swimming as the generation's program moves it: the swimmer's inputs
    /// and the zone's water set, where the session reads them (Titanium).
    pub swim: Option<crate::movement::SwimCalibration>,
    /// Lateral rates when holding longitudinal input in the calibrated ground mode.
    pub diagonal: crate::DiagonalRates,
    /// Whether the session sends falling samples.
    pub falls: bool,
    /// The share of the jump speed a jump starts with: the health and
    /// weight factor that also slows running, 1 when nothing takes from it.
    pub jump_factor: f32,
}

#[derive(Default)]
struct LiquidHazardInputs {
    revision: Option<u64>,
    /// A received own model change can invalidate the admitted head basis.
    own_model_changed: bool,
}

#[derive(Default)]
enum NpcConsider {
    #[default]
    Reported,
    ClassicLevel,
}

/// The chat server link as the session told it.
#[derive(Clone, Copy, Debug, Default)]
struct ChatLinkState {
    /// How many times a link has taken the login, so the autojoin goes once
    /// for each (read R49-12 c, checklist 6.616).
    logins: u64,
    /// Whether a link has taken the login and not ended since.
    up: bool,
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
    /// How many times the player's reported posture has changed, wrapping;
    /// it outlasts every zone, so a reader sees each change even when
    /// several come between two looks.
    player_posture_changes: u32,
    /// How many drowning records aimed at the player the server sent,
    /// wrapping: the program empties the air counter for each while the
    /// player's head is under (read R49-3b), which the breath sampler does.
    drowning_records: u32,

    // The connection to the servers.
    connected: bool,
    ended: bool,
    world_name: Option<String>,
    /// Presentation policy selected by the current connection's worker.
    npc_consider: NpcConsider,
    /// The time in Norrath the server last gave, and when it came.
    time: Option<(crate::clock::GameTime, Instant)>,
    /// Guild names by number, from the world's guild list.
    guild_names: BTreeMap<u32, String>,
    /// The guild's members, retained across a bound ordinary character transfer.
    guild_members: Option<Vec<crate::guild::GuildMember>>,
    guild_membership_revision: u64,
    guild_refusal: (u64, String),
    /// Members added without a public-note field, until a note or full list arrives.
    guild_unknown_notes: std::collections::BTreeSet<String>,
    /// Guild invitations waiting for the player's answer, oldest first.
    guild_invitations: std::collections::VecDeque<guild::GuildInvitation>,
    /// Whether looted coins are shared with the group, once known.
    auto_split: Option<bool>,
    /// `/played`'s fields from the latest Titanium profile, and when it
    /// came (checklist 6.490b).
    played: Option<(crate::played::PlayedTime, Instant)>,
    /// The primary bind point's zone, as this admission's profile said it
    /// (checklist 6.524).
    bound_in: Option<u16>,
    /// The login server's worlds, until the chosen world lists characters.
    servers: Option<ServerList>,
    characters: Option<CharacterList>,
    /// The world's answers to the player's character creations, counted, and
    /// the last of them.
    creation: Option<Creation>,
    name_suggestion: Option<NameSuggestion>,
    name_generator: Option<bool>,
    /// Whether the world turns the petition window on (Titanium's
    /// log-server byte), for [`eq_network_game::reports::petition_form`].
    petition_window: Option<bool>,
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
    liquid_hazards: LiquidHazardInputs,
    alternate_advancement: crate::alternate_advancement::State,
    /// The session's interpreted stun state and latest revision; None is
    /// unknown/unconfigured, not proof of clear control.
    own_stun: Option<(u64, crate::control::StunState)>,
    /// The posture owner's accepted HP basis and admission-scoped control.
    own_unconscious: Option<OwnConsciousness>,
    /// Latest local worker policy feedback, never a server movement ACK.
    movement_options_result: Option<(u64, Result<crate::movement::MovementOptions, String>)>,
    /// What the admission lets the player do.
    capabilities: Vec<crate::Capability>,
    /// What the admission leaves to the player, which they may do once they
    /// turn it on.
    choices: Vec<crate::Capability>,
    zone_name: String,
    /// The zone's number, as the server gave it at entry.
    zone_number: Option<u16>,
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
    /// Whether the world turns on the program's listener garble: Titanium's
    /// log-server word at 16, or `EQMac`'s `gibberish` at the same place
    /// (reads R49-4 (c), R49-4m). Kept like the other rules.
    listener_garble: Option<bool>,
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
    same_zone_relocation: Option<(u64, crate::SameZoneRelocation)>,
    last_local_pad: Option<crate::LocalPadCrossing>,
    pending_local_pad: Option<crate::LocalPadCrossing>,
    /// The resurrection offered and not yet answered.
    resurrection: Option<ConsentOffer<ResurrectionOffer>>,
    /// The translocate offered and not yet answered.
    translocate: Option<ConsentOffer<TranslocateOffer>>,
    /// The challenge to a duel waiting for the player's answer.
    duel: Option<crate::socials::DuelQuestion>,
    /// The session's last reported disposition of a consent prompt.
    last_consent_close: Option<(ConsentId, ConsentClosure)>,
    last_consent_refusal: Option<ConsentId>,
    popup: Option<crate::popups::PopupOffer>,
    popup_serial: u64,
    player_inspection: Option<crate::inspection::PlayerInspection>,
    /// The chat server channels the character is on, in slot order, as the
    /// chat server's last list named them; kept across zones until the link
    /// ends or the character leaves the world.
    chat_channels: Vec<String>,
    /// The chat server link's logins taken, and whether one is up.
    chat_link: ChatLinkState,
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
    /// How many assist answers have named a target, wrapping, and the last
    /// one named: a front end that starts auto attack on an answer
    /// (`AttackOnAssist`, read R49-8 d) sees each once.
    assist_answer: (u32, Option<u16>),
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
    /// Evidence retained separately from taking the local desktop destination.
    camp_completion: Option<crate::camp::Completion>,

    // The character: from admission until camping.
    player: Option<PlayerState>,
    /// `EQMac`'s own spawn level, separate from the character level used by gates.
    displayed_level: Option<u8>,
    vitals: Vitals,
    inventory: Inventory,
    inventory_moves: inventory_moves::Moves,
    held_models: held_models::HeldModels,
    /// Where the player's coins are, as the session keeps and tells them.
    wallet: crate::money::Wallet,
    /// Cursor receipt is independent of the bank balance, which is not carried.
    known_cursor_coins: Option<crate::Coins>,
    /// How fed and watered the player is, as the server last said.
    nourishment: Option<crate::food::Nourishment>,
    casting: Casting,
    buffs: BuffTracker,
    spell_book: Option<SpellBook>,
    /// The spellbook change's latest worker status, until a reset.
    book_action: Option<BookActionStatus>,
    memorization: Option<eq_network_game::spells::MemorizationProgress>,
    /// When each ability timer the session started runs out; servers keep
    /// them across zones.
    ability_timers: std::collections::BTreeMap<crate::abilities::Recovery, Instant>,
    /// The abilities the server type offers, once the session says.
    offered_abilities: Option<Vec<crate::abilities::Ability>>,

    // The zone's contents, the corpse and merchant among them.
    zone: zone::Zone,
}

impl ClientWorld {
    /// This admission's received AA facts, for display and the
    /// environmental effects; nothing in them sends.
    pub const fn alternate_advancement(&self) -> &crate::alternate_advancement::State {
        &self.alternate_advancement
    }

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
            WorldUpdate::Chat(_)
            | WorldUpdate::SpellTable(_)
            | WorldUpdate::CheckingFiles(_)
            | WorldUpdate::MovementPacketSnapshot(_) => Changes::default(),
        }
    }

    /// Runs the world's clocks: doors the server leaves open swing shut,
    /// refreshed gems start their timers once the spells' timing is known,
    /// and casts that nothing ended run out.
    pub fn tick(&mut self, now: Instant, spells: &dyn SpellCatalog) {
        self.zone.doors.close_due(now);
        self.zone.pet_buffs.expire_early(now);
        self.casting.cooldowns.resolve(spells, now);
        if let Some(clocks) = &mut self.casting.gem_clocks {
            clocks.resolve(spells);
        }
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

    /// Selects the classic level scale for NPC consider presentation for
    /// this connection. The received assessment remains available separately.
    pub const fn consider_npcs_by_level(&mut self, enabled: bool) {
        self.npc_consider = if enabled {
            NpcConsider::ClassicLevel
        } else {
            NpcConsider::Reported
        };
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

    /// The player took off in a jump that costs `cost` endurance. The
    /// program takes the cost off the endurance it shows at take-off
    /// (Titanium 0x4acb38), ahead of the server; the server's next report
    /// replaces what is left. Without a report there is nothing to take it
    /// from.
    pub fn spend_endurance(&mut self, cost: u32) {
        if let Some(endurance) = self.vitals.endurance.as_mut() {
            *endurance = endurance.saturating_sub(cost);
        }
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

    /// The last completed camp's scope and evidence, until the next camp or
    /// admission. A local deadline does not confirm server logout or cleanup.
    #[must_use]
    pub fn camp_completion(&self) -> Option<crate::camp::Completion> {
        self.camp_completion
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
            granted: false,
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

    /// How many assist answers have named a target, wrapping, and the
    /// target the last one named. Unlike [`Self::take_assisted`], reading
    /// it takes nothing.
    #[must_use]
    pub const fn assist_answer(&self) -> (u32, Option<u16>) {
        self.assist_answer
    }

    /// The player's HP, with what their items add, when the server's last
    /// report said they bled out, taken once: a front end reports it to the
    /// session (`ClientCommand::BledOut`).
    pub fn take_bled_out(&mut self) -> Option<i64> {
        self.bled_out.take()
    }

    /// Dismisses the exact prompt whose answer the front end queued.
    /// The worker's disposition and the server's result arrive separately.
    pub fn dismiss_consent(&mut self, id: ConsentId) {
        match id.kind {
            ConsentKind::Resurrection => {
                let _ = self.resurrection.take_if(|current| current.id == id);
            }
            ConsentKind::Translocate => {
                let _ = self.translocate.take_if(|current| current.id == id);
            }
            ConsentKind::Duel => {
                let _ = self.duel.take_if(|current| current.id == id);
            }
        }
    }

    /// The titles and suffixes the player may choose from, as the latest
    /// list in this zone named them; none until the server sends one. No
    /// window asks for it yet: the title window is read (R49-14) but waits,
    /// as titles are outside Project 1999 and Quarm (Adam, 11-23).
    #[must_use]
    pub fn titles(&self) -> Option<&[eq_network_game::names::TitleChoice]> {
        self.zone.titles.as_deref()
    }

    /// The player closed the book or note they were reading.
    pub fn close_reading(&mut self) {
        self.reading = None;
    }

    /// The chat server channels the character is on, in slot order: the
    /// first is `/1`.
    pub fn chat_channels(&self) -> &[String] {
        &self.chat_channels
    }

    /// How many times a chat server link has taken the login: a change says
    /// a link has just come up.
    #[must_use]
    pub const fn chat_links_up(&self) -> u64 {
        self.chat_link.logins
    }

    /// Whether a chat server link is up: it has taken the login and not
    /// ended since.
    #[must_use]
    pub const fn chat_link_up(&self) -> bool {
        self.chat_link.up
    }

    /// Puts away the received equipment report.
    pub fn close_player_inspection(&mut self) {
        self.player_inspection = None;
    }

    fn clear_inspected_spawn(&mut self, id: u16) {
        if self
            .player_inspection
            .as_ref()
            .is_some_and(|view| view.report.player_id == id)
        {
            self.close_player_inspection();
        }
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
            self.zone.pet_buffs.accepts_staged_entry = false;
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
        self.zone.pet_buffs.expire_early(now);
        let mut changes = Changes::default();
        let news = &mut changes;
        match event {
            WorldEvent::LiquidHazardRevision { session_id, revision } => {
                if self.session_id == Some(*session_id) {
                    self.liquid_hazards.revision = Some(*revision);
                }
            }
            // A Bard's song stopped, with the program's line for the stop
            // (0x40f06d, read R49-21).
            WorldEvent::SongStopped {
                session_id,
                spell_id,
                string_id,
            } => {
                if self.accepts_reply(*session_id) {
                    // The stop also resets the gems' timers (6.542).
                    if let Some(player) = &self.player {
                        self.casting
                            .cooldowns
                            .song_stopped(&player.memorized_spells);
                        if let Some(clocks) = &mut self.casting.gem_clocks {
                            clocks.song_stopped(spells);
                        }
                    }
                    news.notices.push(Notice::SongStopped {
                        spell_id: *spell_id,
                        string_id: *string_id,
                    });
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::PlayedTime(played) => self.played = Some((*played, now)),
            WorldEvent::HazardReported {
                session_id,
                hazard,
                amount,
                record,
            } => self.hazard_reported(*session_id, *hazard, *amount, *record, news),
            // The connection, the characters on offer and the admission.
            WorldEvent::WorldName { short_name } => {
                if self.world_name.as_deref() != Some(short_name.as_str()) {
                    self.forget_guild();
                }
                self.world_name = Some(short_name.clone());
            }
            // The time of day, and how the zone's sky looks.
            WorldEvent::TimeOfDay(time) => self.time = Some((*time, now)),
            WorldEvent::Sky(sky) => self.zone.sky = Some(*sky),
            WorldEvent::Weather(weather) => {
                // The session forwards this untagged event on its ordered
                // admitted stream. There is no report to carry between zones.
                if self.connected && self.session_id.is_some() && self.pending_transfer.is_none() {
                    self.zone.weather = Some(*weather);
                } else {
                    news.ignored = true;
                }
            }
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
                zone_id,
                player,
                far_clip,
                spell_landing_flag,
            } => {
                return self.entered(
                    (capabilities, choices, *session_id),
                    (zone, *zone_id, *far_clip, *spell_landing_flag),
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
                strafe_walk_units_per_second,
                swim_backward_units_per_second,
                swim,
                diagonal,
                falls,
                jump_factor,
            } => self.grant_motion(
                *session_id,
                MotionGrant {
                    units_per_second: *units_per_second,
                    backward_units_per_second: *backward_units_per_second,
                    walk_units_per_second: *walk_units_per_second,
                    strafe_units_per_second: *strafe_units_per_second,
                    strafe_walk_units_per_second: *strafe_walk_units_per_second,
                    swim_backward_units_per_second: *swim_backward_units_per_second,
                    swim: *swim,
                    diagonal: *diagonal,
                    falls: *falls,
                    jump_factor: *jump_factor,
                },
                news,
            ),
            WorldEvent::OwnStun { session_id, revision, state } =>
                self.project_stun(*session_id, *revision, *state, news),
            WorldEvent::OwnUnconscious { session_id, revision, unconscious, basis } =>
                self.project_unconscious(*session_id, *revision, *unconscious, *basis, news),
            WorldEvent::MotionSent {
                session_id,
                created,
                position,
                refused,
            } => self.motion_sent(*session_id, *created, *position, refused.as_ref(), news),
            WorldEvent::SwimState { session_id, position, created, basis } => {
                if self.connected && self.session_id == Some(*session_id) && self.death.is_none() {
                    news.swim = Some((*position, *created, *basis));
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::MotionRefused { session_id, created, reason } => {
                if self.session_id == Some(*session_id) {
                    news.replies.push(Reply::MotionRefused {
                        created: *created,
                        reason: reason.clone(),
                    });
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::Death(death) => {
                if let Ok(id) = u16::try_from(death.spawn_id) {
                    self.clear_inspected_spawn(id);
                }
                return self.died(death, now);
            }
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
            WorldEvent::SameZoneRelocated(relocation) => {
                self.same_zone_relocated(*relocation, now, news);
            }
            WorldEvent::LocalPadCrossed(crossing) => {
                self.local_pad_crossed(*crossing, now, news);
            }
            WorldEvent::LocalPadCompleted(completed) => {
                self.local_pad_completed(*completed, now, news);
            }
            WorldEvent::LocalPadRefused { session_id, revision, reason } => {
                if self.pending_local_pad.is_some_and(|pad| pad.session_id == *session_id && pad.revision == *revision) {
                    news.local_pad_refused = Some((*revision, reason.clone()));
                    // The host owns the single retry and reports only a final
                    // stop; an initial refusal can still finish successfully.
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::Camp(status) => return self.camp_news(status, now),
            // A sound the server names, for the front end to play.
            WorldEvent::Effect(crate::effects::Effect::Sound(cue)) => news.sounds.push(cue.clone()),

            // The worker sends this untagged update on the ordered zone stream.
            WorldEvent::Marquee(marquee) => {
                if self.in_world() {
                    news.marquee = Some(marquee.clone());
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::PopupOpened(offer) => {
                if self.in_world() && self.accepts_reply(offer.session_id) && offer.serial > self.popup_serial {
                    self.popup_serial = offer.serial;
                    self.popup = Some(offer.clone());
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::PopupClosed { session_id, serial } => {
                if self.accepts_reply(*session_id)
                    && self.popup.as_ref().is_some_and(|offer| offer.serial == *serial)
                {
                    self.popup = None;
                }
            }
            // Another player looked at the player, on the Mac era (line
            // 0x57c, %1 their name; always shown, the program's option for
            // it being unread: 6.621). The words are ours where the
            // installation lacks the line.
            WorldEvent::InspectedBy {
                session_id,
                name,
                string_id,
                ..
            } => {
                if self.accepts_reply(*session_id) {
                    let name = crate::entities::display_name(name);
                    news.notices.push(Notice::ProgramLine {
                        string_id: *string_id,
                        said: format!("{name} inspects you."),
                        arguments: vec![name],
                        lines: Vec::new(),
                    });
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::PlayerInspection(view) => {
                let id = view.report.player_id;
                let target_matches = if self.is_player(id) {
                    self.player().is_some_and(|player| player.name == view.name)
                } else {
                    self.spawn(id).is_some_and(|spawn| {
                        spawn.state.kind == crate::SpawnKind::Player && spawn.state.name == view.name
                    })
                };
                if self.in_world() && self.accepts_reply(view.session_id) && self.is_player(view.report.recipient_id) && target_matches {
                    self.player_inspection = Some(view.clone());
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::PlayerInspectionRefused { session_id, reason } => {
                if self.accepts_reply(*session_id) {
                    news.notices.push(Notice::PlayerInspectionRefused(reason.clone()));
                }
            }

            // Like weather, this untagged update follows the admitted stream.
            WorldEvent::Effect(crate::effects::Effect::Projectile(flight)) => {
                if self.in_world() {
                    news.projectile = Some(flight.clone());
                } else {
                    news.ignored = true;
                }
            }

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
                if let Some(on) = rules.petition_window {
                    self.petition_window = Some(on);
                }
                if let Some(on) = rules.listener_garble.or(rules.gibberish) {
                    self.listener_garble = Some(on);
                }
            }
            WorldEvent::NameSuggestion { selection_id, request_id, name } => {
                self.name_suggestion = Some(NameSuggestion { selection_id: *selection_id, request_id: *request_id, name: name.clone() });
            }
            WorldEvent::Expansions(expansions) => self.expansions = Some(*expansions),

            // Nothing here shows the world's other rules, its chat server,
            // a zone it says is down, or the rest of the zone header. Nor
            // does it apply charm or other server control, spin players or show their
            // hiding's end. Berserk remains informational; raw stun durations
            // are interpreted by the session's OwnStun. The target
            // the server sets and whom it targets, a quest's ding, a
            // sacrifice offer and a player becoming a corpse (pending
            // R49-6e, checklist 6.492) wait for their front ends too; until
            // one answers, an offer goes unanswered, as it did unread. A
            // skill's button ready again is not shown yet either, nor the
            // effects the server plays besides sound and flight: a spell's
            // nimbus, particles and camera shake. The official client's
            // sound for a quest's ding is
            // not known, so it stays silent (inferred, item 6.273). A voice
            // macro waits for its front end.
            // Popups and inspections use the worker's scoped events.
            // The guild's members and their changes, and an
            // invitation to join or rise in it, wait for a guild window; until
            // one answers, an invitation goes unanswered. Newly earned titles
            // wait for a window to choose one from, and what a game master did
            // to a player, or the petitions in the queue, for a front end that
            // shows them. The server's regeneration tick waits for its own.
            // Installed spell providers consume table facts before admission staging.
            unshown @ (WorldEvent::SpellTable(_)
            | WorldEvent::ChatServer(_)
            | WorldEvent::ZoneUnavailable { .. }
            | WorldEvent::ServerTick(_)
            | WorldEvent::Stun(_)
            | WorldEvent::Controlled(_)
            | WorldEvent::Berserk(_)
            | WorldEvent::Spun
            | WorldEvent::HideAndSneakEnded
            | WorldEvent::Target(_)
            | WorldEvent::TargetOfTarget(_)
            | WorldEvent::SacrificeOffer(_)
            | WorldEvent::SacrificeRefused { .. }
            | WorldEvent::BecameCorpse { .. }
            | WorldEvent::SkillReset(_)
            | WorldEvent::Effect(_)
            | WorldEvent::Popup(_)
            | WorldEvent::VoiceMacro(_)
            | WorldEvent::InspectRequest { .. }
            | WorldEvent::InspectAnswer(_)
            | WorldEvent::NewTitles
            | WorldEvent::Gm(_)
            // Pinned past these before their front ends land (repin 6.561);
            // each waits for the client slice named.
            // The Bazaar search: its client slice (6.550).
            | WorldEvent::Bazaar(_)
            | WorldEvent::BazaarRefused { .. }
            // RoF2's buff icons, mana and endurance pools: the RoF2 slices
            // (6.527, 6.538).
            | WorldEvent::BuffIcons(_)
            | WorldEvent::ResourcePercent { .. }
            | WorldEvent::ResourcePool { .. }
            // The bandolier sets and potion belt, unknown sets and the
            // session's refusals: their windows' slice (6.559, 6.573).
            | WorldEvent::Bandoliers(_)
            | WorldEvent::PotionBelt(_)
            | WorldEvent::BandoliersUnknown
            | WorldEvent::BandolierRefused { .. }
            // The task journal, offers and history: the tasks slice (6.560).
            | WorldEvent::TaskDescribed { .. }
            | WorldEvent::TaskStepUpdated { .. }
            | WorldEvent::TaskConcluded { .. }
            | WorldEvent::TaskRemoved { .. }
            | WorldEvent::CompletedTasks { .. }
            | WorldEvent::TasksOffered { .. }
            | WorldEvent::TaskHistory { .. }
            | WorldEvent::TaskRefused { .. }
            // The guild bank as EQEmu tells it: the gap log thread's guild
            // bank slices (6.563).
            | WorldEvent::GuildBank(_)
            // The machine's weekly account left for the world; the session
            // host keeps the date it went (the installed client's own
            // `LastSysInfoDate` stamp). Nothing about the world changed.
            | WorldEvent::SystemInfoReported { .. }
            // A bind spell's landing refused with the program's line (0x69
            // or 0x103, 6.615 slice 2): the client half that says the line
            // is a gap row.
            | WorldEvent::BindRefused { .. }) => {
                // Each still counts in the session's gap file, by kind.
                news.unshown = crate::gaps::variant_name(unshown);
            }
            // The zone and what is in it.
            WorldEvent::BoatControl(packet) => {
                if self.in_world() { self.zone.boat_notice = Some(*packet); }
            }
            WorldEvent::BoatControlState { session_id, state } => {
                if self.accepts_reply(*session_id) { self.zone.boat_control = *state; }
            }
            WorldEvent::Arrival { session_id, arrival } => {
                if self.accepts_reply(*session_id) { self.zone.arrival = Some(arrival.clone()); }
            }
            WorldEvent::Carriers(updates) => { for update in updates { self.zone.carriers.apply(*update); } },
            WorldEvent::Spawns(spawns) => {
                for spawn in spawns {
                    self.clear_inspected_spawn(spawn.spawn_id);
                }
                self.zone.appear(spawns, &mut self.revision, now);
                self.reconcile_pet_buffs();
            }
            WorldEvent::Despawn(id) => {
                if self.target.selected == Some(*id) {
                    // Titanium's DeleteSpawn handler ends attack and clears
                    // exactly the target being removed (0x4581b7, read6.699).
                    self.select_target(None);
                    news.attack_ended = true;
                }
                self.clear_inspected_spawn(*id);
                self.zone.vanish(*id);
                self.reconcile_pet_buffs();
            }
            WorldEvent::Consideration(consideration) => {
                let id = consideration.target_id;
                self.zone.considered.insert(id, consideration.color);
                news.notices.push(Notice::Consideration {
                    consideration: *consideration,
                    name: self.zone.name(id),
                    color: self.consider_color(id),
                });
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
                state,
            } => self.zone.visibility(*spawn_id, *state),
            WorldEvent::ObserverSight { session_id, sees_invisible } => {
                if self.session_id == Some(*session_id) {
                    self.zone.sees_invisible = *sees_invisible;
                }
            }
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
                    use eq_network_game::appearance::AppearanceChange;
                    self.liquid_hazards.own_model_changed |= matches!(
                        change,
                        AppearanceChange::Size(_) | AppearanceChange::Illusion(_)
                    );
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
                    let guild = player.listing.guild;
                    who::relist_own(*change, self.generation, player, &mut self.displayed_level);
                    if guild != player.listing.guild {
                        self.forget_guild();
                    }
                }
            }
            WorldEvent::ListingSet { session_id, change } => {
                self.listing_set(*session_id, *change, news);
            }
            WorldEvent::ListingRequested { session_id, change } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::ListingRequested(*change));
                } else {
                    news.ignored = true;
                }
            }
            // The Options page shows the character's filters; the session
            // keeping them, or refusing a set this client fitted to the
            // generation first, is not news for the player.
            // The network posture owner already lies down and holds movement
            // at zero HP. The unconscious presentation layer owns storing it.
            WorldEvent::ServerFiltersKept { .. }
            | WorldEvent::ServerFiltersRefused { .. }
            | WorldEvent::OwnUnconscious { .. } => {}
            // A report to the staff went or was refused (6.511).
            WorldEvent::ReportSent {
                session_id,
                report,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::ReportSent(*report));
                } else {
                    news.ignored = true;
                }
            }
            // The session's chat pace held a line (6.523).
            WorldEvent::ChatRefused { session_id, reason } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::ChatRefused {
                        reason: reason.clone(),
                    });
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::ReportRefused {
                session_id,
                reason,
                ..
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::ReportRefused {
                        reason: reason.clone(),
                    });
                } else {
                    news.ignored = true;
                }
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
            WorldEvent::Guild(update) => {
                if self.connected && self.session_id.is_some() && self.pending_transfer.is_none() {
                    self.guild_news(update);
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::LastName { name, last_name } => self.last_name(name, last_name),
            // The name tag follows the zone's own last-name update, not this.
            WorldEvent::SurnameAnswer {
                accepted,
                last_name,
            } => match &self.player {
                Some(player) if self.session_id.is_some() => {
                    news.notices.push(Notice::SurnameAnswer {
                        accepted: *accepted,
                        name: player.name.clone(),
                        last_name: last_name.clone(),
                    });
                }
                _ => news.ignored = true,
            },
            // The titles the player may choose: kept, the latest replacing
            // the one before, only where the session offers them, until the
            // zone changes.
            WorldEvent::TitleList(titles) => {
                if self.in_world() && self.can(crate::Capability::Titling) {
                    self.zone.titles = Some(titles.clone());
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::Posture { spawn_id, posture } => {
                let player = self.is_player(*spawn_id);
                // A first standing, as the session reports a player at zone
                // entry, is the posture they enter with (read R24), not a
                // change; any other first posture, and any change of a known
                // one, is. The rule is the same for every generation, so on
                // the Mac-era client, whose entry is unread and whose posture
                // stays unknown after it, a first stand after a zone-in no
                // longer ends autorun as it did before.
                let before = self.zone.player_posture;
                if player
                    && before != Some(*posture)
                    && !(before.is_none() && *posture == PostureState::Standing)
                {
                    self.player_posture_changes = self.player_posture_changes.wrapping_add(1);
                }
                self.zone.posture(*spawn_id, *posture, player);
            }
            WorldEvent::FeignHeld(held) => self.zone.feign_held = *held,
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
                session_id,
                drop_id,
                error,
            } => self.object_refused((*session_id, *drop_id), error.as_ref(), news),

            // The player character.
            WorldEvent::Level {
                current,
                experience,
                ..
            } => self.level_news(*current, *experience, news),
            WorldEvent::Skill { skill_id, value } => self.skill_news(*skill_id, *value, news),
            WorldEvent::Disciplines(list) => self.disciplines_news(list, news),
            WorldEvent::AlternateAdvancement { session_id, updates } => {
                // RoF2 sends no AA facts yet; the session reads Titanium's
                // and EQMac's (checklist 6.565).
                let read = match self.generation {
                    crate::GameDialect::Titanium | crate::GameDialect::EqMac => true,
                    crate::GameDialect::Rof2 => false,
                };
                if read
                    && self.session_id == Some(*session_id)
                    && !self.ended
                    && self.player.is_some()
                {
                    for update in updates {
                        self.alternate_advancement.apply(update);
                    }
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::DisciplineReuse(reuse) => self.casting.discipline_timers.set(*reuse, now),
            WorldEvent::LinkedReuse(reuse) => self.casting.linked_timers.set(*reuse, now),
            WorldEvent::Discipline(discipline) => {
                use crate::disciplines::Active;
                self.casting.active_discipline = discipline.map_or(Active::Inactive, Active::EqMac);
            }
            WorldEvent::DisciplineAdmission { session_id, state } => {
                if self.session_id == Some(*session_id) {
                    self.casting.active_discipline = *state;
                } else {
                    news.ignored = true;
                }
            }
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
                string_id,
            } => self.item_used((*session_id, *request_id), (error.as_ref(), *string_id), news),
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
            WorldEvent::CoinsRefused {
                session_id,
                reason,
                string_id,
            } => self.coins_refused(*session_id, reason, *string_id, news),
            // Whether looted coins are shared: the profile's, and the player's
            // change of it, which the official client always answers with a
            // line (R8).
            WorldEvent::AutoSplit(on) => self.auto_split = Some(*on),
            // The bind point's zone, as each zone-in's profile says it: the
            // server sends no change when a bind lands (inferred, 6.524).
            WorldEvent::BoundIn { zone_id } => self.bound_in = Some(*zone_id),
            WorldEvent::AutoSplitSet { session_id, on } => {
                if self.session_id == Some(*session_id) {
                    self.auto_split = Some(*on);
                    news.notices.push(Notice::AutoSplit(*on));
                } else {
                    news.ignored = true;
                }
            }
            // A banker's change: the purse and the bank as they now stand.
            WorldEvent::CoinsChanged(banker) => {
                self.wallet.purse = Some(banker.purse);
                self.wallet.bank = Some(banker.bank);
                news.trade = true;
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
            WorldEvent::KnightReuse { session_id, remaining, source } => {
                if self.session_id == Some(*session_id) {
                    if let Some(ready) = now.checked_add(*remaining) {
                        self.casting.knight_ready = Some((ready, *source));
                    }
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::AbilityUsed {
                session_id,
                ability,
                ready_in,
                string_id,
            } => self.ability_used((*session_id, *ability, *ready_in, *string_id), now, news),
            WorldEvent::AskAnswered(answer) => Self::ask_answered(answer, news),
            WorldEvent::Tracking(update) => self.tracking_news(update, news),
            WorldEvent::AbilityRefused {
                session_id,
                reason,
                string_id,
                arguments,
            } => self.ability_refused(*session_id, (reason, *string_id, arguments), news),
            WorldEvent::BindWound(update) => Self::bind_wound(update, news),
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
            WorldEvent::Assisted(answer) => {
                self.assisted = answer.target;
                if answer.target.is_some() {
                    self.assist_answer = (self.assist_answer.0.wrapping_add(1), answer.target);
                }
            }
            // The chat server's channels (recording checklist 6.509).
            WorldEvent::Channel(event) => self.channel_event(event, news),
            WorldEvent::ChannelLink(link) => match link {
                eq_network_game::chat_server::ChannelLink::Up => {
                    self.chat_link.logins = self.chat_link.logins.wrapping_add(1);
                    self.chat_link.up = true;
                }
                eq_network_game::chat_server::ChannelLink::Lost { .. } => {
                    self.chat_link.up = false;
                    self.chat_channels.clear();
                    news.notices.push(Notice::Channel(ChannelLine::Lost));
                }
            },
            WorldEvent::ChatChannelRefused { session_id, reason } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::ChannelRefused {
                        reason: reason.clone(),
                    });
                } else {
                    news.ignored = true;
                }
            }
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
                let player = self.player.as_ref().map(|player| player.spawn_id);
                self.zone.pet_owner(*spawn_id, *owner, player);
                self.reconcile_pet_buffs();
            }
            WorldEvent::Charm(charm) => {
                let player = self.player.as_ref().map(|player| player.spawn_id);
                self.zone.pet_charm(*charm, player);
                self.reconcile_pet_buffs();
            }
            WorldEvent::OwnPet { session_id, pet } => {
                if self.session_id == Some(*session_id) {
                    self.zone.set_own_pet(*pet);
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::PetBuffs(buffs) => self.receive_pet_buffs(buffs, now),
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
            WorldEvent::ApplyPoisonRefused {
                session_id,
                reason,
                string_id,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::ApplyPoisonRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                    });
                } else {
                    news.ignored = true;
                }
            }
            // The wait the session runs before it sends (read R49-1).
            WorldEvent::PoisonStarted { session_id, string_id, .. }
            | WorldEvent::PoisonInterrupted {
                session_id,
                string_id,
            } => {
                if self.session_id == Some(*session_id) {
                    let started = matches!(event, WorldEvent::PoisonStarted { .. });
                    news.notices.push(if started {
                        Notice::PoisonStarted {
                            string_id: *string_id,
                        }
                    } else {
                        Notice::PoisonInterrupted {
                            string_id: *string_id,
                        }
                    });
                } else {
                    news.ignored = true;
                }
            }
            // The server's own inventory record takes the poison out.
            WorldEvent::PoisonAnswered(answer) => {
                if self.in_world() {
                    news.notices.push(Notice::PoisonAnswered {
                        applied: answer.applied,
                    });
                } else {
                    news.ignored = true;
                }
            }
            // A book or note to read, and a request the session would not send.
            WorldEvent::BookText(text) => {
                if self.in_world() && text.window(self.generation).is_some() {
                    self.reading = Some(text.clone());
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::AlternateAdvancementRefused {
                session_id,
                reason,
                refusal,
            } => {
                if self.session_id == Some(*session_id) {
                    news.notices.push(Notice::AlternateAdvancementRefused {
                        reason: reason.clone(),
                        refusal: *refusal,
                    });
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
            WorldEvent::GuildRefused {
                session_id,
                reason,
                string_id,
            } => {
                if self.session_id == Some(*session_id) {
                    self.guild_refusal.0 = self.guild_refusal.0.wrapping_add(1);
                    self.guild_refusal.1.clone_from(reason);
                    news.notices.push(Notice::GuildRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                    });
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::Resurrection(offer) => {
                if self.in_world() && self.session_id == Some(offer.id.session_id) {
                    self.resurrection = Some(offer.clone());
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::TranslocateOffer(offer) => {
                if self.in_world() && self.session_id == Some(offer.id.session_id) {
                    self.translocate = Some(offer.clone());
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::DuelOffer(question) => {
                if self.in_world() && self.session_id == Some(question.id.session_id) {
                    self.duel = Some(*question);
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::DuelRefused { session_id, offer_id, reason } => {
                if self.session_id == Some(*session_id) {
                    self.last_consent_refusal = Some(*offer_id);
                    news.notices.push(Notice::Duel(DuelNotice::AnswerRefused(reason.clone())));
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::Duel(update) => self.duel_news(*update, news),
            WorldEvent::DuelNews(told) => self.duel_told(*told, news),
            WorldEvent::YellForHelp { spawn_id } => {
                // A spawn the player cannot name says nothing.
                match self.party(u32::from(*spawn_id)) {
                    Party::Named(name) if self.in_world() => {
                        let bearing = self.yell_bearing(*spawn_id);
                        news.notices.push(Notice::YellForHelp { name, bearing });
                    }
                    _ => news.ignored = true,
                }
            }
            // The program adds the player's own rise to the base stat with
            // no line, which this client does not keep apart from the
            // profile yet (a follow-up to read R49-6f); another spawn's is
            // told with a line that names no one.
            WorldEvent::StatsChanged(change) => match u16::try_from(change.spawn_id) {
                Ok(id) if self.in_world() && !self.is_player(id) => {
                    news.notices.push(Notice::OthersStatChanged {
                        lowered: change.amount < 0,
                    });
                }
                Ok(id) if self.in_world() && self.is_player(id) => {}
                _ => news.ignored = true,
            },
            // The Titanium program says each coin with the giver's name and
            // always plays sound 0x8d (read R49-6g); elsewhere only the coins
            // are told. The items and the experience come with the server's
            // own lines, and the session adds the coins to the purse.
            WorldEvent::Reward(reward) if self.in_world() && prints_r49_6_lines(self.generation) => {
                let giver = reward
                    .giver_id
                    .map(|id| match self.party(u32::from(id)) {
                        Party::Named(name) => name,
                        Party::Player | Party::Unseen => String::new(),
                    })
                    .unwrap_or_default();
                if reward.coins != crate::Coins::default() {
                    news.notices.push(Notice::QuestReward {
                        coins: reward.coins,
                        giver,
                    });
                }
                news.sounds.push(crate::effects::Cue::Number(QUEST_REWARD_SOUND));
            }
            WorldEvent::Reward(reward) => {
                let coins = reward.coins;
                if self.in_world() && coins != crate::Coins::default() {
                    news.notices.push(Notice::Reward(coins));
                }
            }
            // `EQEmu`'s fixed sound record is a quest reward of nothing to the
            // Titanium program, which plays its sound (read R49-6g).
            WorldEvent::Ding => {
                if self.in_world() && prints_r49_6_lines(self.generation) {
                    news.sounds.push(crate::effects::Cue::Number(QUEST_REWARD_SOUND));
                }
            }
            WorldEvent::ConsentClosed { id, closure } => {
                if self.session_id == Some(id.session_id) {
                    self.dismiss_consent(*id);
                    self.last_consent_close = Some((*id, *closure));
                    // A duel's question has no prompt yet (the duel slice), so
                    // its end says nothing, as before the repin (6.561).
                    let prompted = match id.kind {
                        ConsentKind::Resurrection | ConsentKind::Translocate => true,
                        ConsentKind::Duel => false,
                    };
                    if *closure == ConsentClosure::Expired && prompted {
                        news.notices.push(Notice::ConsentExpired(id.kind));
                    }
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::ResurrectionRefused { session_id, offer_id, reason } => {
                if self.session_id == Some(*session_id) {
                    self.last_consent_refusal = Some(*offer_id);
                    news.notices
                        .push(Notice::ResurrectionRefused(reason.clone()));
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::TranslocateRefused { session_id, offer_id, reason } => {
                if self.session_id == Some(*session_id) {
                    self.last_consent_refusal = Some(*offer_id);
                    news.notices.push(Notice::TranslocateRefused(reason.clone()));
                } else {
                    news.ignored = true;
                }
            }
            // Training at a guildmaster, and the practice points it spends.
            WorldEvent::Training(update) => self.training_news(update, news),
            WorldEvent::TrainingRefused {
                session_id,
                reason,
                string_id,
                arguments,
            } => {
                if self.session_id == Some(*session_id) {
                    // The coin line (0x2f21) names the guildmaster, shown
                    // as the program shows a spawn's name.
                    let arguments = if *string_id == Some(0x2f21) {
                        arguments
                            .iter()
                            .map(|name| crate::entities::display_name(name))
                            .collect()
                    } else {
                        arguments.clone()
                    };
                    news.notices.push(Notice::TrainingRefused {
                        reason: reason.clone(),
                        string_id: *string_id,
                        arguments,
                        common_alternate: self
                            .language_zero_selector
                            .map(crate::rules::LanguageZeroSelector::alternate_name),
                    });
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::PracticePoints(points) => {
                if let Some(player) = self.player.as_mut() {
                    player.practice_points = Some(*points);
                    self.zone.training_points_uncertain = false;
                    news.trade = true;
                }
            }

            // The player's spells.
            WorldEvent::Spell(update) => self.spell(update, now, news),
            WorldEvent::CastPending {
                session_id,
                spell_id,
                gem,
            } => self.cast_pending(*session_id, *spell_id, *gem, now, news),
            WorldEvent::CastRequestUnconfirmed {
                session_id,
                spell_id,
            } => self.cast_request_unconfirmed(*session_id, *spell_id, news),
            WorldEvent::CastLateBegin { session_id, requested_spell } => {
                self.cast_late_begin(*session_id, *requested_spell, news);
            }
            // A superseded cast, or one the Titanium program's own hit roll
            // cleared locally (mode 1 of 0x40f8a0, read R49-50; 6.645 S2):
            // the bar drops, with no line.
            WorldEvent::CastSuperseded { session_id, spell_id }
            | WorldEvent::CastCleared { session_id, spell_id } => {
                news.ignored = !self.release_cast_hold(*session_id, *spell_id);
            }
            // The program's own stun roll was saved by a stun resist (read
            // R49-50; 6.646): its line, in colour 10.
            WorldEvent::StunSaved {
                session_id,
                string_id,
            } => {
                if self.accepts_reply(*session_id) && self.death.is_none() {
                    news.notices.push(Notice::ServerString {
                        id: *string_id,
                        arguments: Vec::new(),
                        message_type: Some(STUN_SAVED_COLOUR),
                    });
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::Singing { session_id, spell_id } => {
                if self.accepts_reply(*session_id) {
                    self.casting.singing = *spell_id;
                } else {
                    news.ignored = true;
                }
            }
            WorldEvent::SongStopUnconfirmed { session_id, spell_id } => {
                self.song_stop_unconfirmed(*session_id, *spell_id, news);
            }
            WorldEvent::CastRejected {
                session_id,
                spell_id,
                reason,
                string_id,
            } => self.cast_refused((*session_id, *spell_id), (reason, *string_id), news),
            WorldEvent::BuffSnapshot(buffs) => self.buff_snapshot(buffs, now, news),
            WorldEvent::Buff(update) => {
                if match self.generation {
                    crate::GameDialect::Titanium => true,
                    // Not built for RoF2.
                    crate::GameDialect::EqMac | crate::GameDialect::Rof2 => false,
                } && let Some(player) = self.player.as_ref()
                {
                    self.casting.active_discipline.observe_titanium(player.spawn_id, update);
                }
                self.buff(update, now, news);
            }
            WorldEvent::BuffCancellation(update) => {
                if self.accepts_reply(update.session_id) && self.death.is_none() {
                    self.buffs.cancellation_news(update.clone());
                    news.notices.push(Notice::BuffCancellation(update.clone()));
                } else { news.ignored = true; }
            }
            WorldEvent::SpellEffect(effect) => self.spell_effect(effect, (now, spells), news),
            WorldEvent::SpellBook(book) => self.spell_book = Some(book.clone()),
            WorldEvent::BookAction(status) => self.book_action_news(status),
            WorldEvent::MemorizationProgress { session_id, progress } => {
                if self.accepts_reply(*session_id) {
                    self.memorization = *progress;
                } else { news.ignored = true; }
            }
        }
        changes
    }

    /// A death: a spawn becomes a corpse, and the player's own death holds
    /// them until the server offers to return them home.
    fn died(&mut self, death: &Death, now: Instant) -> Changes {
        let target_died = self.target.selected.map(u32::from) == Some(death.spawn_id);
        if target_died {
            // The received death, not its chat line or later corpse rename,
            // ends this target's attack (0x4a8cd5, read6.699).
            self.select_target(None);
        }
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
            self.reconcile_pet_buffs();
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
        changes.attack_ended |= target_died;
        changes
    }

    /// Who a spawn ID names in a death: the player, a spawn by the name
    /// players see, or no one the player can name.
    /// News of a duel: a challenge with no question to answer, a duel the
    /// player is part of beginning, or a TAKP client turning the player's
    /// down. A challenge the session asks about comes as its question
    /// instead.
    fn duel_news(&mut self, update: crate::socials::DuelUpdate, news: &mut Changes) {
        use crate::socials::DuelUpdate;
        if !self.in_world() {
            news.ignored = true;
            return;
        }
        let name = |world: &Self, id: u16| match world.party(u32::from(id)) {
            Party::Named(name) => name,
            Party::Player | Party::Unseen => "Someone".to_owned(),
        };
        let own = |world: &Self, id: u16| world.is_player(id);
        let notice = match update {
            // The question asks it, where the session offers one; the
            // player's own record is a server passing an acceptance back.
            DuelUpdate::Challenge { from }
                if !own(self, from)
                    && !self.capabilities().contains(&crate::Capability::Dueling) =>
            {
                DuelNotice::Challenged(name(self, from))
            }
            // A Titanium session tells what the program makes of its own
            // refusal ([`WorldEvent::DuelNews`]).
            DuelUpdate::Challenge { .. } | DuelUpdate::AutoDeclined { .. } => return,
            // The Titanium program has no case for an incoming 0x1b09 (read
            // R49-6); its session tells the duel's start.
            DuelUpdate::Accepted { .. } if !tells_duel_accepted(self.generation) => return,
            DuelUpdate::Accepted {
                challenger,
                accepter,
            } => {
                if own(self, challenger) && !own(self, accepter) {
                    DuelNotice::Begun(name(self, accepter))
                } else if own(self, accepter) && !own(self, challenger) {
                    DuelNotice::Begun(name(self, challenger))
                } else {
                    news.ignored = true;
                    return;
                }
            }
            DuelUpdate::Refused { from, reason } => DuelNotice::Refused {
                from: name(self, from),
                reason,
            },
        };
        news.notices.push(Notice::Duel(notice));
    }

    /// What a Titanium duel means to the player, as its session tells it
    /// (read R49-6): each the program's line, with the names it takes.
    fn duel_told(&self, told: crate::socials::DuelNews, news: &mut Changes) {
        use crate::socials::DuelNews;
        if !self.in_world() {
            news.ignored = true;
            return;
        }
        let name = |id: u16| match self.party(u32::from(id)) {
            Party::Named(name) => name,
            Party::Player | Party::Unseen => "Someone".to_owned(),
        };
        let own = || {
            self.player
                .as_ref()
                .map(|player| crate::entities::display_name(&player.name))
                .unwrap_or_default()
        };
        let notice = match told {
            DuelNews::Challenged { by } => DuelNotice::Challenged(name(by)),
            DuelNews::Begun { with } => DuelNotice::Begun(name(with)),
            DuelNews::Challenging { target } => DuelNotice::Challenging(name(target)),
            DuelNews::Accepting { challenger } => DuelNotice::Accepting(name(challenger)),
            DuelNews::NoPlayerTarget => DuelNotice::NoPlayerTarget,
            DuelNews::NotYourself => DuelNotice::NotYourself,
            DuelNews::AlreadyDueling { at_opponent } => DuelNotice::AlreadyDueling { at_opponent },
            DuelNews::ConsideringAnother => DuelNotice::ConsideringAnother,
            DuelNews::Won { over } => DuelNotice::Won {
                own: own(),
                over: name(over),
            },
            DuelNews::Ended { opponent } => DuelNotice::Ended(name(opponent)),
            DuelNews::TurnedDown { by, reason } => DuelNotice::TurnedDown {
                by: name(by),
                reason,
            },
            DuelNews::Lapsed { target } => DuelNotice::Lapsed(name(target)),
            DuelNews::Forfeited { opponent } => DuelNotice::Forfeited {
                own: own(),
                opponent: name(opponent),
            },
            // The Mac era's `/decline` answers (6.622). This client offers
            // no `/decline` yet, so these come only once it does; the
            // program's lines, in our words where the installation lacks
            // them.
            DuelNews::Declining { challenger } => {
                let challenger = name(challenger);
                news.notices.push(Notice::ProgramLine {
                    string_id: 0x33be,
                    said: format!("You decline {challenger}'s challenge."),
                    arguments: vec![challenger],
                    lines: Vec::new(),
                });
                return;
            }
            DuelNews::NothingToDecline => {
                news.notices.push(Notice::ProgramLine {
                    string_id: 0x33bf,
                    said: "You have no challenge to decline.".to_owned(),
                    arguments: Vec::new(),
                    lines: Vec::new(),
                });
                return;
            }
            DuelNews::DecliningWhileDueling => {
                news.notices.push(Notice::ProgramLine {
                    string_id: 0x33bd,
                    said: "You can't decline while dueling.".to_owned(),
                    arguments: Vec::new(),
                    lines: Vec::new(),
                });
                return;
            }
        };
        news.notices.push(Notice::Duel(notice));
    }

    /// Where a yeller stands from the player, as the Titanium program
    /// picks its line (read R49-6d, 0x452dc6): the angle between the
    /// player's heading and the bearing to the yeller, folded to half a
    /// turn. No range is tested. None off Titanium, or without both
    /// places.
    fn yell_bearing(&self, spawn_id: u16) -> Option<YellBearing> {
        if !prints_r49_6_lines(self.generation) {
            return None;
        }
        let own = self.player.as_ref()?.position;
        let yeller = self.zone.spawns.get(&spawn_id)?.state.position;
        Some(YellBearing::between(own, yeller))
    }

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

    /// Keeps the chat server's list of joined channels, and tells the rest
    /// as lines. The server's text is untrusted: its item links are read as
    /// any chat's, and it prints through the ordinary chat path.
    fn channel_event(
        &mut self,
        event: &eq_network_game::chat_server::ChannelEvent,
        news: &mut Changes,
    ) {
        use eq_network_game::chat_server::ChannelEvent;
        let line = match event {
            ChannelEvent::Joined { channels } => {
                self.chat_channels.clone_from(channels);
                return;
            }
            ChannelEvent::Message {
                channel,
                sender,
                text,
            } => ChannelLine::Message {
                slot: self
                    .chat_channels
                    .iter()
                    .position(|joined| joined.eq_ignore_ascii_case(channel))
                    .map(|index| index + 1),
                channel: plain(channel),
                sender: plain(sender),
                text: eq_network_game::chat::message(text.as_bytes(), false).into(),
            },
            ChannelEvent::Notice { text } => {
                ChannelLine::Notice(eq_network_game::chat::message(text.as_bytes(), false).into())
            }
            ChannelEvent::MemberJoined { channel, name } => ChannelLine::Member {
                channel: plain(channel),
                name: plain(name),
                joined: true,
            },
            ChannelEvent::MemberLeft { channel, name } => ChannelLine::Member {
                channel: plain(channel),
                name: plain(name),
                joined: false,
            },
        };
        news.notices.push(Notice::Channel(line));
    }

    /// Forgets what the reason makes stale, and says so.
    fn reset(&mut self, reason: Reset) -> Changes {
        self.zone.boat_control = crate::boats::ControlState::Unknown;
        self.zone.boat_notice = None;
        self.zone.arrival = None;
        self.zone.pet_buffs.suspend();
        self.movement_options_result = None;
        self.held_models = held_models::HeldModels::default();
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
        self.resurrection = None;
        self.translocate = None;
        self.duel = None;
        self.popup = None;
        self.player_inspection = None;
        self.reset_guild(reason);
        if reason != Reset::Died {
            self.camp = None;
            self.camp_to = crate::camp::CampTo::default();
        }
        match reason {
            Reset::Entered => {
                self.camp_completion = None;
                self.forget_admission();
                self.wallet = crate::money::Wallet::default();
                self.known_cursor_coins = None;
                self.nourishment = None;
                // The server sends the inventory again after each admission.
                self.inventory = Inventory::default();
                self.spell_book = None;
                self.book_action = None;
                self.memorization = None;
                self.buffs.clear();
                self.casting = Casting::default();
            }
            Reset::Camped => {
                // Local camp completion invalidates the character projection;
                // its reply/deadline evidence is retained separately.
                self.forget_admission();
                self.book_action = None;
                self.memorization = None;
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
                // A camp ends the chat server link.
                self.chat_channels.clear();
                self.chat_link.up = false;
            }
            Reset::Lost {
                ended,
                transferring,
            } => self.reset_after_loss(ended, transferring),
            Reset::Zoning { .. } => {
                self.zone.weather = None;
                self.drop_actions();
            }
            Reset::Died => self.reset_after_death(),
        }
        Changes {
            reset: Some(reason),
            ..Changes::default()
        }
    }

    /// Withdraws connection-dependent state, retaining a pending transfer's facts.
    fn reset_after_loss(&mut self, ended: bool, transferring: bool) {
        self.zone.weather = None;
        self.drop_actions();
        if self.inventory.ground_drop_projected() {
            // Delivery is uncertain across a lost link. Preserve the local
            // absence, but never resume this estimate as current custody.
            self.inventory
                .apply(crate::inventory::InventoryUpdate::Invalidated);
        }
        if ended || !transferring {
            self.casting.reset_cooldowns();
            self.buffs.clear();
        }
        if ended {
            self.alternate_advancement = crate::alternate_advancement::State::default();
            self.spell_book = None;
            self.servers = None;
            self.characters = None;
            self.pending_transfer = None;
            self.inventory = Inventory::default();
            self.chat_channels.clear();
            self.chat_link.up = false;
            self.forget_zone();
        }
    }

    /// Clears the local pet and spell state that cannot survive death.
    fn reset_after_death(&mut self) {
        self.zone.own_pet = None;
        self.zone.pet_buffs.clear_current();
        self.buffs.clear_cancellation();
        self.casting.cast = None;
        self.casting.interrupted = None;
        self.casting.reset_cooldowns();
        self.book_action = None;
        self.memorization = None;
    }

    /// Drops the actions in flight: a cast, its request and interruption, and
    /// a spellbook change.
    fn drop_actions(&mut self) {
        self.buffs.clear_cancellation();
        self.casting.drop_actions();
        self.book_action = None;
        self.memorization = None;
    }

    /// Forgets the admission and its zone.
    fn forget_admission(&mut self) {
        self.displayed_level = None;
        self.alternate_advancement = crate::alternate_advancement::State::default();
        self.liquid_hazards = LiquidHazardInputs::default();
        self.characters = None;
        self.last_consent_refusal = None;
        self.items = ItemCache::default();
        self.pending_transfer = None;
        self.same_zone_relocation = None;
        self.last_local_pad = None;
        self.pending_local_pad = None;
        self.reading = None;
        self.popup_serial = 0;
        self.combining = None;
        self.group = None;
        self.group_invitation = None;
        self.forget_guild();
        self.auto_split = None;
        self.played = None;
        self.bound_in = None;
        self.joining_group = false;
        self.assisted = None;
        self.raid = None;
        self.raid_invitation = None;
        self.raid_flow = raid::RaidFlow::default();
        self.death = None;
        self.own_stun = None;
        self.own_unconscious = None;
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

/// A name the chat server sent, a channel's or a player's, as plain text:
/// without control characters, the item link marker among them, so that it
/// prints as words and can start no link.
fn plain(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).collect()
}

/// The colour the Titanium program prints a stun save's line in (0x1679,
/// 0x2375; read R49-50).
const STUN_SAVED_COLOUR: u32 = 10;

/// Whether a generation's program tells an incoming duel acceptance. The
/// Titanium program has no case for 0x1b09 (read R49-6); the others are
/// unread and keep this client's line.
const fn tells_duel_accepted(dialect: crate::GameDialect) -> bool {
    match dialect {
        crate::GameDialect::Titanium => false,
        crate::GameDialect::EqMac | crate::GameDialect::Rof2 => true,
    }
}

/// The sound the Titanium program plays at a quest reward, by the sound
/// object's number (read R49-6g); that it is the numbers file's is
/// inferred.
const QUEST_REWARD_SOUND: u16 = 0x8d;

/// Whether a generation's program says the lines read R49-6 found for
/// rewards, yells and levels: the Titanium program's. The others are unread
/// and keep this client's.
pub(crate) const fn prints_r49_6_lines(dialect: crate::GameDialect) -> bool {
    match dialect {
        crate::GameDialect::Titanium => true,
        crate::GameDialect::EqMac | crate::GameDialect::Rof2 => false,
    }
}
