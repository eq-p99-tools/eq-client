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
    /// An ability was not used, and why.
    AbilityRefused(String),
    /// Sense Heading: the compass point the player faces, clockwise from
    /// north (0) to north-west (7).
    Heading(u8),
    /// A cast request was refused, and why.
    CastRefused {
        /// The spell asked for.
        spell_id: u32,
        /// Why.
        reason: String,
    },
}
