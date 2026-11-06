//! Explicit movement departures shared by input and the session settings send.

use super::MovementOptions;
use crate::qol::{Fix, Settings};
use crate::{Capability, world::ClientWorld};

/// The player's movement opt-ins. None supplies a missing movement grant.
#[must_use]
pub const fn options(settings: &Settings) -> MovementOptions {
    MovementOptions {
        walk_while_stunned: settings.on(Fix::WalkWhileWinded),
        run_boost: settings.on(Fix::RunBoost),
        fly: settings.on(Fix::Fly),
    }
}

/// The movement options this session takes: the player's opt-ins where the
/// server type offers gameplay departures, and the official client's
/// movement everywhere else, whatever the settings say. Input and the
/// settings send share this, so a public server never sees a departure and
/// the player is never held waiting for one it would refuse.
#[must_use]
pub fn offered_options(settings: &Settings, world: &ClientWorld) -> MovementOptions {
    if world.can(Capability::GameplayDepartures) {
        options(settings)
    } else {
        MovementOptions::default()
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

    #[test]
    fn departures_are_taken_only_where_the_session_offers_them() {
        let mut settings = Settings::default();
        settings.set(Fix::RunBoost, true);
        settings.set(Fix::Fly, true);
        let world = ClientWorld::default();
        assert!(!world.can(Capability::GameplayDepartures));
        assert_eq!(
            offered_options(&settings, &world),
            MovementOptions::default()
        );
    }
}
