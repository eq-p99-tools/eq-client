use super::{Changes, ClientWorld, Notice};
use crate::control::StunState;

impl ClientWorld {
    /// The existing posture owner decides the transition, from a received
    /// HP report and counted equipment. Its echo never revives a dead player.
    pub(super) fn project_unconscious(
        &mut self,
        session_id: u64,
        revision: u64,
        unconscious: bool,
        basis: crate::control::HitPointBasis,
        news: &mut Changes,
    ) {
        if self.session_id != Some(session_id)
            || self.death.is_some()
            || !self.generation.models_unconscious()
            || basis
                .current()
                .is_none_or(|current| (current < 1) != unconscious)
            || self.own_unconscious.is_some_and(|(previous, state, _)| {
                revision < previous || revision == previous && state != unconscious
            })
        {
            news.ignored = true;
            return;
        }
        let previous = self.own_unconscious.is_some_and(|(_, state, _)| state);
        self.own_unconscious = Some((revision, unconscious, basis));
        self.own_hp_count_current = true;
        if previous != unconscious {
            news.notices.push(Notice::ServerString {
                id: if unconscious { 12877 } else { 12876 },
                arguments: Vec::new(),
                message_type: Some(if unconscious { 15 } else { 13 }),
            });
        }
    }

    /// Project the session owner's interpreted state. The raw opcode is
    /// deliberately not a second client-side timer or control authority.
    pub(super) fn project_stun(
        &mut self,
        session_id: u64,
        revision: u64,
        state: StunState,
        news: &mut Changes,
    ) {
        if self.session_id != Some(session_id)
            || self
                .own_stun
                .is_some_and(|(current, _)| revision <= current)
        {
            news.ignored = true;
            return;
        }
        let previous = self.stun_state();
        self.own_stun = Some((revision, state));
        let id = match (previous, state) {
            (Some(StunState::Stunned), StunState::Clear) => Some(12498),
            (Some(StunState::Stunned), StunState::Stunned) | (_, StunState::Clear) => None,
            (_, StunState::Stunned) => Some(12479),
        };
        if let Some(id) = id {
            news.notices.push(Notice::ServerString {
                id,
                arguments: Vec::new(),
                message_type: Some(340),
            });
        }
    }
}
