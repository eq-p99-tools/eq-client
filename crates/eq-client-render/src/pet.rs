//! The Pet Info window opens as the player gets a pet and closes when the
//! pet is gone; the player may close it in between.
use super::windows::{Shown, WindowId};
use bevy::prelude::*;

/// Opens the pet window for a new pet and closes it after the last one.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn window(
    online: Res<super::online::OnlineState>,
    mut shown: ResMut<Shown>,
    mut had: Local<Option<u16>>,
) {
    let pet = online.world().pet().map(|pet| pet.state.spawn_id);
    if pet == *had {
        return;
    }
    if pet.is_some() {
        shown.open(WindowId::PetInfo);
    } else {
        shown.close(WindowId::PetInfo);
    }
    *had = pet;
}
