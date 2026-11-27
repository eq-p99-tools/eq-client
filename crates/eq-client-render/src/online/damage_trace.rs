//! Opt-in provenance from one applied update to its retained chat rows.
//! Ordinals belong only to this viewer diagnostic, never to the wire.
use bevy::log::tracing::{Level, enabled, trace};
use eq_client_core::{
    WorldEvent, WorldUpdate,
    chat::{ChatHistory, ChatTab},
    combat::Damage,
    world::{Changes, Notice},
};

enum Input {
    Damage(Damage),
    Chat,
    ServerMessage(u32),
}

pub(super) struct Receipt {
    ordinal: u64,
    admission: Option<u64>,
    before: u64,
    input: Input,
}

/// Capture before reduction/presentation, including ignored updates.
/// No chat bodies, string arguments or raw packet payloads are retained.
pub(super) fn begin(
    ordinal: &mut u64,
    update: &WorldUpdate,
    admission: Option<u64>,
    history: &ChatHistory,
) -> Option<Receipt> {
    if !enabled!(target: "eq_client_render::damage_lines", Level::TRACE) {
        return None;
    }
    let input = match update {
        WorldUpdate::Game(WorldEvent::Damage(damage)) => Input::Damage(*damage),
        WorldUpdate::Chat(_) => Input::Chat,
        WorldUpdate::ServerMessage { string_id, .. } => Input::ServerMessage(*string_id),
        _ => return None,
    };
    *ordinal = ordinal.checked_add(1)?;
    Some(Receipt {
        ordinal: *ordinal,
        admission,
        before: history.revision(),
        input,
    })
}

impl Receipt {
    /// Call after both notice formatting and received-chat insertion. The
    /// history IDs pair directly with the existing finalized chat-line trace.
    pub(super) fn finish(self, changes: &Changes, history: &ChatHistory) {
        let line_ids: Vec<_> = history
            .lines(ChatTab::All)
            .into_iter()
            .filter_map(|(id, _)| (id > self.before).then_some(id))
            .collect();
        let damage_notices = changes
            .notices
            .iter()
            .filter(|notice| matches!(notice, Notice::Damage { .. }))
            .count();
        let spell_impact_notices = changes
            .notices
            .iter()
            .filter(|notice| matches!(notice, Notice::SpellImpact { .. }))
            .count();
        let server_strings: Vec<_> = changes
            .notices
            .iter()
            .filter_map(|notice| match notice {
                Notice::ServerString { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        let (source, damage, string_id) = match &self.input {
            Input::Damage(damage) => ("damage", Some(damage), None),
            Input::Chat => ("chat", None, None),
            Input::ServerMessage(id) => ("server_message", None, Some(*id)),
        };
        trace!(
            target: "eq_client_render::damage_lines",
            update_ordinal = self.ordinal,
            admission_id = ?self.admission,
            source,
            damage = ?damage,
            string_id = ?string_id,
            ignored = changes.ignored,
            damage_notices,
            spell_impact_notices,
            server_strings = ?server_strings,
            history_ids = ?line_ids,
            "Applied update chat provenance"
        );
    }
}

#[cfg(test)]
mod tests;
