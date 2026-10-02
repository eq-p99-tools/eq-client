//! The Pet Info window opens as the player gets a pet, unless the player
//! turned the Options window's Pet Window Popup off, and closes when the pet
//! is gone; the player may close it in between.
use super::windows::{Shown, WindowId};
use bevy::prelude::*;

/// Opens the pet window for a new pet and closes it after the last one.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn window(
    online: Res<super::online::OnlineState>,
    options: Res<super::options::OptionsState>,
    mut shown: ResMut<Shown>,
    mut had: Local<Option<u16>>,
) {
    let pet = online.world().pet().map(|pet| pet.state.spawn_id);
    if pet == *had {
        return;
    }
    if pet.is_some() {
        if options.options.pet_window_popup {
            shown.open(WindowId::PetInfo);
        }
    } else {
        shown.close(WindowId::PetInfo);
    }
    *had = pet;
}

#[cfg(test)]
mod tests {
    use super::super::windows::WindowId;
    use crate::online::{OnlineState, testing};
    use bevy::prelude::*;
    use eq_client_core::WorldEvent;

    fn pet_frames(app: &mut App) -> usize {
        let mut frames = app.world_mut().query::<&WindowId>();
        frames
            .iter(app.world())
            .filter(|id| **id == WindowId::PetInfo)
            .count()
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
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some("installation".into());
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        app.insert_resource(online)
            .add_systems(Update, (super::window, crate::skinned::frames).chain());
        app.update();
        assert_eq!(pet_frames(&mut app), 0);
        let pet = testing::pet(8, 7);
        testing::spawn_entry(&mut app.world_mut().resource_mut::<OnlineState>(), 8, pet);
        app.update();
        assert_eq!(pet_frames(&mut app), 1);
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Despawn(8)],
        );
        app.update();
        assert_eq!(pet_frames(&mut app), 0);
    }
}
