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
//!
//! `/guildstatus` is answered here too, once read R49-6k lets it be offered
//! (checklist 6.575): `EQEmu`'s guild status lines, from the target the
//! client knows, since Titanium's patch maps no `OP_GuildStatus`. A string
//! the installation lacks gives no line.
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
    /// `/guildstatus`: the target's guild and rank in it.
    GuildStatus,
}

/// Whether `/guildstatus` is offered: false until read R49-6k says whether
/// the official client answers it itself, from its target, and in which
/// strings (checklist 6.575). Only tests answer it while it is false.
const GUILD_STATUS_READ: bool = false;

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
            .is_some_and(|name| name.eq_ignore_ascii_case("charinfo"))
        {
            // Words after it are ignored: no form with words is known
            // (pending R49-6m). A script's `slash("charinfo ...")` is
            // refused at parse instead: the stricter side for scripts, on
            // purpose, so the two are not to be made to match.
            Some(Self::CharInfo)
        } else if guild_status && name.eq_ignore_ascii_case("guildstatus") {
            Some(Self::GuildStatus)
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
        Asked::GuildStatus => guild_status(world, messages).into_iter().collect(),
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

/// `EQEmu`'s guild status line for the target, in the installed client's
/// words (`Handle_OP_GuildStatus`: 12256 to 12260, inferred to be the
/// program's own until read R49-6k); none where the installation lacks the
/// string or the client can't tell.
fn guild_status(world: &ClientWorld, messages: &Messages) -> Option<Said> {
    use eq_client_core::world::GuildStatus;
    let (id, arguments) = match world.guild_status()? {
        GuildStatus::NoPlayer => (12260, Vec::new()),
        GuildStatus::NoGuild { name } => (12259, vec![name]),
        GuildStatus::Member { name, guild } => (12256, vec![name, guild]),
        GuildStatus::Officer { name, guild } => (12257, vec![name, guild]),
        GuildStatus::Leader { name, guild } => (12258, vec![name, guild]),
    };
    messages.formatted(id, &arguments).map(Said::official)
}

/// Answers a `/loc`, `/time`, `/charinfo` or `/guildstatus` the chat noted,
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
    fn guild_status_waits_for_its_read_then_answers_in_the_installed_strings_only() {
        use eq_client_core::listing::{GuildRank, ListingChange};
        // Not offered until R49-6k: the word passes on as before.
        assert_eq!(Asked::typed("/guildstatus"), None);
        assert_eq!(
            Asked::typed_when("/GuildStatus ", true),
            Some(Asked::GuildStatus)
        );
        let messages = Messages::parse(
            "EQST0002
0 3
12258 %1 leads %2
12259 %1 has no guild
12260 Target a player
",
        );
        let earth = jiff::civil::date(2026, 10, 3).at(12, 0, 0, 0);
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 1, testing::player(7));
        let said = |state: &OnlineState, messages: &Messages| {
            answer(Asked::GuildStatus, state.world(), earth, messages, TITANIUM).unwrap()
        };
        assert_eq!(said(&state, &messages), [Said::official("Target a player")]);
        state.select_target(Some(7));
        let name = state.world().player().unwrap().name.clone();
        assert_eq!(
            said(&state, &messages),
            [Said::official(format!("{name} has no guild"))]
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
            said(&state, &messages),
            [Said::official(format!("{name} leads Seekers"))]
        );
        // A string the installation lacks gives no line, never words of ours.
        testing::news(
            &mut state,
            [WorldEvent::Listing {
                spawn_id: 7,
                change: ListingChange::GuildRank(GuildRank::Member),
            }],
        );
        assert_eq!(said(&state, &messages), []);
        assert_eq!(said(&state, &Messages::default()), []);
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
