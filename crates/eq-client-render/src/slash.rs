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

/// This client's own answer to `/assist` with no target, kept where the
/// installed client has no R46 lines or the server takes no assist.
pub(crate) const NO_TARGET: &str = "Target someone to assist first.";

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
        vec![Said::own(NO_TARGET)]
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
    // A server that takes no assist keeps the answer it had before R46's
    // lines: this client's no-target line, else the outbox's refusal.
    if asked == Asked::Assist && !outbox::offered(online.world(), asked.capability()) {
        chat.refuse(Said::own(if target.is_none() {
            NO_TARGET
        } else {
            outbox::UNAVAILABLE
        }));
        return;
    }
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
                    // The plain form's own test of a self target is unread
                    // (checklist 6.490): it goes as the target is.
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

/// `/assist on` and `/assist off`'s lines: the option's state after the
/// word (read R46 section 5). Which of the two is on is inferred from their
/// order (checklist 6.490).
const ATTACK_ON_ASSIST_LINES: [(u32, &str); 2] = [
    (0x33cf, "Attack on assist is on."),
    (0x33d0, "Attack on assist is off."),
];
/// The main-assist words (0x2252, 0x2253, 0x223d): the group and raid roles
/// are not decoded, so these keep this client's answer.
const MAIN_ASSIST_WORDS: [u32; 3] = [0x2252, 0x2253, 0x223d];
/// `/assist` naming the player: the line, in its colour, and nothing sent
/// (0x46ae21, read R49-8 c).
const ASSIST_SELF: (u32, &str) = (0x57b, "You cannot assist yourself.");
const ASSIST_SELF_COLOUR: u32 = 0x111;
/// This client's answer to a form it doesn't take yet.
const ASSIST_NOT_YET: &str = "This client does not take /assist that way yet.";

/// What an `/assist` with words comes to, on a client that runs R46's
/// forms (0x46ae21, read R49-8 c).
#[derive(Debug, PartialEq, Eq)]
enum AssistWords {
    /// The option word for on or off ([`crate::sharing::words`]), as
    /// `/autosplit` compares them (inferred, checklist 6.490).
    Option(bool),
    /// A main-assist word.
    MainAssist,
    /// A name: the spawn it picks, as 0x47981d picks one, if any.
    Named(Option<u16>),
}

fn assist_words(words: &str, world: &ClientWorld, messages: &Messages) -> AssistWords {
    let typed = words.split_whitespace().next().unwrap_or_default();
    if crate::sharing::names(typed, crate::sharing::words::ON, messages) {
        return AssistWords::Option(true);
    }
    if crate::sharing::names(typed, crate::sharing::words::OFF, messages) {
        return AssistWords::Option(false);
    }
    if MAIN_ASSIST_WORDS
        .into_iter()
        .filter_map(|id| messages.formatted(id, &[]))
        .any(|main| main.eq_ignore_ascii_case(typed))
    {
        return AssistWords::MainAssist;
    }
    AssistWords::Named(crate::emotes::named_spawn(world, typed))
}

/// Answers an `/assist` with words the chat noted. On a client that runs
/// R46's forms (read R49-8 c):
/// - the option words set Attack On Assist and say its state;
/// - a main-assist word keeps this client's answer;
/// - any other word names a spawn by 0x47981d's prefix lookup, and the
///   assist goes for it without changing the target; no match is silent,
///   and the player's own name says 0x57b and sends nothing.
///
/// The program's token expansion of the word (0x492f68) is not modelled
/// (checklist 6.490). Elsewhere, the word names a player in the zone, as
/// this client took it before.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn assist_named(
    mut chat: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    outbox: Res<Outbox>,
    messages: Option<Res<Messages>>,
    (settings, options): (
        Option<Res<crate::ViewerSettings>>,
        Option<ResMut<crate::options::OptionsState>>,
    ),
) {
    let Some(words) = chat.assist_words.take() else {
        return;
    };
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let world = online.world();
    let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
    let assist = |spawn_id| {
        let _ = outbox.post(world, |stamp| ClientCommand::Assist {
            session_id: stamp.session_id,
            spawn_id,
        });
    };
    if !client.runs_r46_forms() {
        match super::chat::assisted(&words, &online) {
            Ok(spawn_id) => assist(spawn_id),
            Err(refusal) => chat.refuse(Said::own(refusal)),
        }
        return;
    }
    match assist_words(&words, world, messages) {
        AssistWords::Option(on) => {
            if let Some(mut options) = options {
                options.options.attack_on_assist = on;
            }
            let (id, ours) = ATTACK_ON_ASSIST_LINES[usize::from(!on)];
            chat.history
                .push(super::chat::system_line(messages.said_text(id, ours)));
        }
        AssistWords::MainAssist => chat.refuse(Said::own(ASSIST_NOT_YET)),
        AssistWords::Named(_) if !outbox::offered(world, Capability::Assisting) => {
            chat.refuse(Said::own(outbox::UNAVAILABLE));
        }
        AssistWords::Named(Some(spawn_id))
            if world
                .player()
                .is_some_and(|player| player.spawn_id == spawn_id) =>
        {
            let (id, ours) = ASSIST_SELF;
            chat.history.push(super::chat::system_line(
                messages.said_text(id, ours).typed(ASSIST_SELF_COLOUR),
            ));
        }
        AssistWords::Named(Some(spawn_id)) => assist(spawn_id),
        AssistWords::Named(None) => (),
    }
}

#[cfg(test)]
mod tests;
