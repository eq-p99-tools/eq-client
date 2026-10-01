//! Which windows are open and which is in front: the floating windows stack
//! in the order the player last clicked them, Escape closes the top one, and
//! the windows the player opens and closes do so from the selector or their
//! key, the same way for each.
use super::registry::{Layer, WindowId};
use crate::theme::{self, Size};
use bevy::{prelude::*, window::PrimaryWindow};
use std::collections::BTreeSet;

/// The floating windows from the back to the front.
#[derive(Resource)]
pub(crate) struct Stack(Vec<WindowId>);

impl Default for Stack {
    fn default() -> Self {
        Self(
            WindowId::ALL
                .into_iter()
                .filter(|id| id.describe().layer == Layer::Floating)
                .collect(),
        )
    }
}

impl Stack {
    /// Brings a window to the front.
    pub(crate) fn raise(&mut self, id: WindowId) {
        if self.0.last() == Some(&id) {
            return;
        }
        self.0.retain(|other| *other != id);
        self.0.push(id);
    }

    /// The windows from the front to the back.
    pub(crate) fn front_to_back(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.0.iter().rev().copied()
    }

    /// Where a floating window draws: in front of every window below it in
    /// the stack.
    fn z(&self, id: WindowId) -> i32 {
        let place = self.0.iter().position(|other| *other == id).unwrap_or(0);
        Layer::Floating.base() + i32::try_from(place).unwrap_or(0)
    }
}

/// The windows the player opens and closes that are open now.
#[derive(Resource)]
pub(crate) struct Shown(BTreeSet<WindowId>);

impl Default for Shown {
    /// The effects window starts open, as in the official client.
    fn default() -> Self {
        Self([WindowId::Effects].into())
    }
}

impl Shown {
    pub(crate) fn is_open(&self, id: WindowId) -> bool {
        self.0.contains(&id)
    }

    pub(crate) fn open(&mut self, id: WindowId) {
        self.0.insert(id);
    }

    pub(crate) fn close(&mut self, id: WindowId) {
        self.0.remove(&id);
    }

    /// The open windows.
    pub(crate) fn ids(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.0.iter().copied()
    }

    fn toggle(&mut self, id: WindowId) {
        if !self.0.remove(&id) {
            self.0.insert(id);
        }
    }
}

/// A selector button that opens and closes a window.
#[derive(Component, Clone, Copy)]
pub(crate) struct SelectorButton(pub WindowId);

/// A selector button's label: the window's name and its key.
#[derive(Component, Clone, Copy)]
pub(crate) struct SelectorLabel(WindowId);

/// Brings the window under a fresh click to the front: the frontmost of the
/// open floating windows the pointer is over.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn raise(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    frames: Query<(&WindowId, &Node, &UiGlobalTransform, &ComputedNode), With<super::Frame>>,
    mut stack: ResMut<Stack>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = windows
        .single()
        .ok()
        .filter(|window| window.focused)
        .and_then(Window::physical_cursor_position)
    else {
        return;
    };
    let under: Vec<WindowId> = frames
        .iter()
        .filter(|(id, node, transform, computed)| {
            id.describe().layer == Layer::Floating
                && node.display != Display::None
                && super::contains(cursor, transform, computed)
        })
        .map(|(id, ..)| *id)
        .collect();
    let front = stack.front_to_back().find(|id| under.contains(id));
    if let Some(front) = front {
        stack.raise(front);
    }
}

/// Draws each floating window at its place in the stack.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn restack(stack: Res<Stack>, mut frames: Query<(&WindowId, &mut GlobalZIndex)>) {
    for (id, mut z) in &mut frames {
        if id.describe().layer != Layer::Floating {
            continue;
        }
        let wanted = stack.z(*id);
        if z.0 != wanted {
            z.0 = wanted;
        }
    }
}

/// Opens and closes the windows the player toggles, from the selector or
/// their key, and closes one when Escape picks it; an opened window comes to
/// the front.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn toggle(
    keys: crate::keys::Keys,
    escape: Res<crate::escape::Escape>,
    buttons: Query<(&Interaction, &SelectorButton), Changed<Interaction>>,
    mut shown: ResMut<Shown>,
    mut stack: ResMut<Stack>,
) {
    if let crate::escape::Escape::Close(id) = *escape
        && id.describe().toggled
    {
        shown.close(id);
    }
    let pressed = WindowId::ALL.into_iter().filter(|id| {
        id.describe().toggled
            && (keys.pressed(crate::keys::Act::Toggle(*id))
                || buttons.iter().any(|(interaction, button)| {
                    *interaction == Interaction::Pressed && button.0 == *id
                }))
    });
    for id in pressed.collect::<Vec<_>>() {
        shown.toggle(id);
        if shown.is_open(id) {
            stack.raise(id);
        }
    }
}

/// Builds the selector: one button for each window the player opens and
/// closes, named as the official client names it, with its key.
pub(crate) fn spawn_selector(commands: &mut Commands) {
    let id = WindowId::Selector;
    let mut node = Node {
        column_gap: px(4),
        ..default()
    };
    id.describe().placement.apply(&mut node);
    commands
        .spawn((
            crate::hud::HudRoot,
            id,
            node,
            GlobalZIndex(id.describe().layer.base()),
        ))
        .with_children(|row| {
            for window in WindowId::ALL
                .into_iter()
                .filter(|window| window.describe().toggled)
            {
                row.spawn((
                    Button,
                    SelectorButton(window),
                    crate::tooltip::Tooltip(format!(
                        "Open or close the {}",
                        name(window).to_lowercase()
                    )),
                    Node {
                        padding: UiRect::axes(px(10), px(5)),
                        ..default()
                    },
                    BackgroundColor(theme::BUTTON),
                ))
                .with_child((
                    SelectorLabel(window),
                    theme::text(name(window), Size::Label, theme::INK_BRIGHT),
                ));
            }
        });
}

/// A window's name as the selector shows it: "Inventory" for "INVENTORY".
fn name(window: WindowId) -> String {
    let title = window.describe().title.to_lowercase();
    let mut letters = title.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// Lights the selector's buttons whose windows are open, and names each
/// window's key from the key map.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn light_selector(
    shown: Res<Shown>,
    map: Res<crate::keys::KeyMap>,
    mut buttons: Query<(&SelectorButton, &Interaction, &mut BackgroundColor)>,
    mut labels: Query<(&SelectorLabel, &mut Text)>,
) {
    for (SelectorLabel(window), mut text) in &mut labels {
        let wanted = map.named(crate::keys::Act::Toggle(*window), &name(*window));
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    for (button, interaction, mut color) in &mut buttons {
        let wanted = theme::button(true, shown.is_open(button.0), *interaction);
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raised_window_draws_in_front_of_the_rest() {
        let mut stack = Stack::default();
        stack.raise(WindowId::Inventory);
        assert!(stack.z(WindowId::Inventory) > stack.z(WindowId::Loot));
        assert_eq!(stack.front_to_back().next(), Some(WindowId::Inventory));
        stack.raise(WindowId::Loot);
        assert_eq!(
            stack.front_to_back().take(2).collect::<Vec<_>>(),
            [WindowId::Loot, WindowId::Inventory]
        );
    }

    #[test]
    fn keys_and_the_selector_open_and_close_the_toggled_windows() {
        let mut app = crate::testing::app();
        app.add_systems(Update, toggle);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyI);
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Inventory));
        assert_eq!(
            app.world().resource::<Stack>().front_to_back().next(),
            Some(WindowId::Inventory)
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        let button = app
            .world_mut()
            .spawn((SelectorButton(WindowId::Inventory), Interaction::Pressed))
            .id();
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Inventory));
        // Typing in chat never toggles a window.
        app.world_mut().entity_mut(button).despawn();
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyB);
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Spellbook));
        // Escape closes the window it picked.
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Spellbook);
        *app.world_mut().resource_mut::<crate::escape::Escape>() =
            crate::escape::Escape::Close(WindowId::Spellbook);
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Spellbook));
    }
}
