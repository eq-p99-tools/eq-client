//! Who an Escape press belongs to. The official client peels one layer per
//! press: an inventory action under way is cancelled first, then the
//! frontmost open window that Escape closes, and only with nothing else open
//! does Escape drop the target. Each of those reads this decision instead of
//! the key, so one press never does two things.
use super::windows::{Stack, WindowId};
use bevy::{prelude::*, window::PrimaryWindow};

/// What this frame's Escape press does, if anything.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub(crate) enum Escape {
    /// No Escape this frame, or nothing it applies to.
    #[default]
    Unused,
    /// Cancels the inventory action under way, such as a split.
    Inventory,
    /// Closes this window.
    Close(WindowId),
    /// Drops the target.
    Target,
}

/// The windows' frames, by window.
type Frames<'w, 's> =
    Query<'w, 's, (&'static WindowId, &'static Node), With<super::windows::Frame>>;

/// Decides what this frame's Escape press does. An Escape that cancelled
/// typing belongs to the chat.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn route(
    keys: Res<ButtonInput<KeyCode>>,
    chat: Res<super::chat::ChatState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    inventory: Res<super::inventory::InventoryState>,
    (stack, frames): (Res<Stack>, Frames),
    online: Res<super::online::OnlineState>,
    mut escape: ResMut<Escape>,
) {
    let pressed = keys.just_pressed(KeyCode::Escape)
        && !chat.composing
        && !chat.escape_consumed
        && windows.single().is_ok_and(|window| window.focused);
    *escape = if !pressed {
        Escape::Unused
    } else if inventory.action_under_way() {
        Escape::Inventory
    } else if let Some(id) = frontmost(&stack, &frames) {
        Escape::Close(id)
    } else if online.world.target().selected.is_some() {
        Escape::Target
    } else {
        Escape::Unused
    };
}

/// The frontmost open window that Escape closes: a popup first, as it draws
/// above the rest, then the floating windows from the front.
fn frontmost(stack: &Stack, frames: &Frames) -> Option<WindowId> {
    let open = |id: &WindowId| {
        id.describe().closes_on_escape
            && frames
                .iter()
                .any(|(frame, node)| frame == id && node.display != Display::None)
    };
    let popups = WindowId::ALL
        .into_iter()
        .filter(|id| id.describe().layer == super::windows::Layer::Popup);
    popups.chain(stack.front_to_back()).find(open)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = crate::testing::app();
        app.add_systems(Update, route);
        app
    }

    fn press(app: &mut App) -> Escape {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        *app.world().resource::<Escape>()
    }

    fn window(app: &mut App, id: WindowId) -> Entity {
        app.world_mut()
            .spawn((id, Node::default(), super::super::windows::Frame::default()))
            .id()
    }

    #[test]
    fn escape_closes_the_frontmost_open_window_first_then_the_target() {
        let mut app = app();
        let inventory = window(&mut app, WindowId::Inventory);
        window(&mut app, WindowId::Spellbook);
        window(&mut app, WindowId::Effects);
        app.world_mut()
            .resource_mut::<Stack>()
            .raise(WindowId::Spellbook);
        assert_eq!(press(&mut app), Escape::Close(WindowId::Spellbook));
        app.world_mut()
            .resource_mut::<Stack>()
            .raise(WindowId::Inventory);
        assert_eq!(press(&mut app), Escape::Close(WindowId::Inventory));
        // A closed window is passed over; the effects window never closes.
        app.world_mut().get_mut::<Node>(inventory).unwrap().display = Display::None;
        assert_eq!(press(&mut app), Escape::Close(WindowId::Spellbook));
        // An inspected item sits above the rest and closes first.
        window(&mut app, WindowId::Item);
        assert_eq!(press(&mut app), Escape::Close(WindowId::Item));
    }

    #[test]
    fn with_no_window_open_escape_drops_the_target() {
        let mut app = app();
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        online.world.select_target(Some(1));
        app.insert_resource(online);
        assert_eq!(press(&mut app), Escape::Target);
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .composing = true;
        assert_eq!(press(&mut app), Escape::Unused);
    }
}
