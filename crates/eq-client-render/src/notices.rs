//! Words the world's notices: what each says to the player and where it
//! shows. The server's strings come from the installed client's table.
use super::{
    combat::{consideration_text, damage_text, display_name},
    hud::messages::Messages,
    trade::coin_text,
};
use eq_client_core::{CampStatus, loot::LootResponse, world::Notice};

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

/// How a notice reads, and where each part shows.
pub(super) fn wording(notice: &Notice, messages: Option<&Messages>) -> Vec<(Place, String)> {
    let chat = |text: String| vec![(Place::Chat, text)];
    match notice {
        Notice::Connection { label, dead } => vec![(
            Place::Status,
            if *dead {
                "Dead - awaiting respawn".into()
            } else {
                label.clone()
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
        Notice::TradeRefused(reason) => chat(reason.clone()),
        Notice::GroundRefused(reason) => chat(super::ground::refusal(reason)),
        Notice::Door { door_id, error } => vec![(
            Place::Door,
            error.as_ref().map_or_else(
                || format!("Door {door_id}: request sent"),
                |error| format!("Door {door_id}: {error}"),
            ),
        )],
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

#[cfg(test)]
mod tests {
    use super::*;
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
            [(Place::Door, "Door 4: request sent".into())]
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
        assert!(wording(&Notice::Camp(CampStatus::Camped), None).is_empty());
    }
}
