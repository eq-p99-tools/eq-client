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

/// The weekdays' names, Sunday first; the Titanium client names day 1 of a
/// month Sunday, and every seventh day after it (from the program).
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// How a client generation writes the date in `/time`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DateStyle {
    /// Whether the day of the month takes an ordinal ending.
    pub ordinal_day: bool,
    /// Whether the weekday comes first.
    pub weekday: bool,
}

/// A time in Norrath as `/time` names it: the month, the day, the year and
/// the hour on a twelve-hour clock. The hour is the one the client shows,
/// from 0 for 12 AM, which a client generation names from the calendar's in
/// its own way, as it writes the date in its own style.
#[must_use]
pub fn norrath(time: GameTime, shown_hour: u8, style: DateStyle) -> String {
    let month = MONTHS
        .get(usize::from(time.month.saturating_sub(1)))
        .copied()
        .unwrap_or_default();
    let (hour, half) = twelve_hour(shown_hour);
    let ending = if style.ordinal_day {
        ordinal(time.day)
    } else {
        ""
    };
    let weekday = if style.weekday {
        let index = usize::from(time.day.saturating_sub(1)) % WEEKDAYS.len();
        format!("{}, ", WEEKDAYS[index])
    } else {
        String::new()
    };
    format!(
        "{weekday}{month} {}{ending}, {} - {hour} {half}",
        time.day, time.year
    )
}

/// The ending a day of the month takes: 1 and 21 "st", 2 and 22 "nd", 3 and
/// 23 "rd", any other "th", as the Mac client gives it (from the program,
/// checklist item 6.155).
const fn ordinal(day: u8) -> &'static str {
    match day {
        1 | 21 => "st",
        2 | 22 => "nd",
        3 | 23 => "rd",
        _ => "th",
    }
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
        let named = |hour| norrath(at(hour), hour, DateStyle::default());
        assert_eq!(named(0), "February 3, 3200 - 12 AM");
        assert_eq!(named(6), "February 3, 3200 - 6 AM");
        assert_eq!(named(12), "February 3, 3200 - 12 PM");
        assert_eq!(named(23), "February 3, 3200 - 11 PM");
        // The hour is the one shown, whatever the calendar's.
        assert_eq!(
            norrath(at(12), 13, DateStyle::default()),
            "February 3, 3200 - 1 PM"
        );
        // Ordinal endings, where the client gives them.
        let ordinals = DateStyle {
            ordinal_day: true,
            weekday: false,
        };
        let day = |day| norrath(GameTime { day, ..at(0) }, 0, ordinals);
        assert_eq!(day(1), "February 1st, 3200 - 12 AM");
        assert_eq!(day(2), "February 2nd, 3200 - 12 AM");
        assert_eq!(day(3), "February 3rd, 3200 - 12 AM");
        assert_eq!(day(11), "February 11th, 3200 - 12 AM");
        assert_eq!(day(21), "February 21st, 3200 - 12 AM");
        assert_eq!(day(22), "February 22nd, 3200 - 12 AM");
        assert_eq!(day(23), "February 23rd, 3200 - 12 AM");
        assert_eq!(day(28), "February 28th, 3200 - 12 AM");
        // Weekdays, where the client gives them: day 1 is a Sunday.
        let weekdays = DateStyle {
            ordinal_day: false,
            weekday: true,
        };
        let weekday = |day| norrath(GameTime { day, ..at(0) }, 0, weekdays);
        assert_eq!(weekday(1), "Sunday, February 1, 3200 - 12 AM");
        assert_eq!(weekday(7), "Saturday, February 7, 3200 - 12 AM");
        assert_eq!(weekday(8), "Sunday, February 8, 3200 - 12 AM");
        assert_eq!(weekday(28), "Saturday, February 28, 3200 - 12 AM");
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 3)
            .unwrap()
            .and_hms_opt(12, 34, 5)
            .unwrap();
        assert_eq!(earth(now), "Saturday, October 03, 2026 12:34:05");
    }
}
