//! Slash commands of the Titanium program that players run in socials and
//! hotbuttons and that the client answers with a line of its own (reads R45
//! and R45b): `/yell` and `/stopcast`; and `/duel`, a challenge to the
//! player targeted (checklist 6.492b), which on Titanium sends the target
//! as it stands and the session words as the program does (read R49-6h),
//! and elsewhere keeps this client's checks and lines; and `/lfg`, looking
//! for a group (checklist 6.515), whose forms and lines are this client's
//! until its handler is read (pending R49-6i). Each line is the
//! installed client's string where it has one; this client's words stand
//! in otherwise. On a server that offers none of the sends, each keeps
//! today's answer.
use super::chat::{ChatState, Said};
use super::hud::messages::Messages;
use crate::outbox::{self, Outbox};
use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand, world::ClientWorld};

/// `/yell`'s line, said as the yell goes (0x46791a).
const YELLED: u32 = 0x331f;
/// `/stopcast`'s refusal without the player's spawn or on a mount (0x46a75a).
const CANNOT_STOP: u32 = 0xde6;
/// `/lfg on`'s line (read R49-6i).
const LFG_ON: u32 = 0x334e;
/// `/lfg off`'s line.
const LFG_OFF: u32 = 0x334f;
/// `/lfg`'s usage line, for any other word or none, where the program
/// would open its LFG window if it had one.
const LFG_USAGE: u32 = 0x3350;
/// The summon-mount effect. A buff with it stands in for the program's own
/// mount test, which is unread (recording checklist 6.487).
const SUMMON_MOUNT: u32 = 113;

/// Which the player asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// `/yell`: a call for help to the players near.
    Yell,
    /// `/stopcast`: ending the player's own cast.
    StopCast,
    /// `/duel`: a challenge to the player targeted; `named` when words
    /// followed it.
    Duel {
        /// Whether words followed the command.
        named: bool,
    },
    /// `/lfg`: looking for a group, as its words ask.
    Lfg(LfgWords),
}

/// What followed `/lfg`. Only `on` and `off` are taken, as this client's
/// own forms: the program's are not read (checklist 6.515, pending
/// R49-6i).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LfgWords {
    /// Nothing: turn it over.
    None,
    /// `on`.
    On,
    /// `off`.
    Off,
    /// Anything else, which sends nothing.
    Other,
}

/// What goes to the session for what the player asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sent {
    /// A yell for help.
    Yell,
    /// The cast's end.
    StopCast,
    /// A challenge to this player.
    Duel {
        /// The player challenged.
        target: u16,
    },
    /// Looking for a group: set, or turned over for None.
    Looking(Option<bool>),
}

impl Asked {
    /// What a typed line asks, spelled out, if it is one of these. Words
    /// after `/yell` or `/stopcast` are ignored, as the handlers read none;
    /// words after `/duel` are noted.
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let command = input.strip_prefix('/')?;
        let mut words = command.split_whitespace();
        let name = words.next().unwrap_or_default();
        if name.eq_ignore_ascii_case("yell") {
            Some(Self::Yell)
        } else if name.eq_ignore_ascii_case("stopcast") {
            Some(Self::StopCast)
        } else if name.eq_ignore_ascii_case("duel") {
            Some(Self::Duel {
                named: words.next().is_some(),
            })
        } else if name.eq_ignore_ascii_case("lfg") {
            let first = words.next();
            Some(Self::Lfg(match (first, words.next()) {
                (None, _) => LfgWords::None,
                (Some(word), None) if word.eq_ignore_ascii_case("on") => LfgWords::On,
                (Some(word), None) if word.eq_ignore_ascii_case("off") => LfgWords::Off,
                _ => LfgWords::Other,
            }))
        } else {
            None
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Yell => "yell",
            Self::StopCast => "stopcast",
            Self::Duel { .. } => "duel",
            Self::Lfg(_) => "lfg",
        }
    }

    const fn capability(self) -> Capability {
        match self {
            Self::Yell => Capability::Yelling,
            Self::StopCast => Capability::StoppingCasts,
            Self::Duel { .. } => Capability::Dueling,
            Self::Lfg(_) => Capability::LookingForGroup,
        }
    }
}

/// Whether a buff the player holds summons a mount, or has effects the
/// spell data can't name, which counts as one (the stricter side).
fn mounted(world: &ClientWorld, spells: Option<&dyn eq_client_core::world::SpellCatalog>) -> bool {
    world.buffs().spells().any(|spell| {
        spells
            .and_then(|spells| spells.effect_ids(spell))
            .is_none_or(|effects| effects.contains(&SUMMON_MOUNT))
    })
}

/// The player targeted, if it is another player in the zone: the rule the
/// session applies before it sends a challenge, so this line never
/// contradicts its refusal.
fn challenged(world: &ClientWorld) -> Option<u16> {
    let id = world.target().selected?;
    if world.is_player(id) {
        return None;
    }
    world
        .spawn(id)
        .filter(|spawn| spawn.state.kind == eq_client_core::SpawnKind::Player)
        .map(|_| id)
}

/// Whether a generation's `/duel`, `/lfg`, `/motd` and `/charinfo` are the
/// Titanium program's as read R49-6 (h, i, j and m) found them. The others
/// are unread and keep this client's checks and lines.
pub(crate) const fn reads_r49_6(dialect: eq_client_core::GameDialect) -> bool {
    match dialect {
        eq_client_core::GameDialect::Titanium => true,
        eq_client_core::GameDialect::EqMac | eq_client_core::GameDialect::Rof2 => false,
    }
}

/// The lines and the command for what the player asked.
fn answer(
    asked: Asked,
    world: &ClientWorld,
    spells: Option<&dyn eq_client_core::world::SpellCatalog>,
    messages: &Messages,
) -> Result<(Option<Said>, Option<Sent>), Said> {
    if !outbox::offered(world, asked.capability()) {
        return Err(Said::own(format!(
            "This client does not take /{} that way yet.",
            asked.name()
        )));
    }
    Ok(match asked {
        Asked::Yell => (
            Some(messages.said_text(YELLED, "You yell for help.")),
            Some(Sent::Yell),
        ),
        Asked::StopCast => {
            if world.player().is_none() || mounted(world, spells) {
                return Err(messages.said_text(CANNOT_STOP, "You can't stop casting now."));
            }
            (None, Some(Sent::StopCast))
        }
        // The Titanium program reads no words and takes the target, whoever
        // it is (read R49-6h, 0x472a67): its session words every outcome
        // as the program's line.
        Asked::Duel { .. } if reads_r49_6(world.generation()) => (
            None,
            Some(Sent::Duel {
                target: world.target().selected.unwrap_or(0),
            }),
        ),
        // Elsewhere the handler is unread: words after `/duel` send nothing
        // rather than challenge whoever is targeted.
        Asked::Duel { named: true } => {
            return Err(Said::own(
                "This client does not take a name after /duel yet; target the player and type /duel.",
            ));
        }
        // The Titanium program says on or off as it sends, and anything
        // else is its usage line, as this client has no LFG window to open
        // (read R49-6i, 0x468694), all in colour 0x111.
        Asked::Lfg(words) if reads_r49_6(world.generation()) => {
            let line = |id, fallback| Some(messages.said_text(id, fallback).typed(0x111));
            match words {
                LfgWords::On => (
                    line(LFG_ON, "You are now looking for a group."),
                    Some(Sent::Looking(Some(true))),
                ),
                LfgWords::Off => (
                    line(LFG_OFF, "You are no longer looking for a group."),
                    Some(Sent::Looking(Some(false))),
                ),
                LfgWords::None | LfgWords::Other => {
                    (line(LFG_USAGE, "Usage: /lfg on or /lfg off"), None)
                }
            }
        }
        // Elsewhere the session tells the change, and says nothing for a
        // state the player is in already.
        Asked::Lfg(words) => match words {
            LfgWords::None => (None, Some(Sent::Looking(None))),
            LfgWords::On => (None, Some(Sent::Looking(Some(true)))),
            LfgWords::Off => (None, Some(Sent::Looking(Some(false)))),
            LfgWords::Other => {
                return Err(Said::own(
                    "This client takes /lfg alone, /lfg on or /lfg off.",
                ));
            }
        },
        // No line on sending: whether the program says one is unread, and
        // the session may still refuse (one challenge at a time, paced).
        Asked::Duel { named: false } => match challenged(world) {
            Some(target) => (None, Some(Sent::Duel { target })),
            None => {
                return Err(Said::own(
                    "You must first target another player to challenge to a duel.",
                ));
            }
        },
    })
}

/// Answers a `/yell`, `/stopcast` or `/duel` the chat noted: the line,
/// then the send.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn answers(
    mut chat: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    outbox: Res<Outbox>,
    messages: Option<Res<Messages>>,
    names: Option<Res<super::spellbook::SpellNames>>,
) {
    let Some(asked) = chat.slash.take() else {
        return;
    };
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let spells = names
        .as_deref()
        .map(|names| names as &dyn eq_client_core::world::SpellCatalog);
    let world = online.world();
    match answer(asked, world, spells, messages) {
        Ok((line, send)) => {
            if let Some(line) = line {
                chat.history.push(super::chat::system_line(line));
            }
            if let Some(send) = send {
                let _ = outbox.post(world, |stamp| match send {
                    Sent::Yell => ClientCommand::YellForHelp {
                        session_id: stamp.session_id,
                        created: stamp.created,
                    },
                    Sent::StopCast => ClientCommand::StopCasting {
                        session_id: stamp.session_id,
                        created: stamp.created,
                    },
                    Sent::Duel { target } => ClientCommand::ChallengeDuel {
                        session_id: stamp.session_id,
                        target,
                        created: stamp.created,
                    },
                    Sent::Looking(on) => ClientCommand::LookForGroup {
                        session_id: stamp.session_id,
                        on,
                    },
                });
            }
        }
        Err(refusal) => chat.refuse(refusal),
    }
}

#[cfg(test)]
mod tests;
