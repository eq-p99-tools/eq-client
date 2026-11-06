use super::*;
use crate::{Player, PlayerBody, camera, chat, inventory, items, online, options, windows};
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::window::PrimaryWindow;
use eq_client_assets::camera::{ChaseLeash, TITANIUM};

fn head_only(head: f32) -> CameraBody {
    CameraBody { head, feet: None }
}

fn orbit() -> OrbitCamera {
    OrbitCamera {
        focus: Vec3::new(0.0, 3.0, 0.0),
        radius: 30.0,
        yaw: 0.7,
        pitch: -0.3,
        pulled: None,
        view: default(),
    }
}

#[test]
fn inward_zoom_enters_first_person_and_outward_zoom_keeps_the_view_heading() {
    let mut camera = orbit();
    let yaw = camera.yaw;
    let body = head_only(2.4);
    let first = camera.attached_view(TITANIUM, 0.0, 100.0, None, body, 0.016);
    assert!(matches!(camera.view, View::FirstPerson { .. }));
    assert!(
        first
            .translation
            .abs_diff_eq(camera.focus + Vec3::Y * 2.4, 1e-5)
    );
    assert!(first.rotation.angle_between(Quat::from_rotation_y(yaw)) < 1e-5);
    let pitch = camera.pitch;
    let close = camera.radius;

    // View rotation belongs to the camera; neither a character transform nor
    // a gameplay target is needed by the mode transition.
    camera.yaw += 0.4;
    let looked = camera.attached_view(TITANIUM, -0.25, 0.0, None, body, 0.016);
    assert!(looked.forward().y > 0.0);
    let expected = Quat::from_euler(EulerRot::YXZ, yaw + 0.4, 0.25, 0.0);
    assert!(looked.rotation.angle_between(expected) < 1e-5);
    assert!((camera.pitch - pitch).abs() < 1e-5);
    assert!((camera.radius - close).abs() < 1e-5);

    let outward = camera.attached_view(TITANIUM, 0.0, -1.0, None, body, 0.016);
    assert!(matches!(camera.view, View::Chase));
    let to_eye = first.translation - outward.translation;
    assert!(to_eye.normalize().abs_diff_eq(*outward.forward(), 1e-5));
    assert!((to_eye.length() - camera.radius).abs() < 1e-5);
    assert!((camera.yaw - yaw - 0.4).abs() < 1e-5);
    assert!(camera.radius > close);
}

#[test]
fn chase_pitch_stops_short_of_both_poles_on_either_leash() {
    for leash in [ChaseLeash::Program, ChaseLeash::Long] {
        let mut camera = orbit();
        camera.radius = 100.0;
        let body = head_only(2.4);
        let eye = camera.focus + Vec3::Y * body.head;
        let heading = Quat::from_rotation_y(camera.yaw) * Vec3::NEG_Z;
        for (tilt, sign) in [(1_000.0, -1.0), (-1_000.0, 1.0)] {
            let pose = camera.attached_view(TITANIUM.on_leash(leash), tilt, 0.0, None, body, 0.016);
            assert!(matches!(camera.view, View::Chase));
            let forward = *pose.forward();
            let horizontal = Vec3::new(forward.x, 0.0, forward.z);
            assert!((forward.y - sign * 89.0_f32.to_radians().sin()).abs() < 1e-6);
            assert!(horizontal.normalize().abs_diff_eq(heading, 1e-4));
            let toward_eye = eye - pose.translation;
            assert!(toward_eye.normalize().abs_diff_eq(forward, 1e-5));
            assert!((toward_eye.length() - 100.0).abs() < 1e-3);
        }
    }
}

#[test]
fn first_person_eye_and_return_do_not_cross_low_ceilings_or_walls() {
    let world = CollisionWorld::new([
        [[-20.0, 4.0, -20.0], [20.0, 4.0, -20.0], [20.0, 4.0, 20.0]],
        [[-20.0, 4.0, -20.0], [20.0, 4.0, 20.0], [-20.0, 4.0, 20.0]],
        [[-20.0, -20.0, 1.0], [20.0, -20.0, 1.0], [20.0, 20.0, 1.0]],
        [[-20.0, -20.0, 1.0], [20.0, 20.0, 1.0], [-20.0, 20.0, 1.0]],
    ])
    .unwrap();
    for head in [0.6, 2.4] {
        let mut camera = orbit();
        camera.yaw = 0.0;
        let body = head_only(head);
        let first = camera.attached_view(TITANIUM, 0.0, 100.0, Some(&world), body, 0.016);
        assert!(first.translation.y < 4.0);
        assert!(first.translation.y >= camera.focus.y);
        for scroll in [-1.0, -100.0, 0.0] {
            for _ in 0..25 {
                let pose = camera.attached_view(TITANIUM, 0.0, scroll, Some(&world), body, 0.016);
                assert!(pose.translation.y < 4.0);
                assert!(pose.translation.z < 1.0);
                let to_eye = first.translation - pose.translation;
                assert!(to_eye.cross(*pose.forward()).length() < 1e-5);
            }
        }
    }
}

#[test]
fn collision_pull_in_is_not_a_first_person_request() {
    let mut camera = orbit();
    camera.pulled = Some(0.01);
    camera.attached_view(TITANIUM, 0.0, 0.0, None, head_only(2.4), 0.0);
    assert!(matches!(camera.view, View::Chase));
    camera.reset_view();
    assert!(camera.pulled.is_none());
}

fn app() -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.016));
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::camera::visibility::VisibilityPlugin,
    ))
    .init_resource::<Assets<Mesh>>()
    .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
    .insert_resource(time)
    .init_resource::<chat::ChatState>()
    .init_resource::<items::ItemState>()
    .init_resource::<inventory::InventoryState>()
    .init_resource::<options::OptionsState>()
    .init_resource::<windows::pointer::Wheel>()
    .add_message::<MouseWheel>()
    .insert_resource(online::OnlineState::new(false))
    .add_systems(
        Update,
        (
            camera::turn,
            windows::pointer::wheel,
            camera::orbit_camera,
            camera::show_player,
        )
            .chain(),
    );
    camera::testing::install_look(&mut app);
    let mut window = Window {
        focused: true,
        ..default()
    };
    window.set_physical_cursor_position(Some(bevy::math::DVec2::new(50.0, 50.0)));
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
            Transform::from_xyz(0.0, 3.0, 0.0).with_rotation(Quat::from_rotation_y(1.3)),
            Visibility::Inherited,
        ))
        .id();
    let camera = app
        .world_mut()
        .spawn((
            orbit(),
            Transform::default(),
            Projection::Perspective(PerspectiveProjection::default()),
        ))
        .id();
    (app, camera, player, window)
}

fn wheel(app: &mut App, window: Entity, lines: f32) {
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        phase: TouchPhase::Moved,
        x: 0.0,
        y: lines,
        window,
    });
    app.update();
}

fn start_look(app: &mut App) {
    camera::testing::button(app, MouseButton::Right, true);
}

fn drag(app: &mut App, delta: Vec2) {
    app.world_mut().write_message(MouseMotion { delta });
    app.update();
}

fn near(app: &App, camera: Entity) -> f32 {
    let Projection::Perspective(projection) = app.world().get::<Projection>(camera).unwrap() else {
        panic!("perspective camera")
    };
    projection.near
}

#[test]
fn ordinary_wheel_steps_snap_in_early_and_need_more_than_one_step_to_reenter() {
    for pixels in [-500.0, 0.0, 500.0] {
        let (mut app, camera, _, window) = app();
        app.world_mut()
            .get_mut::<OrbitCamera>(camera)
            .unwrap()
            .radius = 5.0;
        start_look(&mut app);
        drag(&mut app, Vec2::new(0.0, pixels));

        // At the poles the horizontal offset can be tiny. Entering first
        // person still takes the same wheel input as at the ordinary angle.
        for (lines, first_person) in [
            (1.0, false),
            (1.0, true),
            (1.0, true),
            (-1.0, false),
            (1.0, false),
            (-1.0, false),
            (1.0, false),
            (1.0, true),
        ] {
            wheel(&mut app, window, lines);
            assert_eq!(
                matches!(
                    app.world().get::<OrbitCamera>(camera).unwrap().view,
                    View::FirstPerson { .. }
                ),
                first_person,
                "after drag {pixels}, then wheel turn {lines}"
            );
        }
    }
}

#[test]
fn wheel_moves_toward_the_first_person_eye_without_rotating_chase() {
    for pixels in [-90.0, -60.0, 0.0, 100.0, 500.0] {
        let (mut app, camera, _, window) = app();
        app.world_mut()
            .get_mut::<OrbitCamera>(camera)
            .unwrap()
            .radius = 12.0;
        start_look(&mut app);
        drag(&mut app, Vec2::new(0.0, pixels));
        let initial = *app.world().get::<Transform>(camera).unwrap();
        let forward = *initial.forward();
        let mut previous = initial.translation;

        for lines in [1.0, 1.0, -1.0, -2.0] {
            wheel(&mut app, window, lines);
            let pose = app.world().get::<Transform>(camera).unwrap();
            let movement = pose.translation - previous;
            assert!(movement.dot(forward) * lines > 0.0);
            assert!(movement.cross(forward).length() < 1e-4);
            assert!(pose.forward().abs_diff_eq(forward, 1e-5));
            previous = pose.translation;
        }

        wheel(&mut app, window, 100.0);
        assert!(matches!(
            app.world().get::<OrbitCamera>(camera).unwrap().view,
            View::FirstPerson { .. }
        ));
        let eye = app.world().get::<Transform>(camera).unwrap().translation;
        let to_eye = eye - initial.translation;
        assert!(to_eye.dot(forward) > 0.0);
        assert!(to_eye.cross(forward).length() < 1e-4);
    }
}

#[test]
fn ordinary_look_reaches_both_vertical_directions_after_snapping_in() {
    for direction in [-1.0, 1.0] {
        let (mut app, camera, _, window) = app();
        app.world_mut()
            .get_mut::<OrbitCamera>(camera)
            .unwrap()
            .radius = 4.2;
        wheel(&mut app, window, 1.0);
        assert!(matches!(
            app.world().get::<OrbitCamera>(camera).unwrap().view,
            View::FirstPerson { .. }
        ));

        start_look(&mut app);
        for _ in 0..4 {
            drag(&mut app, Vec2::new(15.0, -100.0 * direction));
        }
        let pose = app.world().get::<Transform>(camera).unwrap();
        assert!(pose.rotation.is_finite());
        assert!((pose.forward().y - direction).abs() < 1e-5);
        assert!(pose.right().is_finite());
    }
}

#[test]
fn chase_looks_nearly_vertical_both_ways_with_normal_and_inverted_mouse_input() {
    for (invert_y, pixels) in [(false, -180.0), (true, 180.0)] {
        let (mut app, camera, player, _) = app();
        app.world_mut()
            .resource_mut::<options::OptionsState>()
            .options
            .invert_y = invert_y;
        start_look(&mut app);
        for _ in 0..4 {
            drag(&mut app, Vec2::new(15.0, pixels));
        }

        let view = app.world().get::<OrbitCamera>(camera).unwrap();
        let pose = app.world().get::<Transform>(camera).unwrap();
        assert!(matches!(view.view, View::Chase));
        let forward = pose.forward();
        let pitch = forward.y.atan2(forward.x.hypot(forward.z)).to_degrees();
        assert!((88.9..89.1).contains(&pitch), "{pitch}");
        assert!(pose.rotation.is_finite());
        assert!(pose.right().is_finite());
        assert!(pose.translation.y > 0.0);
        assert!(pose.translation.y < view.focus.y);
        assert_eq!(
            app.world().get::<Visibility>(player),
            Some(&Visibility::Inherited)
        );

        for _ in 0..4 {
            drag(&mut app, Vec2::new(0.0, -pixels));
        }
        let pose = app.world().get::<Transform>(camera).unwrap();
        let forward = pose.forward();
        let pitch = forward.y.atan2(forward.x.hypot(forward.z)).to_degrees();
        assert!((-89.1..-88.9).contains(&pitch), "{pitch}");
        assert!(pose.rotation.is_finite());
        assert!(pose.translation.y > 30.0);
        let view = app.world().get::<OrbitCamera>(camera).unwrap();
        assert!(matches!(view.view, View::Chase));
    }
}

#[test]
fn upward_chase_shrinks_the_view_at_a_wall_and_raised_ground() {
    let (mut app, camera, _, window) = app();
    app.world_mut().get_mut::<OrbitCamera>(camera).unwrap().yaw = 0.0;
    let world = CollisionWorld::new([
        [[-10.0, 0.0, 2.0], [10.0, 0.0, 2.0], [10.0, 20.0, 2.0]],
        [[-10.0, 0.0, 2.0], [10.0, 20.0, 2.0], [-10.0, 20.0, 2.0]],
        [[-10.0, 1.0, 0.02], [10.0, 1.0, 0.02], [10.0, 1.0, 10.0]],
        [[-10.0, 1.0, 0.02], [10.0, 1.0, 10.0], [-10.0, 1.0, 10.0]],
        [[0.025, 0.0, -1.0], [0.025, 0.0, 1.0], [0.025, 4.25, 1.0]],
        [[0.025, 0.0, -1.0], [0.025, 4.25, 1.0], [0.025, 4.25, -1.0]],
    ])
    .unwrap();
    app.insert_resource(crate::Collision(Some(world)));
    start_look(&mut app);
    drag(&mut app, Vec2::new(0.0, -200.0));
    let at_wall = *app.world().get::<Transform>(camera).unwrap();
    assert!(at_wall.forward().y > 0.35);
    assert!((0.0..2.0).contains(&at_wall.translation.z));

    for _ in 0..3 {
        drag(&mut app, Vec2::new(0.0, -200.0));
    }
    for _ in 0..60 {
        app.update();
    }
    let at_ground = *app.world().get::<Transform>(camera).unwrap();
    assert!(at_ground.forward().y > 0.999);
    assert!(at_ground.translation.y > 1.0);
    assert!((near(&app, camera) - 0.1).abs() < 1e-5);

    // Wheel zoom changes the requested orbit even while the same ground
    // limits the displayed camera. It does not toggle first person.
    let radius = app.world().get::<OrbitCamera>(camera).unwrap().radius;
    wheel(&mut app, window, -1.0);
    let view = app.world().get::<OrbitCamera>(camera).unwrap();
    assert!(view.radius > radius);
    assert!(matches!(view.view, View::Chase));
    let zoomed = app.world().get::<Transform>(camera).unwrap();
    assert!(zoomed.translation.y > 1.0);
    assert!(zoomed.forward().abs_diff_eq(*at_ground.forward(), 1e-5));

    // Turn the near-vertical orbit toward the low wall beside the avatar.
    drag(&mut app, Vec2::new(-314.0, 0.0));
    let turned = app.world().get::<Transform>(camera).unwrap();
    assert!(turned.forward().y > 0.999);
    assert!((0.0..0.025).contains(&turned.translation.x));
    assert!(turned.right().is_finite());
}

#[test]
fn downward_chase_zoom_stays_below_the_ceiling_and_moves_in_when_clear() {
    let (mut app, camera, _, window) = app();
    app.world_mut()
        .get_mut::<OrbitCamera>(camera)
        .unwrap()
        .radius = 8.0;
    let roof = CollisionWorld::new([
        [
            [-10.0, 10.5, -10.0],
            [10.0, 10.5, -10.0],
            [10.0, 10.5, 10.0],
        ],
        [
            [-10.0, 10.5, -10.0],
            [10.0, 10.5, 10.0],
            [-10.0, 10.5, 10.0],
        ],
    ])
    .unwrap();
    app.insert_resource(crate::Collision(Some(roof)));
    start_look(&mut app);
    drag(&mut app, Vec2::new(0.0, 500.0));
    let under_roof = *app.world().get::<Transform>(camera).unwrap();
    assert!(under_roof.forward().y < -0.999);
    assert!(under_roof.translation.y < 10.5);

    wheel(&mut app, window, -1.0);
    let zoomed_out = app.world().get::<Transform>(camera).unwrap();
    assert!(
        zoomed_out
            .translation
            .abs_diff_eq(under_roof.translation, 1e-5)
    );
    wheel(&mut app, window, 6.0);
    let zoomed_in = app.world().get::<Transform>(camera).unwrap();
    assert!(zoomed_in.translation.y < under_roof.translation.y);
    assert!(zoomed_in.forward().abs_diff_eq(*under_roof.forward(), 1e-5));
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::Chase
    ));
}

#[test]
fn head_centered_chase_pulls_in_when_a_low_wall_blocks_the_body() {
    let (mut app, camera, _, window) = app();
    let mut orbit = app.world_mut().get_mut::<OrbitCamera>(camera).unwrap();
    orbit.yaw = 0.0;
    orbit.pitch = 0.0;
    orbit.radius = 8.0;
    let wall = CollisionWorld::new([
        [[-10.0, 0.0, 1.0], [10.0, 0.0, 1.0], [10.0, 4.5, 1.0]],
        [[-10.0, 0.0, 1.0], [10.0, 4.5, 1.0], [-10.0, 4.5, 1.0]],
    ])
    .unwrap();
    app.insert_resource(crate::Collision(Some(wall)));
    wheel(&mut app, window, 1.0);
    let pose = app.world().get::<Transform>(camera).unwrap();
    assert!(pose.translation.y > 4.5);
    assert!((0.0..1.0).contains(&pose.translation.z));
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::Chase
    ));
}

#[test]
fn zoom_out_restores_the_existing_body_and_gear_on_the_first_frame() {
    let (mut app, camera, player, window) = app();
    let facing = *app.world().get::<Transform>(player).unwrap();
    let body = app
        .world_mut()
        .spawn((Visibility::Inherited, ChildOf(player)))
        .id();
    let gear = app
        .world_mut()
        .spawn((Visibility::Inherited, ChildOf(body)))
        .id();
    let hidden_gear = app
        .world_mut()
        .spawn((Visibility::Hidden, ChildOf(body)))
        .id();
    let remote = app.world_mut().spawn(Visibility::Inherited).id();
    let visible = |app: &App, entity| {
        app.world()
            .get::<InheritedVisibility>(entity)
            .unwrap()
            .get()
    };
    app.update();
    assert!(visible(&app, body) && visible(&app, gear));

    wheel(&mut app, window, 100.0);
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::FirstPerson { .. }
    ));
    assert_eq!(
        *app.world().get::<Visibility>(player).unwrap(),
        Visibility::Hidden
    );
    assert!(!visible(&app, body) && !visible(&app, gear));
    assert!(visible(&app, remote));
    assert!((near(&app, camera) - 0.1).abs() < 1e-5);
    assert_eq!(*app.world().get::<Transform>(player).unwrap(), facing);

    wheel(&mut app, window, -1.0);
    assert_eq!(
        *app.world().get::<Visibility>(player).unwrap(),
        Visibility::Inherited
    );
    assert!(visible(&app, body) && visible(&app, gear));
    assert!(!visible(&app, hidden_gear));
    assert!(visible(&app, remote));
    assert!((near(&app, camera) - 1.0).abs() < 1e-5);
    assert_eq!(*app.world().get::<Transform>(player).unwrap(), facing);
}

#[test]
fn ui_owned_wheel_and_disabled_or_unfocused_zoom_do_not_change_modes() {
    let (mut app, camera, _, window) = app();
    let surface = app
        .world_mut()
        .spawn((
            windows::pointer::TakesWheel,
            ComputedNode {
                size: Vec2::splat(200.0),
                ..default()
            },
            UiGlobalTransform::default(),
            bevy::ui::ComputedStackIndex(1),
        ))
        .id();
    wheel(&mut app, window, 100.0);
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::Chase
    ));
    app.world_mut().despawn(surface);
    app.update(); // A consumed wheel input must not replay.
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::Chase
    ));
    app.world_mut()
        .resource_mut::<options::OptionsState>()
        .options
        .wheel_zoom = false;
    wheel(&mut app, window, 100.0);
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::Chase
    ));
    app.world_mut()
        .resource_mut::<options::OptionsState>()
        .options
        .wheel_zoom = true;
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    wheel(&mut app, window, 100.0);
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::Chase
    ));
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    wheel(&mut app, window, 100.0);
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::FirstPerson { .. }
    ));
    app.world_mut()
        .get_mut::<OrbitCamera>(camera)
        .unwrap()
        .reset_view();
    app.update();
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::Chase
    ));
}

#[test]
fn detached_and_reset_views_show_the_body_again() {
    let (mut app, camera, player, window) = app();
    wheel(&mut app, window, 100.0);
    app.world_mut()
        .entity_mut(camera)
        .insert(camera::FreeCamera {
            yaw: 0.0,
            pitch: 0.0,
            admission: None,
        });
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(player).unwrap(),
        Visibility::Inherited
    );
    assert!((near(&app, camera) - 1.0).abs() < 1e-5);
    app.world_mut()
        .entity_mut(camera)
        .remove::<camera::FreeCamera>();
    app.world_mut()
        .get_mut::<OrbitCamera>(camera)
        .unwrap()
        .reset_view();
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(player).unwrap(),
        Visibility::Inherited
    );
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::Chase
    ));
}

#[test]
fn zone_camp_and_death_resets_clear_first_person_and_show_the_local_model() {
    use bevy::ecs::system::RunSystemOnce;
    use eq_client_core::world::Reset;

    let (mut app, camera, player, window) = app();
    app.init_resource::<Assets<Image>>()
        .init_resource::<Assets<StandardMaterial>>();
    for reason in [
        Reset::Entered,
        Reset::Camped,
        Reset::Died,
        Reset::Zoning { to_bind: false },
    ] {
        wheel(&mut app, window, 100.0);
        assert_eq!(
            *app.world().get::<Visibility>(player).unwrap(),
            Visibility::Hidden
        );
        app.world_mut()
            .run_system_once(
                move |mut scene: crate::zone::Scene, mut commands: Commands| {
                    scene.forget(reason, &mut commands);
                },
            )
            .unwrap();
        app.update();
        assert!(matches!(
            app.world().get::<OrbitCamera>(camera).unwrap().view,
            View::Chase
        ));
        assert_eq!(
            *app.world().get::<Visibility>(player).unwrap(),
            Visibility::Inherited
        );
    }
}
