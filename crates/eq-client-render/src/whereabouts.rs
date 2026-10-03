//! `/loc` and `/time`, which the client answers itself, as the official
//! client does: where the player stands, in the installed string 13177, and
//! the time in Norrath and on Earth, in 6706 and 12389. That the official
//! client words them so is inferred from what each string says; this
//! client's words stand in without the strings.
use super::chat::{ChatState, Said};
use super::hud::messages::Messages;
use bevy::prelude::*;
use eq_client_core::{whereabouts, world::ClientWorld};

/// Which the player asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// `/loc`: where the player stands.
    Location,
    /// `/time`: the time in Norrath and on Earth.
    Time,
}

impl Asked {
    /// What a typed line asks, if it is `/loc` or `/time`.
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let name = input.strip_prefix('/')?.trim();
        if name.eq_ignore_ascii_case("loc") {
            Some(Self::Location)
        } else if name.eq_ignore_ascii_case("time") {
            Some(Self::Time)
        } else {
            None
        }
    }
}

/// The lines that answer what the player asked; a refusal before they are
/// in the world.
fn answer(
    asked: Asked,
    world: &ClientWorld,
    earth: chrono::NaiveDateTime,
    messages: &Messages,
) -> Result<Vec<Said>, String> {
    let player = world.player().ok_or("Enter the world first")?;
    Ok(match asked {
        Asked::Location => {
            let numbers = whereabouts::location(player.position);
            let fallback = format!("You stand at {}.", numbers.join(", "));
            vec![messages.said_or(13177, &numbers, &fallback)]
        }
        Asked::Time => {
            let norrath = world.game_time(std::time::Instant::now()).map_or_else(
                || Said::own("The time in Norrath is not known yet."),
                |time| {
                    let time = whereabouts::norrath(time);
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

/// Answers a `/loc` or `/time` the chat noted, in the chat.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn answers(
    mut chat: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    messages: Option<Res<Messages>>,
) {
    let Some(asked) = chat.asked.take() else {
        return;
    };
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let now = chrono::Local::now().naive_local();
    match answer(asked, online.world(), now, messages) {
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
    use eq_client_core::WorldEvent;

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
        let earth = chrono::NaiveDate::from_ymd_opt(2026, 10, 3)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        assert_eq!(
            answer(Asked::Location, state.world(), earth, &messages),
            Err("Enter the world first".into())
        );
        testing::admit(&mut state, 1, player);
        assert_eq!(
            answer(Asked::Location, state.world(), earth, &messages),
            Ok(vec![Said::official("At 1.00 2.00 3.00")])
        );
        // Before the server gives the time, Norrath's is not known.
        let lines = answer(Asked::Time, state.world(), earth, &messages).unwrap();
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
        let lines = answer(Asked::Time, state.world(), earth, &messages).unwrap();
        assert_eq!(lines[0], Said::official("Norrath February 3, 3200 - 6 PM"));
        // Without the strings, the client says it in its own words.
        let lines = answer(Asked::Location, state.world(), earth, &Messages::default()).unwrap();
        assert_eq!(lines, [Said::own("You stand at 1.00, 2.00, 3.00.")]);
        assert_eq!(Asked::typed("/LOC"), Some(Asked::Location));
        assert_eq!(Asked::typed("/time "), Some(Asked::Time));
        assert_eq!(Asked::typed("/timer"), None);
    }
}
