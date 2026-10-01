//! The player's target: the spawn they chose, and whether the session has
//! told the server.

/// The player's target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Target {
    /// The spawn chosen, the player themselves, or nothing.
    pub selected: Option<u16>,
    /// Whether the session has sent the choice to the server.
    pub sent: bool,
    /// The chosen spawn's revision, so that a spawn replacing it under the
    /// same ID is not mistaken for it.
    pub(super) revision: Option<u64>,
}
