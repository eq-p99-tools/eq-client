//! The Group window opens as the player is invited to a group or joins one,
//! and closes when neither is so: the skin gives it no close box, and the
//! official client's string for an invitation points the player to its
//! buttons (inferred).
use super::windows::{Shown, WindowId};
use bevy::prelude::*;

/// Opens the group window while the player is in a group or invited to one,
/// and closes it otherwise.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn window(
    online: Res<super::online::OnlineState>,
    mut shown: ResMut<Shown>,
    mut had: Local<bool>,
) {
    let world = online.world();
    let wanted = world.group().is_some() || world.group_invitation().is_some();
    if wanted == *had {
        return;
    }
    if wanted {
        shown.open(WindowId::Group);
    } else {
        shown.close(WindowId::Group);
    }
    *had = wanted;
}

#[cfg(test)]
mod tests {
    use super::super::windows::WindowId;
    use crate::online::{OnlineState, testing};
    use bevy::prelude::*;
    use eq_client_core::{WorldEvent, group::GroupUpdate};

    fn group_frames(app: &mut App) -> usize {
        let mut frames = app.world_mut().query::<&WindowId>();
        frames
            .iter(app.world())
            .filter(|id| **id == WindowId::Group)
            .count()
    }

    fn news(app: &mut App, update: GroupUpdate) {
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Group(update)],
        );
        app.update();
    }

    #[test]
    fn the_group_window_opens_with_an_invitation_or_a_group() {
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
        assert_eq!(group_frames(&mut app), 0);
        news(
            &mut app,
            GroupUpdate::Invited {
                inviter: "Leader".into(),
            },
        );
        assert_eq!(group_frames(&mut app), 1);
        // Declining leaves no group and no invitation.
        news(
            &mut app,
            GroupUpdate::Declining {
                inviter: "Leader".into(),
            },
        );
        assert_eq!(group_frames(&mut app), 0);
        news(&mut app, GroupUpdate::Formed);
        assert_eq!(group_frames(&mut app), 1);
        news(&mut app, GroupUpdate::Disbanded);
        assert_eq!(group_frames(&mut app), 0);
    }
}
