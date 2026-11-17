//! The pace log measures the final drawn world pose, including carrier motion.
use super::{Controls, boats, interpolate};
use bevy::{
    ecs::schedule::SingleThreadedExecutor,
    log::{
        tracing::{
            Event,
            field::{Field, Visit},
            subscriber,
        },
        tracing_subscriber::{
            Layer, Registry,
            layer::{Context, SubscriberExt},
        },
    },
    prelude::*,
};
use eq_client_core::{
    GameDialect, WorldEvent, WorldPosition,
    movement::{Carriers, carrier_world},
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct PaceCapture(Arc<Mutex<Vec<f64>>>);

impl Layer<Registry> for PaceCapture {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, Registry>) {
        if event.metadata().target() == "eq_client_render::motion::pace" {
            event.record(&mut SpeedVisitor(&self.0));
        }
    }
}

struct SpeedVisitor<'a>(&'a Mutex<Vec<f64>>);

impl Visit for SpeedVisitor<'_> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        if field.name() == "shown_speed" {
            self.0.lock().unwrap().push(value);
        }
    }

    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Two actual interpolation frames and their logged world pace"
)]
fn carrier_pace_uses_world_endpoints_after_local_playback_and_carry() {
    let now = Instant::now();
    // A distant world origin makes a local/world subtraction fail visibly.
    let pose = WorldPosition {
        x: 1000.0,
        y: 2000.0,
        z: 10.0,
        heading: 128.0,
    };
    let local_from = WorldPosition {
        x: 20.0,
        y: 5.0,
        z: 3.0,
        heading: 0.0,
    };
    let local_to = WorldPosition {
        x: 21.0,
        ..local_from
    };
    let from = carrier_world(pose, local_from);
    let to = carrier_world(pose, local_to);
    let mut spawn = crate::online::testing::pet(8, 7);
    spawn.race = 72;
    spawn.pet_owner = None;
    spawn.position = pose;
    let spawned = WorldEvent::Spawns(vec![spawn]);
    let mut carriers = Carriers::default();
    let reports = carriers.observe(&spawned, GameDialect::Titanium, now);
    let mut state = crate::online::OnlineState::new(true);
    let mut player = crate::online::testing::player(7);
    player.position = from;
    crate::online::testing::admit(&mut state, 11, player);
    crate::online::testing::news(&mut state, [spawned, WorldEvent::Carriers(reports)]);
    let drawn =
        Transform::from_translation(Vec3::from_array(eq_client_core::render_position(from)))
            .with_rotation(Quat::from_rotation_y(eq_client_core::render_heading(
                from.heading,
            )));
    let mut controls = Controls {
        boat: Some(boats::Riding {
            sample: carriers.latest(8).unwrap().sample(),
            pose,
            position: to,
        }),
        sent_span: 0.1,
        ..Controls::default()
    };
    controls.display_carrier_sample(drawn, to, &carriers);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(50));
    let mut app = App::new();
    app.insert_resource(state)
        .insert_resource(controls)
        .insert_resource(time)
        .add_systems(Update, interpolate)
        .edit_schedule(Update, |schedule| {
            schedule.set_executor(SingleThreadedExecutor::new());
        });
    let player = app.world_mut().spawn((crate::Player, drawn)).id();
    let changed_pose = WorldPosition {
        x: pose.x + 1.0,
        heading: pose.heading + 1.0,
        ..pose
    };
    let expected_first = Vec3::from_array(eq_client_core::render_position(carrier_world(
        pose,
        WorldPosition {
            x: 20.5,
            ..local_from
        },
    )));
    let expected_second = Vec3::from_array(eq_client_core::render_position(carrier_world(
        changed_pose,
        local_to,
    )));
    let logged = Arc::new(Mutex::new(Vec::new()));
    let capture = Registry::default().with(PaceCapture(Arc::clone(&logged)));
    let (first, second) = subscriber::with_default(capture, || {
        app.update();
        let first = *app.world().get::<Transform>(player).unwrap();
        let changed = WorldEvent::Position {
            spawn_id: 8,
            position: changed_pose,
            velocity: [0.0; 3],
            forward: 0.0,
        };
        let reports = carriers.observe(&changed, GameDialect::Titanium, Instant::now());
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            [changed, WorldEvent::Carriers(reports)],
        );
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(50));
        app.update();
        (first, *app.world().get::<Transform>(player).unwrap())
    });
    assert!(first.translation.distance(expected_first) < 0.001);
    assert!(second.translation.distance(expected_second) < 0.001);
    let expected = [
        (expected_first - drawn.translation).with_y(0.0).length() / 0.05,
        (expected_second - expected_first).with_y(0.0).length() / 0.05,
    ];
    let logged = logged.lock().unwrap();
    assert_eq!(logged.len(), expected.len());
    for (actual, expected) in logged.iter().zip(expected) {
        assert!(
            (actual - f64::from(expected)).abs() < 0.05,
            "{actual} vs {expected}"
        );
    }
}
