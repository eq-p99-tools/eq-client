//! Sharing coins with the group, typed as the Titanium client takes it
//! (program reads R6 and R8): `/split` reads platinum, gold, silver and
//! copper in that order, each read as a C `atoi` reads it, so a word that is
//! not a number, or a number below one, shares none of that kind; the
//! session refuses outside a group, a split of nothing and more than the
//! purse holds, in the official client's lines. `/autosplit` takes the
//! installed client's words for off and on, compared over their first three
//! and two letters, and anything else, or nothing, turns it the other way;
//! the session then says where it was left. Whether the official compare
//! minds letter case was not read; this one does not (recording checklist
//! 6.412).
use crate::chat::{ChatState, Said, system_line};
use crate::contacts::abbreviates;
use crate::hud::messages::Messages;
use crate::online::OnlineState;
use crate::outbox::Outbox;
use bevy::prelude::*;
use eq_client_core::{ClientCommand, Coins};

/// The installed client's words that turn `/autosplit` off and on.
mod words {
    /// Off, compared over three letters.
    pub(super) const OFF: (u32, &str, usize) = (0x3292, "off", 3);
    /// On, compared over two letters.
    pub(super) const ON: (u32, &str, usize) = (0x3293, "on", 2);
}

/// A sharing command typed in the chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// `/split p g s c`.
    Split(Coins),
    /// `/autosplit [off|on]`, with the word typed.
    AutoSplit(String),
}

/// A number as C's `atoi` reads it, kept at zero or more: leading spaces,
/// a sign, then digits up to the first that is not one.
fn atoi(word: &str) -> u32 {
    let word = word.trim_start();
    let (negative, digits) = match word.as_bytes().first() {
        Some(b'-') => (true, &word[1..]),
        Some(b'+') => (false, &word[1..]),
        _ => (false, word),
    };
    let value = digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0_u32, |value, digit| {
            value
                .saturating_mul(10)
                .saturating_add(u32::from(digit - b'0'))
                .min(i32::MAX.unsigned_abs())
        });
    if negative { 0 } else { value }
}

impl Command {
    /// The sharing command a typed line is, by the official parser's
    /// prefix rule; None for any other line.
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let command = input.strip_prefix('/')?.trim();
        let (name, words) = command
            .split_once(char::is_whitespace)
            .unwrap_or((command, ""));
        if abbreviates(name, "split", 2) {
            let mut amounts = words.split_whitespace().map(atoi);
            let mut next = || amounts.next().unwrap_or(0);
            Some(Self::Split(Coins {
                platinum: next(),
                gold: next(),
                silver: next(),
                copper: next(),
            }))
        } else if abbreviates(name, "autosplit", 2) {
            Some(Self::AutoSplit(words.trim().to_owned()))
        } else {
            None
        }
    }
}

/// Whether the word typed is the installed client's word, over its first
/// letters.
fn names(typed: &str, (id, fallback, length): (u32, &str, usize), messages: &Messages) -> bool {
    let word = messages
        .formatted(id, &[])
        .unwrap_or_else(|| fallback.to_owned());
    let start = |text: &str| text.chars().take(length).collect::<String>();
    !typed.is_empty() && start(typed).eq_ignore_ascii_case(&start(&word))
}

/// Where `/autosplit` with this word leaves sharing, from where it is.
fn auto_split(typed: &str, now: Option<bool>, messages: &Messages) -> Option<bool> {
    if names(typed, words::OFF, messages) {
        Some(false)
    } else if names(typed, words::ON, messages) {
        Some(true)
    } else {
        now.map(|on| !on)
    }
}

/// Runs the sharing command the chat noted.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn commands(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut chat: ResMut<ChatState>,
    messages: Option<Res<Messages>>,
) {
    let Some(asked) = chat.sharing_asked.take() else {
        return;
    };
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let world = online.world();
    // The outbox and the session say a refusal themselves.
    match asked {
        Command::Split(coins) => {
            let _ = outbox.post(world, |stamp| ClientCommand::SplitCoins {
                session_id: stamp.session_id,
                coins,
            });
        }
        Command::AutoSplit(typed) => match auto_split(&typed, world.auto_split(), messages) {
            Some(on) => {
                let _ = outbox.post(world, |stamp| ClientCommand::SetAutoSplit {
                    session_id: stamp.session_id,
                    on,
                });
            }
            None => chat
                .history
                .push(system_line(Said::own("Your character is not ready yet."))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_reads_four_amounts_as_atoi_does() {
        let split = |line| match Command::typed(line) {
            Some(Command::Split(coins)) => {
                Some((coins.platinum, coins.gold, coins.silver, coins.copper))
            }
            _ => None,
        };
        assert_eq!(split("/split 1 2 3 4"), Some((1, 2, 3, 4)));
        assert_eq!(split("/sp 0 5"), Some((0, 5, 0, 0)));
        assert_eq!(split("/spl -3 x 7c +2"), Some((0, 0, 7, 2)));
        assert_eq!(
            split("/split 99999999999"),
            Some((i32::MAX.unsigned_abs(), 0, 0, 0))
        );
        assert_eq!(split("/s 1"), None);
        assert_eq!(split("/splitter 1"), None);
    }

    #[test]
    fn autosplit_takes_off_and_on_over_their_first_letters_or_toggles() {
        let messages = Messages::default();
        assert_eq!(
            Command::typed("/au"),
            Some(Command::AutoSplit(String::new()))
        );
        assert_eq!(
            Command::typed("/autosplit Off"),
            Some(Command::AutoSplit("Off".into()))
        );
        assert_eq!(auto_split("off", Some(false), &messages), Some(false));
        assert_eq!(auto_split("offer", Some(true), &messages), Some(false));
        assert_eq!(auto_split("of", Some(false), &messages), Some(true));
        assert_eq!(auto_split("on", Some(true), &messages), Some(true));
        assert_eq!(auto_split("once", None, &messages), Some(true));
        assert_eq!(auto_split("", Some(true), &messages), Some(false));
        assert_eq!(auto_split("yes", Some(false), &messages), Some(true));
        assert_eq!(auto_split("", None, &messages), None);
    }
}
