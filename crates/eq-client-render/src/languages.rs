//! Spoken languages: `/language`, the tongue each chat window speaks, the
//! tags and garbles of heard lines, and the away reply (parity item 9,
//! recording checklist 6.495).
//!
//! The Titanium program's rules are read (R49-4):
//!
//! - **The choice is per chat window**, an index into the languages the
//!   character knows (each of 0 to 31 the profile gives skill in, in order).
//!   It lives with the window for the session and is never saved (Adam,
//!   11-23: no saved choice).
//! - **`/language`** answers in the installed lines, message type 0x111:
//!   the help word lists the known languages under 0x33cc, nothing says the
//!   current one (0x33ce), a number in the list chooses it, and anything
//!   else is 0x33cd.
//! - **Every line goes out in the active window's tongue**, on every channel,
//!   with the skill as the program words it ([`Tongue::spoken`]).
//! - **A tell heard while away** gets the program's away reply (0x58b), in
//!   the same tongue at the skill as it is, unless it is itself an away
//!   reply.
//!
//! The `EQMac` program's `/language`, send and away reply aren't read, so the
//! Titanium rules are used there (inferred).
use crate::{
    chat::{ChatState, Said, system_line},
    hud::messages::Messages,
    online::OnlineState,
    outbox::Outbox,
    windows::WindowId,
};
use bevy::prelude::*;
use eq_client_assets::skills::SkillRules;
use eq_client_core::{
    ClientCommand, OutboundChat, PlayerState,
    chat::Tongue,
    tongues::{self, Heard},
    world::ClientWorld,
};
use std::collections::BTreeMap;

/// The message type every `/language` line comes in (read R49-4 (a)).
const LANGUAGE_TYPE: u32 = 0x111;
/// The list's header.
const HEADER: u32 = 0x33cc;
/// A number outside the known list.
const BAD_NUMBER: u32 = 0x33cd;
/// The current language: %1 its name, %2 its number in the list.
const CURRENT: u32 = 0x33ce;
/// The word that lists the languages.
const HELP_WORD: u32 = 0x33f4;
/// A heard tongue's tag with its name (%1).
const NAMED_TAG: u32 = 0x583;
/// A heard tongue's tag when it isn't known well enough.
const UNKNOWN_TAG: u32 = 0x584;
/// The lines an away reply begins with, which get no reply in turn.
const AWAY_PREFIXES: [u32; 2] = [0x589, 0x58a];
/// The away reply.
const AWAY_REPLY: u32 = 0x58b;

/// Each chat window's choice: an index into the known list, 0 until chosen.
#[derive(Default)]
pub(crate) struct Spoken(BTreeMap<WindowId, usize>);

impl Spoken {
    fn index(&self, window: WindowId) -> usize {
        self.0.get(&window).copied().unwrap_or(0)
    }

    fn choose(&mut self, window: WindowId, index: usize) {
        self.0.insert(window, index);
    }

    /// Forgets every window's choice, as the session ends.
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    /// The language a window speaks: its index into the known list, or 0
    /// when the index is outside it (read R49-4 (b)).
    fn language(&self, window: WindowId, player: Option<&PlayerState>) -> u8 {
        let known = known(player);
        known.get(self.index(window)).copied().unwrap_or(0)
    }
}

/// A tell heard while away, waiting for its reply.
pub(crate) struct AwayTell {
    /// Who told.
    pub sender: String,
    /// What they said, to test for an away reply's own lines.
    pub text: String,
}

/// The words of a typed `/language`, if it is one.
pub(crate) fn typed(input: &str) -> Option<String> {
    let command = input.strip_prefix('/')?;
    let (name, words) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    name.eq_ignore_ascii_case("language")
        .then(|| words.trim().to_owned())
}

/// Whether the player carries the GM flag: the own spawn record's
/// (Titanium's spawn +0x256, as read R49-4 takes it), or the listing's.
pub(crate) fn game_master(player: &PlayerState) -> bool {
    player.appearance.titanium_gm.is_some_and(|flag| flag != 0) || player.listing.game_master
}

fn known(player: Option<&PlayerState>) -> Vec<u8> {
    player
        .and_then(|player| player.languages.as_deref())
        .map(tongues::known)
        .unwrap_or_default()
}

/// A language's installed name, as the program's name routine gives it: the
/// common tongue by the world's selector.
pub(crate) fn language_name(
    world: &ClientWorld,
    messages: Option<&Messages>,
    language: u8,
) -> String {
    let selector = (language == tongues::COMMON).then(|| {
        world
            .language_zero_selector()
            .is_some_and(eq_client_core::rules::LanguageZeroSelector::alternate_name)
    });
    SkillRules::language_name_string(language, selector)
        .and_then(|id| messages?.argument_free(id))
        .map_or_else(|| format!("Language {language}"), str::to_owned)
}

/// What fills a heard line's tongue place: 0x583 with the name, or 0x584
/// (read R49-4 (c)), in this client's words where the installation lacks
/// them.
pub(crate) fn tongue_place(messages: Option<&Messages>, heard: Heard) -> String {
    // Language 0, the only one whose name the world's selector picks, has
    // no tag.
    let name = SkillRules::language_name_string(heard.language, None)
        .and_then(|id| messages?.argument_free(id))
        .map_or_else(|| format!("Language {}", heard.language), str::to_owned);
    if heard.named {
        messages
            .and_then(|messages| messages.formatted(NAMED_TAG, std::slice::from_ref(&name)))
            .unwrap_or_else(|| format!(", in {name}"))
    } else {
        messages
            .and_then(|messages| messages.argument_free(UNKNOWN_TAG))
            .map_or_else(|| ", in an unknown tongue".to_owned(), str::to_owned)
    }
}

/// What the program weighs as it hears a line from `sender`.
pub(crate) fn hearing<'a>(world: &'a ClientWorld, sender: &str) -> Option<tongues::Hearing<'a>> {
    let player = world.player()?;
    Some(tongues::Hearing {
        player: &player.name,
        own: player.languages.as_deref(),
        game_master: game_master(player),
        sender_game_master: world.game_master_in_zone(sender),
        listener_stage: world.listener_garble(),
        letters: match world.generation() {
            eq_client_core::GameDialect::EqMac => tongues::Letters::EqMac,
            // RoF2's are Titanium's, inferred (6.489).
            eq_client_core::GameDialect::Titanium | eq_client_core::GameDialect::Rof2 => {
                tongues::Letters::Titanium
            }
        },
    })
}

impl ChatState {
    /// Hears a line in a tongue, after the filters let it through: its tag
    /// and garble, and an away reply owed for a tell.
    pub(crate) fn hear_tongue(
        &mut self,
        line: &mut eq_client_core::chat::ChatLine,
        world: &ClientWorld,
        sender: &str,
    ) {
        // The away test reads the tell as it arrived, before any garble: a
        // garbled away reply that no longer starts with its line would
        // otherwise be answered, and two away players could answer each
        // other for ever.
        if line.channel == eq_client_core::chat::ChannelName::Tell
            && world.player().is_some_and(|player| player.listing.away)
            && !sender.eq_ignore_ascii_case(world.player().map_or("", |player| &player.name))
        {
            self.away_tells.push(AwayTell {
                sender: sender.to_owned(),
                text: line.message.text.clone(),
            });
        }
        if let Some(hearing) = hearing(world, sender) {
            tongues::hear(line, &hearing);
        }
    }
}

/// Answers a `/language` (read R49-4 (a)).
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn command(
    mut chat: ResMut<ChatState>,
    online: Res<OnlineState>,
    messages: Option<Res<Messages>>,
) {
    let Some(words) = chat.language_asked.take() else {
        return;
    };
    let messages = messages.as_deref();
    let world = online.world();
    let window = chat.active_window();
    let known = known(world.player());
    let say = |chat: &mut ChatState, said: Said| {
        chat.history.push(system_line(said.typed(LANGUAGE_TYPE)));
    };
    let official = |id: u32, arguments: &[String], fallback: String| {
        messages
            .and_then(|messages| messages.formatted(id, arguments))
            .map_or_else(|| Said::own(fallback), Said::official)
    };
    let current = |chat: &ChatState| {
        let index = chat.languages.index(window);
        let language = known.get(index).copied().unwrap_or(0);
        let name = language_name(world, messages, language);
        let number = (index + 1).to_string();
        official(
            CURRENT,
            &[name.clone(), number.clone()],
            format!("You are speaking {name} ({number})."),
        )
    };
    let help = messages
        .and_then(|messages| messages.argument_free(HELP_WORD))
        .is_some_and(|word| words.eq_ignore_ascii_case(word.trim()));
    if help {
        say(
            &mut chat,
            official(HEADER, &[], "Languages you know:".to_owned()),
        );
        for (index, language) in known.iter().enumerate() {
            let name = language_name(world, messages, *language);
            say(&mut chat, Said::official(format!("{} - {name}", index + 1)));
        }
        let line = current(&chat);
        say(&mut chat, line);
        return;
    }
    if words.is_empty() {
        let line = current(&chat);
        say(&mut chat, line);
        return;
    }
    // atol: leading digits, so "abc" is 0 and refused like any number
    // outside the list.
    let number: usize = words
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    match number.checked_sub(1).filter(|index| *index < known.len()) {
        Some(index) => {
            chat.languages.choose(window, index);
            let line = current(&chat);
            say(&mut chat, line);
        }
        None => say(
            &mut chat,
            official(
                BAD_NUMBER,
                &[],
                "That is not a language you know.".to_owned(),
            ),
        ),
    }
}

/// Keeps the outbox speaking the active window's language.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn speak(chat: Res<ChatState>, online: Res<OnlineState>, outbox: Res<Outbox>) {
    let language = chat
        .languages
        .language(chat.active_window(), online.world().player());
    outbox.set_language(language);
}

/// Sends the away reply to each tell heard while away (read R49-4 (b)): a
/// tell back on channel 7, in the active window's tongue at the skill as
/// it is, unless the tell begins with an away reply's own line.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn away(
    mut chat: ResMut<ChatState>,
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    messages: Option<Res<Messages>>,
) {
    if chat.away_tells.is_empty() {
        return;
    }
    let tells = std::mem::take(&mut chat.away_tells);
    let messages = messages.as_deref();
    let world = online.world();
    let Some(player) = world.player() else {
        return;
    };
    let Some(reply) = messages.and_then(|messages| messages.argument_free(AWAY_REPLY)) else {
        return;
    };
    let prefixes: Vec<&str> = AWAY_PREFIXES
        .iter()
        .filter_map(|id| messages?.argument_free(*id))
        .filter(|prefix| !prefix.is_empty())
        .collect();
    let language = chat.languages.language(chat.active_window(), Some(player));
    let tongue = Tongue::unslurred(player, language, game_master(player)).unwrap_or(Tongue::COMMON);
    for tell in tells {
        if prefixes.iter().any(|prefix| tell.text.starts_with(prefix)) {
            continue;
        }
        let _ = outbox.post(world, |stamp| ClientCommand::SendChatInAdmission {
            session_id: stamp.session_id,
            message: OutboundChat::Tell {
                recipient: tell.sender.clone(),
                message: reply.to_owned(),
            },
            tongue,
        });
    }
}

#[cfg(test)]
mod tests;
