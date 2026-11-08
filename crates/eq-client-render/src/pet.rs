//! The Pet Info window opens as the player gets a pet and closes when the
//! pet is gone. A new pet shows it unless the player turned the Options
//! window's Pet Window Popup off; the player may hide it and show it again
//! from the selector in between.
use super::windows::{Shown, WindowId};
use bevy::prelude::*;

/// Routes /pet target through the ordinary selection path.
pub(super) fn target_command(
    input: &str,
    online: &mut super::online::OnlineState,
    outbox: &super::outbox::Outbox,
) -> Option<Result<(), String>> {
    let mut words = input.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("/pet") || !words.next()?.eq_ignore_ascii_case("target")
    {
        return None;
    }
    Some((|| {
        if words.next().is_some() {
            return Err("Use /pet target without arguments".into());
        }
        let world = online.world();
        let pet = world
            .pet()
            .filter(|pet| world.can_see(&pet.state))
            .and(world.pet_intent())
            .ok_or("No visible pet to target")?;
        super::target::try_select(online, outbox, Some(pet.spawn_id), Some(pet))
            .map_err(|refusal| refusal.text().to_owned())
    })())
}

/// Opens the pet window for a new pet, shown as the Pet Window Popup option
/// says, and closes it after the last one. The retained admission's pet
/// still owns the window during a temporary interruption.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn window(
    online: Res<super::online::OnlineState>,
    options: Res<super::options::OptionsState>,
    mut shown: ResMut<Shown>,
    mut had: Local<Option<(u64, u16, u64)>>,
) {
    let world = online.world();
    shown.available(WindowId::PetInfo, world.in_world());
    let pet = match (world.session_id(), world.player()) {
        (Some(session_id), Some(player)) if !world.ended() && !world.is_dead() => world
            .pet_of(player.spawn_id)
            .map(|pet| (session_id, pet.state.spawn_id, pet.revision)),
        _ => None,
    };
    if pet == *had {
        return;
    }
    if pet.is_some() {
        shown.open(WindowId::PetInfo);
        shown.hide(WindowId::PetInfo, !options.options.pet_window_popup);
    } else {
        shown.close(WindowId::PetInfo);
    }
    *had = pet;
}

#[cfg(test)]
mod tests {
    use super::super::windows::{Shown, WindowId};
    use crate::online::{OnlineState, testing};
    use bevy::prelude::*;
    use eq_client_core::{WorldEvent, world::Link};

    #[test]
    fn pet_target_uses_normal_selection_and_rejects_an_invisible_pet() {
        use eq_client_core::ClientCommand;
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(sender));
        let qol = eq_client_core::qol::Settings::default();
        let target_pet = |online: &mut OnlineState| {
            crate::chat::submit_game_command("/PeT  TaRgEt", online, &outbox, &qol)
        };
        assert!(target_pet(&mut online).is_err());
        assert!(receiver.try_recv().is_err());

        testing::spawn_entry(&mut online, 8, testing::pet(8, 7));
        let identity = eq_client_core::pets::PetIdentity {
            spawn_id: 8,
            revision: 2,
        };
        testing::news(
            &mut online,
            [WorldEvent::OwnPet {
                session_id: 1,
                pet: Some(identity),
            }],
        );
        target_pet(&mut online).unwrap();
        assert!(
            matches!(receiver.try_recv().unwrap(), ClientCommand::SelectTarget {
            session_id: 1, spawn_id: Some(8), expected_pet: Some(pet), ..
        } if pet == identity)
        );
        assert_eq!(online.world().target().selected, Some(8));
        testing::news(
            &mut online,
            [WorldEvent::Visibility {
                spawn_id: 8,
                state: eq_client_core::vision::Invisibility::Titanium(1),
            }],
        );
        assert!(target_pet(&mut online).is_err());
        assert!(receiver.try_recv().is_err());
    }

    fn pet_frames(app: &mut App) -> usize {
        let mut frames = app.world_mut().query::<&WindowId>();
        frames
            .iter(app.world())
            .filter(|id| **id == WindowId::PetInfo)
            .count()
    }

    fn window_app() -> App {
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some("installation".into());
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        app.insert_resource(online)
            .add_systems(Update, (super::window, crate::skinned::frames).chain());
        app
    }

    fn spawn_pet(app: &mut App) {
        testing::spawn_entry(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            8,
            testing::pet(8, 7),
        );
    }

    #[test]
    fn a_pet_buff_slot_shows_only_while_it_holds_a_buff() {
        use eq_client_core::pets::{PetBuff, PetBuffs};
        let mut app = crate::testing::app();
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::spawn_entry(&mut online, 8, testing::pet(8, 7));
        testing::news(
            &mut online,
            [WorldEvent::PetBuffs(PetBuffs {
                pet: 8,
                slots: vec![
                    Some(PetBuff {
                        spell_id: 278,
                        ticks: 5,
                    }),
                    None,
                ],
            })],
        );
        app.insert_resource(online)
            .add_systems(Update, crate::skinned::show);
        let shows = |slot| (crate::skinned::Shows::PetBuff(slot), Visibility::Hidden);
        let held = app.world_mut().spawn(shows(0)).id();
        let empty = app.world_mut().spawn(shows(1)).id();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(held),
            Some(&Visibility::Inherited)
        );
        assert_eq!(
            app.world().get::<Visibility>(empty),
            Some(&Visibility::Hidden)
        );
    }

    #[test]
    fn the_pet_window_opens_with_a_pet_and_closes_when_it_is_gone() {
        let mut app = window_app();
        app.update();
        assert_eq!(pet_frames(&mut app), 0);
        spawn_pet(&mut app);
        app.update();
        assert_eq!(pet_frames(&mut app), 1);
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Despawn(8)],
        );
        app.update();
        assert_eq!(pet_frames(&mut app), 0);
    }

    #[test]
    fn same_admission_resume_preserves_the_pet_windows_visibility_choice() {
        let mut app = window_app();
        spawn_pet(&mut app);
        app.update();
        assert_eq!(pet_frames(&mut app), 1);

        // First the window is open, then the player hides it using its close box.
        for shown_frames in [1, 0] {
            if shown_frames == 0 {
                app.world_mut()
                    .resource_mut::<Shown>()
                    .dismiss(WindowId::PetInfo);
                app.update();
                assert_eq!(pet_frames(&mut app), 0);
            }
            testing::link(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                Link::Zoning,
            );
            app.update();
            assert_eq!(pet_frames(&mut app), 0);
            assert!(app.world().resource::<Shown>().is_open(WindowId::PetInfo));
            assert!(
                app.world()
                    .resource::<OnlineState>()
                    .world()
                    .pet()
                    .is_none()
            );

            testing::link(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                Link::Connected,
            );
            app.update();
            assert_eq!(pet_frames(&mut app), shown_frames);
        }

        // A new incarnation still honors the popup option if the ID is reused.
        spawn_pet(&mut app);
        app.update();
        assert_eq!(pet_frames(&mut app), 1);
    }

    #[test]
    fn a_new_admission_opens_pet_info_after_death() {
        let mut app = window_app();
        spawn_pet(&mut app);
        app.update();
        app.world_mut()
            .resource_mut::<Shown>()
            .dismiss(WindowId::PetInfo);
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::DeathBind { session_id: 1 }],
        );
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::PetInfo));

        testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            2,
            testing::player(7),
        );
        spawn_pet(&mut app);
        app.update();
        assert_eq!(pet_frames(&mut app), 1);
    }

    #[test]
    fn without_the_popup_a_new_pets_window_waits_for_the_player() {
        let mut app = window_app();
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .pet_window_popup = false;
        spawn_pet(&mut app);
        app.update();
        assert_eq!(pet_frames(&mut app), 0);
        // The selector shows it, and its close box hides it again.
        app.world_mut()
            .resource_mut::<Shown>()
            .hide(WindowId::PetInfo, false);
        app.update();
        assert_eq!(pet_frames(&mut app), 1);
        app.world_mut()
            .resource_mut::<Shown>()
            .dismiss(WindowId::PetInfo);
        app.update();
        assert_eq!(pet_frames(&mut app), 0);
    }
}
