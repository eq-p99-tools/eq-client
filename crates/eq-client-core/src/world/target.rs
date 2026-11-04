//! Pending local choice and the admission-scoped session target projection.

/// The player's target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Target {
    /// Pending local choice or reconciled server selection, including an
    /// unresolved spawn ID, the player themselves, or nothing.
    pub selected: Option<u16>,
    /// Whether the displayed choice has reconciled with session authority.
    /// This never means server acknowledgment or action success.
    pub sent: bool,
    /// The chosen spawn's revision, so that a spawn replacing it under the
    /// same ID is not mistaken for it.
    pub(super) revision: Option<u64>,
    /// Incoming authority is never automatically echoed to the server.
    pub server_selected: bool,
    pub(super) request_id: u64,
    pub(super) reported_request: u64,
    pub(super) authority: Option<eq_network_game::targets::TargetState>,
    /// Entered opens projection before Connected; every reset closes it.
    pub(super) accepts_updates: bool,
}
