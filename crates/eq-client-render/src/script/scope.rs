//! An optional caller-authored world identity, checked before every driver turn.
use crate::online::OnlineState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExpectedWorld {
    pub admission_id: u64,
    pub zone: String,
}

impl ExpectedWorld {
    /// Transfers withdraw the identity immediately, even before a new admission.
    pub(super) fn current(online: &OnlineState) -> Option<Self> {
        let world = online.world();
        let admission_id = world.session_id().filter(|_| {
            online.enabled && world.connected() && world.pending_transfer().is_none()
        })?;
        Some(Self {
            admission_id,
            zone: world.zone().to_owned(),
        })
    }

    pub(super) fn refusal(&self, online: &OnlineState) -> Option<String> {
        let observed = Self::current(online);
        if observed.as_ref() == Some(self) {
            return None;
        }
        let actual = observed.map_or_else(
            || "no connected admission outside a zone transfer".to_owned(),
            |actual| format!("admission {} in zone {}", actual.admission_id, actual.zone),
        );
        Some(format!(
            "World scope refused: expected admission {} in zone {}; observed {actual}. Read status and explicitly start a new run for the intended world.",
            self.admission_id, self.zone
        ))
    }
}
