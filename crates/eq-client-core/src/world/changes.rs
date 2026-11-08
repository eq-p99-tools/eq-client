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

/// The worker's local result for the player's movement proposal.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    /// Original process-local stamp of the proposal.
    pub created: std::time::Instant,
    /// The position the worker accepts, which can stay unchanged.
    pub position: WorldPosition,
    /// Why the movement guard refused the proposal. None means locally accepted.
    pub refused: Option<String>,
}

/// The session's answer to a request the front end made itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Local deletion lifecycle with explicitly scoped list evidence; never an ACK.
    CharacterDeletion(crate::deletion::Update),
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
        /// Whether to present this refusal. A dependent tail retains its typed
        /// error while suppressing a second notice for an already reported root.
        present_error: bool,
    },
    /// Local disposition of the original loot action; never server completion.
    LootRequest {
        /// The admission named by the original command.
        session_id: u64,
        /// Original open/item creation time; existing close commands have none.
        created: Option<std::time::Instant>,
        /// Every original corpse/player/place/placement parameter.
        request: crate::loot::LootRequest,
        /// Transport submission or local refusal, separate from server updates.
        status: crate::loot::LootRequestStatus,
    },
    /// The server received a request and retired its corpse-view place.
    /// This says nothing about delivery; ordinary inventory events alone
    /// establish what the player holds.
    LootReceived {
        /// The requested place on the corpse, from 0.
        place: u16,
    },
    /// The session's existing result for a request for an item on the corpse.
    LootTaken {
        /// The corpse's place asked for, from 0.
        place: u16,
        /// Whether the item was handed over.
        accepted: bool,
    },
    /// The worker dropped a move before returning a movement position.
    MotionRefused {
        /// Original process-local request stamp.
        created: std::time::Instant,
        /// Why it was refused.
        reason: String,
    },
    /// A zone line could not be crossed; the move that asked is over.
    ZoneLineRefused,
    /// A local boundary stayed in this admission. Normal movement can resume.
    SameZoneBoundary,
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
    /// This placement was a received same-zone move.
    pub same_zone_relocation: bool,
    /// The login server listed its worlds, or refused the one chosen.
    pub servers: bool,
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
    /// The corpse being looted, the merchant's stock, the coins, or the
    /// training at a guildmaster changed.
    pub trade: bool,
    /// Newly slain known NPCs whose death names the player as killer. These
    /// IDs identify the locally projected corpses, not permission to loot.
    pub kills: Vec<u16>,
    /// Received combat outcomes, independent of whether they produce chat.
    pub damage: Vec<crate::combat::Damage>,
    /// What the news tells the player.
    pub notices: Vec<Notice>,
    /// The sounds the server asks the player's client to play, in the order
    /// sent.
    pub sounds: Vec<crate::effects::Cue>,
    /// Received flight record, independent of combat outcomes and inventory.
    pub projectile: Option<crate::effects::Projectile>,
    /// Answers to the front end's own requests.
    pub replies: Vec<Reply>,
    /// The update was for an earlier admission, or came when the world takes
    /// no such news, so nothing changed.
    pub ignored: bool,
}
