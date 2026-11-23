//! Slash commands of the Titanium program that players run in socials and
//! hotbuttons and that the client answers with a line of its own (reads R45,
//! R45b and R46): `/yell`, `/stopcast` and a plain `/assist`. Each line is the installed client's
//! string where it has one; this client's words stand in otherwise. On a
//! server that offers neither send, each keeps today's answer.
use super::chat::{ChatState, Said};
use super::hud::messages::Messages;
use crate::outbox::{self, Outbox};
use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand, world::ClientWorld};

/// `/yell`'s line, said as the yell goes (0x46791a).
const YELLED: u32 = 0x331f;
/// `/stopcast`'s refusal without the player's spawn or on a mount (0x46a75a).
const CANNOT_STOP: u32 = 0xde6;
/// The summon-mount effect. A buff with it stands in for the program's own
/// mount test, which is unread (recording checklist 6.487).
const SUMMON_MOUNT: u32 = 113;
/// A plain `/assist` with no target: two lines, and nothing sent (0x46abc4,
/// read R46 section 5).
const UNASSISTED: [(u32, &str); 2] = [
    (0x33d1, "You have no target to assist."),
    (0x33d2, "Target someone, then use /assist."),
];

/// Which the player asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// `/yell`: a call for help to the players near.
    Yell,
    /// `/stopcast`: ending the player's own cast.
    StopCast,
    /// `/assist` with no word: taking the target's target.
    Assist,
}

impl Asked {
    /// What a typed line asks, spelled out, if it is one of these. Words
    /// after either are ignored, as the handlers read none.
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let command = input.strip_prefix('/')?;
        let name = command.split_whitespace().next().unwrap_or_default();
        if name.eq_ignore_ascii_case("yell") {
            Some(Self::Yell)
        } else if name.eq_ignore_ascii_case("stopcast") {
            Some(Self::StopCast)
        } else if name.eq_ignore_ascii_case("assist") && command.split_whitespace().nth(1).is_none()
        {
            // A word after it is an option, a main-assist form or a name,
            // which the chat's own `/assist` answers.
            Some(Self::Assist)
        } else {
            None
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Yell => "yell",
            Self::StopCast => "stopcast",
            Self::Assist => "assist",
        }
    }

    const fn capability(self) -> Capability {
        match self {
            Self::Yell => Capability::Yelling,
            Self::StopCast => Capability::StoppingCasts,
            Self::Assist => Capability::Assisting,
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

/// The lines and the command for what the player asked.
fn answer(
    asked: Asked,
    world: &ClientWorld,
    spells: Option<&dyn eq_client_core::world::SpellCatalog>,
    messages: &Messages,
) -> Result<(Option<Said>, Option<Asked>), Said> {
    if !outbox::offered(world, asked.capability()) {
        return Err(Said::own(format!(
            "This client does not take /{} that way yet.",
            asked.name()
        )));
    }
    Ok(match asked {
        Asked::Yell => (
            Some(messages.said_text(YELLED, "You yell for help.")),
            Some(asked),
        ),
        Asked::StopCast => {
            if world.player().is_none() || mounted(world, spells) {
                return Err(messages.said_text(CANNOT_STOP, "You can't stop casting now."));
            }
            (None, Some(asked))
        }
        Asked::Assist => (None, Some(asked)),
    })
}

/// A plain `/assist`'s refusal with no target: the two installed lines on
/// a client that runs R46's forms, else this client's earlier answer.
fn unassisted(client: eq_client_assets::ui::InstalledClient, messages: &Messages) -> Vec<Said> {
    if client.runs_r46_forms() {
        UNASSISTED
            .into_iter()
            .map(|(id, ours)| messages.said_text(id, ours))
            .collect()
    } else {
        vec![Said::own("Target someone to assist first.")]
    }
}

/// Answers a `/yell` or `/stopcast` the chat noted: the line, then the send.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn answers(
    mut chat: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    outbox: Res<Outbox>,
    messages: Option<Res<Messages>>,
    (names, settings): (
        Option<Res<super::spellbook::SpellNames>>,
        Option<Res<crate::ViewerSettings>>,
    ),
) {
    let Some(asked) = chat.slash.take() else {
        return;
    };
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let target = online.world().target().selected;
    if asked == Asked::Assist && target.is_none() {
        let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
        for line in unassisted(client, messages) {
            chat.refuse(line);
        }
        return;
    }
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
                    Asked::Yell => ClientCommand::YellForHelp {
                        session_id: stamp.session_id,
                        created: stamp.created,
                    },
                    Asked::StopCast => ClientCommand::StopCasting {
                        session_id: stamp.session_id,
                        created: stamp.created,
                    },
                    // A self target is the session's refusal (0x57b).
                    Asked::Assist => ClientCommand::Assist {
                        session_id: stamp.session_id,
                        spawn_id: target.unwrap_or_default(),
                    },
                });
            }
        }
        Err(refusal) => chat.refuse(refusal),
    }
}

#[cfg(test)]
mod tests;
