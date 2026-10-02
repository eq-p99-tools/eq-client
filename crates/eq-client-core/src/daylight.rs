//! How light it is in Norrath at a time of day, and how day and night look.
//! Every number here is provisional: see [`PROVISIONAL_DAY_NIGHT`].
use crate::clock::GameTime;

/// How Norrath's day and night look: when dawn and dusk come, and how the
/// night and the twilight change the picture and the sky.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DayNight {
    /// When dawn begins and ends, in minutes after midnight.
    pub dawn: (u16, u16),
    /// When dusk begins and ends.
    pub dusk: (u16, u16),
    /// How much darker midnight is than noon, in stops.
    pub night_exposure: f32,
    /// How much of the day's color is left at midnight.
    pub night_saturation: f32,
    /// How blue midnight is: a shift in color temperature.
    pub night_temperature: f32,
    /// How red the middle of dawn and dusk is.
    pub twilight_temperature: f32,
    /// The sky's color by day, by night and in the middle of the twilight.
    pub day_sky: [f32; 3],
    /// By night.
    pub night_sky: [f32; 3],
    /// In the middle of the twilight.
    pub twilight_sky: [f32; 3],
}

/// Provisional: guessed, not measured from the official client. Full day
/// from seven in the morning to seven at night, darkness from nine at night
/// to five in the morning, midnight two stops darker, bluer and greyer, and
/// dawn and dusk warm. How dark the official client's night looks also
/// depends on the character's race vision (infravision, ultravision) and on
/// the light they carry, which this does not model. Replace these from noon
/// and midnight captures of the official client taken from the same spot.
pub const PROVISIONAL_DAY_NIGHT: DayNight = DayNight {
    dawn: (5 * 60, 7 * 60),
    dusk: (19 * 60, 21 * 60),
    night_exposure: -2.2,
    night_saturation: 0.6,
    night_temperature: -0.12,
    twilight_temperature: 0.15,
    day_sky: [0.53, 0.70, 0.92],
    night_sky: [0.01, 0.02, 0.06],
    twilight_sky: [0.85, 0.45, 0.3],
};

impl DayNight {
    /// How much daylight there is, from 0 at night to 1 by day.
    #[must_use]
    pub fn daylight(&self, time: GameTime) -> f32 {
        let minute = u16::from(time.hour) * 60 + u16::from(time.minute);
        let ramp = |(start, end): (u16, u16)| f32::from(minute - start) / f32::from(end - start);
        match minute {
            _ if minute < self.dawn.0 || minute >= self.dusk.1 => 0.0,
            _ if minute < self.dawn.1 => ramp(self.dawn),
            _ if minute < self.dusk.0 => 1.0,
            _ => 1.0 - ramp(self.dusk),
        }
    }

    /// How far into dawn or dusk the time is, from 0 outside them to 1 at
    /// their middle, when the sky is most red.
    #[must_use]
    pub fn twilight(&self, time: GameTime) -> f32 {
        let light = self.daylight(time);
        if light <= 0.0 || light >= 1.0 {
            0.0
        } else {
            1.0 - (light - 0.5).abs() * 2.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u8, minute: u8) -> GameTime {
        GameTime {
            hour,
            minute,
            day: 1,
            month: 1,
            year: 3100,
        }
    }

    #[test]
    fn day_and_night_turn_at_dawn_and_dusk() {
        let look = PROVISIONAL_DAY_NIGHT;
        assert!((look.daylight(at(0, 0))).abs() < f32::EPSILON);
        assert!((look.daylight(at(4, 59))).abs() < f32::EPSILON);
        assert!((look.daylight(at(6, 0)) - 0.5).abs() < f32::EPSILON);
        assert!((look.daylight(at(12, 0)) - 1.0).abs() < f32::EPSILON);
        assert!((look.daylight(at(20, 0)) - 0.5).abs() < f32::EPSILON);
        assert!((look.daylight(at(21, 0))).abs() < f32::EPSILON);
        assert!((look.twilight(at(6, 0)) - 1.0).abs() < f32::EPSILON);
        assert!(look.twilight(at(12, 0)).abs() < f32::EPSILON);
        assert!(look.twilight(at(2, 0)).abs() < f32::EPSILON);
    }
}
