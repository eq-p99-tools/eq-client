//! Ordinary entry transaction regressions; no new swim admission authority.
use super::*;
use bevy::ecs::system::RunSystemOnce;
use eq_client_core::movement::{Liquid, Liquids, water};

#[derive(Clone)]
struct Shore;

impl Liquids for Shore {
    fn liquid_at(&self, point: Vec3) -> Option<Liquid> {
        (point.x >= 0. && point.y <= 0.).then_some(Liquid::Water)
    }

    fn passes_through(&self, from: Vec3, to: Vec3, liquid: Liquid) -> bool {
        if liquid != Liquid::Water {
            return false;
        }
        let (mut enter, mut leave) = (0_f32, 1_f32);
        for (a, b) in [(from.x, to.x), (-from.y, -to.y)] {
            if a < 0. && b < 0. {
                return false;
            }
            if a < 0. {
                enter = enter.max(-a / (b - a));
            } else if b < 0. {
                leave = leave.min(-a / (b - a));
            }
        }
        enter <= leave
    }

    fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
        Some(Box::new(self.clone()))
    }
}

fn prior_wet() -> water::WetVelocity {
    let mut wet = water::WetVelocity::from_units_per_second([1.2, -0.8], -2.);
    wet.advance(
        water::LIQUID_5,
        water::WetInput::default(),
        water::WET_FRAME_SECONDS * 0.37,
    );
    wet
}

type AirState = Option<(f32, Vec3, Option<f32>)>;

#[derive(Clone, Copy)]
enum Producer {
    Walk,
    WalkWithoutFall,
    Fall,
}

const PRODUCERS: [Producer; 3] = [Producer::Walk, Producer::WalkWithoutFall, Producer::Fall];

fn airborne(state: &Controls) -> AirState {
    state
        .airborne
        .as_ref()
        .map(|air| (air.velocity(), air.drift(), air.peak()))
}

fn entry(producer: Producer, wet: water::WetVelocity) -> (Native, MovementRequest, AirState) {
    let falling = matches!(producer, Producer::Fall);
    let mut f = Native::new(false, if falling { 0.2 } else { -4. });
    if falling {
        f.app
            .world_mut()
            .resource_mut::<Controls>()
            .airborne
            .as_mut()
            .unwrap()
            .carry_from_water(-8., Vec3::X * 2., 0.2 - 3.75);
    } else {
        // A real collision walk reaches the shore. Exercise both its ordinary
        // dry controller and the achieved-travel branch without that controller.
        let shore = CollisionWorld::new([
            [
                [-200., -7.75, -200.],
                [200., -7.75, 200.],
                [200., -7.75, -200.],
            ],
            [
                [-200., -7.75, -200.],
                [-200., -7.75, 200.],
                [200., -7.75, 200.],
            ],
        ])
        .unwrap()
        .with_liquids(Shore)
        .with_own_liquids(Shore)
        .with_swim_origin(3.75);
        f.app.insert_resource(Collision(Some(shore)));
        let start = world_position([-0.1, -4., 0.], 0.);
        online::testing::place(
            &mut f.app.world_mut().resource_mut::<online::OnlineState>(),
            start,
        );
        f.app
            .world_mut()
            .get_mut::<Transform>(f.player)
            .unwrap()
            .translation = Vec3::new(-0.1, -4., 0.);
        if matches!(producer, Producer::WalkWithoutFall) {
            f.app.world_mut().resource_mut::<Controls>().airborne = None;
        }
        crate::keys::testing::bind(
            &mut f.app,
            Act::StrafeLeft,
            crate::keys::Chord::key(KeyCode::KeyZ),
        );
        latency_tests::key(&mut f.app, KeyCode::KeyZ, true);
    }
    {
        let mut controls = f.app.world_mut().resource_mut::<Controls>();
        controls.wet = wet;
        controls.suppress_surface = true;
    }
    let before_air = airborne(f.app.world().resource::<Controls>());
    let mut proposals = f.frame(0.);
    assert_eq!(proposals.len(), 1);
    let request = proposals.pop().unwrap();
    let controls = f.app.world().resource::<Controls>();
    assert_ne!(controls.wet, wet, "the real entry seeded a trial");
    assert_eq!(request.mode == MovementMode::Fall, falling);
    (f, request, before_air)
}

fn receive(f: &mut Native, event: eq_client_core::WorldEvent) {
    f.news
        .send(eq_client_core::WorldUpdate::Game(event))
        .unwrap();
    // Run the production reducer without consuming a new physics frame.
    f.app.world_mut().run_system_once(online::receive).unwrap();
}

#[test]
fn ordinary_entry_acceptance_keeps_the_actual_converted_state() {
    for producer in PRODUCERS {
        let (mut f, request, _) = entry(producer, prior_wet());
        let converted = f.app.world().resource::<Controls>().wet;
        receive(
            &mut f,
            eq_client_core::WorldEvent::MotionSent {
                session_id: 11,
                created: request.created,
                position: request.position,
                refused: None,
            },
        );
        let controls = f.app.world().resource::<Controls>();
        assert_eq!(controls.wet, converted);
        assert!(controls.water_entry_rollback.is_none());
        assert_eq!(
            f.app
                .world()
                .resource::<online::OnlineState>()
                .world()
                .player()
                .unwrap()
                .position,
            request.position
        );
    }
}

fn refusals_restore(producer: Producer) {
    for discarded in [false, true] {
        for wet in [water::WetVelocity::default(), prior_wet()] {
            let (mut f, request, before_air) = entry(producer, wet);
            let origin = f
                .app
                .world()
                .resource::<online::OnlineState>()
                .world()
                .player()
                .unwrap()
                .position;
            let event = if discarded {
                eq_client_core::WorldEvent::MotionRefused {
                    session_id: 11,
                    created: request.created,
                    reason: "entry fixture discarded".into(),
                }
            } else {
                eq_client_core::WorldEvent::MotionSent {
                    session_id: 11,
                    created: request.created,
                    position: origin,
                    refused: Some("entry fixture refused".into()),
                }
            };
            receive(&mut f, event);
            let controls = f.app.world().resource::<Controls>();
            assert_eq!(controls.wet, wet);
            assert_eq!(airborne(controls), before_air);
            assert!(controls.water_entry_rollback.is_none());
            assert_eq!(
                f.app
                    .world()
                    .resource::<online::OnlineState>()
                    .world()
                    .player()
                    .unwrap()
                    .position,
                origin
            );
        }
    }
}

#[test]
fn fall_entry_refusal_and_discard_restore_both_sides_of_the_conversion() {
    refusals_restore(Producer::Fall);
}

#[test]
fn walk_entry_refusal_and_discard_restore_both_sides_of_the_conversion() {
    for producer in [Producer::Walk, Producer::WalkWithoutFall] {
        refusals_restore(producer);
    }
}

#[test]
fn stale_entry_refusal_cannot_restore_state_across_a_correction() {
    for producer in PRODUCERS {
        let (mut f, request, _) = entry(producer, prior_wet());
        let trial = f.app.world().resource::<Controls>().wet;
        receive(
            &mut f,
            eq_client_core::WorldEvent::MotionRefused {
                session_id: 11,
                created: request.created - Duration::from_nanos(1),
                reason: "older entry result".into(),
            },
        );
        assert_eq!(f.app.world().resource::<Controls>().wet, trial);
        receive(
            &mut f,
            eq_client_core::WorldEvent::Position {
                spawn_id: 7,
                position: request.position,
                velocity: [0.; 3],
                forward: 0.,
            },
        );
        receive(
            &mut f,
            eq_client_core::WorldEvent::MotionRefused {
                session_id: 11,
                created: request.created,
                reason: "retired entry result".into(),
            },
        );
        let controls = f.app.world().resource::<Controls>();
        assert_eq!(controls.wet, water::WetVelocity::default());
        assert!(controls.water_entry_rollback.is_none());
    }
}
