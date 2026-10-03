//! Being resurrected, as the official client asks it: an offer opens the
//! skin's confirmation dialog (`confirm`) with the official question, which
//! names the caster, and Yes or No answers it. On Yes the server moves the
//! player to the corpse as it moves them anywhere.
use crate::{online::OnlineState, outbox::Outbox};
use eq_client_core::{ClientCommand, world::ClientWorld};

/// The installed client's question for a resurrection, with the caster's
/// name.
const QUESTION: u32 = 9046;

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
        || format!("{caster} offers to resurrect you. Do you accept?"),
        |messages| messages.format(QUESTION, std::slice::from_ref(&caster)),
    )
}

/// Answers the offer waiting, Yes (true) or No.
pub(crate) fn answer(online: &mut OnlineState, outbox: &Outbox, accept: bool) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        online::testing,
        windows::{Shown, WindowId},
    };
    use bevy::prelude::*;
    use eq_client_core::WorldEvent;

    #[test]
    fn an_offer_opens_the_dialog_with_the_casters_name_until_it_is_answered() {
        let mut app = crate::testing::app();
        app.init_resource::<crate::confirm::Asked>()
            .add_systems(Update, crate::confirm::window);
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
            "Tester offers to resurrect you. Do you accept?"
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
