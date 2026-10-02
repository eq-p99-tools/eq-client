//! Being resurrected, as the official client asks it: an offer opens the
//! skin's confirmation dialog with "%1 wants to RESURRECT you. Do you wish
//! this?", and Yes or No answers it. On Yes the server moves the player to
//! the corpse as it moves them anywhere.
use crate::{
    online::OnlineState,
    outbox::Outbox,
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, world::ClientWorld};

/// The installed client's question for a resurrection, with the caster's
/// name.
const QUESTION: u32 = 9046;

/// The confirmation dialog's Yes (true) or No (false).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AnswerButton(pub(crate) bool);

/// The confirmation dialog's text, which says what is asked.
#[derive(Component)]
pub(crate) struct QuestionText;

/// What the dialog asks about the offer, in the installed client's words
/// where it has them.
pub(crate) fn question(
    world: &ClientWorld,
    messages: Option<&crate::hud::messages::Messages>,
) -> String {
    let Some(offer) = world.resurrection() else {
        return String::new();
    };
    let caster = eq_client_core::entities::display_name(&offer.caster);
    messages.map_or_else(
        || format!("{caster} wants to RESURRECT you. Do you wish this?"),
        |messages| messages.format(QUESTION, std::slice::from_ref(&caster)),
    )
}

/// Keeps the confirmation dialog open while a resurrection waits for an
/// answer.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(online: Res<OnlineState>, mut shown: ResMut<Shown>) {
    let asking = online.world().resurrection().is_some();
    if asking != shown.is_open(WindowId::Confirmation) {
        if asking {
            shown.open(WindowId::Confirmation);
        } else {
            shown.close(WindowId::Confirmation);
        }
    }
}

/// Answers the offer when Yes or No is pressed.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    buttons: Query<(&Interaction, &AnswerButton), Changed<Interaction>>,
) {
    let Some(accept) = buttons
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, answer)| answer.0)
    else {
        return;
    };
    if online.world().resurrection().is_none() {
        return;
    }
    let sent = outbox
        .post(online.world(), |stamp| ClientCommand::AnswerResurrection {
            session_id: stamp.session_id,
            accept,
        })
        .is_ok();
    if sent {
        online.answer_resurrection();
    }
}

/// Shows the dialog's question.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    online: Res<OnlineState>,
    messages: Option<Res<crate::hud::messages::Messages>>,
    mut texts: Query<&mut Text, With<QuestionText>>,
) {
    if texts.is_empty() {
        return;
    }
    let wanted = question(online.world(), messages.as_deref());
    for mut text in &mut texts {
        if text.0 != wanted {
            text.0.clone_from(&wanted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::WorldEvent;

    #[test]
    fn an_offer_opens_the_dialog_with_the_casters_name_until_it_is_answered() {
        let mut app = crate::testing::app();
        app.add_systems(Update, window);
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        // An offer from Tester_00 to Example's corpse: the caster's name at 92
        // and the corpse's at 160 of the 228 bytes.
        let mut body = vec![0; 228];
        body[92..100].copy_from_slice(b"Tester00");
        body[160..177].copy_from_slice(b"Example's corpse0");
        let offer = eq_client_core::resurrection::titanium_offer(&body).unwrap();
        testing::news(&mut online, [WorldEvent::Resurrection(offer)]);
        assert_eq!(
            question(online.world(), None),
            "Tester wants to RESURRECT you. Do you wish this?"
        );
        app.insert_resource(online);
        app.update();
        assert!(
            app.world()
                .resource::<Shown>()
                .is_open(WindowId::Confirmation)
        );
        app.world_mut()
            .resource_mut::<OnlineState>()
            .answer_resurrection();
        app.update();
        assert!(
            !app.world()
                .resource::<Shown>()
                .is_open(WindowId::Confirmation)
        );
    }
}
