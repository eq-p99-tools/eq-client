//! Which windows are open and which is in front: the floating windows stack
//! in the order the player last clicked or opened them, Escape closes the top
//! one, and the windows the player opens and closes do so from the selector
//! or their key, the same way for each.
use super::registry::{Layer, Toggle, WindowId};
use crate::theme;
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
                .filter(|id| id.describe().layer.stacked())
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

    /// A detached chat group stays open across HUD rebuilds. Its membership
    /// determines when it joins or leaves the stack.
    pub(crate) fn sync_chat_groups(&mut self, groups: &crate::chat::Groups) {
        self.0
            .retain(|id| !matches!(id, WindowId::ChatGroup(_)) || groups.contains(*id));
        for id in groups.detached() {
            if !self.0.contains(&id) {
                self.0.push(id);
            }
        }
    }

    /// The windows from the front to the back.
    pub(crate) fn front_to_back(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.0.iter().rev().copied()
    }

    /// Where a floating window draws: in front of every window below it in
    /// the stack.
    pub(crate) fn z(&self, id: WindowId) -> i32 {
        let popup = id.describe().layer == Layer::Popup;
        let place = self
            .0
            .iter()
            .copied()
            .filter(|other| (other.describe().layer == Layer::Popup) == popup)
            .position(|other| other == id)
            .unwrap_or(0);
        let base = if popup { Layer::Popup } else { Layer::Floating };
        base.base() + i32::try_from(place).unwrap_or(0)
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
    /// Windows the active character cannot use, independent of saved choices.
    unavailable: BTreeSet<WindowId>,
}

impl Default for Shown {
    /// The effects window starts open, as in the official client, and the
    /// hotbar and spell gems are always open.
    fn default() -> Self {
        Self {
            open: [WindowId::Effects, WindowId::Spells, WindowId::Actions].into(),
            hidden: BTreeSet::new(),
            unavailable: BTreeSet::new(),
        }
    }
}

impl Shown {
    pub(crate) fn is_open(&self, id: WindowId) -> bool {
        self.open.contains(&id)
    }

    /// Whether the window shows: open, and not hidden by the player.
    pub(crate) fn displayed(&self, id: WindowId) -> bool {
        self.is_open(id) && !self.hidden.contains(&id) && !self.unavailable.contains(&id)
    }

    /// Re-evaluates character eligibility without changing the player's
    /// saved show/hide choice. Returns whether the eligibility changed.
    pub(crate) fn available(&mut self, id: WindowId, available: bool) -> bool {
        if available {
            self.unavailable.remove(&id)
        } else {
            self.unavailable.insert(id)
        }
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
pub(super) struct SelectorLabel(pub(super) WindowId);

/// Brings the window under a fresh click to the front: the frontmost of the
/// open floating windows the pointer is over.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn raise(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    frames: Query<
        (
            &WindowId,
            &Node,
            &UiGlobalTransform,
            &ComputedNode,
            &GlobalZIndex,
        ),
        With<super::Frame>,
    >,
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
    let front = frames
        .iter()
        .filter(|(id, node, transform, computed, _)| {
            id.describe().layer.stacked()
                && node.display != Display::None
                && super::contains(cursor, transform, computed)
        })
        .max_by_key(|(_, _, _, _, z)| z.0)
        .map(|(id, ..)| *id);
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
    shown: Res<Shown>,
    mut open: Local<BTreeSet<WindowId>>,
    mut stack: ResMut<Stack>,
) {
    // A player-opened window can lose its frame during a HUD rebuild.
    // Forget the opening when its logical state closes, even without a frame.
    open.retain(|id| id.describe().toggle != Toggle::Opens || shown.displayed(*id));
    for (id, node) in &frames {
        let description = id.describe();
        // Inspectors raise when their contents are presented, including
        // updates to a panel that's already visible.
        if !description.layer.stacked()
            || description.layer == Layer::Popup
            || matches!(id, WindowId::ChatGroup(_))
        {
            continue;
        }
        let player_opened = description.toggle == Toggle::Opens;
        if node.display == Display::None {
            if !player_opened {
                open.remove(id);
            }
        } else if (!player_opened || shown.displayed(*id)) && open.insert(*id) {
            stack.raise(*id);
        }
    }
}

/// Draws each floating window at its place in the stack.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn restack(
    stack: Res<Stack>,
    front: Res<crate::login::FrontEnd>,
    online: Res<crate::online::OnlineState>,
    mut frames: Query<(&WindowId, &mut GlobalZIndex)>,
) {
    let in_world = front.screen(&online) == crate::login::Screen::World;
    for (id, mut z) in &mut frames {
        let layer = id.describe().layer;
        if !layer.stacked() {
            continue;
        }
        let wanted = stack.z(*id)
            + if layer == Layer::Tool && !in_world {
                Layer::Tool.base() - Layer::Floating.base()
            } else {
                0
            };
        if z.0 != wanted {
            z.0 = wanted;
        }
    }
}

/// Applies the order of popups raised after pointer routing, including frames
/// spawned during presentation.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn restack_popups(stack: Res<Stack>, mut frames: Query<(&WindowId, &mut GlobalZIndex)>) {
    for (id, mut z) in &mut frames {
        if id.describe().layer != Layer::Popup {
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
    buttons: crate::clicks::Clicks<SelectorButton>,
    (mut shown, mut stack): (ResMut<Shown>, ResMut<Stack>),
) {
    if let crate::escape::Escape::Close(id) = *escape
        && id.describe().toggle == Toggle::Opens
    {
        shown.close(id);
    }
    let typed = chat.toggled.take();
    let pressed = WindowId::ALL.into_iter().filter(|id| {
        id.describe().toggle != Toggle::Never
            && (keys.pressed(crate::keys::Act::Toggle(*id))
                || typed == Some(*id)
                || buttons.iter().any(|button| button.0 == *id))
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

/// A window's name as the selector shows it: "Inventory" for "INVENTORY".
pub(super) fn name(window: WindowId) -> String {
    if window == WindowId::ShortEffects {
        return "Short effects".into();
    }
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
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Inventory);
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
        // A close while the frame is absent still ends that opening.
        app.world_mut().despawn(inventory);
        app.world_mut()
            .resource_mut::<Shown>()
            .close(WindowId::Inventory);
        app.update();
        let inventory = frame(&mut app, WindowId::Inventory);
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Inventory);
        set(&mut app, inventory, Node::default());
        app.update();
        assert_eq!(front(&app), Some(WindowId::Inventory));
    }

    #[test]
    fn tools_share_the_world_stack_but_stay_above_the_front_end() {
        let mut app = crate::testing::app();
        app.add_systems(Update, (raise_opened, restack, restack_popups).chain());
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Scripts);
        let scripts = app
            .world_mut()
            .spawn((
                super::super::Frame::default(),
                WindowId::Scripts,
                Node::default(),
                GlobalZIndex(0),
            ))
            .id();
        app.update();
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Options);
        let options = app
            .world_mut()
            .spawn((
                super::super::Frame::default(),
                WindowId::Options,
                Node::default(),
                GlobalZIndex(0),
            ))
            .id();
        app.update();
        let z = |app: &App, entity| app.world().get::<GlobalZIndex>(entity).unwrap().0;
        assert!(z(&app, options) > z(&app, scripts));
        app.world_mut()
            .resource_mut::<Stack>()
            .raise(WindowId::Scripts);
        app.update();
        assert!(z(&app, scripts) > z(&app, options));
        assert!(z(&app, scripts) < Layer::Popup.base());
        // With an online run still at login, the tool clears the full-screen UI.
        app.insert_resource(crate::online::OnlineState::new(true));
        app.update();
        assert!(z(&app, scripts) > Layer::Screen.base());
        app.insert_resource(crate::online::OnlineState::new(false));
        app.update();
        assert!(z(&app, scripts) < Layer::Popup.base());
    }

    #[test]
    fn native_map_keys_toggle_and_control_m_keeps_its_native_reservation() {
        use eq_client_assets::ui::InstalledClient;
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let mut app = crate::testing::app();
            app.insert_resource(crate::keys::KeyMap::for_client(client));
            app.add_systems(Update, toggle);
            for (key, opened) in [(KeyCode::Backspace, true), (KeyCode::KeyM, false)] {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.reset_all();
                keys.press(key);
                app.update();
                assert_eq!(
                    app.world().resource::<Shown>().is_open(WindowId::Map),
                    opened
                );
            }
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::KeyM);
            app.update();
            assert!(!app.world().resource::<Shown>().is_open(WindowId::Map));
        }
    }

    #[test]
    fn the_map_opens_without_a_server_capability() {
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
                spell_landing_flag: None,
            }],
        );
        app.insert_resource(online);
        let button = app
            .world_mut()
            .spawn((SelectorButton(WindowId::Map), Interaction::None))
            .id();
        crate::clicks::testing::click(&mut app, button);
        assert!(app.world().resource::<Shown>().is_open(WindowId::Map));
        // A second click closes it, without changing server capabilities.
        crate::clicks::testing::click(&mut app, button);
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Map));
        // Offered, it opens just the same.
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(7));
        app.insert_resource(online);
        crate::clicks::testing::click(&mut app, button);
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
            .spawn((SelectorButton(WindowId::Inventory), Interaction::None))
            .id();
        crate::clicks::testing::click(&mut app, button);
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Inventory));
        // Typing in chat never toggles a window.
        app.world_mut().entity_mut(button).despawn();
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
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
                .spawn((SelectorButton(window), Interaction::None))
                .id();
            crate::clicks::testing::click(app, button);
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
    fn spell_gem_frames_follow_the_profile_without_overwriting_saved_choices() {
        for client in [
            eq_client_assets::ui::InstalledClient::Titanium,
            eq_client_assets::ui::InstalledClient::EqMac,
        ] {
            let mut app = crate::testing::app();
            app.world_mut()
                .resource_mut::<crate::ViewerSettings>()
                .0
                .installed_client = client;
            app.add_systems(Update, (crate::hud::spell_visibility, hide).chain());
            // Both kinds of HUD frame share the same window visibility owner.
            let frames: Vec<_> = (0..2)
                .map(|_| {
                    app.world_mut()
                        .spawn((
                            WindowId::Spells,
                            super::super::Frame::default(),
                            Node::default(),
                        ))
                        .id()
                })
                .collect();
            let enter = |app: &mut App, class| {
                let mut player = crate::online::testing::player(1);
                player.class = Some(class);
                player.level = 65;
                // Contradictory mana/gems on melee classes must not expose the UI.
                let caster = !matches!(class, 1 | 7 | 9 | 16);
                player.mana = if caster { 0 } else { 100 };
                player.memorized_spells = if caster { [None; 8] } else { [Some(42); 8] };
                crate::online::testing::admit(
                    &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
                    7,
                    player,
                );
                app.update();
            };
            let display = |app: &App, wanted| {
                for frame in &frames {
                    assert_eq!(app.world().get::<Node>(*frame).unwrap().display, wanted);
                }
            };
            for class in 1..=16 {
                enter(&mut app, class);
                display(
                    &app,
                    if matches!(class, 1 | 7 | 9 | 16) {
                        Display::None
                    } else {
                        Display::Flex
                    },
                );
            }
            app.world_mut()
                .resource_mut::<Shown>()
                .hide(WindowId::Spells, true);
            enter(&mut app, 1);
            enter(&mut app, 2);
            display(&app, Display::None);
            app.world_mut()
                .resource_mut::<Shown>()
                .hide(WindowId::Spells, false);
            app.update();
            display(&app, Display::Flex);
        }
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
