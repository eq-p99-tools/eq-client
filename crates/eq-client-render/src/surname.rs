//! `/surname`, as the Titanium program runs it (read R49-10, checklist
//! 6.507): its local refusals in order, then always a timed yes-or-no
//! question before the request goes, and the server's answer.
//!
//! - Under level 20: line 0x328c, and nothing else.
//! - No word: with a surname already set, the program asks a question
//!   whose yes path isn't read (R49-10e), so this client says in its own
//!   words that the form waits for its read, and sends nothing (a gap). With
//!   none set: the usage line 0x328d.
//! - The words after the command 32 bytes or more: line 0x8bf. The read
//!   gives `strlen >= 0x20` on the handler's argument; that the argument is
//!   every word after the command, not the first word alone, is inferred.
//! - Otherwise the first word, read as the program's word reader reads it
//!   (' and " quotes taken), cut to 19 bytes at a character boundary and
//!   its case left: the question 0x235a names exactly that word, for 10 s.
//!   Yes sends it and says 0x235b; No, the time running out or the session
//!   ending sends nothing.
//!
//! The server's answer is always shown: refused, 0x55e; taken, 0x55f
//! naming the player and the surname. The name tag follows the zone's own
//! last-name update, not the answer.
//!
//! The lines come in colour 15 (the question in 0xc). Scripts run the same
//! handler, and answer the question as a person does. The `EQMac`
//! program's `/surname` isn't read (R49-10d); its servers' handlers say the
//! client refuses under 20 and cuts to 19, as the Titanium read does, so the
//! same flow runs there (inferred).
use crate::{
    chat::{ChatState, Said, system_line},
    confirm::{Asked, Question},
    hud::messages::Messages,
    online::OnlineState,
    outbox::Outbox,
};
use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand};
use std::time::{Duration, Instant};

/// The level under which the program refuses.
const LEVEL: u8 = 20;
/// The argument length the program refuses from, in bytes.
const TOO_LONG: usize = 0x20;
/// The longest word the program's 20-byte buffer keeps.
const LONGEST: usize = 19;
/// How long the question stays open.
pub(crate) const QUESTION_WAIT: Duration = Duration::from_secs(10);
/// The colour of the program's surname lines.
const LINE_TYPE: u32 = 15;
/// Under level 20.
const UNDER_LEVEL: u32 = 0x328c;
/// The usage line.
const USAGE: u32 = 0x328d;
/// Too long.
const LONG: u32 = 0x8bf;
/// The question: %1 the surname.
const QUESTION: u32 = 0x235a;
/// Said once the request is sent.
const SENT: u32 = 0x235b;
/// The server refused the surname.
const REFUSED: u32 = 0x55e;
/// The server took it: %1 the player, %2 the surname.
const TAKEN: u32 = 0x55f;

/// The words of a typed `/surname`, if it is one.
pub(crate) fn typed(input: &str) -> Option<String> {
    let command = input.strip_prefix('/')?;
    let (name, words) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    name.eq_ignore_ascii_case("surname")
        .then(|| words.trim_start().to_owned())
}

/// The first word as the program's word reader takes it: a quoted run
/// whole (' or "), else up to the first space.
fn first_word(words: &str) -> &str {
    let words = words.trim_start();
    if let Some(quote) = words
        .chars()
        .next()
        .filter(|first| matches!(first, '\'' | '"'))
    {
        let rest = &words[1..];
        return rest.find(quote).map_or(rest, |end| &rest[..end]);
    }
    words.split_whitespace().next().unwrap_or_default()
}

/// The word cut to the program's 19 bytes, at a character boundary.
fn cut(word: &str) -> &str {
    let mut end = word.len().min(LONGEST);
    while !word.is_char_boundary(end) {
        end -= 1;
    }
    &word[..end]
}

/// The surname the question asks about, waiting for its answer.
#[derive(Resource, Default)]
pub(crate) struct Pending {
    /// The word, the admission it was asked in, and when.
    pub(crate) asking: Option<(String, u64, Instant)>,
}

impl Pending {
    /// What the question says: 0x235a with the word.
    pub(crate) fn question(&self, messages: Option<&Messages>) -> String {
        let Some((word, ..)) = &self.asking else {
            return String::new();
        };
        messages
            .and_then(|messages| messages.formatted(QUESTION, std::slice::from_ref(word)))
            .unwrap_or_else(|| format!("Take {word} as your surname?"))
    }
}

/// What a `/surname` does, by the program's order.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// One of the program's lines, and nothing more.
    Line(u32, &'static str),
    /// This client's own line.
    Own(&'static str),
    /// Ask about this word.
    Ask(String),
}

fn outcome(words: &str, level: u8, surname_set: bool) -> Outcome {
    if level < LEVEL {
        return Outcome::Line(
            UNDER_LEVEL,
            "You must be level 20 or higher to choose a surname.",
        );
    }
    let words = words.trim_end();
    let word = cut(first_word(words));
    if word.is_empty() {
        return if surname_set {
            Outcome::Own("Changing a surname you already have waits for its program read.")
        } else {
            Outcome::Line(USAGE, "Use /surname and the surname you want.")
        };
    }
    if words.len() >= TOO_LONG {
        return Outcome::Line(LONG, "That surname is too long.");
    }
    Outcome::Ask(word.to_owned())
}

/// Answers a `/surname`: a line, or the question.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn command(
    (mut chat, online, messages): (ResMut<ChatState>, Res<OnlineState>, Option<Res<Messages>>),
    (outbox, mut dialog, mut pending): (Res<Outbox>, ResMut<Asked>, ResMut<Pending>),
) {
    let messages = messages.as_deref();
    let world = online.world();
    // The question's answer, its time running out, or its session ending.
    if let Some((word, session_id, since)) = pending.asking.clone() {
        if let Some(yes) = dialog.answer(Question::Surname) {
            pending.asking = None;
            if yes && world.session_id() == Some(session_id) {
                let sent = outbox.send(
                    world,
                    ClientCommand::Surname {
                        session_id,
                        surname: word,
                    },
                );
                if sent.is_ok() {
                    chat.history.push(system_line(line(
                        SENT,
                        "Your surname request was sent.",
                        messages,
                    )));
                }
            }
        } else if since.elapsed() >= QUESTION_WAIT || world.session_id() != Some(session_id) {
            if dialog.question() == Some(Question::Surname) {
                dialog.withdraw();
            }
            pending.asking = None;
        }
    }
    let Some(words) = chat.surname_asked.take() else {
        return;
    };
    let Some(player) = world.player() else {
        chat.refuse(Said::own("Choose a surname once in the world."));
        return;
    };
    // A server that doesn't take it from this client (Project 1999 among
    // them, until an attended use) is told nothing.
    if !world.can(Capability::Surnaming) {
        chat.refuse(Said::own(
            "This server does not take /surname from this client yet.",
        ));
        return;
    }
    let surname_set = !player.name_parts.last_name.is_empty();
    match outcome(&words, player.level, surname_set) {
        Outcome::Line(id, fallback) => chat.history.push(system_line(line(id, fallback, messages))),
        Outcome::Own(said) => chat
            .history
            .push(system_line(Said::own(said).typed(LINE_TYPE))),
        Outcome::Ask(surname) => {
            let Some(session_id) = world.session_id() else {
                return;
            };
            if pending.asking.is_none() && dialog.ask(Question::Surname) {
                pending.asking = Some((surname, session_id, Instant::now()));
            } else {
                chat.refuse(Said::own("Answer the open question first."));
            }
        }
    }
}

/// One of the program's surname lines, in colour 15.
fn line(id: u32, fallback: &str, messages: Option<&Messages>) -> Said {
    messages
        .and_then(|messages| messages.formatted(id, &[]))
        .map_or_else(|| Said::own(fallback), Said::official)
        .typed(LINE_TYPE)
}

/// The server's answer, as the program words it.
pub(crate) fn answer(
    accepted: bool,
    name: &str,
    last_name: &str,
    messages: Option<&Messages>,
) -> Said {
    let said = if accepted {
        let arguments = [name.to_owned(), last_name.to_owned()];
        messages
            .and_then(|messages| messages.formatted(TAKEN, &arguments))
            .map_or_else(
                || Said::own(format!("Your surname is now {last_name}.")),
                Said::official,
            )
    } else {
        messages
            .and_then(|messages| messages.formatted(REFUSED, &[]))
            .map_or_else(
                || Said::own("The server refused that surname."),
                Said::official,
            )
    };
    said.typed(LINE_TYPE)
}

#[cfg(test)]
mod tests;
