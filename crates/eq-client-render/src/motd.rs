//! `/motd`, which says the server's message of the day again (checklist
//! 6.520). The Titanium program reads no words and sends nothing: it says
//! the message it kept in line 0x32a5, colour 15, empty when none came
//! (read R49-6j, 0x46f698). Elsewhere the handler is unread, so the
//! message is said again as it came, and without one the line is this
//! client's.
use super::chat::{ChatState, Said};
use bevy::prelude::*;

/// The Titanium program's `/motd` line, with the kept message (read
/// R49-6j).
const MOTD_AGAIN: u32 = 0x32a5;

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
pub(super) fn answers(
    mut chat: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    messages: Option<Res<super::hud::messages::Messages>>,
) {
    if chat.motd_asked.take().is_none() {
        return;
    }
    if super::slash::reads_r49_6(online.world().generation()) {
        let kept = chat.kept_motd(&online).unwrap_or_default();
        let fallback = format!("Message of the day: {kept}");
        let line = messages.as_deref().map_or_else(
            || Said::own(fallback.clone()),
            |messages| messages.said_or(MOTD_AGAIN, std::slice::from_ref(&kept), &fallback),
        );
        chat.history.push(super::chat::system_line(line.typed(15)));
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
