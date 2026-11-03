//! Local loot dispositions release only the action this window actually queued.
use super::TradeState;
use crate::{online::OnlineState, outbox::Stamp};
use eq_client_core::loot::{LootRequest, LootRequestStatus};
use std::time::Instant;

/// Original local context, retained through submission until server news arrives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PendingLoot {
    stamp: Stamp,
    request: LootRequest,
    submitted: bool,
}

impl PendingLoot {
    pub(super) const fn new(stamp: Stamp, request: LootRequest) -> Self {
        Self {
            stamp,
            request,
            submitted: false,
        }
    }

    pub(super) const fn place(self) -> Option<u16> {
        match self.request {
            LootRequest::Item { place, .. } => Some(place),
            _ => None,
        }
    }

    fn matches(self, session_id: u64, created: Option<Instant>, request: LootRequest) -> bool {
        !self.submitted
            && self.stamp.session_id == session_id
            && Some(self.stamp.created) == created
            && self.request == request
    }
}

impl TradeState {
    /// Says a matching local refusal once. Submission keeps item pending until
    /// server news; neither path removes a corpse row or changes holdings.
    pub(crate) fn loot_result(
        &mut self,
        online: &mut OnlineState,
        session_id: u64,
        created: Option<Instant>,
        request: LootRequest,
        status: &LootRequestStatus,
    ) -> Option<String> {
        if !online.world().accepts_reply(session_id) {
            return None;
        }
        let (corpse_id, opening) = match request {
            LootRequest::Open { corpse_id } => (corpse_id, true),
            LootRequest::Item { corpse_id, .. } => (corpse_id, false),
            // Existing close commands have no creation time. The reason is
            // informational, through the chat's ordinary duplicate handling;
            // it cannot close, reopen or release any current request/window.
            LootRequest::Close { .. } if created.is_none() => {
                return match status {
                    LootRequestStatus::Refused(reason) => Some(reason.clone()),
                    _ => None,
                };
            }
            _ => return None,
        };
        if online.world().loot()?.corpse_id != corpse_id {
            return None;
        }
        let window = self.loot.as_mut()?;
        let pending = if opening {
            window.opening.as_mut()?
        } else {
            window.pending.as_mut()?
        };
        if !pending.matches(session_id, created, request) {
            return None;
        }
        match status {
            LootRequestStatus::Sent => {
                // This exact command cannot subsequently be refused locally.
                // Retain item exclusion until its independent server result.
                pending.submitted = true;
                None
            }
            LootRequestStatus::Refused(reason) => {
                if opening {
                    online.close_loot();
                    self.loot = None;
                } else {
                    window.pending = None;
                    window.loot_all = false;
                }
                self.changed();
                Some(reason.clone())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
