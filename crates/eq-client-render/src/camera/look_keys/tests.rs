use super::*;
use crate::camera::orbit_transform;

const FRAME: f32 = 1.0 / 60.0;

fn units(radians: f32) -> f32 {
    radians * UNITS_PER_QUARTER_TURN / FRAC_PI_2
}

#[test]
fn a_fresh_press_takes_the_view_and_restarts_the_ramp() {
    let mut hold = LookHold::default();
    hold.follow(true, false);
    // The first step turns at 20 units a second, the next at 40.
    assert!((units(hold.tilt(FRAME).unwrap()) - 20.0 * FRAME).abs() < 1e-4);
    assert!((units(hold.tilt(FRAME).unwrap()) - 40.0 * FRAME).abs() < 1e-4);
    // Down pressed while up is still held takes the view from its first step.
    hold.follow(true, true);
    assert!((units(hold.tilt(FRAME).unwrap()) + 20.0 * FRAME).abs() < 1e-4);
    // Letting down go leaves up held but holding nothing until pressed again.
    hold.follow(true, false);
    assert_eq!(hold.tilt(FRAME), None);
    hold.follow(false, false);
    hold.follow(true, false);
    assert!(hold.tilt(FRAME).unwrap() > 0.0);
    // The ramp tops out at 12 steps, 240 units a second, a fifth of a
    // second in.
    let first = units(hold.tilt(1.0).unwrap());
    assert!(first > 200.0 && first < 240.0, "{first}");
    assert!((units(hold.tilt(1.0).unwrap()) - 240.0).abs() < 1e-3);
}

fn app(view: View) -> App {
    let mut app = App::new();
    crate::keys::testing::install(&mut app);
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(FRAME));
    app.insert_resource(time)
        .init_resource::<crate::navigation::NavigationKeys>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<MouseLook>()
        .init_resource::<options::OptionsState>()
        .add_systems(Update, look_keys);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        bevy::window::PrimaryWindow,
    ));
    app.world_mut()
        .spawn((Player, Transform::from_rotation(Quat::from_rotation_y(0.5))));
    let orbit = OrbitCamera {
        focus: Vec3::ZERO,
        radius: 30.0,
        yaw: 0.0,
        pitch: -0.6,
        pulled: None,
        view,
    };
    app.world_mut().spawn((orbit_transform(&orbit), orbit));
    app
}

fn camera(app: &mut App) -> Mut<'_, OrbitCamera> {
    app.world_mut()
        .query::<&mut OrbitCamera>()
        .single_mut(app.world_mut())
        .unwrap()
}

fn hold(app: &mut App, key: Option<KeyCode>, frames: usize) {
    for _ in 0..frames {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.clear();
        match key {
            Some(key) => input.press(key),
            None => input.release_all(),
        }
        app.update();
    }
}

#[test]
fn the_look_keys_tilt_the_first_person_view_within_straight_up_and_down() {
    let mut app = app(View::FirstPerson { pitch: 0.0 });
    hold(&mut app, Some(KeyCode::PageUp), 1);
    assert!((units(camera(&mut app).view_pitch()) - 20.0 * FRAME).abs() < 1e-3);
    hold(&mut app, Some(KeyCode::PageUp), 120);
    assert!((camera(&mut app).view_pitch() - FRAC_PI_2).abs() < 1e-5);
    hold(&mut app, None, 1);
    hold(&mut app, Some(KeyCode::Numpad3), 240);
    assert!((camera(&mut app).view_pitch() + FRAC_PI_2).abs() < 1e-5);
    // Look Spring is off by default, so the view stays where it was put.
    hold(&mut app, None, 60);
    assert!((camera(&mut app).view_pitch() + FRAC_PI_2).abs() < 1e-5);
    // Center View levels it, and the key still held then holds nothing.
    hold(&mut app, Some(KeyCode::Home), 1);
    assert!(camera(&mut app).view_pitch().abs() < 1e-6);
    // A look key with Shift is another command.
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    input.press(KeyCode::ShiftLeft);
    input.press(KeyCode::PageUp);
    app.update();
    assert!(camera(&mut app).view_pitch().abs() < 1e-6);
}

#[test]
fn the_chase_view_ignores_the_tilt_and_center_view_brings_it_back_behind() {
    let mut app = app(View::Chase);
    hold(&mut app, Some(KeyCode::PageUp), 30);
    assert!((camera(&mut app).pitch + 0.6).abs() < 1e-6);
    hold(&mut app, None, 1);
    hold(&mut app, Some(KeyCode::Numpad5), 1);
    let orbit = camera(&mut app);
    assert!((orbit.pitch - CHASE_PITCH).abs() < 1e-6);
    assert!((orbit.yaw - super::super::behind(0.5)).abs() < 1e-5);
    assert!(matches!(orbit.view, View::Chase));
}

#[test]
fn look_spring_levels_the_first_person_view_unless_a_look_holds_it() {
    let mut app = app(View::FirstPerson { pitch: 0.5 });
    app.world_mut()
        .resource_mut::<options::OptionsState>()
        .options
        .look_spring = true;
    app.world_mut().resource_mut::<MouseLook>().held = true;
    hold(&mut app, None, 10);
    assert!((camera(&mut app).view_pitch() - 0.5).abs() < 1e-6);
    app.world_mut().resource_mut::<MouseLook>().held = false;
    // 10 units a frame.
    hold(&mut app, None, 1);
    assert!((units(0.5 - camera(&mut app).view_pitch()) - 10.0).abs() < 1e-3);
    hold(&mut app, None, 10);
    assert!(camera(&mut app).view_pitch().abs() < 1e-6);
    // A held look key keeps it tilted.
    hold(&mut app, Some(KeyCode::PageDown), 30);
    assert!(camera(&mut app).view_pitch() < -0.1);
}
