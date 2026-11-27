//! Dry-ground pursuit, native ownership and ordinary input-priority regressions.
use super::*;
use eq_client_core::{SpawnKind, WorldEvent, WorldPosition, qol::Fix};

mod background;

fn pursuit(known: bool, bridge: bool) -> Scene {
    pursuit_with_speed(known, bridge, 6.)
}

fn pursuit_with_speed(known: bool, bridge: bool, speed: f32) -> Scene {
    let mut scene = Scene::with_speed(0, known, speed);
    scene
        .app
        .add_message::<bevy::window::WindowFocused>()
        .add_systems(
            PreUpdate,
            crate::keys::release_on_focus_loss.after(bevy::input::InputSystems),
        )
        .init_resource::<crate::combat::CombatState>()
        .init_resource::<crate::hud::hotbar::Bindings>()
        .add_systems(Update, crate::combat::input.before(input));
    crate::keys::testing::bind(
        &mut scene.app,
        Act::Attack,
        crate::keys::Chord::key(KeyCode::F10),
    );
    if bridge {
        scene
            .app
            .world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .script_input_mode = crate::script::InputMode::Background;
        crate::script::control::Bridge::install_idle_test_receiver(&mut scene.app);
    }
    {
        let mut online = scene.app.world_mut().resource_mut::<online::OnlineState>();
        let mut target = crate::online::testing::pet(5, 0);
        target.kind = SpawnKind::Npc;
        target.pet_owner = None;
        target.race = 1;
        target.size = 6.;
        target.position = WorldPosition {
            y: 100.,
            z: 3.,
            ..WorldPosition::default()
        };
        crate::online::testing::spawn_entry(&mut online, 5, target);
        let request_id = online.world().next_target_request() - 1;
        crate::online::testing::news(
            &mut online,
            [WorldEvent::TargetChanged {
                session_id: 11,
                request_id,
                state: eq_network_game::targets::TargetState {
                    revision: 1,
                    spawn_id: Some(5),
                    available: true,
                },
            }],
        );
        assert!(online.world().target_intent().is_some());
    }
    scene
        .app
        .world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(Fix::MeleePositioning, true);
    // Let the ordinary owner start at rest before attack is requested.
    for _ in 0..30 {
        scene.step(false);
    }
    latency_tests::key(&mut scene.app, KeyCode::F10, true);
    scene.step(false);
    latency_tests::key(&mut scene.app, KeyCode::F10, false);
    scene.step(false);
    assert!(
        scene
            .app
            .world()
            .resource::<crate::combat::CombatState>()
            .requested_in(Some(11))
    );
    scene
}

fn observe(scene: &mut Scene, frames: usize) -> (u64, u64, f32) {
    let start = scene.owner.position();
    let before = scene.app.world().resource::<Controls>();
    let counts = (before.test_melee.decisions, before.test_melee.proposals);
    for _ in 0..frames {
        scene.step(false);
    }
    let controls = scene.app.world().resource::<Controls>();
    let end = scene.owner.position();
    let result = (
        controls.test_melee.decisions - counts.0,
        controls.test_melee.proposals - counts.1,
        (end.x - start.x).hypot(end.y - start.y),
    );
    println!(
        "pursuit6716 frames={frames} decisions={} proposals={} accepted_xy={} ground_active={} native_observations={} ordinary_requests={} refusals={}",
        result.0,
        result.1,
        result.2,
        controls.ground.active(),
        scene.observations,
        scene.ordinary_modes.len(),
        scene.refusals
    );
    assert_eq!(scene.refusals, 0);
    result
}

#[test]
fn pursuit_6716_known_dry_floor_reaches_the_assist() {
    let mut scene = pursuit(true, false);
    let (decisions, proposals, travel) = observe(&mut scene, 300);
    assert!(
        decisions > 0 && proposals > 0 && travel > 1.,
        "dry ground starved the assist"
    );
}

#[test]
fn pursuit_6716_known_dry_floor_with_idle_bridge_reaches_the_assist() {
    let mut scene = pursuit(true, true);
    assert!(
        !scene
            .app
            .world()
            .contains_resource::<crate::script::Script>()
    );
    let (decisions, proposals, travel) = observe(&mut scene, 300);
    assert!(
        decisions > 0 && proposals > 0 && travel > 1.,
        "dry ground starved the idle-bridge row"
    );
}

#[test]
fn pursuit_6716_unknown_floor_with_and_without_idle_bridge_controls() {
    for bridge in [false, true] {
        let mut scene = pursuit(false, bridge);
        let (decisions, proposals, travel) = observe(&mut scene, 300);
        assert!(decisions > 0 && proposals > 0 && travel > 1.);
        assert!(
            !scene
                .app
                .world()
                .contains_resource::<crate::script::Script>()
        );
    }
}

#[test]
fn pursuit_6716_real_script_and_manual_look_yield_then_release() {
    for known in [false, true] {
        for script in [false, true] {
            let mut scene = pursuit(known, true);
            if script {
                scene
                    .app
                    .insert_resource(crate::script::Script::new(Vec::new()).paused());
            } else {
                scene.app.insert_resource(crate::camera::MouseLook {
                    held: true,
                    left: false,
                });
            }
            let (decisions, proposals, _) = observe(&mut scene, 60);
            assert_eq!((decisions, proposals), (0, 0));
            scene
                .app
                .world_mut()
                .remove_resource::<crate::script::Script>();
            *scene
                .app
                .world_mut()
                .resource_mut::<crate::camera::MouseLook>() = crate::camera::MouseLook::default();
            let (decisions, proposals, travel) = observe(&mut scene, 90);
            assert!(decisions > 0 && proposals > 0 && travel > 1.);
        }
    }
}

#[test]
fn pursuit_6716_manual_movement_yields_and_focus_loss_keeps_pursuing() {
    for known in [false, true] {
        let mut scene = pursuit(known, true);
        let before = scene
            .app
            .world()
            .resource::<Controls>()
            .test_melee
            .proposals;
        for _ in 0..60 {
            scene.step(true);
        }
        assert_eq!(
            scene
                .app
                .world()
                .resource::<Controls>()
                .test_melee
                .proposals,
            before
        );
        let (_, proposals, travel) = observe(&mut scene, 90);
        assert!(proposals > 0 && travel > 1.);
        let world = scene.app.world_mut();
        world
            .query::<&mut Window>()
            .single_mut(world)
            .unwrap()
            .focused = false;
        let (_, proposals, travel) = observe(&mut scene, 60);
        assert!(proposals > 0 && travel > 1.);
        let world = scene.app.world_mut();
        world
            .query::<&mut Window>()
            .single_mut(world)
            .unwrap()
            .focused = true;
        let (_, proposals, travel) = observe(&mut scene, 90);
        assert!(proposals > 0 && travel > 1.);
    }
}

#[test]
fn pursuit_6716_manual_native_ground_keeps_its_owner_until_release() {
    let mut scene = pursuit(true, true);
    scene
        .app
        .world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(Fix::MeleePositioning, false);
    observe(&mut scene, 60);
    for _ in 0..30 {
        scene.step(true);
    }
    let observations = scene.observations;
    assert!(observations > 0 && scene.app.world().resource::<Controls>().ground.active());
    scene
        .app
        .world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(Fix::MeleePositioning, true);
    let before = scene
        .app
        .world()
        .resource::<Controls>()
        .test_melee
        .proposals;
    for _ in 0..60 {
        scene.step(true);
    }
    assert!(scene.observations > observations);
    assert_eq!(
        scene
            .app
            .world()
            .resource::<Controls>()
            .test_melee
            .proposals,
        before
    );
    let (_, proposals, travel) = observe(&mut scene, 90);
    assert!(proposals > 0 && travel > 1.);
}
