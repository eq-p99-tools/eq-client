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

/// News of a duel the player is part of. The server sends no line with
/// any of it on `EQEmu` (its own refusal text excepted), so this client
/// words it itself; the official client's strings and colours for these
/// are not read (checklist 6.492, pending R49-6a).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DuelNotice {
    /// Another player challenged the player, with no question to answer:
    /// the server type does not answer duels yet.
    Challenged(String),
    /// The player's duel with this player began.
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
    /// on); this client's words stand until the official client's are read
    /// (checklist 6.492, pending R49-6d).
    YellForHelp(String),
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
    /// A character the player asked for was not created.
    CreationRefused {
        /// The name asked for.
        name: String,
        /// Which refusal: the name, the creation, or the client's own.
        outcome: crate::creation::CreationOutcome,
    },
}
