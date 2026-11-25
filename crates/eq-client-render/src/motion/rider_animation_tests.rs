//! Display frame ownership across idle carrier samples; no input/session changes.
use super::*;
use crate::online::{OnlineState, testing as session};
use eq_client_core::movement::{Carriers, carrier_world};
use eq_client_core::{GameDialect, WorldEvent, WorldPosition};

struct Ferry {
    app: App,
    root: Entity,
    reports: Carriers,
    local: WorldPosition,
}

fn ferry(walking: bool) -> Ferry {
    let pose = WorldPosition {
        x: 620.0,
        y: -1772.0,
        z: -16.375,
        heading: 81.5,
    };
    let local = WorldPosition {
        x: 40.0,
        y: 70.0,
        z: 25.0,
        heading: 12.0,
    };
    let position = carrier_world(pose, local);
    let mut spawn = session::pet(97, 7);
    spawn.race = 72;
    spawn.pet_owner = None;
    spawn.position = pose;
    let event = WorldEvent::Spawns(vec![spawn]);
    let mut reports = Carriers::default();
    let updates = reports.observe(&event, GameDialect::Titanium, Instant::now());
    let mut online = OnlineState::new(true);
    let mut player = session::player(7);
    player.position = position;
    session::admit(&mut online, 11, player);
    session::news(&mut online, [event, WorldEvent::Carriers(updates)]);
    let drawn =
        Transform::from_translation(Vec3::from_array(eq_client_core::render_position(position)))
            .with_rotation(Quat::from_rotation_y(eq_client_core::render_heading(
                position.heading,
            )));
    let mut controls = Controls {
        walking,
        boat: Some(boats::Riding {
            sample: reports.latest(97).unwrap().sample(),
            pose,
            position,
        }),
        sent_span: 0.1,
        ..default()
    };
    controls.display_carrier_sample(drawn, position, &reports);
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<crate::spellbook::SpellNames>()
        .insert_resource(online)
        .insert_resource(controls)
        .insert_resource(Collision(None))
        .add_systems(Update, (interpolate, crate::character::animate).chain());
    crate::character::testing::stores(app.world_mut());
    let asset = eq_client_assets::characters::CharacterAsset::testing_animations(&["L01", "L02"]);
    let prepared = crate::character::testing::prepared_from(app.world_mut(), asset);
    let root = crate::character::testing::drawn(app.world_mut(), &prepared);
    app.world_mut().entity_mut(root).insert((Player, drawn));
    let mut result = Ferry {
        app,
        root,
        reports,
        local,
    };
    frame(&mut result);
    frame(&mut result);
    assert!(result.app.world().resource::<Controls>().visual.is_none());
    result
}

fn frame(ferry: &mut Ferry) {
    ferry
        .app
        .world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(50));
    ferry.app.update();
}

fn floor_at(root: Vec3) -> CollisionWorld {
    CollisionWorld::new(
        eq_client_assets::testing::flat_surface(root.y)
            .into_iter()
            .map(|triangle| {
                triangle.map(|mut point| {
                    point[0] += root.x;
                    point[2] += root.z;
                    point
                })
            }),
    )
    .unwrap()
}

fn sample(ferry: &mut Ferry, index: u16, floor: bool, baseline: bool) {
    let step = f32::from(index);
    let pose = WorldPosition {
        x: 620.0 + step * 4.0,
        y: -1772.0 + step * 3.0,
        z: -16.375,
        heading: 81.5 + step * 3.0,
    };
    let event = WorldEvent::Position {
        spawn_id: 97,
        position: pose,
        velocity: [0.0; 3],
        forward: 0.0,
    };
    let updates = ferry
        .reports
        .observe(&event, GameDialect::Titanium, Instant::now());
    let position = carrier_world(pose, ferry.local);
    session::news(
        &mut ferry.app.world_mut().resource_mut::<OnlineState>(),
        [
            event,
            WorldEvent::Carriers(updates),
            WorldEvent::Position {
                spawn_id: 7,
                position,
                velocity: [0.0; 3],
                forward: 0.0,
            },
        ],
    );
    let from = *ferry.app.world().get::<Transform>(ferry.root).unwrap();
    {
        let mut controls = ferry.app.world_mut().resource_mut::<Controls>();
        controls.boat = Some(boats::Riding {
            sample: ferry.reports.latest(97).unwrap().sample(),
            pose,
            position,
        });
        controls.display_carrier_sample(from, position, &ferry.reports);
        if baseline {
            // Exact pre-fix translation basis: the old root is inverted using
            // the newer sample's pose. Keep the same rotation in both runs.
            controls.visual.as_mut().unwrap().from.translation =
                boats::to_local(pose, from).translation;
        }
    }
    let root = Vec3::from_array(eq_client_core::render_position(position));
    let collision = floor.then(|| floor_at(root));
    assert_eq!(
        collision
            .as_ref()
            .and_then(|world| world.nearest_ground(root, 0.01, 1.0))
            .is_some(),
        floor
    );
    ferry.app.insert_resource(Collision(collision));
    frame(ferry);
    assert_eq!(
        ferry
            .app
            .world()
            .resource::<OnlineState>()
            .world()
            .player()
            .unwrap()
            .position,
        position
    );
    let controls = ferry.app.world().resource::<Controls>();
    assert!(
        controls.boat.is_some(),
        "a missing floor probe revoked the retained link"
    );
    assert!(controls.drawn_carrier().is_some());
    assert!(
        controls.drawn_pose().is_none(),
        "own-jump selection must remain excluded on a carrier"
    );
}

#[test]
fn rider_animation_keeps_standing_across_idle_samples_and_intermittent_floor() {
    for walking in [false, true] {
        let mut fixed = ferry(walking);
        let mut before = ferry(walking);
        for index in 1..=8 {
            sample(&mut fixed, index, index % 2 == 0, false);
            sample(&mut before, index, index % 2 == 0, true);
            assert_eq!(
                crate::character::testing::clip(fixed.app.world(), fixed.root),
                "P01"
            );
            assert_eq!(
                crate::character::testing::clip(before.app.world(), before.root),
                if walking { "L01" } else { "L02" },
                "negative control did not reproduce the reported walk/run"
            );
            assert_eq!(
                fixed
                    .app
                    .world()
                    .get::<Transform>(fixed.root)
                    .unwrap()
                    .rotation,
                before
                    .app
                    .world()
                    .get::<Transform>(before.root)
                    .unwrap()
                    .rotation
            );
            frame(&mut fixed);
            frame(&mut before);
        }
        // Actual own travel on the deck still selects the chosen pace.
        fixed.local.x += 1.0;
        sample(&mut fixed, 9, false, false);
        assert_eq!(
            crate::character::testing::clip(fixed.app.world(), fixed.root),
            if walking { "L01" } else { "L02" }
        );
        frame(&mut fixed);
    }
}
