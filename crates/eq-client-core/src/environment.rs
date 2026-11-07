//! Locally sampled environmental hazards. These calculations never apply HP.
//!
//! Titanium `eqgame.exe` at 0x422198 records the current time when the
//! periodic check runs. The heat calculation at 0x41d4ba reads signed fire
//! resistance and selects the amount by zone. Zone 200 uses a different
//! resist and damage type. The host's supported resistance range is a
//! conservative bound on its input model (recording checklist 6.393).
use std::time::{Duration, Instant};

/// Titanium checks the environment at roughly one-second intervals. A
/// stalled frame runs one check and leaves the missed intervals behind.
#[derive(Debug, Default)]
pub struct Tick(Option<Instant>);

impl Tick {
    /// Starts a fresh interval after a placement or an observation gap.
    pub const fn interrupt(&mut self) {
        self.0 = None;
    }

    /// Whether one environmental check is due for this continuous sample.
    pub fn due(&mut self, now: Instant) -> bool {
        let last = self.0.get_or_insert(now);
        if now.saturating_duration_since(*last) < Duration::from_secs(1) {
            return false;
        }
        *last = now;
        true
    }
}

/// Type-250 damage for known lava contact. Fire resistance is the resolved
/// effective value. This slice admits ordinary resistances only. Zone 200
/// selects a different damage type and resistance, outside this request.
#[must_use]
pub fn lava_damage(zone_id: u16, fire_resistance: i64) -> Option<u32> {
    if matches!(zone_id, 0 | 200) || !(0..=255).contains(&fire_resistance) {
        return None;
    }
    let (base, divisor, minimum) = match zone_id {
        108 | 212 | 217 | 343 => (200, 3, 10),
        278 => (100, 5, 25),
        341 => (100, 3, 10),
        342 => (150, 3, 10),
        _ => (50, 3, 10),
    };
    u32::try_from((base - fire_resistance / divisor).max(minimum)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delayed_environment_check_starts_a_full_new_interval() {
        let mut clock = Tick::default();
        let start = Instant::now();
        let at = |millis| start + Duration::from_millis(millis);
        assert!(!clock.due(at(0)));
        assert!(clock.due(at(1_500)));
        assert!(!clock.due(at(2_000)));
        assert!(!clock.due(at(2_499)));
        assert!(clock.due(at(2_500)));
    }

    #[test]
    fn titanium_lava_uses_effective_resist_and_its_zone_floor() {
        assert_eq!(lava_damage(27, 25), Some(42));
        assert_eq!(lava_damage(27, 240), Some(10));
        assert_eq!(lava_damage(108, 25), Some(192));
        assert_eq!(lava_damage(278, 100), Some(80));
        assert_eq!(lava_damage(341, 30), Some(90));
        assert_eq!(lava_damage(342, 30), Some(140));
        assert_eq!(lava_damage(200, 25), None);
        assert_eq!(lava_damage(27, -1), None);
        assert_eq!(lava_damage(27, 300), None);
    }
}
