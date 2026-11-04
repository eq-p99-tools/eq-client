use super::*;
use crate::{Player, PlayerBody, camera, chat, inventory, items, online, options, windows};
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::window::PrimaryWindow;
use eq_client_assets::camera::TITANIUM;

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
    let first = camera.attached_view(TITANIUM, 0.0, 100.0, None, 2.4, 0.016);
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
    let looked = camera.attached_view(TITANIUM, -0.25, 0.0, None, 2.4, 0.016);
    assert!(looked.forward().y > 0.0);
    let expected = Quat::from_euler(EulerRot::YXZ, yaw + 0.4, 0.25, 0.0);
    assert!(looked.rotation.angle_between(expected) < 1e-5);
    assert!((camera.pitch - pitch).abs() < 1e-5);
    assert!((camera.radius - close).abs() < 1e-5);

    let outward = camera.attached_view(TITANIUM, 0.0, -1.0, None, 2.4, 0.016);
    let target = super::super::orbit_transform(&camera);
    assert!(
        outward.translation.distance(looked.translation)
            < target.translation.distance(looked.translation)
    );
    assert!(matches!(camera.view, View::Returning { .. }));
    for _ in 0..100 {
        camera.attached_view(TITANIUM, 0.0, 0.0, None, 2.4, 0.016);
    }
    assert!(matches!(camera.view, View::Chase));
    assert!((camera.yaw - yaw - 0.4).abs() < 1e-5);
    assert!(camera.radius > close);
}

#[test]
fn first_person_looks_up_and_down_without_degenerate_vertical_transforms() {
    for tilt in [-100.0, 100.0] {
        let mut camera = orbit();
        camera.attached_view(TITANIUM, 0.0, 100.0, None, 1.0, 0.016);
        let pose = camera.attached_view(TITANIUM, tilt, 0.0, None, 1.0, 0.016);
        assert!(pose.rotation.is_finite());
        assert!((pose.forward().y.abs() - 1.0).abs() < 1e-5);
        assert!(pose.right().is_finite());
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
        let first = camera.attached_view(TITANIUM, 0.0, 100.0, Some(&world), head, 0.016);
        assert!(first.translation.y < 4.0);
        assert!(first.translation.y >= camera.focus.y);
        for scroll in [-1.0, -100.0, 0.0] {
            for _ in 0..25 {
                let pose = camera.attached_view(TITANIUM, 0.0, scroll, Some(&world), head, 0.016);
                assert!(pose.translation.y < 4.0);
                assert!(pose.translation.z < 1.0);
            }
        }
    }
}

#[test]
fn collision_pull_in_is_not_a_first_person_request() {
    let mut camera = orbit();
    camera.pulled = Some(0.01);
    camera.attached_view(TITANIUM, 0.0, 0.0, None, 2.4, 0.0);
    assert!(matches!(camera.view, View::Chase));
    camera.reset_view();
    assert!(camera.pulled.is_none());
}

fn app() -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.016));
    app.insert_resource(time)
        .init_resource::<chat::ChatState>()
        .init_resource::<items::ItemState>()
        .init_resource::<inventory::InventoryState>()
        .init_resource::<options::OptionsState>()
        .init_resource::<camera::MouseLook>()
        .init_resource::<windows::pointer::Wheel>()
        .add_message::<MouseMotion>()
        .add_message::<MouseWheel>()
        .insert_resource(online::OnlineState::new(false))
        .add_systems(
            Update,
            (
                windows::pointer::wheel,
                camera::orbit_camera,
                camera::show_player,
            )
                .chain(),
        );
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

fn near(app: &App, camera: Entity) -> f32 {
    let Projection::Perspective(projection) = app.world().get::<Projection>(camera).unwrap() else {
        panic!("perspective camera")
    };
    projection.near
}

#[test]
fn production_wheel_hides_only_local_body_and_restores_projection_and_visibility() {
    let (mut app, camera, player, window) = app();
    let facing = *app.world().get::<Transform>(player).unwrap();
    let remote = app.world_mut().spawn(Visibility::Inherited).id();
    wheel(&mut app, window, 100.0);
    assert!(matches!(
        app.world().get::<OrbitCamera>(camera).unwrap().view,
        View::FirstPerson { .. }
    ));
    assert_eq!(
        *app.world().get::<Visibility>(player).unwrap(),
        Visibility::Hidden
    );
    assert_eq!(
        *app.world().get::<Visibility>(remote).unwrap(),
        Visibility::Inherited
    );
    assert!((near(&app, camera) - 0.1).abs() < 1e-5);
    assert_eq!(*app.world().get::<Transform>(player).unwrap(), facing);

    wheel(&mut app, window, -1.0);
    for _ in 0..100 {
        app.update();
    }
    assert_eq!(
        *app.world().get::<Visibility>(player).unwrap(),
        Visibility::Inherited
    );
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
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>();
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
