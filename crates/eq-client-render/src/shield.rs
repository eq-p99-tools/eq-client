//! A Warrior's `/shield` (checklist 6.517). On Titanium it runs as read
//! R49-22d reads the program's handler (0x46aa32), in this order:
//! - a character that is not a Warrior of level 30 or more says 0xfc1;
//! - with words, they name a spawn by 0x47981d's prefix lookup, the whole
//!   text as one name: no match is silent, the player says 0xcce, and any
//!   other spawn is sent;
//! - with none, the target is taken: no target says 0xfbf then 0xfc2, the
//!   player says 0xfbe, and any other target is sent.
//!
//! The program checks no kind, distance or recast and keeps no state: NPCs,
//! corpses, distance, the recast and the server's rule are the server's to
//! answer, with its own lines, and the session paces the sends. Its general
//! "can act" gate (0x466244) and its silence while a trade or one of three
//! windows is open are not modelled here, as for `/assist` (row 6.517a).
//! Each line is the installed client's string, in colour 0x111, with this
//! client's words where it has none.
//!
//! Elsewhere the handler is unread, and `/shield` keeps this client's
//! earlier checks and words: the target alone, no words, and the class,
//! level and self checks, stricter than the server, never looser.
use super::chat::{ChatState, Said};
use super::hud::messages::Messages;
use crate::outbox::{self, Outbox};
use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand, GameDialect, world::ClientWorld};

/// The Warrior's class number.
const WARRIOR: u32 = 1;
/// The least level that may `/shield` (read R49-22d 3).
const SHIELD_LEVEL: u8 = 30;
/// The longest name the program takes (read R49-22d 4).
const LONGEST_NAME: usize = 128;
/// The colour of every line here.
const COLOUR: u32 = 0x111;
/// Not a Warrior of level 30 or more: one line for both.
const NOT_ABLE: (u32, &str) = (
    0xfc1,
    "Only a Warrior of level 30 or more can shield another.",
);
/// The player named.
const NAMED_SELF: (u32, &str) = (0xcce, "You cannot shield yourself.");
/// No target, and no words.
const NO_TARGET: [(u32, &str); 2] = [
    (0xfbf, "You have no target to shield."),
    (0xfc2, "Target someone, or name them, then use /shield."),
];
/// The player targeted.
const TARGETED_SELF: (u32, &str) = (0xfbe, "You cannot shield yourself.");

/// The words after `/shield`, trimmed, if the line is one.
pub(crate) fn typed(input: &str) -> Option<String> {
    let command = input.strip_prefix('/')?;
    let (name, words) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    name.eq_ignore_ascii_case("shield")
        .then(|| words.trim().to_owned())
}

/// Whether a generation's `/shield` is the Titanium program's as read
/// R49-22d found it. The others are unread.
const fn reads_r49_22d(dialect: GameDialect) -> bool {
    match dialect {
        GameDialect::Titanium => true,
        GameDialect::EqMac | GameDialect::Rof2 => false,
    }
}

/// What a `/shield` comes to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Shield this spawn.
    Send(u16),
    /// Say these lines, and send nothing.
    Say(Vec<Said>),
    /// Nothing at all.
    Silent,
}

/// The program's line, in its colour.
fn line(messages: &Messages, (id, ours): (u32, &str)) -> Said {
    messages.said_text(id, ours).typed(COLOUR)
}

/// What `/shield` with these words comes to in this world.
pub(crate) fn decide(world: &ClientWorld, words: &str, messages: &Messages) -> Outcome {
    let Some(player) = world.player() else {
        return Outcome::Silent;
    };
    if !reads_r49_22d(world.generation()) {
        return earlier(world, words);
    }
    // An unknown class counts as not a Warrior, the stricter side.
    if player.class != Some(WARRIOR) || player.level < SHIELD_LEVEL {
        return Outcome::Say(vec![line(messages, NOT_ABLE)]);
    }
    if !words.is_empty() {
        let name: String = words.chars().take(LONGEST_NAME).collect();
        return match crate::emotes::named_spawn(world, &name) {
            None => Outcome::Silent,
            Some(id) if id == player.spawn_id => Outcome::Say(vec![line(messages, NAMED_SELF)]),
            Some(id) => Outcome::Send(id),
        };
    }
    // A target gone from the spawn table counts as none (reviewer S3).
    match world
        .target()
        .selected
        .filter(|id| *id == player.spawn_id || world.spawn(*id).is_some())
    {
        None => Outcome::Say(
            NO_TARGET
                .into_iter()
                .map(|said| line(messages, said))
                .collect(),
        ),
        Some(id) if id == player.spawn_id => Outcome::Say(vec![line(messages, TARGETED_SELF)]),
        Some(id) => Outcome::Send(id),
    }
}

/// The earlier checks and words, where the handler is unread.
fn earlier(world: &ClientWorld, words: &str) -> Outcome {
    let refuse = |words: &str| Outcome::Say(vec![Said::own(words)]);
    let Some(player) = world.player() else {
        return Outcome::Silent;
    };
    if !words.is_empty() {
        return refuse("Use /shield without arguments");
    }
    if player.class != Some(WARRIOR) {
        return refuse("Only a Warrior can shield another player");
    }
    if player.level < SHIELD_LEVEL {
        return Outcome::Say(vec![Said::own(format!(
            "You must be level {SHIELD_LEVEL} to shield"
        ))]);
    }
    match world
        .target()
        .selected
        .filter(|id| world.spawn(*id).is_some())
    {
        None => refuse("Target the player you want to shield first"),
        Some(id) if id == player.spawn_id => {
            refuse("You cannot shield yourself; target another player")
        }
        Some(id) => Outcome::Send(id),
    }
}

/// Answers a `/shield` the chat noted: its lines, or the send.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn answer(
    mut chat: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    outbox: Res<Outbox>,
    messages: Option<Res<Messages>>,
) {
    let Some(words) = chat.shield_words.take() else {
        return;
    };
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let world = online.world();
    match decide(world, &words, messages) {
        Outcome::Silent => (),
        Outcome::Say(lines) => {
            for said in lines {
                chat.history.push(super::chat::system_line(said));
            }
        }
        Outcome::Send(_) if !outbox::offered(world, Capability::Shielding) => {
            chat.refuse(Said::own(outbox::UNAVAILABLE));
        }
        Outcome::Send(spawn_id) => {
            let _ = outbox.post(world, |stamp| ClientCommand::Shield {
                session_id: stamp.session_id,
                spawn_id,
                created: stamp.created,
            });
        }
    }
}

#[cfg(test)]
mod tests;
