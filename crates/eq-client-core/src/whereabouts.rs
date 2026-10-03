//! Where and when the player is, as `/loc` and `/time` name it. The official
//! client answers both itself, from the player's position and the time the
//! server last gave (no packet asks the server), in its own strings, which
//! take what these functions write. How it writes the numbers and dates in
//! them is inferred: a recording will check it.
use crate::{WorldPosition, clock::GameTime};

/// The months' names, January first; Norrath's calendar names its months
/// as Earth's.
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `/loc`'s three numbers: the server's y first, then x and the height, as
/// the official client orders them, each to two places.
#[must_use]
pub fn location(position: WorldPosition) -> [String; 3] {
    [position.y, position.x, position.z].map(|value| format!("{value:.2}"))
}

/// A time in Norrath as `/time` names it: the month, the day, the year and
/// the hour on a twelve-hour clock.
#[must_use]
pub fn norrath(time: GameTime) -> String {
    let month = MONTHS
        .get(usize::from(time.month.saturating_sub(1)))
        .copied()
        .unwrap_or_default();
    let (hour, half) = twelve_hour(time.hour);
    format!("{month} {}, {} - {hour} {half}", time.day, time.year)
}

/// A time on Earth as `/time` names it: the weekday, the date and the time
/// to the second.
#[must_use]
pub fn earth(now: chrono::NaiveDateTime) -> String {
    now.format("%A, %B %d, %Y %H:%M:%S").to_string()
}

/// An hour of the day, from 0 at midnight, on a twelve-hour clock.
const fn twelve_hour(hour: u8) -> (u8, &'static str) {
    let half = if hour < 12 { "AM" } else { "PM" };
    match hour % 12 {
        0 => (12, half),
        hour => (hour, half),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_names_y_first_to_two_places() {
        let position = WorldPosition {
            x: -12.345,
            y: 1500.0,
            z: 3.75,
            heading: 0.0,
        };
        assert_eq!(location(position), ["1500.00", "-12.35", "3.75"]);
    }

    #[test]
    fn a_time_names_the_date_and_the_hour_on_a_twelve_hour_clock() {
        let at = |hour| GameTime {
            hour,
            minute: 30,
            day: 3,
            month: 2,
            year: 3200,
        };
        assert_eq!(norrath(at(0)), "February 3, 3200 - 12 AM");
        assert_eq!(norrath(at(6)), "February 3, 3200 - 6 AM");
        assert_eq!(norrath(at(12)), "February 3, 3200 - 12 PM");
        assert_eq!(norrath(at(23)), "February 3, 3200 - 11 PM");
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 3)
            .unwrap()
            .and_hms_opt(12, 34, 5)
            .unwrap();
        assert_eq!(earth(now), "Saturday, October 03, 2026 12:34:05");
    }
}
