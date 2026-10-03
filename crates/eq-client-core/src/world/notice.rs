//! What the session's news tells the player, as data. The world decides
//! whether news applies and what it says; a front end words it, with the
//! installed client's strings where the server uses them.
use crate::{
    CampStatus, Coins, ZoneRejection,
    combat::{Consideration, Damage},
    loot::LootResponse,
};

/// Something to tell the player.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
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
        arguments: Vec<String>,
    },
    /// The player considered a spawn.
    Consideration {
        /// The server's answer.
        consideration: Consideration,
        /// The spawn's server name, when it is known.
        name: Option<String>,
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
    },
    /// A zone transfer was refused.
    TransferRefused(ZoneRejection),
    /// A zone line could not be crossed, and why.
    ZoneLineRefused(String),
    /// A target request was refused, and why.
    TargetRefused(String),
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
    /// An answer to a resurrection was not sent, and why.
    ResurrectionRefused(String),
    /// A request to read was not sent, and why.
    ReadRefused(String),
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
    /// A cast request was refused, and why.
    CastRefused {
        /// The spell asked for.
        spell_id: u32,
        /// Why.
        reason: String,
    },
    /// The player's cast was interrupted. The server names why by a string
    /// that takes no arguments.
    CastInterrupted {
        /// The official client's string for why, in `eqstr_us.txt`.
        string_id: u32,
    },
}
