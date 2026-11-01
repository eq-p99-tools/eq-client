use super::*;
use crate::online::testing as session;
use eq_client_core::WorldEvent;

#[test]
fn cardinal_travel_arrows_point_in_map_coordinates() {
    for direction in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
        assert!((rotation(direction) * Vec2::NEG_Y - direction).length() < 0.0001);
    }
}

#[test]
fn local_travel_is_distinct_from_facing_and_clears_on_stopping_or_readmission() {
    let mut motion = MapMotion::default();
    let now = Instant::now();
    let root = Entity::PLACEHOLDER;
    let start = WorldPosition::default(); // Facing north.
    motion.observe(Some(1), Some((root, start)), now);
    let back = WorldPosition { y: -2.0, ..start };
    motion.observe(Some(1), Some((root, back)), now + Duration::from_millis(20));
    assert_eq!(motion.direction, Some(Vec2::Y)); // Travelling south.
    motion.observe(
        Some(1),
        Some((root, back)),
        now + Duration::from_millis(200),
    );
    assert_eq!(motion.direction, None);
    let strafe = WorldPosition { x: 2.0, ..back };
    motion.observe(
        Some(1),
        Some((root, strafe)),
        now + Duration::from_millis(220),
    );
    assert_eq!(motion.direction, Some(Vec2::NEG_X));
    motion.observe(
        Some(2),
        Some((root, start)),
        now + Duration::from_millis(240),
    );
    assert_eq!(motion.direction, None);
    let teleport = WorldPosition { x: 1000.0, ..start };
    motion.observe(
        Some(2),
        Some((root, teleport)),
        now + Duration::from_millis(260),
    );
    assert_eq!(motion.direction, None);
}

#[test]
fn remote_travel_uses_velocity_on_both_clients_and_stops_or_expires() {
    let mut online = OnlineState::new(true);
    session::admit(&mut online, 1, session::player(1));
    let mut moving = session::pet(2, 1);
    moving.position.heading = 0.0; // North, while its velocity is west.
    moving.velocity = [4.0, 0.0, 0.0];
    session::spawns(&mut online, vec![moving]);
    let now = Instant::now();
    for dialect in [GameDialect::Titanium, GameDialect::EqMac] {
        assert_eq!(travel(online.world(), 2, dialect, now), Some(Vec2::NEG_X));
        assert_eq!(
            travel(
                online.world(),
                2,
                dialect,
                now + REPORT_FRESHNESS + Duration::from_secs(1)
            ),
            None
        );
    }
    session::news(
        &mut online,
        [WorldEvent::Position {
            spawn_id: 2,
            position: WorldPosition {
                x: 5.0,
                ..default()
            },
            velocity: [0.0; 3],
            forward: 0.0,
        }],
    );
    assert_eq!(
        travel(online.world(), 2, GameDialect::Titanium, Instant::now()),
        None
    );
    let mut invalid = session::pet(3, 1);
    invalid.velocity = [f32::NAN, 0.0, 0.0];
    let mut corpse = session::pet(4, 1);
    corpse.kind = SpawnKind::NpcCorpse;
    corpse.forward = 1.0;
    session::spawns(&mut online, vec![invalid, corpse]);
    for id in [3, 4] {
        assert_eq!(
            travel(online.world(), id, GameDialect::Titanium, Instant::now()),
            None
        );
    }
}

#[test]
fn local_pose_tracks_the_scene_each_frame_and_falls_back_while_loading() {
    let mut app = crate::testing::app();
    session::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        1,
        session::player(1),
    );
    app.insert_resource(crate::SceneInfo {
        zone_name: "qeytoqrg".into(),
    })
    .add_systems(Update, sample);
    let root = app
        .world_mut()
        .spawn((
            crate::Player,
            crate::zone::placement(WorldPosition::default()),
        ))
        .id();
    app.update();
    let pose = WorldPosition {
        x: 20.0,
        y: 3.0,
        heading: 128.0,
        ..default()
    };
    *app.world_mut().get_mut::<Transform>(root).unwrap() = crate::zone::placement(pose);
    app.update();
    let world = app.world().resource::<OnlineState>().world();
    let displayed = app.world().resource::<MapMotion>().player(world).unwrap();
    assert!(
        (displayed.x - pose.x).abs() < 0.001 && (displayed.heading - pose.heading).abs() < 0.001
    );
    assert_eq!(world.player().unwrap().position, WorldPosition::default()); // No network-state write.
    let release = session::hold_scene(&mut app.world_mut().resource_mut::<OnlineState>());
    app.update();
    let world = app.world().resource::<OnlineState>().world();
    assert_eq!(
        app.world().resource::<MapMotion>().player(world),
        Some(WorldPosition::default())
    );
    assert!(app.world().resource::<MapMotion>().direction.is_none());
    drop(release);
}

#[test]
fn markers_and_arrows_refresh_from_events_and_search_without_node_replacement() {
    let mut app = crate::testing::app();
    session::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        1,
        session::player(1),
    );
    let mut npc = session::pet(2, 1);
    npc.velocity = [4.0, 0.0, 0.0];
    session::spawns(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        vec![npc],
    );
    app.world_mut()
        .resource_mut::<super::super::MapView>()
        .projection = super::super::Projection::new(
        (Vec2::splat(-500.0), Vec2::splat(500.0)),
        Vec2::splat(500.0),
        0,
        Vec2::ZERO,
    );
    app.world_mut()
        .spawn((super::super::MapCanvas, Node::default()));
    app.add_systems(Update, (super::super::entities::update, draw).chain());
    app.update();
    let world = app.world_mut();
    let marker = world
        .query::<(Entity, &Marker)>()
        .iter(world)
        .find(|(_, marker)| marker.0 == 2)
        .unwrap()
        .0;
    let arrow = world
        .query::<(Entity, &ChildOf)>()
        .iter(world)
        .find(|(_, parent)| parent.parent() == marker)
        .unwrap()
        .0;
    assert_eq!(world.get::<Node>(arrow).unwrap().display, Display::Flex);
    assert!(
        (world.get::<UiTransform>(arrow).unwrap().rotation * Vec2::NEG_Y - Vec2::NEG_X).length()
            < 0.001
    );
    let before = world.get::<Node>(marker).unwrap().left;
    world.resource_mut::<super::super::MapView>().query = "different name".into();
    session::news(
        &mut world.resource_mut::<OnlineState>(),
        [WorldEvent::Position {
            spawn_id: 2,
            position: WorldPosition {
                x: 100.0,
                ..default()
            },
            velocity: [4.0, 0.0, 0.0],
            forward: 0.0,
        }],
    );
    app.update();
    assert_ne!(app.world().get::<Node>(marker).unwrap().left, before);
    assert!(app.world().get::<ImageNode>(arrow).unwrap().color.alpha() < 0.3);
    app.world_mut()
        .resource_mut::<super::super::WatchState>()
        .add("Gabober");
    app.update();
    assert!(app.world().get::<ImageNode>(arrow).unwrap().color.alpha() > 0.9);
    session::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Despawn(2)],
    );
    app.update();
    assert!(app.world().get_entity(marker).is_err() && app.world().get_entity(arrow).is_err());
}
