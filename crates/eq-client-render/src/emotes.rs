//! The social emote commands, as Titanium's worker 0x49e0c1 runs them
//! (reads R47, R47b and R47c): the target by the first word or the current
//! one, the 60-unit range, the still emotes' standing check, the text sent
//! and the player's own line, and the animation, sent and played at once
//! while the player stands still. The table and the wording are
//! [`eq_client_core::emotes`].
use crate::{
    chat::{ChatState, Said, system_line},
    hud::messages::Messages,
    online::OnlineState,
    outbox::Outbox,
};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, PostureState,
    emotes::{self, Emote, Refusal, Spoken, Strings, Target},
    world::ClientWorld,
};

/// The emote chat colour (Options colour row 7, 0x107), which the local
/// line prints in (0x4982c6, read R47c 3).
const EMOTE_COLOUR: u32 = 0x107;

/// The installed string table, as the emote wording reads it.
struct Installed<'a>(&'a Messages);

impl Strings for Installed<'_> {
    fn line(&self, id: u32, arguments: &[String]) -> Option<String> {
        self.0.formatted(id, arguments)
    }

    fn word(&self, id: u32) -> Option<String> {
        self.0.argument_free(id).map(str::to_owned)
    }
}

/// What an emote command comes to.
#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    /// Refused, with its line and nothing sent.
    Refused(Refusal),
    /// Nothing sent and nothing printed, as `/fidget` with a target.
    Silent,
    /// Said: the text sent, the player's own line and the animation due.
    Said {
        spoken: Spoken,
        animation: Option<u8>,
    },
}

/// A name as 0x492ec2 compares it: digits and '#' dropped, '_' read as a
/// space, case ignored (read R47c 1). The program's rewrite of the server's
/// pet and corpse suffixes into their shown form (0x473cc3) is stood in for
/// by the player corpse's apostrophe only (recording checklist 6.488).
fn normalised(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_ascii_digit() && *c != '#')
        .map(|c| match c {
            '_' => ' ',
            '`' => '\'',
            c => c,
        })
        .collect::<String>()
        .to_lowercase()
}

/// The spawn a typed name picks, as 0x47981d does (read R47c 1): any spawn
/// the client holds, the player's own included, whose name the word
/// begins; the shortest server name wins, then the first in the list
/// (this client's list runs by spawn id).
pub(crate) fn named_spawn(world: &ClientWorld, typed: &str) -> Option<u16> {
    let wanted = normalised(typed);
    if wanted.is_empty() {
        return None;
    }
    let own = world
        .player()
        .map(|player| (player.spawn_id, player.name.as_str()));
    world
        .spawns()
        .iter()
        .map(|(id, spawn)| (*id, spawn.state.name.as_str()))
        .chain(own.filter(|(id, _)| world.spawn(*id).is_none()))
        .filter(|(_, name)| normalised(name).starts_with(&wanted))
        .min_by_key(|(id, name)| (name.len(), *id))
        .map(|(id, _)| id)
}

/// A spawn's name as the lines show it.
fn shown(raw: &str) -> String {
    eq_client_core::entities::display_name(&raw.replace('`', "'"))
}

/// What the command comes to for this world, with the player `moving` or
/// not. `words` follow the command: a first word names the target, unless
/// it is the installed "my target" word; the rest are extra words.
pub(crate) fn decide(
    world: &ClientWorld,
    emote: &Emote,
    words: &str,
    moving: bool,
    messages: &Messages,
) -> Outcome {
    let Some(player) = world.player() else {
        return Outcome::Silent;
    };
    // An unknown posture counts as standing, the player's posture until
    // the server says otherwise (inferred, recording checklist 6.488).
    let standing = world
        .posture(player.spawn_id)
        .is_none_or(|posture| posture == PostureState::Standing);
    // The still emotes' own stubs (0x466e40 and the like, R47b g).
    if emote.stands_still && (!standing || moving) {
        return Outcome::Refused(Refusal::StandStill);
    }
    let words = words.trim_start();
    let (first, rest) = words.split_once(char::is_whitespace).unwrap_or((words, ""));
    let mine = messages.argument_free(emotes::MY_TARGET_WORD);
    let chosen = if first.is_empty() || mine.is_some_and(|mine| mine.eq_ignore_ascii_case(first)) {
        world.target().selected
    } else {
        match named_spawn(world, first) {
            Some(id) => Some(id),
            None => return Outcome::Refused(Refusal::NotFound),
        }
    };
    // A target that is the player counts as none, as does one the client
    // no longer holds.
    let target = chosen
        .filter(|id| *id != player.spawn_id)
        .and_then(|id| world.spawn(id));
    if let Some(target) = target {
        // Origin to origin, level only (0x439d16 with no height scale).
        let (from, to) = (player.position, target.state.position);
        if (to.x - from.x).hypot(to.y - from.y) > emotes::RANGE {
            return Outcome::Refused(Refusal::TooFar);
        }
    }
    let name = target.map(|target| shown(&target.state.name));
    let aimed = target.zip(name.as_deref()).map(|(target, name)| Target {
        name,
        gender: target.state.gender,
    });
    match emotes::speak(emote, player.gender, aimed, rest, &Installed(messages)) {
        Some(spoken) => Outcome::Said {
            spoken,
            animation: emote.animation.filter(|_| standing && !moving),
        },
        None => Outcome::Silent,
    }
}

/// Runs the emote commands typed or run by a social: refuses, or sends the
/// text, prints the player's own line in the emote colour, and sends and
/// plays the animation. The Mac-era client keeps this client's earlier
/// answer.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn run(
    mut chat: ResMut<ChatState>,
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    messages: Option<Res<Messages>>,
    settings: Option<Res<crate::ViewerSettings>>,
    controls: Option<Res<crate::motion::Controls>>,
) {
    if chat.emotes.is_empty() {
        return;
    }
    let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let moving = controls.is_some_and(|controls| controls.moving());
    for (case, words) in std::mem::take(&mut chat.emotes) {
        let Some(emote) = emotes::EMOTES.get(usize::from(case)) else {
            continue;
        };
        if !client.runs_social_emotes() {
            chat.refuse(Said::own(format!(
                "This client does not take /{} that way yet.",
                emote.name
            )));
            continue;
        }
        match decide(online.world(), emote, &words, moving, messages) {
            Outcome::Refused(refusal) => {
                let (id, ours) = refusal.line();
                chat.refuse(messages.said_text(id, ours));
            }
            Outcome::Silent => (),
            Outcome::Said { spoken, animation } => {
                say(&mut chat, &mut online, &outbox, &spoken, animation);
            }
        }
    }
}

/// Sends an emote's text as the program built it, spaces kept (the session
/// echoes nothing, as this prints the line), then prints the player's line
/// and sends and plays the animation, in the worker's order.
fn say(
    chat: &mut ChatState,
    online: &mut OnlineState,
    outbox: &Outbox,
    spoken: &Spoken,
    animation: Option<u8>,
) {
    let sent = outbox.post(online.world(), |stamp| ClientCommand::SocialEmote {
        session_id: stamp.session_id,
        text: spoken.sent.clone(),
    });
    if sent.is_err() {
        return;
    }
    let mut line = if spoken.official {
        Said::official(spoken.local.clone())
    } else {
        Said::own(spoken.local.clone())
    };
    line.message_type = Some(EMOTE_COLOUR);
    chat.history.push(system_line(line));
    let Some(animation) = animation else {
        return;
    };
    let posted = outbox.post(online.world(), |stamp| ClientCommand::EmoteAnimation {
        session_id: stamp.session_id,
        animation,
        created: stamp.created,
    });
    if posted.is_ok()
        && let Some(own) = online.world().player().map(|player| player.spawn_id)
    {
        online.play_motion(own, u16::from(animation));
    }
}

#[cfg(test)]
mod tests;
