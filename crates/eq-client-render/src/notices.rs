//! Words the world's notices: what each says to the player and where it
//! shows. The server's strings come from the installed client's table.
use super::{
    combat::{consideration_text, damage_text},
    hud::messages::Messages,
    trade::coin_text,
};
use eq_client_core::{
    CampStatus,
    entities::display_name,
    loot::LootResponse,
    world::{Link, Notice},
};

/// How long feedback on the player's own action stays on screen.
pub(crate) const FLASH: std::time::Duration = std::time::Duration::from_secs(3);

/// One line on screen besides the chat, which notices and the player's own
/// actions set: what it says, and until when if it says it for a moment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Line {
    text: String,
    until: Option<std::time::Instant>,
}

impl Line {
    /// Says this until something else is said.
    pub(crate) fn set(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.until = None;
    }

    /// Says this for a moment ([`FLASH`]).
    pub(crate) fn flash(&mut self, text: impl Into<String>, now: std::time::Instant) {
        self.text = text.into();
        self.until = Some(now + FLASH);
    }

    /// Says nothing.
    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.until = None;
    }

    /// Moves a moment's words this much closer to passing.
    #[cfg(test)]
    pub(crate) fn age(&mut self, by: std::time::Duration) {
        self.until = self.until.and_then(|until| until.checked_sub(by));
    }

    /// What the line says now: nothing once a moment's words have passed.
    pub(crate) fn text(&self, now: std::time::Instant) -> &str {
        if self.until.is_some_and(|until| now >= until) {
            ""
        } else {
            &self.text
        }
    }
}

/// The client's lines besides the chat, one for each place a notice shows.
/// Each has one writer API, so every line is set, cleared or expires the
/// same way.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct Lines {
    /// The status line: the connection, death and camping.
    pub status: Line,
    /// The target window's line.
    pub target: Line,
    /// Feedback on the player's last action, for a moment.
    pub feedback: Line,
    /// What became of the last door the player used.
    pub door: Line,
}

/// Where a notice shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Place {
    /// A system line in the chat window.
    Chat,
    /// The HUD's status line.
    Status,
    /// The target window's status line.
    Target,
    /// The action bar's short-lived feedback.
    Feedback,
    /// The door line under the zone status.
    Door,
}

/// Sense Heading in the official client's own words, with the point's name.
fn heading(point: u8, messages: &Messages) -> String {
    let point = messages.format(12427 + u32::from(point), &[]);
    messages.format(12435, &[point])
}

/// The official client's words for food or drink it could not find.
fn nothing_to_eat((food, water): (bool, bool), messages: &Messages) -> String {
    messages.format(
        match (food, water) {
            (true, true) => 12491,
            (true, false) => 12488,
            _ => 12490,
        },
        &[],
    )
}

/// How a notice reads, and where each part shows.
pub(super) fn wording(notice: &Notice, messages: Option<&Messages>) -> Vec<(Place, String)> {
    let chat = |text: String| vec![(Place::Chat, text)];
    match notice {
        Notice::Connection { link, dead } => vec![(
            Place::Status,
            if *dead {
                "Dead - awaiting respawn".into()
            } else {
                link_text(*link).into()
            },
        )],
        Notice::ServerString { id, arguments } => chat(messages.map_or_else(
            || format!("Server message {id}"),
            |messages| messages.format(*id, arguments),
        )),
        Notice::Consideration {
            consideration,
            name,
        } => messages.map_or_else(Vec::new, |messages| {
            let name = name.as_deref().map(display_name).unwrap_or_default();
            chat(consideration_text(messages, &name, consideration))
        }),
        Notice::Damage {
            damage,
            own_id,
            source,
            target,
        } => messages
            .and_then(|messages| {
                damage_text(
                    messages,
                    *own_id,
                    |id| {
                        let name = if id == damage.source_id {
                            source
                        } else {
                            target
                        };
                        name.as_deref()
                            .map_or_else(|| "someone".to_owned(), display_name)
                    },
                    damage,
                )
            })
            .map_or_else(Vec::new, chat),
        Notice::Heading(point) => messages
            .map(|messages| heading(*point, messages))
            .map_or_else(Vec::new, chat),
        Notice::Camp(status) => match status {
            CampStatus::Preparing => messages.map(|messages| messages.format(12293, &[])),
            CampStatus::Abandoned => messages.map(|messages| messages.format(12290, &[])),
            CampStatus::LoggingOut => Some("Logging out...".into()),
            CampStatus::Camped => None,
            CampStatus::Rejected(reason) => Some(reason.clone()),
        }
        .map_or_else(Vec::new, chat),
        Notice::LootCoins(coins) => chat(format!(
            "You receive {} from the corpse.",
            coin_text(coins.total_copper())
        )),
        Notice::LootRefused(response) => chat(
            match response {
                LootResponse::SomeoneElse => "Someone else is looting that corpse.",
                LootResponse::NotAtThisTime => "You cannot loot that corpse at this time.",
                LootResponse::Hostiles => "You cannot loot while a hostile is nearby.",
                LootResponse::TooFar => "You are too far away to loot that corpse.",
                LootResponse::Normal | LootResponse::Other(_) => "You cannot loot that corpse.",
            }
            .into(),
        ),
        Notice::ItemRefused => chat("You cannot take that item.".into()),
        Notice::ShopRefused => chat("That merchant will not trade with you.".into()),
        Notice::TradeRefused(reason)
        | Notice::AbilityRefused(reason)
        | Notice::ConsumeRefused(reason) => chat(reason.clone()),
        // The official client's words for what it could not find.
        Notice::NothingToEat { food, water } => messages
            .map(|messages| nothing_to_eat((*food, *water), messages))
            .map_or_else(Vec::new, chat),
        // "You are too far away to trade."
        Notice::GiveRefused(reason) | Notice::GroundRefused(reason) => {
            chat(super::ground::refusal(reason))
        }
        // A click sent says nothing, as in the official client; a refused
        // one says why, and a later click clears it.
        Notice::Door { error, .. } => vec![(Place::Door, error.clone().unwrap_or_default())],
        Notice::TransferRefused(reason) => vec![
            (Place::Status, reason.to_string()),
            (Place::Chat, reason.to_string()),
        ],
        Notice::ZoneLineRefused(reason) => {
            vec![(Place::Status, format!("Cannot cross zone line: {reason}"))]
        }
        Notice::TargetRefused(reason) => {
            vec![(Place::Target, format!("Target rejected: {reason}"))]
        }
        Notice::CastRefused { spell_id, reason } => vec![(
            Place::Feedback,
            format!("Cast rejected (spell {spell_id}): {reason}"),
        )],
    }
}

/// The status line for where the connection stands; nothing while the
/// player is in the world.
const fn link_text(link: Link) -> &'static str {
    match link {
        Link::Offline | Link::Connected => "",
        Link::LoggingIn => "Logging in",
        Link::Entering => "Entering the world",
        Link::Zoning => "Zoning",
        Link::Ended => "Disconnected",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_says_its_words_until_cleared_and_a_flash_passes() {
        let now = std::time::Instant::now();
        let mut line = Line::default();
        line.set("Zoning");
        assert_eq!(line.text(now + FLASH * 10), "Zoning");
        line.flash("Request queue is full", now);
        assert_eq!(line.text(now), "Request queue is full");
        assert_eq!(line.text(now + FLASH), "");
        line.set("Camped - choose a character");
        line.clear();
        assert_eq!(line.text(now), "");
    }
    use eq_client_core::Coins;

    #[test]
    fn loot_replies_say_what_the_corpse_gave_or_refused() {
        let line = |notice| wording(&notice, None);
        assert_eq!(
            line(Notice::LootCoins(Coins {
                platinum: 0,
                gold: 1,
                silver: 0,
                copper: 2,
            })),
            [(Place::Chat, "You receive 1g 2c from the corpse.".into())]
        );
        assert_eq!(
            line(Notice::ItemRefused),
            [(Place::Chat, "You cannot take that item.".into())]
        );
        assert_eq!(
            line(Notice::LootRefused(LootResponse::TooFar)),
            [(
                Place::Chat,
                "You are too far away to loot that corpse.".into()
            )]
        );
        assert_eq!(
            line(Notice::ShopRefused),
            [(Place::Chat, "That merchant will not trade with you.".into())]
        );
    }

    #[test]
    fn refusals_show_where_their_request_was_made() {
        assert_eq!(
            wording(&Notice::TargetRefused("Too far away".into()), None),
            [(Place::Target, "Target rejected: Too far away".into())]
        );
        assert_eq!(
            wording(
                &Notice::Door {
                    door_id: 4,
                    error: None
                },
                None
            ),
            [(Place::Door, String::new())]
        );
        let transfer = wording(
            &Notice::TransferRefused(eq_client_core::ZoneRejection::Cancelled),
            None,
        );
        assert_eq!(
            transfer.iter().map(|(place, _)| *place).collect::<Vec<_>>(),
            [Place::Status, Place::Chat]
        );
        // Without the installed strings, a server string still says which it was.
        assert_eq!(
            wording(
                &Notice::ServerString {
                    id: 12293,
                    arguments: Vec::new()
                },
                None
            ),
            [(Place::Chat, "Server message 12293".into())]
        );
        assert_eq!(wording(&Notice::Camp(CampStatus::Camped), None), []);
    }
}
