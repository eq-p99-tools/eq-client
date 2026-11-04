//! Explicit movement departures shared by input and the session settings send.

use super::MovementOptions;
use crate::qol::{Fix, Settings};

/// The player's movement opt-ins. None supplies a missing movement grant.
#[must_use]
pub const fn options(settings: &Settings) -> MovementOptions {
    MovementOptions {
        walk_while_stunned: settings.on(Fix::WalkWhileWinded),
        run_boost: settings.on(Fix::RunBoost),
        fly: settings.on(Fix::Fly),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_departures_start_off_and_remain_independent() {
        let mut settings = Settings::default();
        assert_eq!(options(&settings), MovementOptions::default());
        for fix in [Fix::WalkWhileWinded, Fix::RunBoost, Fix::Fly] {
            settings.set(fix, true);
            let selected = options(&settings);
            assert_eq!(selected.walk_while_stunned, fix == Fix::WalkWhileWinded);
            assert_eq!(selected.run_boost, fix == Fix::RunBoost);
            assert_eq!(selected.fly, fix == Fix::Fly);
            settings.set(fix, false);
            assert_eq!(options(&settings), MovementOptions::default());
        }
    }
}
