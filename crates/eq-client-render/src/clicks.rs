//! What counts as a click: the one place the windows' controls learn they
//! were clicked. As in the official client, a button acts when the left
//! button is let go over it after a press that began on it; letting go
//! anywhere else, or outside the client's window, cancels the click (Adam
//! 11-01, from the live client), and moving off and back on before letting
//! go still clicks. Push buttons, check boxes, tabs, close boxes and links
//! read their clicks here through [`Clicks`].
//!
//! Two kinds of control act differently, each saying so where it is built:
//! one a hold picks a hotkey up from ([`crate::hud::hotbar::Pickable`])
//! follows the hold rule of [`crate::hud::hotbar::carry`], and one marked
//! [`OnPress`] acts as the press begins. Controls that take hold of
//! something as the press begins (a text box, a title bar, a scroll arrow,
//! a slider, a list row that selects, a drop-down, an item or coin place)
//! read the press itself, not a click. That those act at the press in the
//! official client too is inferred, not checked (recording checklist
//! 6.325); so is that the Mac-era client's controls follow the same rules as
//! the Titanium client's (6.326).
//!
//! A screen drawn again while the button is held replaces its controls, so
//! that press clicks nothing and the player clicks again.
use bevy::{ecs::system::SystemParam, prelude::*};

/// A control that acts as the left button's press begins on it, not when it
/// is let go: one that picks something up onto the cursor or puts it down
/// there, such as an item's place.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct OnPress;

/// The left button's presses on the controls, frame by frame ([`track`]).
#[derive(Resource, Default, Debug)]
pub(crate) struct LeftButton {
    /// The controls the press under way began on.
    pressed_on: Vec<Entity>,
    /// The controls the button was let go over this frame, the press having
    /// begun on them: this frame's clicks.
    let_go_on: Vec<Entity>,
}

impl LeftButton {
    /// Whether this control was clicked this frame by the release rule.
    pub(crate) fn clicked(&self, control: Entity) -> bool {
        self.let_go_on.contains(&control)
    }

    /// Leaves a press to another rule, which decides what it does: the hold
    /// rule takes a press on a control a hold picks up from.
    pub(crate) fn claim(&mut self, control: Entity) {
        self.pressed_on.retain(|pressed| *pressed != control);
        self.let_go_on.retain(|released| *released != control);
    }

    /// Ends the press under way without a click, as when it put a hotkey down
    /// from the cursor and so does nothing else.
    pub(crate) fn cancel(&mut self) {
        self.pressed_on.clear();
    }
}

/// Follows the left button over the controls. Bevy marks each control the
/// press lands on pressed, keeps it so while the button is held, even off
/// it, and as the button is let go marks the control under the pointer
/// hovered; one the pointer is off, or that the pointer left the window
/// from, it marks neither. So a control the press began on is clicked when
/// it is let go while it shows anything but no interaction.
///
/// It runs after Bevy's focus and after a script's pointer, which presses and
/// lets go as the player does.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn track(
    mouse: Res<ButtonInput<MouseButton>>,
    controls: Query<(Entity, Ref<Interaction>)>,
    mut button: ResMut<LeftButton>,
) {
    button.let_go_on.clear();
    if mouse.just_pressed(MouseButton::Left) {
        button.pressed_on = controls
            .iter()
            .filter(|(_, interaction)| {
                interaction.is_changed() && **interaction == Interaction::Pressed
            })
            .map(|(control, _)| control)
            .collect();
    }
    if mouse.just_released(MouseButton::Left) {
        let pressed = std::mem::take(&mut button.pressed_on);
        button.let_go_on = pressed
            .into_iter()
            .filter(|control| {
                controls
                    .get(*control)
                    .is_ok_and(|(_, interaction)| *interaction != Interaction::None)
            })
            .collect();
    }
}

/// A control of one kind, with what decides when it clicks.
type Control<'a, T> = (
    Entity,
    Ref<'a, Interaction>,
    &'a T,
    Has<crate::hud::hotbar::Pickable>,
    Has<OnPress>,
);

/// The controls of one kind clicked this frame: one a hold picks up from as
/// the hold rule decides, one marked [`OnPress`] as its press begins, and
/// any other as the button is let go over it ([`track`]).
#[derive(SystemParam)]
pub(crate) struct Clicks<'w, 's, T: Component> {
    button: Option<Res<'w, LeftButton>>,
    hold: Option<Res<'w, crate::hud::hotbar::carry::Presses>>,
    controls: Query<'w, 's, Control<'static, T>>,
}

impl<T: Component> Clicks<'_, '_, T> {
    /// The controls clicked this frame.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.controls
            .iter()
            .filter(move |(control, interaction, _, pickable, on_press)| {
                let pressed = || interaction.is_changed() && **interaction == Interaction::Pressed;
                let let_go = || {
                    self.button
                        .as_ref()
                        .is_some_and(|button| button.clicked(*control))
                };
                match self.hold.as_deref() {
                    Some(hold) if *pickable => hold.clicked(*control) || (!*on_press && let_go()),
                    _ if *on_press => pressed(),
                    _ => let_go(),
                }
            })
            .map(|(_, _, control, ..)| control)
    }

    /// Whether any control of this kind was clicked this frame.
    pub(crate) fn any(&self) -> bool {
        self.iter().next().is_some()
    }
}

/// Clicks for tests, pressed and let go as Bevy marks them.
#[cfg(test)]
pub(crate) mod testing {
    use bevy::prelude::*;

    /// Gives a hand-built test app the click rule.
    pub(crate) fn install(app: &mut App) {
        app.init_resource::<super::LeftButton>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(PreUpdate, super::track);
    }

    /// Presses the left button on a control, or on nothing, as Bevy marks
    /// it, and runs a frame.
    pub(crate) fn press(app: &mut App, on: Option<Entity>) {
        if let Some(on) = on {
            *app.world_mut().get_mut::<Interaction>(on).unwrap() = Interaction::Pressed;
        }
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.press(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    }

    /// Lets the left button go over a control, or over nothing, which Bevy
    /// then marks hovered, and runs a frame.
    pub(crate) fn release(app: &mut App, over: Option<Entity>) {
        let world = app.world_mut();
        let mut interactions = world.query::<&mut Interaction>();
        for mut interaction in interactions.iter_mut(world) {
            interaction.set_if_neq(Interaction::None);
        }
        // A control drawn again since the press is gone, and the pointer
        // rests on its replacement, which the press never began on.
        if let Some(mut interaction) = over.and_then(|over| world.get_mut::<Interaction>(over)) {
            *interaction = Interaction::Hovered;
        }
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    }

    /// Clicks a control as the player does: the press on it, then the
    /// release over it, each its own frame.
    pub(crate) fn click(app: &mut App, control: Entity) {
        press(app, Some(control));
        release(app, Some(control));
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{click, install, press, release};
    use super::*;

    /// A control the tests click.
    #[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
    struct Control(u8);

    /// The controls clicked so far, in order.
    #[derive(Resource, Default)]
    struct Clicked(Vec<u8>);

    #[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
    fn record(clicks: Clicks<Control>, mut clicked: ResMut<Clicked>) {
        clicked.0.extend(clicks.iter().map(|control| control.0));
    }

    fn app() -> App {
        let mut app = App::new();
        install(&mut app);
        app.init_resource::<Clicked>().add_systems(Update, record);
        app
    }

    fn control(app: &mut App, number: u8) -> Entity {
        app.world_mut()
            .spawn((Control(number), Interaction::None))
            .id()
    }

    fn clicked(app: &App) -> &[u8] {
        &app.world().resource::<Clicked>().0
    }

    #[test]
    fn a_button_clicks_once_when_let_go_over_it_not_at_the_press() {
        let mut app = app();
        let button = control(&mut app, 1);
        press(&mut app, Some(button));
        assert_eq!(clicked(&app), [] as [u8; 0]);
        release(&mut app, Some(button));
        assert_eq!(clicked(&app), [1]);
        app.update();
        assert_eq!(clicked(&app), [1]);
    }

    #[test]
    fn letting_go_off_the_button_or_outside_the_window_clicks_nothing() {
        let mut app = app();
        let button = control(&mut app, 1);
        let other = control(&mut app, 2);
        press(&mut app, Some(button));
        release(&mut app, Some(other));
        press(&mut app, Some(button));
        release(&mut app, None);
        assert_eq!(clicked(&app), [] as [u8; 0]);
    }

    #[test]
    fn moving_off_and_back_on_before_letting_go_still_clicks() {
        let mut app = app();
        let button = control(&mut app, 1);
        press(&mut app, Some(button));
        // Off it, Bevy keeps it pressed; back over it, it is let go there.
        app.update();
        release(&mut app, Some(button));
        assert_eq!(clicked(&app), [1]);
    }

    #[test]
    fn a_press_and_release_in_one_frame_clicks() {
        let mut app = app();
        let button = control(&mut app, 1);
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.press(MouseButton::Left);
        mouse.release(MouseButton::Left);
        app.update();
        assert_eq!(clicked(&app), [1]);
    }

    #[test]
    fn a_control_that_acts_on_the_press_acts_once_as_it_begins() {
        let mut app = app();
        let place = control(&mut app, 3);
        app.world_mut().entity_mut(place).insert(OnPress);
        press(&mut app, Some(place));
        assert_eq!(clicked(&app), [3]);
        release(&mut app, Some(place));
        assert_eq!(clicked(&app), [3]);
    }

    #[test]
    fn a_cancelled_press_clicks_nothing_when_let_go() {
        let mut app = app();
        let button = control(&mut app, 1);
        press(&mut app, Some(button));
        app.world_mut().resource_mut::<LeftButton>().cancel();
        release(&mut app, Some(button));
        assert_eq!(clicked(&app), [] as [u8; 0]);
        click(&mut app, button);
        assert_eq!(clicked(&app), [1]);
    }
}
