//! Which windows are open and which is in front: the floating windows stack
//! in the order the player last clicked or opened them, Escape closes the top
//! one, and the windows the player opens and closes do so from the selector
//! or their key, the same way for each.
use super::registry::{Layer, Toggle, WindowId};
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

/// The windows open now, whether the player or something else opened them,
/// and those the player hid.
#[derive(Resource)]
pub(crate) struct Shown {
    open: BTreeSet<WindowId>,
    /// Windows something else opens that the player hid ([`Toggle::Hides`]):
    /// hidden, open or not, until the player shows them again.
    hidden: BTreeSet<WindowId>,
}

impl Default for Shown {
    /// The effects window starts open, as in the official client, and the
    /// hotbar and spell gems are always open.
    fn default() -> Self {
        Self {
            open: [WindowId::Effects, WindowId::Spells, WindowId::Actions].into(),
            hidden: BTreeSet::new(),
        }
    }
}

impl Shown {
    pub(crate) fn is_open(&self, id: WindowId) -> bool {
        self.open.contains(&id)
    }

    /// Whether the window shows: open, and not hidden by the player.
    pub(crate) fn displayed(&self, id: WindowId) -> bool {
        self.is_open(id) && !self.hidden.contains(&id)
    }

    pub(crate) fn open(&mut self, id: WindowId) {
        self.open.insert(id);
    }

    pub(crate) fn close(&mut self, id: WindowId) {
        self.open.remove(&id);
    }

    /// Hides a window something else opens, or shows it again, as a new pet
    /// shows the pet window while the player wants it to pop up.
    pub(crate) fn hide(&mut self, id: WindowId, hidden: bool) {
        if hidden {
            self.hidden.insert(id);
        } else {
            self.hidden.remove(&id);
        }
    }

    /// The player closes a window, with its close box, its Done button or
    /// Escape: one something else opens is hidden, any other closed.
    pub(crate) fn dismiss(&mut self, id: WindowId) {
        if id.describe().toggle == Toggle::Hides {
            self.hidden.insert(id);
        } else {
            self.open.remove(&id);
        }
    }

    /// The open windows.
    pub(crate) fn ids(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.open.iter().copied()
    }

    /// The selector, a key or a slash command flips the window: one the
    /// player opens opens or closes, one something else opens is hidden or
    /// shown.
    fn toggle(&mut self, id: WindowId) {
        let windows = if id.describe().toggle == Toggle::Hides {
            &mut self.hidden
        } else {
            &mut self.open
        };
        if !windows.remove(&id) {
            windows.insert(id);
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

/// The window frames whose layout changed this frame.
type ChangedFrames<'w, 's> =
    Query<'w, 's, (&'static WindowId, &'static Node), (With<super::Frame>, Changed<Node>)>;

/// Brings a floating window to the front when it opens, whoever opened it:
/// the player, or the server with a merchant, a corpse or a give window. The
/// newest window is the one Escape closes first.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn raise_opened(
    frames: ChangedFrames,
    mut open: Local<BTreeSet<WindowId>>,
    mut stack: ResMut<Stack>,
) {
    for (id, node) in &frames {
        if id.describe().layer != Layer::Floating {
            continue;
        }
        if node.display == Display::None {
            open.remove(id);
        } else if open.insert(*id) {
            stack.raise(*id);
        }
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

/// Opens and closes the windows the player toggles, from the selector,
/// their key or a slash command, and closes one when Escape picks it; hides
/// and shows the ones something else opens the same way. A window shown
/// comes to the front.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn toggle(
    keys: crate::keys::Keys,
    (escape, mut chat): (Res<crate::escape::Escape>, ResMut<crate::chat::ChatState>),
    buttons: Query<(&Interaction, &SelectorButton), Changed<Interaction>>,
    (mut shown, mut stack): (ResMut<Shown>, ResMut<Stack>),
    online: Res<crate::online::OnlineState>,
) {
    if let crate::escape::Escape::Close(id) = *escape
        && id.describe().toggle == Toggle::Opens
    {
        shown.close(id);
    }
    let typed = chat.toggled.take();
    let pressed = WindowId::ALL.into_iter().filter(|id| {
        id.describe().toggle != Toggle::Never
            // A window the session does not offer stays shut.
            && id
                .needs()
                .is_none_or(|needs| crate::outbox::offered(online.world(), needs))
            && (keys.pressed(crate::keys::Act::Toggle(*id))
                || typed == Some(*id)
                || buttons.iter().any(|(interaction, button)| {
                    *interaction == Interaction::Pressed && button.0 == *id
                }))
    });
    for id in pressed.collect::<Vec<_>>() {
        shown.toggle(id);
        if shown.displayed(id) {
            stack.raise(id);
        }
    }
}

/// Hides the HUD windows the player hid, such as the hotbar, and shows them
/// again; a floating one has no frame while hidden (see
/// [`crate::skinned::frames`]).
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn hide(
    shown: Res<Shown>,
    mut frames: Query<(&WindowId, &mut Node), With<super::Frame>>,
) {
    for (id, mut node) in &mut frames {
        let description = id.describe();
        if description.toggle != Toggle::Hides || description.layer != Layer::Hud {
            continue;
        }
        let wanted = if shown.displayed(*id) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// Builds the selector: one button for each window the player opens and
/// closes, named as the official client names it, with its key. It is a
/// frame without chrome of its own, so the skin draws it where the skin is
/// installed.
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
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
            super::Frame::default(),
            GlobalZIndex(id.describe().layer.base()),
        ))
        .with_children(|row| {
            for window in WindowId::ALL
                .into_iter()
                .filter(|window| window.describe().toggle == Toggle::Opens)
            {
                let mut button = row.spawn((
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
                ));
                button.with_child((
                    SelectorLabel(window),
                    theme::text(name(window), Size::Label, theme::INK_BRIGHT),
                ));
                // Greyed where the session does not offer the window.
                if let Some(needs) = window.needs() {
                    button.insert(crate::outbox::Needs::Capability(needs));
                }
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
        let wanted = theme::button(true, shown.displayed(button.0), *interaction);
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
    fn a_window_comes_to_the_front_when_it_opens() {
        let mut app = crate::testing::app();
        app.add_systems(Update, raise_opened);
        let frame = |app: &mut App, id: WindowId| {
            app.world_mut()
                .spawn((
                    super::super::Frame::default(),
                    id,
                    Node {
                        display: Display::None,
                        ..default()
                    },
                ))
                .id()
        };
        let inventory = frame(&mut app, WindowId::Inventory);
        let merchant = frame(&mut app, WindowId::Merchant);
        app.update();
        let set = |app: &mut App, entity, node: Node| {
            *app.world_mut().get_mut::<Node>(entity).unwrap() = node;
        };
        let front = |app: &App| app.world().resource::<Stack>().front_to_back().next();
        let shown = Node {
            display: Display::Flex,
            ..default()
        };
        set(&mut app, inventory, shown.clone());
        app.update();
        assert_eq!(front(&app), Some(WindowId::Inventory));
        // The server opens a shop over the inventory.
        set(&mut app, merchant, shown.clone());
        app.update();
        assert_eq!(front(&app), Some(WindowId::Merchant));
        // Moving an open window leaves it where it is in the stack.
        set(
            &mut app,
            inventory,
            Node {
                left: px(5),
                ..shown
            },
        );
        app.update();
        assert_eq!(front(&app), Some(WindowId::Merchant));
    }

    #[test]
    fn the_map_opens_only_where_the_session_offers_it() {
        use eq_client_core::{Capability, WorldEvent};
        let mut app = crate::testing::app();
        app.add_systems(Update, toggle);
        let mut online = crate::online::OnlineState::new(true);
        // A session that offers everything but the map.
        crate::online::testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: Capability::ALL
                    .into_iter()
                    .filter(|capability| *capability != Capability::Map)
                    .collect(),
                choices: Vec::new(),
                session_id: 1,
                zone: "qeytoqrg".into(),
                player: Box::new(crate::online::testing::player(7)),
                far_clip: None,
            }],
        );
        app.insert_resource(online);
        app.world_mut()
            .spawn((SelectorButton(WindowId::Map), Interaction::Pressed));
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Map));
        // Offered, it opens.
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(7));
        app.insert_resource(online);
        for mut interaction in app
            .world_mut()
            .query::<&mut Interaction>()
            .iter_mut(app.world_mut())
        {
            *interaction = Interaction::Pressed;
        }
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Map));
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

    #[test]
    fn the_selector_hides_and_shows_a_window_something_else_opens() {
        let mut app = crate::testing::app();
        app.add_systems(Update, (toggle, hide).chain());
        let hotbar = app
            .world_mut()
            .spawn((
                WindowId::Actions,
                super::super::Frame::default(),
                Node::default(),
            ))
            .id();
        let press = |app: &mut App, window| {
            let button = app
                .world_mut()
                .spawn((SelectorButton(window), Interaction::Pressed))
                .id();
            app.update();
            app.world_mut().entity_mut(button).despawn();
        };
        let display = |app: &App| app.world().get::<Node>(hotbar).unwrap().display;
        app.update();
        assert_eq!(display(&app), Display::Flex);
        // Hidden, the hotbar stays open but its frame is gone from view.
        press(&mut app, WindowId::Actions);
        let shown = app.world().resource::<Shown>();
        assert!(shown.is_open(WindowId::Actions) && !shown.displayed(WindowId::Actions));
        assert_eq!(display(&app), Display::None);
        press(&mut app, WindowId::Actions);
        assert_eq!(display(&app), Display::Flex);
        // The pet window shows only while it is open and not hidden.
        press(&mut app, WindowId::PetInfo);
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::PetInfo);
        assert!(!app.world().resource::<Shown>().displayed(WindowId::PetInfo));
        press(&mut app, WindowId::PetInfo);
        assert!(app.world().resource::<Shown>().displayed(WindowId::PetInfo));
    }

    #[test]
    fn closing_a_window_something_else_opens_hides_it() {
        let mut shown = Shown::default();
        shown.open(WindowId::PetInfo);
        shown.dismiss(WindowId::PetInfo);
        assert!(shown.is_open(WindowId::PetInfo) && !shown.displayed(WindowId::PetInfo));
        // Its owner closing and opening it again leaves it hidden.
        shown.close(WindowId::PetInfo);
        shown.open(WindowId::PetInfo);
        assert!(!shown.displayed(WindowId::PetInfo));
        // Any other window is closed.
        shown.open(WindowId::Inventory);
        shown.dismiss(WindowId::Inventory);
        assert!(!shown.is_open(WindowId::Inventory));
    }
}
