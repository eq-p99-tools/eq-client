//! Matched native/authored input, accepted path, and local lock lifecycle.
use super::*;
use eq_client_core::{WorldEvent, WorldPosition, qol::Fix};

fn scene(locked: bool, delay: u32) -> Scene {
    let mut scene = camera_locked::scene(locked, delay);
    for (act, code) in [
        (Act::StrafeRight, KeyCode::F8),
        (Act::TurnRight, KeyCode::F7),
    ] {
        crate::keys::testing::bind(&mut scene.app, act, crate::keys::Chord::key(code));
    }
    scene
}

fn hold(scene: &mut Scene, code: KeyCode, authored: bool) {
    if authored {
        scene
            .app
            .insert_resource(crate::script::Script::new(Vec::new()).latched_for_test(vec![code]));
        crate::script::Script::press_keys_for_test(&mut scene.app, &[code]);
    } else {
        latency_tests::key(&mut scene.app, code, true);
    }
}

fn status(scene: &Scene) -> &'static str {
    scene.app.world().resource::<Controls>().melee_status().0
}

fn measured_path(locked: bool, code: KeyCode, authored: bool, delay: u32) -> f32 {
    let mut scene = scene(locked, delay);
    let mut previous = scene.owner.position();
    let mut path = 0.;
    let mut elapsed = Duration::ZERO;
    let mut frame = 0;
    hold(&mut scene, code, authored);
    while elapsed < Duration::from_secs(10) {
        let seconds = Duration::from_millis([8, 12, 16, 33][frame % 4])
            .min(Duration::from_secs(10).saturating_sub(elapsed));
        scene.step_for(false, seconds);
        elapsed += seconds;
        frame += 1;
        let next = scene.owner.position();
        path += (next.x - previous.x).hypot(next.y - previous.y);
        previous = next;
        if locked {
            let radius = next.x.hypot(next.y - 10.);
            assert!(
                (radius - 10.).abs() < 0.03,
                "orbit radius drifted: {radius}"
            );
            assert!(
                scene
                    .app
                    .world()
                    .resource::<Controls>()
                    .key_facing
                    .as_ref()
                    .is_some_and(|facing| matches!(
                        facing.source,
                        Some(key_turn::Source::Melee { .. })
                    ))
            );
        }
    }
    assert_eq!(scene.refusals, 0);
    assert!(!scene.submissions.is_empty());
    println!(
        "orbit6719 locked={locked} key={code:?} authored={authored} delay={delay} seconds=10 accepted_path={path}"
    );
    path
}

#[test]
fn melee_orbit_ten_second_accepted_arc_matches_authored_and_native_and_bounds_turn_by_strafe() {
    for delay in [0, 6] {
        let manual = measured_path(false, KeyCode::F8, false, delay);
        let native = measured_path(true, KeyCode::F8, false, delay);
        let authored = measured_path(true, KeyCode::F8, true, delay);
        let turn = measured_path(true, KeyCode::F7, false, delay);
        let authored_turn = measured_path(true, KeyCode::F7, true, delay);
        assert!(native > 20. && native <= manual + 0.02);
        assert!(turn > 20. && turn <= native + 0.02);
        assert!((native - authored).abs() < 0.002);
        assert!((turn - authored_turn).abs() < 0.002);
    }
}

#[test]
fn melee_orbit_idle_native_ground_released_intent_and_explicit_rearm_are_distinct() {
    let mut scene = scene(true, 0);
    let started = scene.now();
    for _ in 0..60 {
        scene.step(false);
    }
    let controls = scene.app.world().resource::<Controls>();
    assert!(
        controls
            .ground
            .current
            .is_some_and(|frame| frame.at > started),
        "idle lock {:?}, accepted {:?}, observations {}, guided {:?}",
        controls.melee.status(),
        scene.owner.position(),
        scene.observations,
        controls.key_facing.as_ref().map(|f| f.heading)
    );
    assert_eq!(status(&scene), "locked");
    assert_eq!(controls.melee_facing_owner(), "melee");
    assert!(!controls.melee_proposed());
    scene.step(true);
    assert_eq!(status(&scene), "released");
    let before = scene
        .app
        .world()
        .resource::<Controls>()
        .test_melee
        .proposals;
    for _ in 0..90 {
        scene.step(false);
    }
    assert_eq!(status(&scene), "released");
    assert_eq!(
        scene
            .app
            .world()
            .resource::<Controls>()
            .test_melee
            .proposals,
        before
    );
    scene.attack_on();
    assert!(matches!(status(&scene), "approaching" | "locked"));
    scene
        .app
        .world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(Fix::MeleePositioning, false);
    scene.step(false);
    assert_eq!(status(&scene), "disabled");
    scene
        .app
        .world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(Fix::MeleePositioning, true);
    scene.step(false);
    assert!(matches!(status(&scene), "approaching" | "locked"));
}

#[test]
fn melee_orbit_held_turn_crosses_acquisition_and_forward_break_without_two_facing_owners() {
    for authored in [false, true] {
        for rmb in [false, true] {
            let mut scene = scene(false, 0);
            hold(&mut scene, KeyCode::F7, authored);
            for _ in 0..20 {
                scene.step(false);
            }
            assert!(
                scene
                    .app
                    .world()
                    .resource::<Controls>()
                    .key_facing
                    .is_some()
            );
            scene
                .app
                .world_mut()
                .resource_mut::<crate::options::OptionsState>()
                .options
                .qol
                .set(Fix::MeleePositioning, true);
            for _ in 0..60 {
                scene.step(false);
            }
            assert_eq!(status(&scene), "locked");
            assert!(
                scene
                    .app
                    .world()
                    .resource::<Controls>()
                    .key_facing
                    .as_ref()
                    .is_some_and(|facing| matches!(
                        facing.source,
                        Some(key_turn::Source::Melee { .. })
                    ))
            );
            if rmb {
                crate::camera::testing::button(&mut scene.app, MouseButton::Right, true);
                for _ in 0..30 {
                    scene.step(false);
                    assert_eq!(status(&scene), "locked");
                }
            }
            scene.step(true);
            assert_eq!(status(&scene), "released");
            if rmb {
                crate::camera::testing::button(&mut scene.app, MouseButton::Right, false);
            }
            let mut previous = scene.owner.position().heading;
            let mut turned = 0.;
            for _ in 0..60 {
                scene.step(false);
                let heading = scene.owner.position().heading;
                let change = arc(previous, heading);
                assert!(
                    change.abs() <= TURN_RATE * 0.25 + 0.01,
                    "replayed held turn: {change}"
                );
                turned += change.abs();
                previous = heading;
            }
            assert!(turned > 10., "held turn was lost across deliberate release");
            assert!(
                scene
                    .app
                    .world()
                    .resource::<Controls>()
                    .key_facing
                    .is_some()
            );
        }
    }
}

fn rates(scene: &mut Scene, run: f32, lateral: f32, walking: bool) {
    let at = scene.now();
    let root = scene
        .app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .generation()
        .root();
    {
        let mut controls = scene.app.world_mut().resource_mut::<Controls>();
        let mut grant = controls.speeds();
        grant.units_per_second = Some(run);
        grant.walk_units_per_second = Some(run * 0.5);
        grant.strafe_units_per_second = Some(lateral);
        grant.strafe_walk_units_per_second = Some(lateral * 0.5);
        controls.grant(grant, root);
        if let Some((arrived, _)) = controls.straddle.as_mut() {
            *arrived = at;
        }
        controls.walking = walking;
    }
    for owner in [&mut scene.owner, &mut scene.independent_owner] {
        if run == 0. {
            owner.root(at).unwrap();
        } else {
            owner
                .calibrate(
                    eq_client_core::MotionCalibration {
                        units_per_second: run,
                        velocity_scale: 1.,
                        animation: 12,
                        strafe: Some(eq_network_game::movement::StrafeCalibration {
                            units_per_second: lateral,
                            velocity_scale: 1.,
                            animation: 8,
                            walk_units_per_second: Some(lateral * 0.5),
                        }),
                        walk: Some(eq_network_game::movement::WalkCalibration {
                            units_per_second: run * 0.5,
                            velocity_scale: 1.,
                            animation: 4,
                        }),
                        backward: None,
                        diagonal: DiagonalRates {
                            run: Some(lateral),
                            walk: Some(lateral * 0.5),
                        },
                        swim: None,
                        swim_backward_units_per_second: None,
                    },
                    at,
                )
                .unwrap();
        }
    }
}

#[test]
fn melee_orbit_current_snare_root_increase_and_walk_grants_bound_every_accepted_sample() {
    for authored in [false, true] {
        let mut scene = scene(true, 0);
        hold(&mut scene, KeyCode::F8, authored);
        let mut previous = scene.owner.position();
        let mut previous_at = scene.owner.sampled();
        for (run, lateral, walking) in [
            (6., 3., false),
            (3., 1.5, false),
            (0., 0., false),
            (6., 3., false),
            (6., 3., true),
        ] {
            rates(&mut scene, run, lateral, walking);
            let allowance = if walking { lateral * 0.5 } else { lateral };
            let mut travel = 0.;
            for _ in 0..120 {
                scene.step(false);
                let next = scene.owner.position();
                let at = scene.owner.sampled();
                let distance = (next.x - previous.x).hypot(next.y - previous.y);
                let seconds = at.saturating_duration_since(previous_at).as_secs_f32();
                assert!(
                    distance <= allowance * seconds + 0.002,
                    "orbit exceeded current grant: {distance} > {allowance} * {seconds}"
                );
                travel += distance;
                previous = next;
                previous_at = at;
            }
            if run == 0. {
                assert!(travel < 0.002, "root accrued travel");
            }
            assert!(travel <= allowance * 2. + 0.002);
        }
        assert_eq!(scene.refusals, 0);
    }
}

#[test]
fn melee_orbit_moving_target_recovers_radially_within_one_ordinary_component() {
    let mut scene = scene(true, 0);
    hold(&mut scene, KeyCode::F8, false);
    let mut previous = scene.owner.position();
    let mut previous_at = scene.owner.sampled();
    for frame in 0..600 {
        let y = 10. + f32::from(u16::try_from(frame).unwrap()) / 60.;
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
        scene.step(false);
        let next = scene.owner.position();
        let at = scene.owner.sampled();
        let displacement = (next.x - previous.x).hypot(next.y - previous.y);
        let seconds = at.saturating_duration_since(previous_at).as_secs_f32();
        // Recovery and tangent are time-shared, never two full-rate vectors.
        let mode = scene.ordinary_modes.last().copied();
        let cap = if matches!(mode, Some(MovementMode::Strafe(_))) {
            3.
        } else {
            6.
        };
        assert!(displacement <= cap * seconds + 0.002);
        assert!(next.x.hypot(next.y - y) < 12.);
        previous = next;
        previous_at = at;
    }
    assert_eq!(scene.refusals, 0);
}

#[test]
fn melee_orbit_rmb_alone_retains_lock_but_two_button_forward_and_autorun_release_it() {
    let mut scene = scene(true, 0);
    crate::camera::testing::button(&mut scene.app, MouseButton::Right, true);
    scene.step(false);
    assert_eq!(status(&scene), "locked");
    crate::camera::testing::button(&mut scene.app, MouseButton::Left, true);
    scene.step(false);
    assert_eq!(status(&scene), "released");
    crate::camera::testing::button(&mut scene.app, MouseButton::Right, false);
    crate::camera::testing::button(&mut scene.app, MouseButton::Left, false);
    for _ in 0..60 {
        scene.step(false);
    }
    assert_eq!(status(&scene), "released");
    scene.attack_on();
    crate::keys::testing::bind(
        &mut scene.app,
        Act::Autorun,
        crate::keys::Chord::key(KeyCode::F6),
    );
    latency_tests::key(&mut scene.app, KeyCode::F6, true);
    scene.step(false);
    assert_eq!(status(&scene), "released");
    assert!(scene.app.world().resource::<Controls>().autorun());
}

#[test]
fn melee_orbit_typing_casting_and_question_suspensions_do_not_bank_held_strafe() {
    for reason in 0..3 {
        let mut scene = scene(true, 0);
        hold(&mut scene, KeyCode::F8, false);
        for _ in 0..30 {
            scene.step(false);
        }
        match reason {
            0 => {
                scene
                    .app
                    .world_mut()
                    .resource_mut::<crate::keys::Typing>()
                    .composing = true;
            }
            1 => crate::online::testing::news(
                &mut scene.app.world_mut().resource_mut::<online::OnlineState>(),
                [WorldEvent::CastPending {
                    session_id: 11,
                    spell_id: Some(42),
                    gem: None,
                }],
            ),
            _ => {
                let mut asked = crate::confirm::Asked::default();
                assert!(asked.ask(crate::confirm::Question::BugReport));
                scene.app.insert_resource(asked);
            }
        }
        let before = scene
            .app
            .world()
            .resource::<Controls>()
            .test_melee
            .proposals;
        for _ in 0..120 {
            scene.step(false);
        }
        assert_eq!(status(&scene), "suspended");
        assert_eq!(
            scene
                .app
                .world()
                .resource::<Controls>()
                .test_melee
                .proposals,
            before
        );
        match reason {
            0 => {
                scene
                    .app
                    .world_mut()
                    .resource_mut::<crate::keys::Typing>()
                    .composing = false;
            }
            1 => crate::online::testing::news(
                &mut scene.app.world_mut().resource_mut::<online::OnlineState>(),
                [WorldEvent::CastCleared {
                    session_id: 11,
                    spell_id: 42,
                }],
            ),
            _ => {
                scene
                    .app
                    .world_mut()
                    .remove_resource::<crate::confirm::Asked>();
            }
        }
        let mut previous = scene.owner.position();
        let mut previous_at = scene.owner.sampled();
        let mut path = 0.;
        for _ in 0..120 {
            scene.step(false);
            let next = scene.owner.position();
            let at = scene.owner.sampled();
            let distance = (next.x - previous.x).hypot(next.y - previous.y);
            assert!(
                distance <= 3. * at.saturating_duration_since(previous_at).as_secs_f32() + 0.002,
                "distance {distance} over {:?}, from {previous:?} to {next:?}",
                at.saturating_duration_since(previous_at)
            );
            path += distance;
            previous = next;
            previous_at = at;
        }
        assert!(path > 1. && path <= 6.01);
        assert_eq!(status(&scene), "locked");
    }
}

#[test]
fn melee_orbit_jump_suspends_through_supported_landing_without_deliberate_release() {
    let mut scene = scene(true, 0);
    let fall_speed = Some(physics(InstalledClient::Titanium, None).terminal_speed);
    scene.owner = scene.owner.with_falls(true).with_fall_speed(fall_speed);
    scene.independent_owner = scene
        .independent_owner
        .with_falls(true)
        .with_fall_speed(fall_speed);
    crate::keys::testing::bind(
        &mut scene.app,
        Act::Jump,
        crate::keys::Chord::key(KeyCode::F6),
    );
    scene.app.world_mut().resource_mut::<Controls>().airborne = Some(AirborneController::default());
    latency_tests::key(&mut scene.app, KeyCode::F6, true);
    scene.step(false);
    assert_eq!(status(&scene), "suspended");
    latency_tests::key(&mut scene.app, KeyCode::F6, false);
    let mut rose = false;
    for _ in 0..300 {
        scene.step(false);
        rose |= scene.owner.position().z > 3.01;
        assert_ne!(status(&scene), "released");
    }
    assert!(rose, "ordinary jump did not leave its supported floor");
    assert_eq!(status(&scene), "locked");
    assert!((scene.owner.position().z - 3.).abs() < 0.02);
}

#[test]
fn melee_orbit_opposing_keys_and_released_taps_cannot_invent_a_held_interval() {
    let mut scene = scene(true, 0);
    crate::keys::testing::bind(
        &mut scene.app,
        Act::StrafeLeft,
        crate::keys::Chord::key(KeyCode::F5),
    );
    let start = scene.owner.position();
    latency_tests::key(&mut scene.app, KeyCode::F8, true);
    latency_tests::key(&mut scene.app, KeyCode::F5, true);
    for _ in 0..60 {
        scene.step(false);
    }
    assert!((scene.owner.position().x - start.x).hypot(scene.owner.position().y - start.y) < 0.001);
    latency_tests::key(&mut scene.app, KeyCode::F8, false);
    latency_tests::key(&mut scene.app, KeyCode::F5, false);
    scene.step(false);
    latency_tests::key(&mut scene.app, KeyCode::F8, true);
    latency_tests::key(&mut scene.app, KeyCode::F8, false);
    for _ in 0..60 {
        scene.step(false);
    }
    assert!((scene.owner.position().x - start.x).hypot(scene.owner.position().y - start.y) < 0.001);
    assert_eq!(status(&scene), "locked");
}

#[test]
fn melee_orbit_paused_or_idle_script_cannot_borrow_native_keys_as_authored_orbit() {
    for paused in [false, true] {
        let mut scene = scene(true, 0);
        let script = crate::script::Script::new(Vec::new());
        scene
            .app
            .insert_resource(if paused { script.paused() } else { script });
        let before = scene
            .app
            .world()
            .resource::<Controls>()
            .test_melee
            .proposals;
        latency_tests::key(&mut scene.app, KeyCode::F8, true);
        for _ in 0..60 {
            scene.step(false);
        }
        assert_eq!(status(&scene), "suspended");
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
}
