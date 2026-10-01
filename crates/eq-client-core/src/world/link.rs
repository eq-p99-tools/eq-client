//! Where the connection stands, in the player's terms rather than the
//! protocol's milestones.

/// Where the connection stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    /// No session yet, as in the offline viewer before a demo admits one.
    #[default]
    Offline,
    /// Logging in: the login server, the world server and the character list.
    LoggingIn,
    /// Entering a zone: the character and the zone are loading.
    Entering,
    /// In the world.
    Connected,
    /// Moving to another zone.
    Zoning,
    /// The session ended and will not reconnect.
    Ended,
}

impl Link {
    /// Whether the player is in a zone the session has admitted them to.
    #[must_use]
    pub const fn connected(self) -> bool {
        matches!(self, Self::Connected)
    }

    /// Whether the session is over for good.
    #[must_use]
    pub const fn ended(self) -> bool {
        matches!(self, Self::Ended)
    }
}
