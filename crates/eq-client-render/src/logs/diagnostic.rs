//! Explicit diagnostic chat tracing, separate from the character's `/log`.
use bevy::prelude::*;
use eq_client_core::chat::{ChatHistory, ChatTab};

/// A private tester can opt in with `eq_client_render::chat_lines=trace`.
/// Default logging emits no chat contents. Only finalized visible history is
/// used: no login input, command drafts, packet bytes or inventory records.
#[allow(clippy::needless_pass_by_value)] // Bevy's system parameters.
pub(crate) fn trace_lines(
    online: Res<crate::online::OnlineState>,
    chat: Res<crate::chat::ChatState>,
    messages: Option<Res<crate::hud::messages::Messages>>,
    mut seen: Local<u64>,
) {
    trace_history(
        &chat.history,
        online.world().player().map(|player| player.name.as_str()),
        messages.as_deref(),
        &mut seen,
    );
}

fn trace_history(
    history: &ChatHistory,
    player: Option<&str>,
    messages: Option<&crate::hud::messages::Messages>,
    seen: &mut u64,
) {
    // Disabled or pre-admission lines are passed over, never replayed when
    // a diagnostic subscriber is enabled or a character enters later.
    let after = std::mem::replace(seen, history.revision());
    let Some(player) = player.filter(|_| {
        bevy::log::tracing::enabled!(
            target: "eq_client_render::chat_lines",
            bevy::log::Level::TRACE
        )
    }) else {
        return;
    };
    for (id, line) in history
        .lines(ChatTab::All)
        .into_iter()
        .filter(|(id, _)| *id > after)
    {
        let message = super::shown(line, player, messages);
        trace!(
            target: "eq_client_render::chat_lines",
            line_id = id,
            channel = ?line.channel,
            text = %message.text,
            "Chat line"
        );
    }
}

#[cfg(test)]
mod tests;
