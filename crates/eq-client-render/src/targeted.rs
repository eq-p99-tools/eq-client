//! `/ttell` and `/whotarget`: a tell to the target and the zone's `/who` of
//! its name (recording checklist 6.528).
//!
//! On Titanium both follow the program (read R49-18):
//! - `/ttell` (0x4679dc) says nothing and sends nothing with no words or no
//!   target. It takes the target's name as the server sent it. A target of
//!   type 3 (here a creature's corpse) is taken whenever it has a name, as
//!   it stands; any other target only when its name has no `#` and no `_`,
//!   and cut at its apostrophe, so a player's corpse's tell goes to its
//!   owner. Anything else is refused with the program's line 0x3322. A name
//!   that comes to the player's own (themself, or their own corpse) gets the
//!   line 0x3023 and sends nothing; otherwise the tell goes as a typed
//!   `/tell` would.
//! - `/whotarget` (0x46a871) is the zone's `/who` of the target's name,
//!   whatever the target is, passing over any words after it, and says
//!   nothing with no target.
//!
//! The `EQMac` and `RoF2` programs are unread, so they keep this client's
//! own rules and words: a tell only to another player whose name is
//! letters alone, and a refusal in words for no target.
use super::chat::{ChatState, Said};
use super::hud::messages::Messages;
use bevy::prelude::*;
use eq_client_core::{GameDialect, SpawnKind, who::WhoFilter, world::ClientWorld};

/// The Titanium program's line for a target `/ttell` does not take.
const REFUSED: u32 = 0x3322;
/// The Titanium program's line for a tell to the player themself.
const TO_SELF: u32 = 0x3023;
/// The chat colour of both lines (0x498436).
const COLOUR: u32 = 0x111;

/// This client's refusal of a tell on the unread programs.
const OWN_REFUSAL: &str = "Target another player to send them a tell.";
/// This client's refusal of `/whotarget` with no target on the unread
/// programs.
const OWN_UNSEEN: &str = "Target someone to list with /who first.";

/// What `/ttell` or `/whotarget` stands for.
#[derive(Debug, PartialEq)]
pub(crate) enum Targeted {
    /// `/ttell message`: the `/tell` to the target it stands for, or this
    /// client's own refusal.
    Tell(Result<String, String>),
    /// `/whotarget`: the zone's `/who` of the target's name.
    Who(Result<WhoFilter, String>),
    /// A program line, said by [`say`], and nothing sent.
    Say(Line),
    /// Nothing said and nothing sent, as the program does.
    Nothing,
}

/// A program line `/ttell` says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// The target is not one a tell goes to (0x3322).
    Refused,
    /// The tell would go to the player themself (0x3023).
    ToSelf,
}

/// Whether the generation's program has been read for these commands
/// (read R49-18): only Titanium's.
const fn reads_r49_18(dialect: GameDialect) -> bool {
    match dialect {
        GameDialect::Titanium => true,
        GameDialect::EqMac | GameDialect::Rof2 => false,
    }
}

/// `/ttell` and `/whotarget` as what they stand for, from a spelled-out
/// line. None for any other line.
pub(crate) fn targeted(command: &str, world: &ClientWorld) -> Option<Targeted> {
    let rest = command.strip_prefix('/')?;
    let (name, words) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    if name.eq_ignore_ascii_case("ttell") {
        Some(if reads_r49_18(world.generation()) {
            tell(words.trim(), world)
        } else {
            own_tell(words, world)
        })
    } else if name.eq_ignore_ascii_case("whotarget") {
        Some(if reads_r49_18(world.generation()) {
            who(world)
        } else {
            own_who(world)
        })
    } else {
        None
    }
}

/// The target's name as the server sent it and its kind: the player's own
/// name where they target themself, who are no spawn of the zone's.
fn target(world: &ClientWorld) -> Option<(&str, SpawnKind)> {
    let id = world.target().selected?;
    if let Some(player) = world.player().filter(|player| player.spawn_id == id) {
        return Some((player.name.as_str(), SpawnKind::Player));
    }
    world
        .spawn(id)
        .map(|spawn| (spawn.state.name.as_str(), spawn.state.kind))
}

/// The name the Titanium program sends a target's tell to, if it takes
/// the target (read R49-18 a, step 4).
fn recipient(name: &str, kind: SpawnKind) -> Option<&str> {
    if name.is_empty() {
        return None;
    }
    match kind {
        // Type 3 keeps the first test: a name, and not an NPC.
        SpawnKind::NpcCorpse => Some(name),
        SpawnKind::Player
        | SpawnKind::Npc
        | SpawnKind::PlayerCorpse
        | SpawnKind::Unknown(_) => (!name.contains(['#', '_']))
            .then(|| name.split('\'').next().unwrap_or(name)),
    }
}

/// Titanium's `/ttell` (read R49-18 a).
fn tell(message: &str, world: &ClientWorld) -> Targeted {
    if message.is_empty() {
        return Targeted::Nothing;
    }
    let Some((name, kind)) = target(world) else {
        return Targeted::Nothing;
    };
    // Labelled: the program gives type 2 to a creature that dies in view
    // (0x484e79), so it refuses such a corpse (its name has a `_`), where
    // this client, which keeps a creature's corpse apart, takes it.
    let Some(recipient) = recipient(name, kind) else {
        return Targeted::Say(Line::Refused);
    };
    if world
        .player()
        .is_some_and(|player| player.name.eq_ignore_ascii_case(recipient))
    {
        return Targeted::Say(Line::ToSelf);
    }
    // This client's guard, which no server's name meets: the tell is a
    // typed line, so a name with a space or a control in it is refused, as
    // it could not be carried as one.
    if recipient
        .chars()
        .any(|c| c.is_whitespace() || c.is_control())
    {
        return Targeted::Say(Line::Refused);
    }
    Targeted::Tell(Ok(format!("/tell {recipient} {message}")))
}

/// Titanium's `/whotarget` (read R49-18 b): the program runs its `/who`
/// word parser on the target's name alone.
fn who(world: &ClientWorld) -> Targeted {
    let Some((name, _)) = target(world) else {
        return Targeted::Nothing;
    };
    Targeted::Who(eq_client_core::who::parse(name).map(|request| request.filter))
}

/// This client's own `/ttell` for the unread programs: only another
/// player whose name is letters alone, as a typed `/tell` would name them.
fn own_tell(words: &str, world: &ClientWorld) -> Targeted {
    let own = world.player().map(|player| player.spawn_id);
    let recipient = world
        .target()
        .selected
        .and_then(|id| world.spawn(id))
        .filter(|spawn| {
            spawn.state.kind == SpawnKind::Player
                && Some(spawn.state.spawn_id) != own
                && !spawn.state.name.is_empty()
                && spawn
                    .state
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphabetic())
        })
        .map(|spawn| spawn.state.name.as_str());
    Targeted::Tell(
        recipient
            .map(|recipient| format!("/tell {recipient} {}", words.trim()))
            .ok_or_else(|| OWN_REFUSAL.to_owned()),
    )
}

/// This client's own `/whotarget` for the unread programs.
fn own_who(world: &ClientWorld) -> Targeted {
    Targeted::Who(
        target(world)
            .map(|(name, _)| WhoFilter {
                text: name.to_owned(),
                ..Default::default()
            })
            .ok_or_else(|| OWN_UNSEEN.to_owned()),
    )
}

/// Says the program line a `/ttell` asked for: the installation's words,
/// or this client's own where the installation lacks them.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn say(mut chat: ResMut<ChatState>, messages: Option<Res<Messages>>) {
    let Some(line) = chat.targeted_line.take() else {
        return;
    };
    let (id, fallback) = match line {
        Line::Refused => (REFUSED, "You cannot send a tell to that target."),
        Line::ToSelf => (TO_SELF, "You cannot send a tell to yourself."),
    };
    let said = messages
        .as_deref()
        .map_or_else(|| Said::own(fallback), |messages| {
            messages.said_or(id, &[], fallback)
        })
        .typed(COLOUR);
    chat.history.push(super::chat::system_line(said));
}

#[cfg(test)]
mod tests;
