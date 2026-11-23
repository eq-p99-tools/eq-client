//! What the session's news tells the player, as data. The world decides
//! whether news applies and what it says; a front end words it, with the
//! installed client's strings where the server uses them.
use crate::{
    CampStatus, Coins, ZoneRejection,
    combat::{ConColor, Consideration, Damage},
    loot::LootResponse,
};

/// The player's own listing changed, as they asked; the server says nothing
/// of it, so the official client words it itself (inferred from its
/// strings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListingNotice {
    /// Away from the keyboard (true), or back.
    Away(bool),
    /// Anonymous, or no longer.
    Anonymous(bool),
    /// Roleplaying, or no longer.
    Roleplaying(bool),
    /// Looking for a group, or no longer.
    Looking(bool),
}

/// News of a duel the player is part of. On Titanium each is one of the
/// program's lines (read R49-6 a, d, e and h), told by the session as
/// [`crate::socials::DuelNews`]; elsewhere the server sends no line with it
/// (its own refusal text excepted), so this client words it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DuelNotice {
    /// Another player challenged the player (0x592, %1 their name).
    Challenged(String),
    /// The player's duel with this player began (0x590, %1 their name).
    Begun(String),
    /// The player this names turned the player's duel down, and why
    /// (TAKP's clients' own answer).
    Refused {
        /// Who.
        from: String,
        /// Why.
        reason: crate::socials::DuelRefusal,
    },
    /// An answer to a challenge was not sent, and why.
    AnswerRefused(String),
    /// `/duel` challenged this player (0x33c6).
    Challenging(String),
    /// `/duel` accepted this player's challenge, and the duel began
    /// (0x33c4).
    Accepting(String),
    /// `/duel` with no target, or one that is not a player (0x33c0).
    NoPlayerTarget,
    /// `/duel` at the player themselves (0x33c1).
    NotYourself,
    /// `/duel` during a duel: at the opponent (0x33c2), or at someone else
    /// (0x33c3).
    AlreadyDueling {
        /// Whether the target is the opponent.
        at_opponent: bool,
    },
    /// `/duel` at one player while another's challenge waits (0x33c5).
    ConsideringAnother,
    /// The player won the duel (0x2f44, %1 the player's name, %2 the
    /// opponent's).
    Won {
        /// The player's name.
        own: String,
        /// The opponent's.
        over: String,
    },
    /// The duel partner died otherwise (0x2f45, %1 and %2 their name).
    Ended(String),
    /// The one the player challenged turned it down on their program's
    /// own: 0x567 they ignore the player, 0x568 they are dueling, 0x569
    /// another challenge waits; no line for a reason the program has none
    /// for.
    TurnedDown {
        /// Who.
        by: String,
        /// Why.
        reason: Option<crate::socials::DuelAutoDecline>,
    },
    /// The one the player challenged left before accepting (0x581).
    Lapsed(String),
    /// The opponent left during the duel, which the player wins (0x580, %1
    /// the player's name, %2 and %3 theirs).
    Forfeited {
        /// The player's name.
        own: String,
        /// The opponent's.
        opponent: String,
    },
}

/// Where a yeller stands from the player, by the line the Titanium program
/// picks (read R49-6d): the angle between the player's heading and the
/// bearing to them, folded to half a turn. The sides of each band have two
/// lines, and which is which is unread, so the first stands for both
/// (labelled).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YellBearing {
    /// Up to 32 of the 512 heading units: 0x326b.
    Ahead,
    /// Over 32 up to 96: 0x326d (or 0x3270).
    AheadAside,
    /// Over 96 up to 160: 0x326e (or 0x3271).
    Aside,
    /// Over 160 up to 224: 0x326f (or 0x3272).
    BehindAside,
    /// Over 224: 0x326c.
    Behind,
}

impl YellBearing {
    /// The band a yeller at `yeller` is in for a player at `own`, facing
    /// its heading. A step along heading `h` moves `(sin h, cos h)` in X
    /// and Y.
    #[must_use]
    pub fn between(own: crate::WorldPosition, yeller: crate::WorldPosition) -> Self {
        let turn = 512.0_f32;
        let bearing = (yeller.x - own.x).atan2(yeller.y - own.y) / std::f32::consts::TAU * turn;
        let difference = (bearing - own.heading).rem_euclid(turn);
        let folded = if difference > turn / 2.0 {
            turn - difference
        } else {
            difference
        };
        match folded {
            angle if angle <= 32.0 => Self::Ahead,
            angle if angle <= 96.0 => Self::AheadAside,
            angle if angle <= 160.0 => Self::Aside,
            angle if angle <= 224.0 => Self::BehindAside,
            _ => Self::Behind,
        }
    }

    /// The program's line for the band.
    #[must_use]
    pub const fn string_id(self) -> u32 {
        match self {
            Self::Ahead => 0x326b,
            Self::AheadAside => 0x326d,
            Self::Aside => 0x326e,
            Self::BehindAside => 0x326f,
            Self::Behind => 0x326c,
        }
    }
}

/// Who took part in a death the player saw, or in news of their group or
/// raid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Party {
    /// The player.
    Player,
    /// Another spawn, or a group member, by the name players see.
    Named(String),
    /// No one the player can name: no killer, or one not in view.
    Unseen,
}

impl Notice {
    /// The message type the server gave what the notice says, by which the
    /// official client colours its line; None for what the world says
    /// itself.
    #[must_use]
    pub const fn message_type(&self) -> Option<u32> {
        match self {
            Self::ServerString { message_type, .. } => *message_type,
            _ => None,
        }
    }
}

/// Something to tell the player.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// The server contradicted a buff slot modelled from the program's slot
    /// finder, for the first time in this zone visit: a diagnostic, not a
    /// message the official client gives.
    BuffSlotMismatch,
    /// The server still holds a buff the program's slot finder removes for
    /// a landing (step 4's replace or step 6's eviction, read R49-22e): the
    /// client's prediction was wrong, and the server's word stands. A
    /// diagnostic for the gap log (6.612 T1), not a message the official
    /// client gives.
    BuffPredictionContradicted(crate::buffs::Disagreement),
    /// Where the connection stands.
    Connection {
        /// Where it stands now.
        link: super::Link,
        /// Whether the player is dead, awaiting their return home.
        dead: bool,
    },
    /// A message from the installed client's string table.
    ServerString {
        /// The string's ID.
        id: u32,
        /// Its `%1`, `%2`, ... substitutions, in order.
        arguments: Vec<crate::chat::RichText>,
        /// The message type the server gave it, by which the official client
        /// colours its line.
        message_type: Option<u32>,
    },
    /// The player considered a spawn.
    Consideration {
        /// The server's answer.
        consideration: Consideration,
        /// The spawn's server name, when it is known.
        name: Option<String>,
        /// The level color available for presentation when this answer arrived.
        color: Option<ConColor>,
    },
    /// Damage the player dealt or took, or anything else nearby.
    Damage {
        /// The damage record.
        damage: Damage,
        /// The player's spawn ID.
        own_id: u16,
        /// The attacker's server name, when known.
        source: Option<String>,
        /// The defender's server name, when known.
        target: Option<String>,
    },
    /// Environmental damage the player took, in the lines the Titanium
    /// program prints for it (reads R49-3 e and R49-3b): after its own
    /// report the amount line, then, as for a record the server sends, the
    /// record's line.
    EnvironmentalDamage {
        /// The amount of the player's own report, whose line
        /// ([`crate::environment::REPORT_LINE`]) comes first; None for a
        /// record the server sent.
        reported: Option<u32>,
        /// The record's line and chat colour
        /// ([`crate::environment::record_line`]); None where no record went
        /// with the report, as on the program's death path.
        record: Option<(u32, u32)>,
    },
    /// Camping began, was abandoned or refused, or the logout began.
    Camp(CampStatus),
    /// The corpse handed over coins.
    LootCoins(Coins),
    /// The corpse could not be looted.
    LootRefused(LootResponse),
    /// The server kept an item on the corpse.
    ItemRefused,
    /// The merchant would not trade.
    ShopRefused,
    /// Nothing was bought or sold, and why.
    TradeRefused(String),
    /// A merchant has not answered; no purchase or sale outcome is established.
    MerchantUnanswered(String),
    /// No give window opened, or it closed, and why.
    GiveRefused(String),
    /// An item on the ground could not be picked up, and why.
    GroundRefused(String),
    /// A door request went to the server, or was refused and why.
    Door {
        /// The door.
        door_id: u8,
        /// Why it was refused.
        error: Option<String>,
        /// The program's string for the refusal, where the session names
        /// one: 1462 for a door out of reach.
        string_id: Option<u32>,
    },
    /// A zone transfer was refused.
    TransferRefused(ZoneRejection),
    /// A zone line could not be crossed, and why.
    ZoneLineRefused(String),
    /// The player arrived in a zone, named by its long name: at Enter World,
    /// through a transfer or back at the bind.
    Arrived(String),
    /// A target request was refused, and why.
    TargetRefused(String),
    /// A normal attack or consider request was refused locally, and why.
    CombatRefused(String),
    /// An ability was not used, and why: in the official client's own words
    /// where the session names its string, else in the session's.
    AbilityRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
        /// What the official string names, in order.
        arguments: Vec<String>,
    },
    /// One of the official client's own lines that the session names, such
    /// as a fishing cast, a beg's or a pick's answer, or where a tracked
    /// spawn lies; in the client's words where the installation lacks it.
    ProgramLine {
        /// The line, in `eqstr_us.txt`.
        string_id: u32,
        /// What it names, in order.
        arguments: Vec<String>,
        /// Which arguments are lines themselves, by `eqstr_us.txt` string
        /// ID, that the line names by their words: a bearing's side word.
        lines: Vec<usize>,
        /// The client's own words for it.
        said: String,
    },
    /// The server's answer to a surname request, shown whenever it comes
    /// as the Titanium program shows it (read R49-10): no request of ours
    /// gates it.
    SurnameAnswer {
        /// Whether the server took it.
        accepted: bool,
        /// The player's own name, which the taken line names first.
        name: String,
        /// The surname the answer names.
        last_name: String,
    },
    /// A bandaging started or ended.
    BindWound(crate::bind_wound::BindWoundUpdate),
    /// Sense Heading: the compass point the player faces, clockwise from
    /// north (0) to north-west (7).
    Heading(u8),
    /// The player turned hungry or thirsty with nothing the session eats or
    /// drinks on its own.
    NothingToEat {
        /// Why a hungry player went without food.
        food: Option<crate::food::Shortage>,
        /// Why a thirsty player went without drink.
        water: Option<crate::food::Shortage>,
    },
    /// An item was not eaten or drunk, and why: in the official client's own
    /// words where the session names its string, else in the session's.
    ConsumeRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
    },
    /// A command to the pet was not sent, and why, worded as
    /// [`Notice::ConsumeRefused`].
    PetRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
    },
    /// One of the player's skills rose, to this value.
    SkillUp {
        /// The skill's number.
        skill: u32,
        /// Its value now.
        value: u32,
    },
    /// A training request was not sent, and why.
    TrainingRefused(String),
    /// A training request or projection from the current admission.
    TrainingPractice(crate::training::PracticeUpdate),
    /// An answer to a resurrection was not sent, and why.
    ResurrectionRefused(String),
    /// An answer to a translocate was not sent, and why.
    TranslocateRefused(String),
    /// The worker could not send the inspection request.
    PlayerInspectionRefused(String),
    /// A consent prompt reached its local deadline.
    ConsentExpired(crate::confirmations::ConsentKind),
    /// A request to read was not sent, and why.
    ReadRefused(String),
    /// An AA request was not sent, and why: the session's words, and its
    /// kind of reason where it names one (none for a request no feature
    /// owns, or from an earlier admission).
    AlternateAdvancementRefused {
        /// The session's words.
        reason: String,
        /// The kind of reason, for a window to show beside its rank.
        refusal: Option<crate::alternate_advancement::Refusal>,
    },
    /// The world container the player asked to open is in use by someone
    /// else.
    ContainerInUse,
    /// A combine was not sent, and why: in the official client's own words
    /// where the session names its string, else in the session's.
    CombineRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
    },
    /// A consent to drag a player's corpses was given or taken back.
    Consent {
        /// What the server said.
        consent: crate::corpses::Consent,
        /// Whether the corpses are the player's own; otherwise the player
        /// was the one consented.
        own: bool,
    },
    /// A consent, summon or drag was not sent, and why, worded as
    /// [`Notice::ConsumeRefused`].
    CorpseRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
    },
    /// The world's answer to `/who all`.
    WhoList(crate::who::WhoList),
    /// Submission or an observed outcome of an explicit buff cancellation.
    BuffCancellation(crate::buffs::cancel::Update),
    /// A manual song stop whose server outcome is unknown.
    SongStopUnconfirmed {
        /// Whether the worker released a local casting hold.
        released_cast: bool,
    },
    /// A cast request was refused, and why.
    CastRefused {
        /// The spell asked for.
        spell_id: u32,
        /// Why.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`, where
        /// the session names one.
        string_id: Option<u32>,
    },
    /// An item click was refused before it was sent, worded as
    /// [`Notice::ConsumeRefused`].
    ItemUseRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
    },
    /// The player's cast was interrupted. The server names why by a string
    /// that takes no arguments.
    CastInterrupted {
        /// The official client's string for why, in `eqstr_us.txt`.
        string_id: u32,
    },
    /// The player, or a spawn the player knows, died. No line comes from
    /// the server with the death, so the official client words it itself
    /// (inferred): who died, and who killed them.
    Slain {
        /// Who died: the player, or another spawn by name.
        victim: Party,
        /// Who killed them.
        killer: Party,
    },
    /// News of the player's group, which the official client words itself
    /// (inferred from its strings).
    Group(super::GroupNotice),
    /// The player's own listing changed, as they asked.
    Listing(ListingNotice),
    /// A listing request went out. Received listing state is separate.
    ListingRequested(crate::listing::ListingChange),
    /// A die the server rolled for a player nearby, the player among them;
    /// the server sends no line with it, so the official client words it
    /// itself (inferred from its strings).
    Roll(crate::socials::Roll),
    /// News of the player's raid, which the official client words itself
    /// (inferred from its strings).
    Raid(super::RaidNotice),
    /// A raid request was not sent, and why, worded as
    /// [`Notice::ConsumeRefused`].
    RaidRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
        /// What the string names, in its order: a member's name.
        arguments: Vec<String>,
    },
    /// A petition, bug report or feedback went to the server (6.511).
    ReportSent(eq_network_game::reports::ReportKind),
    /// A petition, bug report or feedback was not sent, and why, in the
    /// session's words, which never repeat the report.
    ReportRefused {
        /// Why, in the session's words.
        reason: String,
    },
    /// A line of chat was not sent because the session's chat pace held it
    /// (recording checklist 6.523), and how long to wait, in the session's
    /// words, which never repeat the line.
    ChatRefused {
        /// Why, in the session's words.
        reason: String,
    },
    /// A roll, emote or assist was not sent, and why, worded as
    /// [`Notice::ConsumeRefused`].
    SocialRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
    },
    /// A listing change was not sent, and why, worded as
    /// [`Notice::ConsumeRefused`].
    ListingRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
    },
    /// A group request was not sent, and why, worded as
    /// [`Notice::ConsumeRefused`].
    GroupRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
    },
    /// Looted coins are now shared with the group (true) or kept.
    AutoSplit(bool),
    /// News of a duel the player is part of.
    Duel(DuelNotice),
    /// A player near the player yelled for help, by name. The server sends
    /// no line with it (`Client::Handle_OP_YellForHelp` passes the record
    /// on). On Titanium the program's line names where the yeller stands
    /// (read R49-6d); elsewhere this client's words stand.
    YellForHelp {
        /// The yeller's shown name.
        name: String,
        /// Where they stand from the player, on Titanium.
        bearing: Option<YellBearing>,
    },
    /// A quest gave the player these coins, never all zero, on Titanium:
    /// the program says each with the giver's shown name (read R49-6g).
    QuestReward {
        /// The coins.
        coins: Coins,
        /// The giver's shown name, empty for none or one not in view.
        giver: String,
    },
    /// The player lost levels, as the Titanium program says it (read
    /// R49-6c): 0x1ba for one, 0x1bb for more, colour 13.
    LevelLost {
        /// How many levels.
        lost: u8,
        /// The level now.
        level: u8,
    },
    /// A stat increase named another spawn, which the Titanium program tells
    /// with a line that names no one, as it does (read R49-6f, 0x455fff:
    /// 0x3273 when the amount is below zero, else 0x3274, colour 15). One
    /// for the player's own spawn says nothing.
    OthersStatChanged {
        /// Whether the amount was below zero.
        lowered: bool,
    },
    /// A quest or task gave the player these coins, which the server added
    /// to the purse with no line of its own (`Client::QuestReward`,
    /// `Client::CashReward`); never all zero (pending R49-6g).
    Reward(Coins),
    /// A guild request was not sent, and why, worded as
    /// [`Notice::ConsumeRefused`].
    GuildRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
    },
    /// Coins were not moved, split or changed, and why, worded as
    /// [`Notice::ConsumeRefused`].
    CoinsRefused {
        /// Why, in the session's words.
        reason: String,
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: Option<u32>,
    },
    /// Another caster nearby had a cast interrupted. The server names it by
    /// a string that takes the caster's name, which it sends with it.
    OtherCastInterrupted {
        /// The official client's string for it, in `eqstr_us.txt`.
        string_id: u32,
        /// The caster's name, as the server sent it.
        caster: String,
    },
    /// A line from the chat server's channels (recording checklist 6.509;
    /// how the official client words and colours each is pending read
    /// R49-12 b).
    Channel(ChannelLine),
    /// A chat channel command was not sent, and why, in this library's
    /// words.
    ChannelRefused {
        /// Why.
        reason: String,
    },
    /// A character the player asked for was not created.
    CreationRefused {
        /// The name asked for.
        name: String,
        /// Which refusal: the name, the creation, or the client's own.
        outcome: crate::creation::CreationOutcome,
    },
}

/// A line the chat server's channels gave.
#[derive(Clone, Debug, PartialEq)]
pub enum ChannelLine {
    /// Someone's message on a channel.
    Message {
        /// The channel's name, as the server sent it.
        channel: String,
        /// Its slot, from 1, in the last list of joined channels; None
        /// before a list names it.
        slot: Option<usize>,
        /// Who said it.
        sender: String,
        /// What they said, with its item links.
        text: crate::chat::RichText,
    },
    /// The chat server's own line, as it sent it.
    Notice(crate::chat::RichText),
    /// Someone joined or left a channel the character is on.
    Member {
        /// The channel's name.
        channel: String,
        /// Who.
        name: String,
        /// Joined (true) or left.
        joined: bool,
    },
    /// The link to the chat server ended.
    Lost,
}
