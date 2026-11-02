use super::*;
use crate::{clicks::testing as clicks, online::testing as session};
use eq_client_core::{ClientCommand, SpawnKind, WorldEvent, WorldPosition};
use std::sync::mpsc::Receiver;

fn app() -> (App, Receiver<ClientCommand>) {
    let mut app = crate::testing::app();
    let mut online = OnlineState::new(true);
    session::enter(&mut online, 1, session::player(1), Some(100.0));
    let mut npc = session::pet(2, 1);
    npc.position.x = 20.0;
    session::spawns(&mut online, vec![npc]);
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(send)));
    app.insert_resource(MapView {
        admission: Some(1),
        projection: super::super::Projection::new(
            (Vec2::splat(-500.0), Vec2::splat(500.0)),
            Vec2::splat(500.0),
            0,
            Vec2::ZERO,
        ),
        ..default()
    });
    app.world_mut()
        .spawn((super::super::MapCanvas, Node::default()));
    app.add_systems(
        Update,
        (
            entities::inspect,
            super::super::controls::actions,
            input,
            entities::update,
            show,
        )
            .chain(),
    );
    app.update();
    (app, receive)
}

fn pin(app: &mut App, id: u16) {
    let world = app.world_mut();
    let marker = world
        .query::<(Entity, &entities::Marker)>()
        .iter(world)
        .find(|(_, marker)| marker.0 == id)
        .unwrap()
        .0;
    clicks::click(app, marker);
}

fn button(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<TargetButton>>()
        .single(world)
        .unwrap()
}

#[test]
fn pin_and_track_are_local_and_target_posts_exactly_one_selection_command() {
    let (mut app, receive) = app();
    pin(&mut app, 2);
    let world = app.world_mut();
    let track = world
        .query::<(Entity, &super::super::controls::Control)>()
        .iter(world)
        .find(|(_, control)| matches!(control, super::super::controls::Control::WatchSelected))
        .unwrap()
        .0;
    clicks::click(&mut app, track);
    assert!(
        app.world()
            .resource::<super::super::WatchState>()
            .names
            .contains("Gabober")
    );
    assert!(receive.try_recv().is_err());
    assert_eq!(
        app.world()
            .resource::<OnlineState>()
            .world()
            .target()
            .selected,
        None
    );
    let target = button(&mut app);
    clicks::click(&mut app, target);
    assert!(matches!(
        receive.try_recv().unwrap(),
        ClientCommand::SelectTarget {
            session_id: 1,
            spawn_id: Some(2)
        }
    ));
    assert!(receive.try_recv().is_err());
    assert_eq!(app.world().get::<Text>(target).unwrap().0, "Targeted");
    clicks::click(&mut app, target);
    assert!(receive.try_recv().is_err());
    // A target may be retained after it leaves acquisition range. Keep the
    // button's state accurate while refusing a new selection there.
    session::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Position {
            spawn_id: 2,
            position: WorldPosition {
                x: 200.0,
                ..default()
            },
            velocity: [0.0; 3],
            forward: 0.0,
        }],
    );
    app.update();
    assert_eq!(app.world().get::<Text>(target).unwrap().0, "Targeted");
    assert!(receive.try_recv().is_err());
}

#[test]
fn a_replaced_spawn_between_press_and_release_is_not_targeted() {
    let (mut app, receive) = app();
    pin(&mut app, 2);
    let target = button(&mut app);
    clicks::press(&mut app, Some(target));
    session::spawns(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        vec![session::pet(2, 1)],
    );
    clicks::release(&mut app, Some(target));
    assert!(receive.try_recv().is_err());
    assert_eq!(
        app.world()
            .resource::<OnlineState>()
            .world()
            .target()
            .selected,
        None
    );
}

#[test]
fn unavailable_entities_stay_inspectable_but_cannot_be_targeted() {
    for (x, invisible, kind, reason) in [
        (200.0, false, SpawnKind::Npc, "range"),
        (20.0, true, SpawnKind::Npc, "invisible"),
        (20.0, false, SpawnKind::Unknown(99), "type"),
    ] {
        let (mut app, receive) = app();
        let mut npc = session::pet(2, 1);
        npc.position.x = x;
        npc.invisible = invisible;
        npc.kind = kind;
        session::spawns(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            vec![npc],
        );
        app.update();
        pin(&mut app, 2);
        let target = button(&mut app);
        assert!(
            app.world()
                .get::<Tooltip>(target)
                .unwrap()
                .0
                .contains(reason)
        );
        assert_eq!(
            app.world().get::<TextColor>(target).unwrap().0,
            theme::INK_DIM
        );
        clicks::click(&mut app, target);
        assert!(receive.try_recv().is_err());
        assert!(app.world().resource::<MapView>().selected.is_some());
    }
}

#[test]
fn full_outbox_keeps_the_old_target_and_self_target_needs_no_spawn_entry() {
    let (mut app, receive) = app();
    let online = app.world().resource::<OnlineState>();
    app.world()
        .resource::<Outbox>()
        .post(online.world(), |stamp| ClientCommand::SelectTarget {
            session_id: stamp.session_id,
            spawn_id: None,
        })
        .unwrap();
    pin(&mut app, 2);
    let target = button(&mut app);
    clicks::click(&mut app, target);
    assert_eq!(
        app.world()
            .resource::<OnlineState>()
            .world()
            .target()
            .selected,
        None
    );
    receive.try_recv().unwrap();
    assert!(receive.try_recv().is_err());
    pin(&mut app, 1);
    clicks::click(&mut app, target);
    assert!(matches!(
        receive.try_recv().unwrap(),
        ClientCommand::SelectTarget {
            spawn_id: Some(1),
            ..
        }
    ));
    session::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Position {
            spawn_id: 1,
            position: WorldPosition {
                x: 5.0,
                ..default()
            },
            velocity: [0.0; 3],
            forward: 0.0,
        }],
    );
    app.update();
    assert!(receive.try_recv().is_err());
}
