use super::{Changes, ClientWorld, Notice};
use crate::control::StunState;

impl ClientWorld {
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
