//! What an update changed that a front end must redo, and the answers to the
//! front end's own requests that came with it.
use super::{CastNews, Notice};
use crate::WorldPosition;

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

/// The player's own move as the session sent it.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    /// Where the player is: the position sent, or after a refusal, the last
    /// one the session accepted.
    pub position: WorldPosition,
    /// Why the session's movement guard refused the move; None when it was
    /// sent.
    pub refused: Option<String>,
}

/// The session's answer to a request the front end made itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// An item use was submitted to the server, or refused.
    ItemUse {
        /// The admission it was made in.
        session_id: u64,
        /// The request.
        request_id: u64,
        /// Why it was refused.
        error: Option<String>,
    },
    /// An inventory move was submitted to the server, or refused.
    InventoryMove {
        /// The admission it was made in.
        session_id: u64,
        /// The inventory revision it was made against.
        revision: u64,
        /// Why it was refused.
        error: Option<String>,
    },
    /// The server answered a request for an item on the corpse.
    LootTaken {
        /// The corpse slot asked for.
        slot: u16,
        /// Whether the item was handed over.
        accepted: bool,
    },
    /// A zone line could not be crossed; the move that asked is over.
    ZoneLineRefused,
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
    /// The session granted or withdrew calibrated motion.
    pub motion: bool,
    /// The session sent the player's own move, or refused it.
    pub moved: Option<Moved>,
    /// The corpse being looted, the merchant's stock or the coins changed.
    pub trade: bool,
    /// What the news tells the player.
    pub notices: Vec<Notice>,
    /// Answers to the front end's own requests.
    pub replies: Vec<Reply>,
    /// The update was for an earlier admission, or came when the world takes
    /// no such news, so nothing changed.
    pub ignored: bool,
}
