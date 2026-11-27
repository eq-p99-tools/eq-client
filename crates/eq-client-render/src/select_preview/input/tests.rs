use super::*;
use crate::{Player, PlayerBody, chat, inventory, items, online};
use bevy::math::DVec2;

fn app() -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<options::OptionsState>()
        .init_resource::<chat::ChatState>()
        .init_resource::<items::ItemState>()
        .init_resource::<inventory::InventoryState>()
        .init_resource::<windows::pointer::Wheel>()
        .insert_resource(Preview {
            wanted: Some((0, 7, 2)),
            shown: Some((0, 7, 2)),
            turning: true,
            ..default()
        });
    let mut online = online::OnlineState::new(true);
    online.selection = Some(crate::character_select::Selection::new(7, Vec::new()));
    app.insert_resource(online);
    camera::testing::install_look(&mut app);
    app.add_systems(Update, (orbit_input, camera::orbit_camera).chain());
    let mut window = Window {
        focused: true,
        ..default()
    };
    window.set_physical_cursor_position(Some(DVec2::splat(50.0)));
    let window = app.world_mut().spawn((window, PrimaryWindow)).id();
    let player = app
        .world_mut()
        .spawn((
            Player,
            PlayerBody {
                feet_offset: 3.0,
                height: 6.0,
                size: 6.0,
            },
            Transform::from_xyz(0.0, 3.0, 0.0),
        ))
        .id();
    let camera = app
        .world_mut()
        .spawn((
            OrbitCamera {
                focus: Vec3::new(0.0, 3.0, 0.0),
                radius: 25.0,
                yaw: 1.0,
                pitch: -0.2,
                pulled: None,
                view: default(),
                look: 0.0,
            },
            Transform::default(),
        ))
        .id();
    (app, camera, player, window)
}

fn drag(app: &mut App, delta: Vec2) {
    app.world_mut().write_message(MouseMotion { delta });
    app.update();
}

fn angles(app: &App, camera: Entity) -> (f32, f32) {
    let c = app.world().get::<OrbitCamera>(camera).unwrap();
    (c.yaw, c.pitch)
}

fn press(app: &mut App, down: bool) {
    camera::testing::button(app, MouseButton::Right, down);
    app.update();
}

#[test]
fn preview_drag_orbits_both_axes_and_renders_without_turning_the_character() {
    let (mut app, camera, player, _) = app();
    app.update();
    let body = *app.world().get::<Transform>(player).unwrap();
    press(&mut app, true);
    let turn = app
        .world()
        .resource::<options::OptionsState>()
        .options
        .turn_per_pixel(1280.0);
    // More than one full revolution is allowed; upward and downward pitch
    // remain independent of yaw and never switch into gameplay first person.
    drag(&mut app, Vec2::new(7.0 / turn, -0.8 / turn));
    let (yaw, pitch) = angles(&app, camera);
    assert!((yaw + 6.0).abs() < 1e-4);
    assert!((pitch - 0.6).abs() < 1e-4);
    let orbit = app.world().get::<OrbitCamera>(camera).unwrap();
    assert_eq!(orbit.radius, 25.0);
    assert_eq!(orbit.focus, body.translation);
    assert!(!orbit.is_first_person());
    let pose = app.world().get::<Transform>(camera).unwrap();
    let pivot = body.translation + Vec3::Y * 6.0 * camera::HEAD_LINE;
    assert!((pose.translation.distance(pivot) - 25.0).abs() < 1e-4);
    assert!(
        (pivot - pose.translation)
            .normalize()
            .abs_diff_eq(*pose.forward(), 1e-5)
    );
    assert_eq!(*app.world().get::<Transform>(player).unwrap(), body);
    assert_eq!(
        app.world()
            .resource::<online::OnlineState>()
            .selection
            .as_ref()
            .unwrap()
            .id(),
        7
    );
    assert!(!app.world().resource::<Preview>().turning);
    assert!(!app.world().resource::<camera::MouseLook>().held);
    press(&mut app, false);
    drag(&mut app, Vec2::splat(100.0));
    assert_eq!(angles(&app, camera), (yaw, pitch));
}

#[test]
fn preview_press_over_ui_never_acquires_but_capture_survives_crossing_ui() {
    let (mut app, camera, _, window) = app();
    let panel = app
        .world_mut()
        .spawn((
            Button,
            ComputedNode {
                size: Vec2::splat(100.0),
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::splat(50.0)),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    press(&mut app, true);
    drag(&mut app, Vec2::splat(100.0));
    assert_eq!(angles(&app, camera), (1.0, -0.2));
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(DVec2::splat(250.0)));
    drag(&mut app, Vec2::splat(100.0));
    assert_eq!(angles(&app, camera), (1.0, -0.2));
    press(&mut app, false);
    press(&mut app, true);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(DVec2::splat(50.0)));
    drag(&mut app, Vec2::splat(20.0));
    assert_ne!(angles(&app, camera), (1.0, -0.2));
    assert!(app.world().get_entity(panel).is_ok());
}

#[test]
fn preview_focus_cancel_and_selection_changes_require_a_fresh_press() {
    for boundary in 0..4 {
        let (mut app, camera, _, window) = app();
        press(&mut app, true);
        match boundary {
            0 => {
                app.world_mut().write_message(WindowFocused {
                    window,
                    focused: false,
                });
            }
            1 => {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Escape);
            }
            2 => {
                let mut p = app.world_mut().resource_mut::<Preview>();
                p.wanted = Some((0, 7, 3));
                p.shown = p.wanted;
            }
            _ => {
                app.world_mut().resource_mut::<Preview>().shown = None;
            }
        }
        drag(&mut app, Vec2::splat(20.0));
        assert_eq!(angles(&app, camera), (1.0, -0.2));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        drag(&mut app, Vec2::splat(20.0));
        assert_eq!(angles(&app, camera), (1.0, -0.2));
    }
}

#[test]
fn preview_pitch_uses_saved_inversion_and_stays_clear_of_the_poles() {
    for invert in [false, true] {
        let (mut app, camera, _, _) = app();
        app.world_mut()
            .resource_mut::<options::OptionsState>()
            .options
            .invert_y = invert;
        press(&mut app, true);
        drag(&mut app, Vec2::new(0.0, 1_000_000.0));
        assert_eq!(angles(&app, camera).1, if invert { 1.45 } else { -1.45 });
    }
}
