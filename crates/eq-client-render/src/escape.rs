//! Who an Escape press belongs to. The official client peels one layer per
//! press: an inventory action under way is cancelled first, then the
//! frontmost open window that Escape closes, and only with nothing else open
//! does Escape drop the target. Before the world, at the login screens and
//! character select, the last layer goes back one screen instead (Adam
//! 11-01, from the live client). Each of those reads this decision instead
//! of the key, so one press never does two things.
use super::windows::{Stack, WindowId};
use bevy::prelude::*;

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
    /// Goes back one screen before the world: from character select or the
    /// login server's list of worlds to the login screen, or from a login
    /// under way to the login screen as it was. It never leaves the game:
    /// on the login screen with nothing logging in, Escape only leaves the
    /// box being typed in. That the Mac-era client goes back the same way is
    /// inferred (recording checklist 6.326).
    Back,
}

/// The windows' frames, by window.
type Frames<'w, 's> =
    Query<'w, 's, (&'static WindowId, &'static Node), With<super::windows::Frame>>;

/// Decides what this frame's Escape press does. An Escape that cancelled
/// typing belongs to the chat. The login screens hold the keyboard for their
/// boxes, so there Escape is theirs whatever the typing.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn route(
    keys: super::keys::Keys,
    inventory: Res<super::inventory::InventoryState>,
    (stack, frames): (Res<Stack>, Frames),
    (online, front): (Res<super::online::OnlineState>, Res<super::login::FrontEnd>),
    mut escape: ResMut<Escape>,
    menu: Res<super::windows::menu::Menu>,
) {
    use super::login::Screen;
    let screen = front.screen(&online);
    let key = keys.input.just_pressed(KeyCode::Escape);
    *escape = match screen {
        _ if !key || keys.escape_claimed() || !keys.window_focused() => Escape::Unused,
        _ if menu.open => Escape::Close(WindowId::Selector),
        _ if screen != Screen::World
            && frames
                .iter()
                .any(|(id, node)| *id == WindowId::Scripts && node.display != Display::None) =>
        {
            Escape::Close(WindowId::Scripts)
        }
        Screen::Login | Screen::Servers if !front.has_login() || !keys.window_focused() => {
            Escape::Unused
        }
        // A login under way stops; with none, Escape leaves the box.
        Screen::Login if front.running() => Escape::Back,
        Screen::Login => Escape::Unused,
        Screen::Servers => Escape::Back,
        _ if !keys.escape_free() => Escape::Unused,
        _ if inventory.action_under_way() => Escape::Inventory,
        _ => match frontmost(&stack, &frames) {
            Some(id) => Escape::Close(id),
            // Character select goes back to the login screen, as its Quit
            // does, until Enter World is asked; a run without a login screen,
            // such as the offline preview, has nowhere to go back to.
            None if screen == Screen::Characters => {
                let entering = online
                    .selection
                    .as_ref()
                    .is_some_and(crate::character_select::Selection::entering);
                if front.has_login() && !entering {
                    Escape::Back
                } else {
                    Escape::Unused
                }
            }
            None if online.world().target().selected.is_some() => Escape::Target,
            None => Escape::Unused,
        },
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
    fn the_corner_menu_consumes_escape_before_an_open_workbench() {
        let mut app = app();
        window(&mut app, WindowId::Scripts);
        app.world_mut()
            .resource_mut::<super::super::windows::menu::Menu>()
            .open = true;
        assert_eq!(press(&mut app), Escape::Close(WindowId::Selector));
        app.world_mut()
            .resource_mut::<super::super::windows::menu::Menu>()
            .open = false;
        assert_eq!(press(&mut app), Escape::Close(WindowId::Scripts));
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
    fn scripts_follow_the_same_escape_order_as_other_world_windows() {
        let mut app = app();
        window(&mut app, WindowId::Scripts);
        window(&mut app, WindowId::Options);
        app.world_mut()
            .resource_mut::<Stack>()
            .raise(WindowId::Options);
        assert_eq!(press(&mut app), Escape::Close(WindowId::Options));
        app.world_mut()
            .resource_mut::<Stack>()
            .raise(WindowId::Scripts);
        assert_eq!(press(&mut app), Escape::Close(WindowId::Scripts));
        window(&mut app, WindowId::Item);
        assert_eq!(press(&mut app), Escape::Close(WindowId::Item));
        app.insert_resource(crate::online::OnlineState::new(true));
        assert_eq!(press(&mut app), Escape::Close(WindowId::Scripts));
    }

    #[test]
    fn with_no_window_open_escape_drops_the_target() {
        let mut app = app();
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        online.select_target(Some(1));
        app.insert_resource(online);
        assert_eq!(press(&mut app), Escape::Target);
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        assert_eq!(press(&mut app), Escape::Unused);
    }
}
