//! Explicit movement departures shared by input and the session settings send.

use super::{MovementOptions, PostureOptions};
use crate::qol::{Fix, Settings};
use crate::{Capability, world::ClientWorld};

/// The player's movement opt-ins. None supplies a missing movement grant.
#[must_use]
pub const fn options(settings: &Settings) -> MovementOptions {
    MovementOptions {
        walk_while_stunned: settings.on(Fix::WalkWhileWinded),
        run_speed_multiplier: settings.run_speed_multiplier,
        fly: settings.on(Fix::Fly),
        posture: PostureOptions {
            feign_standing: settings.on(Fix::MoveWhileFeigned),
            // On is the original client's stand; off is the departure.
            stay_seated_when_hit: !settings.on(Fix::StandWhenHit),
        },
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
    use crate::movement::RunSpeedMultiplier;

    #[test]
    fn movement_departures_start_off_and_remain_independent() {
        let mut settings = Settings::default();
        assert_eq!(options(&settings), MovementOptions::default());
        for fix in [Fix::WalkWhileWinded, Fix::Fly] {
            settings.set(fix, true);
            let selected = options(&settings);
            assert_eq!(selected.walk_while_stunned, fix == Fix::WalkWhileWinded);
            assert_eq!(selected.run_speed_multiplier, RunSpeedMultiplier::NORMAL);
            assert_eq!(selected.fly, fix == Fix::Fly);
            settings.set(fix, false);
            assert_eq!(options(&settings), MovementOptions::default());
        }
        settings.run_speed_multiplier = RunSpeedMultiplier::MAX;
        assert_eq!(
            options(&settings),
            MovementOptions {
                run_speed_multiplier: RunSpeedMultiplier::MAX,
                ..Default::default()
            }
        );
    }

    #[test]
    fn the_feign_and_seat_choices_reach_the_session_options() {
        let mut settings = Settings::default();
        // The defaults are the original client's: lie down, stand when hit.
        assert_eq!(options(&settings).posture, PostureOptions::default());
        settings.set(Fix::MoveWhileFeigned, true);
        settings.set(Fix::StandWhenHit, false);
        assert_eq!(
            options(&settings).posture,
            PostureOptions {
                feign_standing: true,
                stay_seated_when_hit: true,
            }
        );
    }

    #[test]
    fn departures_are_taken_only_where_the_session_offers_them() {
        let mut settings = Settings::default();
        settings.run_speed_multiplier = RunSpeedMultiplier::MAX;
        settings.set(Fix::Fly, true);
        let world = ClientWorld::default();
        assert!(!world.can(Capability::GameplayDepartures));
        assert_eq!(
            offered_options(&settings, &world),
            MovementOptions::default()
        );
    }
}
