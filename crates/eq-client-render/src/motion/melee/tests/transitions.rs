use super::*;
use eq_client_assets::regions::{ZoneLine, ZoneRegions};
use eq_client_core::movement::{CollisionMesh, CollisionWorld};
use std::sync::Arc;

#[test]
fn melee_refuses_a_thin_mapped_zone_trigger_but_keeps_clear_ground() {
    let mut online = world(30.);
    let body = body(&online);
    let now = Instant::now();
    let line = ZoneRegions::test_zone_line_slab(0.2, 0.3, ZoneLine::Reference(7));
    assert!(line.zone_line_at([0., 3., 0.]).is_none());
    assert!(line.zone_line_at([0.6, 3., 0.]).is_none());
    online.regions = line;
    let held = Assist::default()
        .step(online.world(), &plane(), &online.regions, &body, now)
        .unwrap();
    assert!(
        !held.forward,
        "the segment enters a trigger even when both ends are outside"
    );
    let clear = Assist::default()
        .step(
            online.world(),
            &plane(),
            &ZoneRegions::default(),
            &body,
            now,
        )
        .unwrap();
    assert!(clear.forward);
}

pub(super) fn deck(carrier: bool) -> CollisionWorld {
    let base = [
        [[-300., 0., -300.], [300., 0., 300.], [300., 0., -300.]],
        [[-300., 0., -300.], [-300., 0., 300.], [300., 0., 300.]],
    ];
    let patch = [
        [[2., 0., -5.], [12., 0., 5.], [12., 0., -5.]],
        [[2., 0., -5.], [2., 0., 5.], [12., 0., 5.]],
    ];
    if carrier {
        let mut collision = CollisionWorld::new(base).unwrap();
        collision.set_spawn_obstacles(vec![(9, Arc::new(CollisionMesh::new(patch).unwrap()))]);
        collision
    } else {
        CollisionWorld::new(base.into_iter().chain(patch)).unwrap()
    }
}

#[test]
fn melee_refuses_new_carrier_support_but_allows_the_same_static_floor() {
    let online = world(30.);
    let body = Body {
        speeds: (29.866_667, 12.8),
        ..body(&online)
    };
    let boat = deck(true);
    assert!(boat.ground_spawn(Vec3::ZERO, 0.01, 100.).is_none());
    let ordinary = boat.step(Vec3::ZERO, Vec3::X * 2.986_666_7, body.height);
    assert_eq!(
        boat.ground_spawn(ordinary, 0.01, 100.),
        Some(9),
        "ordinary motion can reach this actual carrier floor"
    );
    let now = Instant::now();
    assert!(
        !Assist::default()
            .step(online.world(), &boat, &online.regions, &body, now)
            .unwrap()
            .forward
    );
    assert!(
        Assist::default()
            .step(online.world(), &deck(false), &online.regions, &body, now)
            .unwrap()
            .forward
    );
}

#[test]
fn melee_zone_guard_preserves_manual_entry_and_ordinary_crossing() {
    use crate::motion::latency_tests;
    for manual in [false, true] {
        let (mut app, requests, news) = latency_tests::timing_app();
        let mut online = world(30.);
        online.regions = ZoneRegions::test_zone_line_slab(0.2, 0.3, ZoneLine::Reference(7));
        app.insert_resource(online)
            .init_resource::<crate::combat::CombatState>()
            .init_resource::<crate::hud::hotbar::Bindings>()
            .add_systems(
                Update,
                crate::combat::input
                    .before(motion::input)
                    .in_set(crate::systems::Stage::Input),
            );
        crate::keys::testing::bind(
            &mut app,
            crate::keys::Act::Attack,
            crate::keys::Chord::key(KeyCode::F10),
        );
        let world = app.world_mut();
        let mut cameras = world.query::<&mut crate::camera::OrbitCamera>();
        cameras.single_mut(world).unwrap().yaw =
            crate::camera::behind(eq_client_core::render_heading(0.));
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(Fix::MeleePositioning, true);
        let now = Instant::now();
        {
            let mut controls = app.world_mut().resource_mut::<motion::Controls>();
            controls.last_accepted = now.checked_sub(Duration::from_millis(100)).unwrap();
            controls.last_sent = None;
            controls.test_now = Some(now);
            controls.test_span = Some(0.1);
        }
        app.world_mut()
            .resource_mut::<crate::outbox::Outbox>()
            .set_test_time(now);
        latency_tests::key(&mut app, KeyCode::F10, true);
        if manual {
            latency_tests::key(&mut app, KeyCode::KeyW, true);
        }
        app.update();
        let commands: Vec<_> = requests.try_iter().collect();
        let moving = commands.into_iter().find_map(|command| match command {
            ClientCommand::Move(request) => Some(request),
            _ => None,
        });
        if manual {
            let moving = moving.expect("manual movement still enters the mapped trigger");
            assert!(
                app.world()
                    .resource::<OnlineState>()
                    .regions
                    .zone_line_at(eq_client_core::render_position(moving.position))
                    .is_some()
            );
            news.send(eq_client_core::WorldUpdate::Game(WorldEvent::MotionSent {
                session_id: 11,
                created: moving.created,
                position: moving.position,
                refused: None,
            }))
            .unwrap();
            app.world_mut().resource_mut::<motion::Controls>().test_now =
                Some(now + Duration::from_millis(200));
            app.world_mut()
                .resource_mut::<crate::outbox::Outbox>()
                .set_test_time(now + Duration::from_millis(200));
            app.update();
            // Receive stamps the local result at this frame. Ordinary input
            // waits another100ms before observing a fresh crossing sample.
            app.world_mut().resource_mut::<motion::Controls>().test_now =
                Some(now + Duration::from_millis(300));
            app.world_mut()
                .resource_mut::<crate::outbox::Outbox>()
                .set_test_time(now + Duration::from_millis(300));
            app.update();
            let after: Vec<_> = requests.try_iter().collect();
            assert!(
                after.iter().any(|command| matches!(
                    command,
                    ClientCommand::CrossZoneLine {
                        destination: eq_client_core::ZoneLineDestination::Reference(7),
                        ..
                    }
                )),
                "ordinary crossing after its local result and100ms gate: {after:?}"
            );
        } else {
            assert!(
                moving.is_none(),
                "assistance must not queue a trigger entry"
            );
        }
    }
}
