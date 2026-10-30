//! How an installed client tells the time, from the calendar the server
//! keeps: the hour it shows, when its dawn and dusk come, the zones whose sky
//! keeps one hour, and the zones where `/time` answers with a line in place
//! of the time. A client generation's rules are its [`ClientClock`], from
//! [`crate::ui::InstalledClient::clock`].
//!
//! Both clients' rules are from the program: each official client's
//! executable, read statically on 10-30 (the TAKP client for recording
//! checklist item 6.155, the Titanium client for items 1.2 and 6.115), except
//! where a rule says it is this client's reading or not yet known.

/// How a client generation tells the time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientClock {
    /// Hours the client adds to the calendar's hour, the hours since the day
    /// began, to name it in `/time`.
    pub shown_after: u8,
    /// Hours it adds to the calendar's hour for the hour its sky follows.
    pub sky_after: u8,
    /// When dawn and dusk come; None until checked for this client.
    pub twilight: Option<Twilight>,
    /// Zones whose sky keeps one hour, whatever the time: the zone's number
    /// and the hour shown, at minute 0. `/time` still names the time.
    pub fixed_sky_hours: &'static [(u16, u8)],
    /// Zones where `/time` answers with a string, by its id, in place of the
    /// time: the zone's number and the string.
    pub timeless_zones: &'static [(u16, u32)],
    /// Whether `/time` gives the day of the month an ordinal ending.
    pub ordinal_days: bool,
    /// Whether `/time` names the weekday, Sunday for day 1 and every seventh
    /// day after.
    pub weekdays: bool,
}

/// When dawn and dusk begin and end, in minutes after the sky hour's 12 AM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Twilight {
    /// Dawn, from the last of the night to the full day.
    pub dawn: (u16, u16),
    /// Dusk, from the last of the day to the full night.
    pub dusk: (u16, u16),
}

/// The zones whose sky keeps one hour, the same in both clients (from the
/// programs): 63 (`unrest`) and 76 (`hateplane`) keep 6:00, 71 (`airplane`)
/// 18:00 and 123 (`necropolis`) 10:00.
const FIXED_SKY_HOURS: &[(u16, u8)] = &[(63, 6), (71, 18), (76, 6), (123, 10)];

/// Zones 219 (`potimea`) and 223 (`potimeb`), where both clients answer
/// `/time` with string 4036 (from the programs).
const TIMELESS_ZONES: &[(u16, u32)] = &[(219, 4036), (223, 4036)];

/// Both clients' skies change at the wire's hours 3, 9, 15 and 21, over the
/// six game hours between. The Mac client's light values make 3:00 to 9:00
/// the change toward day and 15:00 to 21:00 the change toward night; that
/// the Titanium client's do the same is inferred from the same order.
/// Taking each change as an even ramp over its six hours is this client's
/// reading of the programs' 6-hour period.
const TWILIGHT: Twilight = Twilight {
    dawn: (3 * 60, 9 * 60),
    dusk: (15 * 60, 21 * 60),
};

/// The Titanium client. Its `/time` shows the wire's hour less one, the
/// calendar's, so wire hour 1 is 12 AM, after the weekday, Sunday for day 1;
/// its sky follows the wire's hour as sent. It answers `/time` in the Plane
/// of Time with string 4036 alone.
pub const TITANIUM: ClientClock = ClientClock {
    shown_after: 0,
    sky_after: 1,
    twilight: Some(TWILIGHT),
    fixed_sky_hours: FIXED_SKY_HOURS,
    timeless_zones: TIMELESS_ZONES,
    ordinal_days: false,
    weekdays: true,
};

/// The Mac client. Its `/time` and its sky both take the wire's hour as
/// sent, one past the calendar's, with 24 as 12 AM. It answers `/time` in
/// zones 219 and 223 with string 4036 alone; that no Earth line follows is
/// this client's reading of the program. Its days take ordinal endings; it
/// names weekdays too, but the program's names are not read, so this client
/// leaves them out.
pub const EQMAC: ClientClock = ClientClock {
    shown_after: 1,
    sky_after: 1,
    twilight: Some(TWILIGHT),
    fixed_sky_hours: FIXED_SKY_HOURS,
    timeless_zones: TIMELESS_ZONES,
    ordinal_days: true,
    weekdays: false,
};

impl ClientClock {
    /// The hour the client shows for the calendar's hour, from 0 for 12 AM
    /// to 23 for 11 PM.
    #[must_use]
    pub const fn shown_hour(self, hour: u8) -> u8 {
        (hour % 24 + self.shown_after) % 24
    }

    /// The hour the client's sky follows for the calendar's hour, from 0
    /// for 12 AM to 23 for 11 PM.
    #[must_use]
    pub const fn sky_hour_of(self, hour: u8) -> u8 {
        (hour % 24 + self.sky_after) % 24
    }

    /// The hour shown that the zone's sky keeps, if it keeps one.
    #[must_use]
    pub fn sky_hour(self, zone: u16) -> Option<u8> {
        self.fixed_sky_hours
            .iter()
            .find(|(fixed, _)| *fixed == zone)
            .map(|(_, hour)| *hour)
    }

    /// The string `/time` answers with in the zone, if it doesn't name the
    /// time there.
    #[must_use]
    pub fn timeless(self, zone: u16) -> Option<u32> {
        self.timeless_zones
            .iter()
            .find(|(timeless, _)| *timeless == zone)
            .map(|(_, string)| *string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mac_client_shows_the_wire_hour_and_titanium_one_less() {
        // The calendar's hour is the wire's less one: wire 1, 12, 13, 24.
        let calendar = [0, 11, 12, 23];
        let wire = [1, 12, 13, 0];
        assert_eq!(calendar.map(|hour| EQMAC.shown_hour(hour)), wire);
        assert_eq!(calendar.map(|hour| TITANIUM.shown_hour(hour)), calendar);
        // Both skies follow the wire's hour.
        assert_eq!(calendar.map(|hour| EQMAC.sky_hour_of(hour)), wire);
        assert_eq!(calendar.map(|hour| TITANIUM.sky_hour_of(hour)), wire);
    }

    #[test]
    fn both_clients_keep_zone_hours_and_timeless_zones() {
        for clock in [EQMAC, TITANIUM] {
            assert_eq!(clock.sky_hour(71), Some(18));
            assert_eq!(clock.sky_hour(123), Some(10));
            assert_eq!(clock.sky_hour(1), None);
            assert_eq!(clock.timeless(223), Some(4036));
            assert_eq!(clock.timeless(71), None);
        }
    }
}
