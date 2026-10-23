//! Whether a moving character walks or runs, from the speed it moves at and
//! the walk speed its spawn record carries.
//!
//! The server sends a moving spawn's speed, never whether it walks or runs,
//! so the clip follows the speed. `EQEmu` gives a spawn a walk speed value of
//! its record's walk speed times 40 (`zone/mob.cpp`, `Mob::Mob`: `walkspeed =
//! base_walkspeed * 0.025`, and `FillSpawnStruct` sends the float), and moves
//! a spawn [`ANIMATION_UNITS_PER_SECOND`] units a second per speed value. It
//! slows a badly hurt fleeing NPC to its walk speed value plus one "so they do
//! the run animation" (`Mob::_GetFearSpeed`): the client runs as soon as a
//! spawn moves faster than its walk. Inferred from that, recording checklist
//! item 6.232.

pub use eq_network_game::world::ANIMATION_UNITS_PER_SECOND;

/// A spawn record's speed float times this is its speed value.
const VALUE_PER_RECORD_SPEED: f32 = 40.0;

/// How far past its walk speed value a character moves before it runs: half
/// the one value `EQEmu` adds to make a spawn run, so one moving at exactly
/// its walk value never sits on the edge.
const PAST_WALK: f32 = 0.5;

/// Where a character whose record carries no walk speed runs, in units a
/// second: between `EQEmu`'s default NPC walk (value 18, about 10.4) and a
/// player's base run (28, about 16.2). Inferred, 6.232.
const FALLBACK_RUN: f32 = 12.5;

/// The units a second a spawn record's walk or run speed moves.
#[must_use]
pub fn record_speed(speed: f32) -> f32 {
    speed * VALUE_PER_RECORD_SPEED * ANIMATION_UNITS_PER_SECOND
}

/// Whether a character moving `speed` units a second runs: faster than its
/// record's `walk_speed`, by more than [`PAST_WALK`] of a speed value, or
/// than [`FALLBACK_RUN`] where the record carries no walk speed.
#[must_use]
pub fn runs(speed: f32, walk_speed: f32) -> bool {
    if walk_speed.is_finite() && walk_speed > 0.0 {
        speed / ANIMATION_UNITS_PER_SECOND > walk_speed * VALUE_PER_RECORD_SPEED + PAST_WALK
    } else {
        speed > FALLBACK_RUN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Units a second for a speed value.
    fn value(speed: f32) -> f32 {
        speed * ANIMATION_UNITS_PER_SECOND
    }

    #[test]
    fn a_character_runs_once_it_moves_faster_than_its_walk() {
        // A default NPC: run speed 1.25 (value 50), walk 50 * 100 / 265 = 18.
        let walk = 18.0 * 0.025;
        assert!(!runs(value(18.0), walk), "wandering at its walk");
        assert!(runs(value(50.0), walk), "chasing at its run");
        // Badly hurt and fleeing: one past its walk runs, half its walk walks.
        assert!(runs(value(19.0), walk));
        assert!(!runs(value(9.0), walk));
        // A run speed 2.0 NPC (value 80) wanders at its walk, 30.
        let fast = 30.0 * 0.025;
        assert!(!runs(value(30.0), fast));
        assert!(runs(value(80.0), fast));
        // A player: run speed 0.7 (value 28), walk 0.3 (12).
        assert!(runs(value(28.0), 0.3));
        assert!(!runs(value(12.0), 0.3));
    }

    #[test]
    fn a_record_without_a_walk_speed_runs_past_the_fallback() {
        for walk in [0.0, f32::NAN] {
            assert!(!runs(12.4, walk));
            assert!(runs(12.6, walk));
        }
    }

    #[test]
    fn a_record_speed_is_its_value_over_forty() {
        assert!((record_speed(0.3) - 12.0 * 0.58).abs() < 0.001);
        assert!((record_speed(0.7) - 28.0 * 0.58).abs() < 0.001);
    }
}
