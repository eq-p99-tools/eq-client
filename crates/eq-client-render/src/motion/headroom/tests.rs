use super::*;
use crate::{online::testing, outbox::Outbox};
use eq_client_core::{GameDialect, WorldEvent};
use std::sync::mpsc;

const HUMAN: PlayerBody = PlayerBody {
    feet_offset: 3.75,
    height: 6.677_319_5,
    size: 6.0,
};

fn plane(y: f32, up: bool) -> Vec<[[f32; 3]; 3]> {
    let mut faces = vec![
        [[-20.0, y, -20.0], [-20.0, y, 20.0], [20.0, y, 20.0]],
        [[-20.0, y, -20.0], [20.0, y, 20.0], [20.0, y, -20.0]],
    ];
    if !up {
        for face in &mut faces {
            face.swap(1, 2);
        }
    }
    faces
}

fn room(ceiling: f32) -> CollisionWorld {
    CollisionWorld::new(plane(0.0, true).into_iter().chain(plane(ceiling, false))).unwrap()
}

fn app(client: InstalledClient, ceiling: f32) -> (App, mpsc::Receiver<ClientCommand>) {
    let (mut app, _) = crate::motion::tests::fixture();
    let (sender, receiver) = mpsc::sync_channel(16);
    app.insert_resource(Outbox::new(Some(sender)))
        .insert_resource(Collision(Some(room(ceiling))))
        .add_systems(Update, (crate::posture::refresh, input).chain());
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .installed_client = client;
    {
        let world = app.world_mut();
        let mut player = world.query::<(&mut PlayerBody, &mut Transform)>();
        let (mut body, mut transform) = player.single_mut(world).unwrap();
        *body = HUMAN;
        transform.translation = Vec3::Y * HUMAN.feet_offset;
    }
    let mut online = app.world_mut().resource_mut::<online::OnlineState>();
    testing::place(&mut online, world_position([0.0, 3.75, 0.0], 0.0));
    testing::generation(
        &mut online,
        if client == InstalledClient::EqMac {
            GameDialect::EqMac
        } else {
            GameDialect::Titanium
        },
    );
    testing::news(
        &mut online,
        [WorldEvent::Posture {
            spawn_id: 7,
            posture: PostureState::Standing,
        }],
    );
    (app, receiver)
}

fn posture(app: &mut App, posture: PostureState) {
    testing::news(
        &mut app.world_mut().resource_mut::<online::OnlineState>(),
        [WorldEvent::Posture {
            spawn_id: 7,
            posture,
        }],
    );
}

fn commands(receiver: &mpsc::Receiver<ClientCommand>) -> Vec<Posture> {
    receiver
        .try_iter()
        .filter_map(|command| match command {
            ClientCommand::SetPosture { posture, .. } => Some(posture),
            _ => None,
        })
        .collect()
}

#[test]
fn actual_input_refuses_then_ducks_once_on_the_next_pass_for_keys_and_scripts() {
    for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
        for enabled in [false, true] {
            for scripted in [false, true] {
                let (mut app, receiver) = app(client, 7.9);
                app.world_mut()
                    .resource_mut::<crate::options::OptionsState>()
                    .options
                    .allow_auto_stand = enabled;
                if scripted {
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .reset_all();
                    crate::script::Script::press_keys_for_test(&mut app, &[KeyCode::KeyW]);
                }
                app.update();
                assert!(app.world().resource::<Controls>().headroom_refused);
                let first: Vec<_> = receiver.try_iter().collect();
                assert!(
                    !first
                        .iter()
                        .any(|command| matches!(command, ClientCommand::SetPosture { .. }))
                );
                assert!(first.iter().any(|command| matches!(command, ClientCommand::Move(request)
                    if request.position.x == 0.0 && request.position.y == 0.0 && request.position.z == 3.75)), "{first:?}");
                app.update();
                let expected = if enabled || client == InstalledClient::EqMac {
                    vec![Posture::Ducking]
                } else {
                    vec![]
                };
                assert_eq!(
                    commands(&receiver),
                    expected,
                    "{client:?} option={enabled} script={scripted}"
                );
                app.update();
                assert!(
                    commands(&receiver).is_empty(),
                    "no duplicate before an observed posture"
                );
                assert_eq!(
                    app.world()
                        .resource::<online::OnlineState>()
                        .world()
                        .posture(7),
                    Some(PostureState::Standing)
                );
            }
        }
    }
}

#[test]
fn automatic_return_waits_past_two_seconds_and_uses_the_native_return_predicate() {
    for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
        let (mut app, receiver) = app(client, 7.9);
        let now = Instant::now();
        app.world_mut().resource_mut::<Controls>().test_now = Some(now);
        app.update();
        let _ = commands(&receiver);
        app.update();
        assert_eq!(commands(&receiver), vec![Posture::Ducking]);
        posture(&mut app, PostureState::Ducking);
        app.insert_resource(Collision(Some(room(8.1))));
        for millis in [1_999, 2_000] {
            app.world_mut().resource_mut::<Controls>().test_now =
                Some(now + Duration::from_millis(millis));
            app.update();
            assert!(commands(&receiver).is_empty());
        }
        app.world_mut().resource_mut::<Controls>().test_now =
            Some(now + Duration::from_millis(2_001));
        app.update();
        if client == InstalledClient::EqMac {
            assert_eq!(commands(&receiver), vec![Posture::Standing]);
        } else {
            // Manual standing fits at >8.0, but Titanium return needs >2S+r=8.15.
            assert!(commands(&receiver).is_empty());
            app.insert_resource(Collision(Some(room(8.2))));
            app.update();
            assert_eq!(commands(&receiver), vec![Posture::Standing]);
        }
        app.update();
        assert!(commands(&receiver).is_empty());
        posture(&mut app, PostureState::Standing);
        app.update();
        assert!(
            app.world()
                .resource::<Controls>()
                .headroom
                .automatic
                .is_none()
        );
    }
}

#[test]
fn an_impossible_titanium_duck_clears_the_flag_and_sends_nothing() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
    app.update();
    let _ = commands(&receiver);
    // Geometry changes before the next pass; Duck's own room test fails.
    app.insert_resource(Collision(Some(room(5.25))));
    app.update();
    assert!(commands(&receiver).is_empty());
    assert!(
        app.world()
            .resource::<Controls>()
            .headroom
            .automatic
            .is_none()
    );
}

#[test]
fn a_refused_jump_endpoint_keeps_the_eligible_launch_command() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
    app.world_mut().resource_mut::<Controls>().airborne = Some(AirborneController::default());
    app.world_mut().resource_mut::<Controls>().climb_sent = None;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.update();
    let sent: Vec<_> = receiver.try_iter().collect();
    assert!(
        sent.iter()
            .any(|command| matches!(command, ClientCommand::Jump { .. })),
        "{sent:?}"
    );
    assert!(app.world().resource::<Controls>().headroom_refused);
    assert!(
        sent.iter()
            .any(|command| matches!(command, ClientCommand::Move(request)
        if request.position.x == 0.0 && request.position.y == 0.0 && request.position.z == 3.75)),
        "{sent:?}"
    );
}

#[test]
fn a_new_admission_or_lost_geometry_cannot_drive_an_old_automatic_cycle() {
    for lost_geometry in [false, true] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
        app.update();
        let _ = commands(&receiver);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        if lost_geometry {
            app.insert_resource(Collision(None));
        } else {
            let mut online = app.world_mut().resource_mut::<online::OnlineState>();
            let player = online.world().player().unwrap().clone();
            testing::admit(&mut online, 12, player);
        }
        app.update();
        assert!(commands(&receiver).is_empty());
        assert!(
            app.world()
                .resource::<Controls>()
                .headroom
                .automatic
                .is_none()
        );
    }
}

#[test]
fn an_intervening_posture_does_not_leave_a_stale_wait_or_queue_another_action() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
    app.update();
    let _ = commands(&receiver);
    app.update();
    assert_eq!(commands(&receiver), vec![Posture::Ducking]);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    posture(&mut app, PostureState::Sitting);
    app.update();
    assert!(commands(&receiver).is_empty());
    assert!(
        app.world()
            .resource::<Controls>()
            .headroom
            .automatic
            .is_none()
    );
}

#[test]
fn a_ducked_refusal_with_the_option_off_arms_only_macs_return() {
    // Exercise the refusal-to-posture contract separately from the capsule:
    // a failed ducked strict-room query arms Mac even though already ducked.
    for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
        let (mut app, receiver) = app(client, 8.1);
        posture(&mut app, PostureState::Ducking);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .allow_auto_stand = false;
        let now = Instant::now();
        assert!(
            refused(
                client,
                &room(4.25),
                (&HUMAN, 3.75),
                (Vec3::ZERO, Vec3::X),
                (true, false)
            )
            .is_some()
        );
        app.world_mut()
            .resource_scope(|world, mut controls: Mut<Controls>| {
                controls.headroom.refused(
                    client,
                    world.resource::<online::OnlineState>().world(),
                    Some(PostureState::Ducking),
                    false,
                    now,
                );
                controls.test_now = Some(now + Duration::from_millis(2_001));
            });
        app.update();
        assert_eq!(
            commands(&receiver),
            if client == InstalledClient::EqMac {
                vec![Posture::Standing]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn a_launch_stand_report_precedes_automatic_duck() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
    posture(&mut app, PostureState::Ducking);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    let now = Instant::now();
    app.world_mut()
        .resource_scope(|world, mut controls: Mut<Controls>| {
            controls.headroom.refused(
                InstalledClient::Titanium,
                world.resource::<online::OnlineState>().world(),
                Some(PostureState::Standing),
                true,
                now,
            );
        });
    app.update();
    assert!(commands(&receiver).is_empty());
    posture(&mut app, PostureState::Standing);
    app.update();
    assert_eq!(commands(&receiver), vec![Posture::Ducking]);
}

#[test]
fn the_bank_sill_is_tested_after_stepping_and_restores_the_airborne_snapshot() {
    for span in [0.01, 1.0 / 60.0, 0.05] {
        for ceiling in [12.0, 12.1] {
            let (mut app, receiver) = app(InstalledClient::Titanium, ceiling);
            let mut triangles = plane(0.0, true);
            triangles.extend(plane(ceiling, false));
            // A four-unit sill and its leading face. Standing's unchanged
            // raised sweep clears it; the candidate floor is then four up.
            triangles.extend([
                [[0.0, 4.0, -20.0], [0.0, 4.0, 20.0], [20.0, 4.0, 20.0]],
                [[0.0, 4.0, -20.0], [20.0, 4.0, 20.0], [20.0, 4.0, -20.0]],
                [[0.0, 0.0, -20.0], [0.0, 0.0, 20.0], [0.0, 4.0, 20.0]],
                [[0.0, 0.0, -20.0], [0.0, 4.0, 20.0], [0.0, 4.0, -20.0]],
            ]);
            app.insert_resource(Collision(Some(CollisionWorld::new(triangles).unwrap())));
            // A synthetic fast grant crosses the whole face within even
            // the .01-second sample, starting outside capsule contact.
            // This isolates post-step gating from initial wall overlap;
            // it does not calibrate a human's ordinary walking speed.
            let speed = 120.0;
            let start = Vec3::new(-speed * span * 0.5, 3.75, 0.0);
            testing::place(
                &mut app.world_mut().resource_mut::<online::OnlineState>(),
                world_position(start.to_array(), 0.0),
            );
            {
                let world = app.world_mut();
                world
                    .query_filtered::<&mut Transform, With<Player>>()
                    .single_mut(world)
                    .unwrap()
                    .translation = start;
                let mut controls = world.resource_mut::<Controls>();
                controls.test_span = Some(span);
                controls.speed = Some(speed);
                controls.airborne = Some(AirborneController::default());
                controls.climb_sent = None;
            }
            app.update();
            let moved: Vec<_> = receiver
                .try_iter()
                .filter_map(|command| match command {
                    ClientCommand::Move(request) => Some(Vec3::from_array(
                        eq_client_core::render_position(request.position),
                    )),
                    _ => None,
                })
                .collect();
            assert_eq!(moved.len(), 1, "{span} {ceiling}: {moved:?}");
            let controls = app.world().resource::<Controls>();
            if ceiling == 12.0 {
                assert!(controls.headroom_refused, "{span}: {moved:?}");
                assert!(moved[0].abs_diff_eq(start, 1e-5), "{moved:?}");
                assert_eq!(controls.airborne.as_ref().unwrap().velocity(), 0.0);
                assert!(controls.airborne.as_ref().unwrap().peak().is_none());
            } else {
                assert!(!controls.headroom_refused);
                assert!(
                    moved[0].x > start.x && (moved[0].y - 7.75).abs() < 0.01,
                    "{span}: {moved:?}"
                );
            }
        }
    }
}

#[test]
fn ducked_room_and_mount_precedence_remain_per_generation() {
    let from = Vec3::ZERO;
    let to = Vec3::X;
    for (client, threshold) in [
        (InstalledClient::Titanium, 5.25),
        (InstalledClient::EqMac, 4.25),
    ] {
        assert!(
            refused(
                client,
                &room(threshold),
                (&HUMAN, 3.75),
                (from, to),
                (true, true)
            )
            .is_some()
        );
        assert!(
            refused(
                client,
                &room(threshold + 0.001),
                (&HUMAN, 3.75),
                (from, to),
                (true, true)
            )
            .is_none()
        );
    }
    assert!(
        refused(
            InstalledClient::Titanium,
            &room(11.75),
            (&HUMAN, 3.75),
            (from, to),
            (false, true)
        )
        .is_some()
    );
    assert!(
        refused(
            InstalledClient::Titanium,
            &room(5.0),
            (&HUMAN, 3.75),
            (from, from),
            (false, false)
        )
        .is_none()
    );
}

#[derive(Clone)]
struct Submerged;

impl eq_client_core::movement::Liquids for Submerged {
    fn liquid_at(&self, _: Vec3) -> Option<eq_client_core::movement::Liquid> {
        Some(eq_client_core::movement::Liquid::Water)
    }
    fn passes_through(&self, _: Vec3, _: Vec3, liquid: eq_client_core::movement::Liquid) -> bool {
        liquid == eq_client_core::movement::Liquid::Water
    }
    fn first_outside(
        &self,
        _: Vec3,
        _: Vec3,
        _: eq_client_core::movement::Liquid,
    ) -> Option<std::ops::Range<f64>> {
        None
    }
    fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn eq_client_core::movement::Liquids>> {
        Some(Box::new(self.clone()))
    }
}

#[test]
fn underwater_forward_and_retreat_keep_the_existing_kedge_exemption() {
    // A synthetic control for the retained exemption, not a Kedge asset or
    // full-passage result. The room clears the capsule but fails standing.
    for key in [KeyCode::KeyW, KeyCode::KeyS] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
        app.insert_resource(Collision(Some(room(7.9).with_liquids(Submerged))));
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
        let sent: Vec<_> = receiver.try_iter().collect();
        assert!(
            sent.iter()
                .any(|command| matches!(command, ClientCommand::Move(request)
            if request.mode == MovementMode::Swim && request.position.x.abs() > 0.01)),
            "{key:?}: {sent:?}"
        );
        assert!(!app.world().resource::<Controls>().headroom_refused);
        assert!(
            app.world()
                .resource::<Controls>()
                .headroom
                .automatic
                .is_none()
        );
    }
}
