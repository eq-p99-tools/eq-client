//! Background pursuit uses the ordinary admission and packet witnesses.
use super::*;

fn focus(scene: &mut Scene, focused: bool) {
    let world = scene.app.world_mut();
    let (entity, mut window) = world
        .query_filtered::<(Entity, &mut Window), With<bevy::window::PrimaryWindow>>()
        .single_mut(world)
        .unwrap();
    window.focused = focused;
    world.write_message(bevy::window::WindowFocused {
        window: entity,
        focused,
    });
}

fn target_at(scene: &mut Scene, y: f32) {
    crate::online::testing::news(
        &mut scene.app.world_mut().resource_mut::<online::OnlineState>(),
        [WorldEvent::Position {
            spawn_id: 5,
            position: WorldPosition {
                y,
                z: 3.,
                ..WorldPosition::default()
            },
            velocity: [0.; 3],
            forward: 0.,
        }],
    );
}

#[test]
fn pursuit_6716_background_moving_target_catches_inside_nominal_server_reach() {
    const RUN: f32 = 29.866_667;
    for bridge in [false, true] {
        let mut scene = pursuit_with_speed(true, bridge, RUN);
        focus(&mut scene, false);
        let mut target = 35.;
        let mut minimum = f32::MAX;
        for frame in 0..900 {
            // Faster than ordinary walking, slower than the current run grant.
            target += 20. / 60.;
            target_at(&mut scene, target);
            scene.step(false);
            let distance = target - scene.owner.position().y;
            if frame >= 600 {
                assert!(
                    distance > 0. && distance < 12.,
                    "catch-up escaped reach: {distance}"
                );
                minimum = minimum.min(distance);
            }
        }
        let controls = scene.app.world().resource::<Controls>();
        assert!(controls.test_melee.decisions > 0 && controls.test_melee.proposals > 0);
        assert!(scene.owner.position().y > 250. && minimum < 10.);
        assert_eq!(scene.refusals, 0);
        // Scene commits proposals through two independent, calibrated
        // MotionSessions and verifies actual transport position bytes.
        assert!(!scene.submissions.is_empty());
        println!(
            "background bridge={bridge} minimum_separation={minimum} final_separation={}",
            target - scene.owner.position().y
        );
    }
}

#[test]
fn pursuit_6716_focus_loss_releases_native_holds_and_reclaim_needs_a_new_press() {
    let mut scene = pursuit(true, false);
    latency_tests::key(&mut scene.app, KeyCode::ArrowUp, true);
    scene
        .app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    *scene
        .app
        .world_mut()
        .resource_mut::<crate::camera::MouseLook>() = crate::camera::MouseLook {
        held: true,
        left: false,
    };
    focus(&mut scene, false);
    scene.step(false);
    assert!(
        !scene
            .app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::ArrowUp)
    );
    assert!(
        !scene
            .app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Right)
    );
    assert!(
        !scene
            .app
            .world()
            .resource::<crate::camera::MouseLook>()
            .held
    );
    let (_, proposals, travel) = observe(&mut scene, 90);
    assert!(proposals > 0 && travel > 1.);
    focus(&mut scene, true);
    let (_, proposals, travel) = observe(&mut scene, 90);
    assert!(proposals > 0 && travel > 1.);
    let before = scene
        .app
        .world()
        .resource::<Controls>()
        .test_melee
        .proposals;
    for _ in 0..30 {
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
}

#[test]
fn pursuit_6716_stationary_old_edge_moves_inward_then_stays_without_backoff() {
    let mut scene = pursuit_with_speed(true, false, 29.866_667);
    let target = scene.owner.position().y + 11.5;
    target_at(&mut scene, target);
    focus(&mut scene, false);
    let mut previous = scene.owner.position().y;
    for _ in 0..300 {
        scene.step(false);
        let next = scene.owner.position().y;
        assert!(next + 0.001 >= previous, "stationary target caused backoff");
        previous = next;
    }
    let separation = target - previous;
    assert!(
        separation > 0. && separation < 10.,
        "old edge did not close inward: {separation}"
    );
    let (_, _, travel) = observe(&mut scene, 300);
    assert!(
        travel < 0.001,
        "stationary target caused oscillation: {travel}"
    );
}

#[test]
fn pursuit_6716_background_typing_question_cast_stun_and_script_gates_remain() {
    for reason in 0..5 {
        let mut scene = pursuit(true, true);
        focus(&mut scene, false);
        match reason {
            0 => {
                scene
                    .app
                    .world_mut()
                    .resource_mut::<crate::keys::Typing>()
                    .composing = true
            }
            1 => {
                let mut asked = crate::confirm::Asked::default();
                assert!(asked.ask(crate::confirm::Question::BugReport));
                scene.app.insert_resource(asked);
            }
            2 => crate::online::testing::news(
                &mut scene.app.world_mut().resource_mut::<online::OnlineState>(),
                [WorldEvent::CastPending {
                    session_id: 11,
                    spell_id: Some(42),
                    gem: None,
                }],
            ),
            3 => crate::online::testing::news(
                &mut scene.app.world_mut().resource_mut::<online::OnlineState>(),
                [WorldEvent::OwnStun {
                    session_id: 11,
                    revision: 1,
                    state: eq_client_core::control::StunState::Stunned,
                }],
            ),
            _ => {
                scene
                    .app
                    .insert_resource(crate::script::Script::new(Vec::new()).paused());
            }
        }
        let (decisions, proposals, travel) = observe(&mut scene, 90);
        assert_eq!(
            (decisions, proposals),
            (0, 0),
            "background bypassed gate {reason}"
        );
        assert!(
            travel < 0.001,
            "background leaked travel past gate {reason}: {travel}"
        );
    }
}
