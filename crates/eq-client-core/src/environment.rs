//! Locally sampled environmental hazards. These calculations never apply HP.
//!
//! Titanium `eqgame.exe`'s character tick at 0x422198 runs the environment
//! checks when more than 1000 ms have passed since the last one, then
//! records the time (read R49-3 a). The heat calculation at 0x41d4ba
//! reads signed fire resistance and selects the amount by zone; zone 200
//! uses its own damage type, 244, and disease resistance (read R49-3 b).
//! The lines the program prints for each type are in [`record_line`]
//! (read R49-3b).
use std::time::{Duration, Instant};

/// Titanium checks the environment a little over once a second. A stalled
/// frame runs one check and leaves the missed intervals behind.
#[derive(Debug, Default)]
pub struct Tick(Option<Instant>);

impl Tick {
    /// Starts a fresh interval after a placement or an observation gap.
    pub const fn interrupt(&mut self) {
        self.0 = None;
    }

    /// Whether one environmental check is due for this continuous sample:
    /// strictly more than a second after the last, as the program's
    /// `now - last > 1000` (read R49-3 a).
    pub fn due(&mut self, now: Instant) -> bool {
        let last = self.0.get_or_insert(now);
        if now.saturating_duration_since(*last) <= Duration::from_secs(1) {
            return false;
        }
        *last = now;
        true
    }
}

/// The zone whose lava is the program's type 244 and is resisted by
/// disease rather than fire (read R49-3 b).
pub const ZONE_HEAT_ZONE: u16 = 200;

/// Type-250 damage for known lava contact: the base less the effective
/// fire resistance over the divisor, in C integer division, raised to the
/// floor, by zone (read R49-3 b). Any signed resistance goes: a negative
/// one makes the amount larger than the base, as the program's
/// arithmetic does. None in zone 0, whose zone is not known yet, and in
/// zone 200, whose heat is [`zone_heat_damage`]'s.
#[must_use]
pub fn lava_damage(zone_id: u16, fire_resistance: i64) -> Option<u32> {
    let (base, divisor, minimum) = match zone_id {
        0 | ZONE_HEAT_ZONE => return None,
        108 | 212 | 217 | 343 => (200, 3, 10),
        278 => (100, 5, 25),
        341 => (100, 3, 10),
        342 => (150, 3, 10),
        _ => (50, 3, 10),
    };
    resisted(base, divisor, minimum, fire_resistance)
}

/// Zone 200's type-244 heat: 100 less the effective disease resistance
/// over 3, floor 10 (read R49-3 b). Nothing samples it yet: the client
/// does not model disease resistance, and zone 200 is past both servers'
/// eras this client puts first (recording checklist 6.491).
#[must_use]
pub fn zone_heat_damage(disease_resistance: i64) -> Option<u32> {
    resisted(100, 3, 10, disease_resistance)
}

/// Type-246 cold, feet values 4 and 8 (region kind 8, Velious water; read
/// R49-26): 50 less the effective cold resistance over 3, floor 10 (read
/// R49-3 b).
#[must_use]
pub fn cold_damage(cold_resistance: i64) -> Option<u32> {
    resisted(50, 3, 10, cold_resistance)
}

fn resisted(base: i64, divisor: i64, minimum: i64, resistance: i64) -> Option<u32> {
    u32::try_from(base.saturating_sub(resistance / divisor).max(minimum)).ok()
}

/// The damage types of the program's environmental records, 243 to 253
/// (read R49-3b).
pub const ENVIRONMENTAL_KINDS: std::ops::RangeInclusive<u8> = 243..=253;

/// The chat colour the program gives the amount line of its own report,
/// 0x11b (read R49-3 e).
pub const REPORT_COLOR: u32 = 0x11b;
/// The string the program prints after its own report, with the amount
/// as `%1`: 0x30c1 (read R49-3 e).
pub const REPORT_LINE: u32 = 0x30c1;
/// The you-form text the program puts in each of [`record_line`]'s lines
/// as `%1`: 0x2f9b (read R49-3b).
pub const YOU_TEXT: u32 = 0x2f9b;

/// The line and chat colour the Titanium program prints for an
/// environmental record of `kind` aimed at the player, by 0x4a56e3 (read
/// R49-3b): drowning's line is another in zone 0xab, and a trap's is
/// chosen by the record's spell field. None outside 243 to 253.
#[must_use]
pub fn record_line(kind: u8, spell_field: Option<u16>, zone_id: u16) -> Option<(u32, u32)> {
    const HEAT: u32 = 0x149;
    const HARM: u32 = 0x10a;
    let line = match kind {
        0xf3 => (0x2378, HEAT),
        0xf4 => (0x165a, HEAT),
        0xf5 => (0x2f6b, HEAT),
        0xf6 => (0x2f6d, HEAT),
        0xf7 => (0x2f69, HEAT),
        0xf8 => (0x2f67, HEAT),
        0xf9 => (0x2f65, HEAT),
        0xfa => (0x2f63, HEAT),
        0xfb if zone_id == 0xab => (0x2f5f, HARM),
        0xfb => (0x2f60, HARM),
        0xfc => (0x2f56, HARM),
        0xfd => (
            match spell_field {
                Some(0x73 | 0x74) => 0x2f59,
                Some(0x78 | 0x7d) => 0x2f5a,
                Some(0x82) => 0x2f5b,
                Some(0x87) => 0x2f5c,
                Some(0x8c | 0x91 | 0x92 | 0x96..=0x98) => 0x2f5d,
                _ => 0x2f5e,
            },
            HARM,
        ),
        _ => return None,
    };
    Some(line)
}

/// The program's type for a hazard this client reports: the record's
/// type byte, as the session encodes it (read R49-3 b).
#[must_use]
pub const fn hazard_kind(hazard: crate::hazards::Hazard) -> u8 {
    use crate::hazards::Hazard;
    match hazard {
        Hazard::Lava => 250,
        Hazard::Drowning => 251,
        Hazard::Falling => 252,
        Hazard::Freezing => 246,
        Hazard::ZoneHeat => 244,
    }
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
        assert!(!clock.due(at(1_000)), "exactly a second is not yet due");
        assert!(clock.due(at(1_500)));
        assert!(!clock.due(at(2_000)));
        assert!(!clock.due(at(2_500)));
        assert!(clock.due(at(2_501)));
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
        assert_eq!(lava_damage(0, 25), None);
        // Any resist goes: a negative one hurts more, C division truncates.
        assert_eq!(lava_damage(27, -1), Some(50));
        assert_eq!(lava_damage(27, -30), Some(60));
        assert_eq!(lava_damage(27, 300), Some(10));
        assert_eq!(lava_damage(278, 1_000), Some(25));
    }

    #[test]
    fn zone_heat_and_cold_follow_their_own_bases() {
        assert_eq!(zone_heat_damage(30), Some(90));
        assert_eq!(zone_heat_damage(500), Some(10));
        assert_eq!(cold_damage(30), Some(40));
        assert_eq!(cold_damage(-3), Some(51));
        assert_eq!(cold_damage(200), Some(10));
    }

    #[test]
    fn each_environmental_type_prints_the_programs_line_in_its_colour() {
        let heat = [
            (0xf3, 0x2378),
            (0xf4, 0x165a),
            (0xf5, 0x2f6b),
            (0xf6, 0x2f6d),
            (0xf7, 0x2f69),
            (0xf8, 0x2f67),
            (0xf9, 0x2f65),
            (0xfa, 0x2f63),
        ];
        for (kind, line) in heat {
            assert_eq!(record_line(kind, None, 1), Some((line, 0x149)), "{kind:#x}");
        }
        assert_eq!(record_line(0xfb, None, 1), Some((0x2f60, 0x10a)));
        assert_eq!(record_line(0xfb, None, 0xab), Some((0x2f5f, 0x10a)));
        assert_eq!(record_line(0xfc, None, 0xab), Some((0x2f56, 0x10a)));
        assert_eq!(record_line(0xf2, None, 1), None);
        assert_eq!(record_line(0xfe, None, 1), None);
        assert_eq!(record_line(231, Some(0x87), 1), None);
    }

    #[test]
    fn a_trap_record_picks_its_line_by_the_spell_field() {
        let branches = [
            (Some(0x73), 0x2f59),
            (Some(0x74), 0x2f59),
            (Some(0x78), 0x2f5a),
            (Some(0x7d), 0x2f5a),
            (Some(0x82), 0x2f5b),
            (Some(0x87), 0x2f5c),
            (Some(0x8c), 0x2f5d),
            (Some(0x91), 0x2f5d),
            (Some(0x92), 0x2f5d),
            (Some(0x96), 0x2f5d),
            (Some(0x97), 0x2f5d),
            (Some(0x98), 0x2f5d),
            (Some(0x99), 0x2f5e),
            (Some(0x75), 0x2f5e),
            (None, 0x2f5e),
        ];
        for (field, line) in branches {
            assert_eq!(
                record_line(0xfd, field, 1),
                Some((line, 0x10a)),
                "{field:?}"
            );
        }
    }

    #[test]
    fn each_reported_hazard_carries_the_programs_type() {
        use crate::hazards::Hazard;
        let kinds = [
            Hazard::Lava,
            Hazard::Drowning,
            Hazard::Falling,
            Hazard::Freezing,
            Hazard::ZoneHeat,
        ]
        .map(hazard_kind);
        assert_eq!(kinds, [250, 251, 252, 246, 244]);
        assert!(kinds.iter().all(|kind| ENVIRONMENTAL_KINDS.contains(kind)));
    }
}
