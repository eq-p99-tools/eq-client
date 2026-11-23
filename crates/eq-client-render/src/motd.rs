//! `/motd`, which says the server's message of the day again (checklist
//! 6.520). The program gives it a handler of its own (0x46f698), next to
//! `/charinfo`'s and `/played`'s rather than `/getguildmotd`'s, so it is
//! taken to say again the message the world sent (`OP_MOTD`), sending
//! nothing (inferred, pending R49-6j). Words after it are ignored, as no
//! form with words is known. Without a message on this connection, the
//! line is this client's.
use super::chat::{ChatState, Said};
use bevy::prelude::*;

/// A `/motd` typed, waiting for [`answers`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Asked;

/// Whether a typed line, spelled out, is `/motd`.
pub(crate) fn typed(input: &str) -> bool {
    input
        .strip_prefix('/')
        .and_then(|command| command.split_whitespace().next())
        .is_some_and(|name| name.eq_ignore_ascii_case("motd"))
}

/// Answers a `/motd` the chat noted: the message as it came, or this
/// client's line when none came on this connection.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn answers(mut chat: ResMut<ChatState>, online: Res<super::online::OnlineState>) {
    if chat.motd_asked.take().is_none() {
        return;
    }
    if !chat.say_motd_again(&online) {
        chat.history.push(super::chat::system_line(Said::own(
            "There is no message of the day.",
        )));
    }
}

#[cfg(test)]
mod tests;
