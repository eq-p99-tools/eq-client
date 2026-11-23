//! `/location` (`/loc`) and `/time`, which the client answers itself, as the official
//! client does: where the player stands, in the installed string 13177, and
//! the time in Norrath and on Earth, in 6706 and 12389, or a zone's line in
//! place of the time where the installed client gives one. The Titanium
//! client answers `/time` in 6706 and 12389 (from the program); the Mac
//! client builds its Norrath line from a format of its own, which this
//! client doesn't copy, so 6706 stands in there. That `/loc` is worded so is
//! inferred from what 13177 says; this client's words stand in without the
//! strings.
//!
//! `/guildstatus` is answered here too (checklist 6.575, read R49-6k): the
//! Titanium program answers it itself and sends nothing, of the player a
//! typed name finds or else the target, in strings 12256 to 12260 by rank,
//! and of a player in no guild in words of its own, for which this client's
//! own words stand. A string the installation lacks gives no line. The Mac
//! client's own answer wasn't read, so there it is refused for now.
//!
//! `/played` too (checklist 6.490b, read R49-8 a): the Titanium program
//! answers from three profile fields, in line 0xe24 the account's entitled
//! time in years (its days × 1/365, one decimal), in 0x32a8 when the
//! character was made, and in 0x32af its played time in days, hours and
//! minutes. It adds a minute each minute after the profile itself
//! (0x422526), and so does this client. The creation time is shown as the
//! program's `%#c` shows it in the C locale (inferred); the Mac client's
//! `/played` wasn't read, so there it is refused for now.
use super::chat::{ChatState, Said};
use super::hud::messages::Messages;
use bevy::prelude::*;
use eq_client_assets::clock::ClientClock;
use eq_client_core::{whereabouts, world::ClientWorld};

/// Which the player asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// `/loc`: where the player stands.
    Location,
    /// `/time`: the time in Norrath and on Earth.
    Time,
    /// `/guildstatus [name]`: a player's guild and rank in it, the named
    /// player's or else the target's.
    GuildStatus(Option<String>),
    /// `/played`: the account's entitled time and the character's age and
    /// played time.
    Played,
}

/// Whether `/guildstatus` is offered: read R49-6k says the official client
/// answers it itself, and in which strings (checklist 6.575).
const GUILD_STATUS_READ: bool = true;

impl Asked {
    /// What a typed line asks, if it is `/location` or `/time`, spelled out.
    pub(crate) fn typed(input: &str) -> Option<Self> {
        Self::typed_when(input, GUILD_STATUS_READ)
    }

    /// As [`Self::typed`], with `/guildstatus` taken only when offered.
    fn typed_when(input: &str, guild_status: bool) -> Option<Self> {
        let name = input.strip_prefix('/')?.trim();
        if name.eq_ignore_ascii_case("location") {
            Some(Self::Location)
        } else if name.eq_ignore_ascii_case("time") {
            Some(Self::Time)
        } else if name
            .split_whitespace()
            .next()
            .is_some_and(|command| command.eq_ignore_ascii_case("played"))
        {
            // The program reads no words after it.
            Some(Self::Played)
        } else {
            // The program takes the first word after the command as a name.
            let mut words = name.split_whitespace();
            let command = words.next()?;
            (guild_status && command.eq_ignore_ascii_case("guildstatus"))
                .then(|| Self::GuildStatus(words.next().map(str::to_owned)))
        }
    }
}

/// The lines that answer what the player asked, the time as the installed
/// client tells it; a refusal before they are in the world.
fn answer(
    asked: Asked,
    world: &ClientWorld,
    earth: jiff::civil::DateTime,
    messages: &Messages,
    clock: ClientClock,
) -> Result<Vec<Said>, String> {
    let player = world.player().ok_or("Enter the world first")?;
    Ok(match asked {
        Asked::GuildStatus(_) if clock == eq_client_assets::clock::EQMAC => {
            return Err("/guildstatus on the Mac client waits for its program read".to_owned());
        }
        Asked::GuildStatus(named) => guild_status(world, named.as_deref(), messages)
            .into_iter()
            .collect(),
        Asked::Played if clock == eq_client_assets::clock::EQMAC => {
            return Err("/played on the Mac client waits for its program read".to_owned());
        }
        Asked::Played => {
            let (time, at) = world.played().ok_or("Your played time is not known yet")?;
            played(time, at.elapsed(), &jiff::tz::TimeZone::system(), messages)
        }
        Asked::Location => {
            let numbers = whereabouts::location(player.position);
            let fallback = format!("You stand at {}.", numbers.join(", "));
            vec![messages.said_or(13177, &numbers, &fallback)]
        }
        Asked::Time => {
            if let Some(string) =
                eq_client_core::zones::number(world.zone()).and_then(|zone| clock.timeless(zone))
            {
                return Ok(vec![
                    messages.said_text(string, "The time can't be told here."),
                ]);
            }
            let norrath = world.game_time(std::time::Instant::now()).map_or_else(
                || Said::own("The time in Norrath is not known yet."),
                |time| {
                    let shown = clock.shown_hour(time.hour);
                    let style = whereabouts::DateStyle {
                        ordinal_day: clock.ordinal_days,
                        weekday: clock.weekdays,
                    };
                    let time = whereabouts::norrath(time, shown, style);
                    let fallback = format!("In Norrath it is {time}.");
                    messages.said_or(6706, std::slice::from_ref(&time), &fallback)
                },
            );
            let earth = whereabouts::earth(earth);
            let fallback = format!("On Earth it is {earth}.");
            vec![
                norrath,
                messages.said_or(12389, std::slice::from_ref(&earth), &fallback),
            ]
        }
    })
}

/// The program's guild status line (read R49-6k, 0x466970), in the
/// installed client's words: 12256 to 12258 by rank with the name and the
/// guild, 12259 with the name alone for any other rank, and 12260 for no
/// player. None where the installation lacks the string or the client
/// can't tell yet. A player in no guild gets this client's own words, as
/// the program's are its own and not in the strings.
fn guild_status(world: &ClientWorld, named: Option<&str>, messages: &Messages) -> Option<Said> {
    use eq_client_core::world::GuildStatus;
    let (id, arguments) = match world.guild_status(named)? {
        GuildStatus::NoPlayer => (12260, Vec::new()),
        GuildStatus::NoGuild { name } => {
            return Some(Said::own(format!("{name} is not in a guild.")).typed(GUILD_STATUS_TYPE));
        }
        GuildStatus::Ranked { name } => (12259, vec![name]),
        GuildStatus::Member { name, guild } => (12256, vec![name, guild]),
        GuildStatus::Officer { name, guild } => (12257, vec![name, guild]),
        GuildStatus::Leader { name, guild } => (12258, vec![name, guild]),
    };
    messages
        .formatted(id, &arguments)
        .map(|line| Said::official(line).typed(GUILD_STATUS_TYPE))
}

/// The program's three `/played` lines (read R49-8 a), `elapsed` after
/// the profile that gave `time`, with the creation time in `zone`.
fn played(
    time: eq_client_core::played::PlayedTime,
    elapsed: std::time::Duration,
    zone: &jiff::tz::TimeZone,
    messages: &Messages,
) -> Vec<Said> {
    // The program's 1/365 ([0x6471c8]), shown "%.1f".
    let years = format!("{:.1}", f64::from(time.entitled_days) * 0.002_739_726_1);
    let made = jiff::Timestamp::from_second(i64::from(time.created)).map_or_else(
        |_| time.created.to_string(),
        |made| {
            made.to_zoned(zone.clone())
                .strftime("%A, %B %-d, %Y, %H:%M:%S")
                .to_string()
        },
    );
    // One played minute per minute since the profile, as the tick adds.
    let minutes = u64::from(time.played_minutes) + elapsed.as_secs() / 60;
    let (days, hours, minutes) = (minutes / 1440, minutes % 1440 / 60, minutes % 60);
    let spent = [days, hours, minutes].map(|part| part.to_string());
    vec![
        messages.said_or(
            0xe24,
            std::slice::from_ref(&years),
            &format!("This account has {years} years of entitled time."),
        ),
        messages.said_or(
            0x32a8,
            std::slice::from_ref(&made),
            &format!("This character was made {made}."),
        ),
        messages.said_or(
            0x32af,
            &spent,
            &format!(
                "You have played {} days, {} hours and {} minutes.",
                spent[0], spent[1], spent[2]
            ),
        ),
    ]
}

/// The message type the program gives every guild status line (0x111).
const GUILD_STATUS_TYPE: u32 = 0x111;

/// Answers a `/loc`, `/time`, `/guildstatus` or `/played` the chat noted,
/// in the chat.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn answers(
    mut chat: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    messages: Option<Res<Messages>>,
    settings: Res<crate::ViewerSettings>,
) {
    let Some(asked) = chat.asked.take() else {
        return;
    };
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let now = jiff::Zoned::now().datetime();
    let clock = settings.0.installed_client.clock();
    match answer(asked, online.world(), now, messages, clock) {
        Ok(lines) => {
            for line in lines {
                chat.history.push(super::chat::system_line(line));
            }
        }
        Err(refusal) => chat.history.push(super::chat::system_line(refusal)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_assets::clock::{EQMAC, TITANIUM};
    use eq_client_core::WorldEvent;

    #[test]
    fn played_says_the_entitled_years_the_creation_and_the_played_time() {
        assert_eq!(Asked::typed("/played"), Some(Asked::Played));
        assert_eq!(Asked::typed("/PLAYED and more"), Some(Asked::Played));
        assert_eq!(Asked::typed("/playedx"), None);
        let time = eq_client_core::played::PlayedTime {
            entitled_days: 730,
            // 2004-11-09 13:20:00 UTC.
            created: 1_100_006_400,
            played_minutes: 1440 + 2 * 60 + 3,
        };
        let utc = jiff::tz::TimeZone::UTC;
        let messages = Messages::parse(
            "EQST0002
0 3
3620 Entitled %1
12968 Made %1
12975 Played %1d %2h %3m
",
        );
        let lines = |elapsed: u64, messages: &Messages| -> Vec<String> {
            played(
                time,
                std::time::Duration::from_secs(elapsed),
                &utc,
                messages,
            )
            .into_iter()
            .map(|said| said.words.text)
            .collect()
        };
        assert_eq!(
            lines(59, &messages),
            [
                "Entitled 2.0",
                "Made Tuesday, November 9, 2004, 13:20:00",
                "Played 1d 2h 3m",
            ]
        );
        // A minute after the profile, the program's tick has added one.
        assert_eq!(lines(60, &messages)[2], "Played 1d 2h 4m");
        // This client's words where the installation lacks the strings.
        assert_eq!(
            lines(0, &Messages::default())[2],
            "You have played 1 days, 2 hours and 3 minutes."
        );
    }

    #[test]
    fn played_waits_for_the_profile_and_for_the_mac_read() {
        let earth = jiff::civil::date(2026, 10, 3).at(12, 0, 0, 0);
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        let empty = Messages::default();
        assert!(answer(Asked::Played, state.world(), earth, &empty, TITANIUM).is_err());
        testing::news(
            &mut state,
            [WorldEvent::PlayedTime(eq_client_core::played::PlayedTime {
                entitled_days: 365,
                created: 0,
                played_minutes: 61,
            })],
        );
        let lines = answer(Asked::Played, state.world(), earth, &empty, TITANIUM).unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines[0].words.text,
            "This account has 1.0 years of entitled time."
        );
        assert!(answer(Asked::Played, state.world(), earth, &empty, EQMAC).is_err());
    }

    #[test]
    fn guild_status_answers_as_the_program_reads_in_the_installed_strings() {
        use eq_client_core::listing::{GuildRank, ListingChange};
        // Offered since R49-6k, with the first word after it as a name.
        assert_eq!(Asked::typed("/guildstatus"), Some(Asked::GuildStatus(None)));
        assert_eq!(
            Asked::typed("/GuildStatus  Zed more"),
            Some(Asked::GuildStatus(Some("Zed".into())))
        );
        assert_eq!(Asked::typed("/guildstatusx"), None);
        let messages = Messages::parse(
            "EQST0002
0 3
12258 %1 leads %2
12259 %1 holds a rank
12260 Target a player
",
        );
        let earth = jiff::civil::date(2026, 10, 3).at(12, 0, 0, 0);
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        let said = |state: &OnlineState, messages: &Messages, named: Option<&str>| {
            let asked = Asked::GuildStatus(named.map(str::to_owned));
            answer(asked, state.world(), earth, messages, TITANIUM).unwrap()
        };
        let official = |text: String| Said::official(text).typed(GUILD_STATUS_TYPE);
        assert_eq!(
            said(&state, &messages, None),
            [official("Target a player".into())]
        );
        state.select_target(Some(7));
        let name = state.world().player().unwrap().name.clone();
        // No guild: the program's words are its own, so ours stand.
        assert_eq!(
            said(&state, &messages, None),
            [Said::own(format!("{name} is not in a guild.")).typed(GUILD_STATUS_TYPE)]
        );
        testing::news(
            &mut state,
            [
                WorldEvent::GuildNames(vec![(3, "Seekers".into())]),
                WorldEvent::Listing {
                    spawn_id: 7,
                    change: ListingChange::Guild(Some(3)),
                },
                WorldEvent::Listing {
                    spawn_id: 7,
                    change: ListingChange::GuildRank(GuildRank::Leader),
                },
            ],
        );
        assert_eq!(
            said(&state, &messages, None),
            [official(format!("{name} leads Seekers"))]
        );
        // By name, whatever the target; a name no one has is refused.
        state.select_target(None);
        assert_eq!(
            said(&state, &messages, Some(&name.to_lowercase())),
            [official(format!("{name} leads Seekers"))]
        );
        assert_eq!(
            said(&state, &messages, Some("Nobody")),
            [official("Target a player".into())]
        );
        state.select_target(Some(7));
        testing::news(
            &mut state,
            [WorldEvent::Listing {
                spawn_id: 7,
                change: ListingChange::GuildRank(GuildRank::Other(4)),
            }],
        );
        assert_eq!(
            said(&state, &messages, None),
            [official(format!("{name} holds a rank"))]
        );
        // A string the installation lacks gives no line, never words of ours.
        testing::news(
            &mut state,
            [WorldEvent::Listing {
                spawn_id: 7,
                change: ListingChange::GuildRank(GuildRank::Member),
            }],
        );
        assert_eq!(said(&state, &messages, None), []);
        assert_eq!(said(&state, &Messages::default(), None), []);
        // The Mac client's answer wasn't read.
        assert!(
            answer(
                Asked::GuildStatus(None),
                state.world(),
                earth,
                &messages,
                EQMAC
            )
            .is_err()
        );
    }

    #[test]
    fn loc_and_time_answer_in_the_installed_strings() {
        let messages = Messages::parse(
            "EQST0002
0 3
6706 Norrath %1
12389 Earth %1
13177 At %1 %2 %3
",
        );
        let mut state = OnlineState::new(true);
        let mut player = testing::player(7);
        player.position.x = 2.0;
        player.position.y = 1.0;
        player.position.z = 3.0;
        let earth = jiff::civil::date(2026, 10, 3).at(12, 0, 0, 0);
        assert_eq!(
            answer(Asked::Location, state.world(), earth, &messages, TITANIUM),
            Err("Enter the world first".into())
        );
        testing::admit(&mut state, 1, player);
        assert_eq!(
            answer(Asked::Location, state.world(), earth, &messages, TITANIUM),
            Ok(vec![Said::official("At 1.00 2.00 3.00")])
        );
        // Before the server gives the time, Norrath's is not known.
        let lines = answer(Asked::Time, state.world(), earth, &messages, TITANIUM).unwrap();
        assert_eq!(lines[0], Said::own("The time in Norrath is not known yet."));
        assert_eq!(
            lines[1],
            Said::official("Earth Saturday, October 03, 2026 12:00:00")
        );
        testing::news(
            &mut state,
            [WorldEvent::TimeOfDay(eq_client_core::clock::GameTime {
                hour: 18,
                minute: 0,
                day: 3,
                month: 2,
                year: 3200,
            })],
        );
        let lines = answer(Asked::Time, state.world(), earth, &messages, TITANIUM).unwrap();
        assert_eq!(
            lines[0],
            Said::official("Norrath Tuesday, February 3, 3200 - 6 PM")
        );
        // Without the strings, the client says it in its own words.
        let lines = answer(
            Asked::Location,
            state.world(),
            earth,
            &Messages::default(),
            TITANIUM,
        )
        .unwrap();
        assert_eq!(lines, [Said::own("You stand at 1.00, 2.00, 3.00.")]);
        assert_eq!(Asked::typed("/LOCATION"), Some(Asked::Location));
        assert_eq!(Asked::typed("/time "), Some(Asked::Time));
        assert_eq!(Asked::typed("/timer"), None);
    }

    #[test]
    fn the_mac_client_names_the_wire_hour_and_two_zones_are_timeless() {
        let messages = Messages::parse(
            "EQST0002
0 3
4036 Timeless
6706 Norrath %1
12389 Earth %1
",
        );
        let earth = jiff::civil::date(2026, 10, 3).at(12, 0, 0, 0);
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        // Wire hour 13, the calendar's 12: the Mac client shows 1 PM.
        testing::news(
            &mut state,
            [WorldEvent::TimeOfDay(eq_client_core::clock::GameTime {
                hour: 12,
                minute: 0,
                day: 3,
                month: 2,
                year: 3200,
            })],
        );
        let lines = answer(Asked::Time, state.world(), earth, &messages, EQMAC).unwrap();
        assert_eq!(
            lines[0],
            Said::official("Norrath February 3rd, 3200 - 1 PM")
        );
        assert_eq!(
            lines[1],
            Said::official("Earth Saturday, October 03, 2026 12:00:00")
        );
        let lines = answer(Asked::Time, state.world(), earth, &messages, TITANIUM).unwrap();
        assert_eq!(
            lines[0],
            Said::official("Norrath Tuesday, February 3, 3200 - 12 PM")
        );
        // In zone 219 (`potimea`) both clients say their line alone.
        testing::news(
            &mut state,
            [WorldEvent::Entered {
                capabilities: eq_client_core::Capability::ALL.to_vec(),
                choices: Vec::new(),
                session_id: 1,
                zone: "potimea".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        assert_eq!(
            answer(Asked::Time, state.world(), earth, &messages, EQMAC),
            Ok(vec![Said::official("Timeless")])
        );
        assert_eq!(
            answer(
                Asked::Time,
                state.world(),
                earth,
                &Messages::default(),
                EQMAC
            ),
            Ok(vec![Said::own("The time can't be told here.")])
        );
        assert_eq!(
            answer(Asked::Time, state.world(), earth, &messages, TITANIUM),
            Ok(vec![Said::official("Timeless")])
        );
    }
}
