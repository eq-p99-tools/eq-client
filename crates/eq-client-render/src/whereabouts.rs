//! `/location` (`/loc`) and `/time`, which the client answers itself, as the official
//! client does: where the player stands, in the installed string 13177, and
//! the time in Norrath and on Earth, in 6706 and 12389, or a zone's line in
//! place of the time where the installed client gives one. The Titanium
//! client answers `/time` in 6706 and 12389 (from the program); the Mac
//! client builds its Norrath line from a format of its own, which this
//! client doesn't copy, so 6706 stands in there. That `/loc` is worded so is
//! inferred from what 13177 says; this client's words stand in without the
//! strings. `/charinfo` says where the player is bound (checklist 6.524):
//! its handler (0x46f6ec) is unread, so its line is this client's (pending
//! R49-6m), and the bind is the profile's, so one that lands mid-session
//! shows from the next zone-in (inferred).
use super::chat::{ChatState, Said};
use super::hud::messages::Messages;
use bevy::prelude::*;
use eq_client_assets::clock::ClientClock;
use eq_client_core::{whereabouts, world::ClientWorld};

/// Which the player asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// `/loc`: where the player stands.
    Location,
    /// `/time`: the time in Norrath and on Earth.
    Time,
    /// `/charinfo`: where the player is bound.
    CharInfo,
}

impl Asked {
    /// What a typed line asks, if it is `/location` or `/time`, spelled out.
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let name = input.strip_prefix('/')?.trim();
        if name.eq_ignore_ascii_case("location") {
            Some(Self::Location)
        } else if name.eq_ignore_ascii_case("time") {
            Some(Self::Time)
        } else if name
            .split_whitespace()
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("charinfo"))
        {
            // Words after it are ignored: no form with words is known
            // (pending R49-6m).
            Some(Self::CharInfo)
        } else {
            None
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
        Asked::CharInfo => vec![bound_in(world)],
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

/// Where the player is bound, in this client's words (pending R49-6m):
/// the zone's long name, its short name where the list has no long one,
/// and its number where it has neither.
fn bound_in(world: &ClientWorld) -> Said {
    let Some(zone) = world.bound_in() else {
        return Said::own("Your bind point is not known yet.");
    };
    let short = eq_client_core::zones::short_name(u32::from(zone));
    let name = short.map_or_else(
        || format!("an unknown zone ({zone})"),
        |short| {
            eq_client_core::zones::long_name(short)
                .unwrap_or(short)
                .to_owned()
        },
    );
    Said::own(format!("Your bind point is in {name}."))
}

/// Answers a `/loc`, `/time` or `/charinfo` the chat noted, in the chat.
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
    fn charinfo_says_the_bind_zone_in_this_clients_words() {
        let messages = Messages::default();
        let earth = jiff::civil::date(2026, 10, 3).at(12, 0, 0, 0);
        let mut state = OnlineState::new(true);
        let said = |state: &OnlineState| {
            answer(Asked::CharInfo, state.world(), earth, &messages, TITANIUM)
        };
        assert_eq!(said(&state), Err("Enter the world first".into()));
        testing::admit(&mut state, 1, testing::player(7));
        assert_eq!(
            said(&state),
            Ok(vec![Said::own("Your bind point is not known yet.")])
        );
        testing::news(&mut state, [WorldEvent::BoundIn { zone_id: 1 }]);
        let short = eq_client_core::zones::short_name(1).unwrap();
        let long = eq_client_core::zones::long_name(short).unwrap();
        assert_eq!(
            said(&state),
            Ok(vec![Said::own(format!("Your bind point is in {long}."))])
        );
        // A zone the list doesn't know is named by its number.
        testing::news(&mut state, [WorldEvent::BoundIn { zone_id: 65_000 }]);
        assert_eq!(
            said(&state),
            Ok(vec![Said::own(
                "Your bind point is in an unknown zone (65000)."
            )])
        );
        // The next admission waits for its own profile.
        testing::admit(&mut state, 2, testing::player(7));
        assert_eq!(
            said(&state),
            Ok(vec![Said::own("Your bind point is not known yet.")])
        );
        assert_eq!(Asked::typed("/charinfo"), Some(Asked::CharInfo));
        assert_eq!(Asked::typed("/CHARINFO please"), Some(Asked::CharInfo));
        assert_eq!(Asked::typed("/charinfox"), None);
        assert_eq!(
            crate::commands::spelled("/char").as_deref(),
            Some("/charinfo")
        );
    }

    /// The answer is said in the chat, and nothing goes to the server (6.524).
    #[test]
    fn charinfo_sends_nothing() {
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        testing::news(&mut state, [WorldEvent::BoundIn { zone_id: 1 }]);
        let (queue, received) = std::sync::mpsc::sync_channel(4);
        let mut app = App::new();
        app.insert_resource(state)
            .insert_resource(crate::outbox::Outbox::new(Some(queue)))
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .init_resource::<ChatState>()
            .add_systems(Update, answers);
        app.world_mut().resource_mut::<ChatState>().asked = Some(Asked::CharInfo);
        app.update();
        let chat = app.world().resource::<ChatState>();
        let lines = chat.history.lines(eq_client_core::chat::ChatTab::System);
        assert!(
            lines
                .iter()
                .any(|(_, line)| line.message.text.starts_with("Your bind point is in "))
        );
        assert!(received.try_recv().is_err());
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
