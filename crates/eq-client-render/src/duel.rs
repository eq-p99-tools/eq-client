//! Another player's challenge to a duel, asked in the skin's confirmation
//! dialog and answered only by its Yes or No: no script, setting or key
//! answers one. The network session owns the question; it has no local
//! deadline (checklist 6.492, pending R49-6a).
use crate::{hud::messages::Messages, online::OnlineState, outbox::Outbox};
use eq_client_core::{ClientCommand, confirmations::ConsentId, world::ClientWorld};

/// The challenger's name, or this client's word for someone it cannot name
/// (a challenger not yet in the spawn list still asks).
fn challenger(world: &ClientWorld, id: u16) -> String {
    world
        .spawn(id)
        .map(|spawn| eq_client_core::entities::display_name(&spawn.state.name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Someone".to_owned())
}

/// What the dialog asks about the challenge waiting: the installed
/// client's string where `string_id` names one, else this client's words.
pub(crate) fn question(
    world: &ClientWorld,
    string_id: Option<u32>,
    messages: Option<&Messages>,
) -> String {
    let Some(question) = world.duel() else {
        return String::new();
    };
    let name = challenger(world, question.challenger);
    string_id
        .zip(messages)
        .and_then(|(id, messages)| messages.formatted(id, std::slice::from_ref(&name)))
        .unwrap_or_else(|| format!("{name} challenges you to a duel. Do you accept?"))
}

/// Answers the challenge waiting, Yes (true) or No, as the dialog's button
/// was pressed.
pub(crate) fn answer(online: &mut OnlineState, outbox: &Outbox, offer_id: ConsentId, accept: bool) {
    if outbox
        .post(online.world(), |stamp| ClientCommand::AnswerDuel {
            session_id: stamp.session_id,
            offer_id,
            accept,
        })
        .is_ok()
    {
        online.dismiss_consent(offer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{WorldEvent, confirmations::ConsentKind, socials::DuelQuestion};

    fn offer(sequence: u64, challenger: u16) -> WorldEvent {
        WorldEvent::DuelOffer(DuelQuestion {
            id: ConsentId {
                session_id: 1,
                kind: ConsentKind::Duel,
                sequence,
            },
            challenger,
        })
    }

    #[test]
    fn an_unlisted_challenger_still_asks() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::news(&mut online, [offer(1, 44)]);
        assert_eq!(
            question(online.world(), None, None),
            "Someone challenges you to a duel. Do you accept?"
        );
    }

    #[test]
    fn only_an_answer_sends_and_it_closes_the_question() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::news(&mut online, [offer(1, 44)]);
        let id = online.world().duel().unwrap().id;
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(tx));
        // The question alone sends nothing.
        assert!(rx.try_recv().is_err());
        answer(&mut online, &outbox, id, false);
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::AnswerDuel { session_id: 1, offer_id, accept: false }) if offer_id == id
        ));
        assert!(online.world().duel().is_none());
    }
}
