use super::*;
use crate::{clicks::testing as clicks, online::testing as session};
use eq_client_core::{SpawnState, WorldEvent};

fn spawn(id: u16, x: f32, kind: SpawnKind) -> SpawnState {
    let mut spawn = session::pet(id, 1);
    spawn.name = format!("Example_entity_{id}");
    spawn.kind = kind;
    spawn.position.x = x;
    spawn.hp_percent = None;
    spawn.pet_owner = None;
    spawn
}

fn online() -> OnlineState {
    let mut online = OnlineState::new(true);
    session::news(
        &mut online,
        [WorldEvent::Entered {
            capabilities: Vec::new(),
            choices: Vec::new(),
            session_id: 1,
            zone: "synthetic".into(),
            player: Box::new(session::player(1)),
            far_clip: None,
            spell_landing_flag: None,
            zone_id: 0,
        }],
    );
    online
}

fn app() -> App {
    let mut app = App::new();
    clicks::install(&mut app);
    app.init_resource::<crate::ViewerSettings>()
        .init_resource::<super::super::WatchState>();
    app.init_resource::<super::super::MapMotion>();
    app.init_resource::<Assets<Image>>();
    app.insert_resource(online())
        .insert_resource(MapView {
            admission: Some(1),
            projection: super::super::Projection::new(
                (Vec2::splat(-1000.0), Vec2::splat(1000.0)),
                Vec2::splat(500.0),
                0,
                Vec2::ZERO,
            ),
            ..default()
        })
        .add_systems(Update, (inspect, update).chain());
    app.world_mut().spawn((MapCanvas, Node::default()));
    app
}

#[test]
fn an_inspector_action_rejects_replaced_ids_and_readmission_before_the_next_render() {
    let mut online = online();
    session::spawns(&mut online, vec![spawn(2, 100.0, SpawnKind::Npc)]);
    let mut view = MapView {
        admission: Some(1),
        selected: Some(entity(online.world(), 2).unwrap().identity()),
        ..default()
    };
    assert!(selected(online.world(), &view).is_some());
    session::spawns(&mut online, vec![spawn(2, 200.0, SpawnKind::Npc)]);
    assert!(selected(online.world(), &view).is_none());
    view.selected = Some(entity(online.world(), 2).unwrap().identity());
    session::admit(&mut online, 2, session::player(1));
    session::spawns(&mut online, vec![spawn(2, 100.0, SpawnKind::Npc)]);
    // Even an identical identity cannot cross admissions.
    view.selected = Some(entity(online.world(), 2).unwrap().identity());
    assert!(selected(online.world(), &view).is_none());
}

fn markers(app: &mut App) -> std::collections::BTreeMap<u16, Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &Marker)>()
        .iter(world)
        .map(|(entity, marker)| (marker.0, entity))
        .collect()
}

#[test]
fn npc_circles_and_stationary_player_arrows_keep_heading_and_search_opacity() {
    let mut app = app();
    let mut player = spawn(2, 10.0, SpawnKind::Player);
    player.position.heading = 128.0;
    session::spawns(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        vec![player, spawn(3, 20.0, SpawnKind::Npc)],
    );
    app.update();
    let drawn = markers(&mut app);
    let player = drawn[&2];
    let npc = drawn[&3];
    assert_eq!(
        app.world().get::<Node>(npc).unwrap().border_radius,
        BorderRadius::all(percent(50))
    );
    assert_eq!(
        app.world().get::<ImageNode>(npc).unwrap().color,
        Color::NONE
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(player).unwrap().0,
        Color::NONE
    );
    assert_eq!(app.world().get::<ImageNode>(player).unwrap().color, PLAYER);
    let facing = app.world().get::<UiTransform>(player).unwrap().rotation * Vec2::NEG_Y;
    assert!((facing - Vec2::NEG_X).length() < 0.001);
    session::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Position {
            spawn_id: 2,
            position: WorldPosition {
                heading: 256.0,
                ..default()
            },
            velocity: [0.0; 3],
            forward: 0.0,
        }],
    );
    app.world_mut().resource_mut::<MapView>().query = "nonmatching".into();
    app.update();
    assert_eq!(markers(&mut app), drawn);
    let facing = app.world().get::<UiTransform>(player).unwrap().rotation * Vec2::NEG_Y;
    assert!((facing - Vec2::Y).length() < 0.001);
    assert!(app.world().get::<ImageNode>(player).unwrap().color.alpha() < 0.3);
}

#[test]
fn all_known_positions_include_far_invisible_spawns_without_a_count_cap() {
    let mut app = app();
    let mut spawns: Vec<_> = (1..=350)
        .map(|id| spawn(id, f32::from(id) * 100.0, SpawnKind::Npc))
        .collect();
    spawns[1].invisible = true;
    spawns.push(spawn(400, f32::NAN, SpawnKind::Npc));
    session::spawns(&mut app.world_mut().resource_mut::<OnlineState>(), spawns);
    app.update();
    let first = markers(&mut app);
    assert_eq!(first.len(), 350); // Own player is present only once.
    assert!(first.contains_key(&2));
    assert!(!first.contains_key(&400));
    app.update();
    assert_eq!(markers(&mut app), first); // No per-frame node churn.
    let world = app.world_mut();
    let legend = &world
        .query_filtered::<&Text, With<Legend>>()
        .single(world)
        .unwrap()
        .0;
    assert!(legend.contains("1 players · 349 NPCs"));
    assert!(legend.contains("No local map lines"));
}

#[test]
fn hover_and_click_show_reported_details_and_follow_movement_without_targeting() {
    let mut app = app();
    let mut npc = spawn(2, 100.0, SpawnKind::Npc);
    npc.name = "A_test_guard001".into();
    npc.level = 12;
    npc.hp_percent = Some(73);
    session::spawns(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        vec![npc],
    );
    app.update();
    let marker = markers(&mut app)[&2];
    *app.world_mut().get_mut::<Interaction>(marker).unwrap() = Interaction::Hovered;
    app.update();
    let tooltip = &app.world().get::<Tooltip>(marker).unwrap().0;
    assert!(tooltip.contains("A test guard"));
    assert!(tooltip.contains("Level 12"));
    assert!(tooltip.contains("73%"));
    clicks::click(&mut app, marker);
    assert_eq!(app.world().resource::<MapView>().selected.unwrap().id, 2);
    let before = app.world().get::<Node>(marker).unwrap().left;
    session::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Position {
            spawn_id: 2,
            position: WorldPosition {
                x: 300.0,
                ..default()
            },
            velocity: [0.0; 3],
            forward: 0.0,
        }],
    );
    app.update();
    assert_ne!(app.world().get::<Node>(marker).unwrap().left, before);
    let world = app.world_mut();
    let details = &world
        .query_filtered::<&Text, With<Details>>()
        .single(world)
        .unwrap()
        .0;
    assert!(details.contains("X 300.0"));
    // No Outbox resource exists: inspection cannot send a target command.
    session::news(
        &mut world.resource_mut::<OnlineState>(),
        [WorldEvent::Despawn(2)],
    );
    app.update();
    assert!(!markers(&mut app).contains_key(&2));
    assert!(app.world().resource::<MapView>().selected.is_none());
}

#[test]
fn overlapping_clicks_cycle_and_reused_ids_drop_the_old_selection() {
    let mut app = app();
    session::spawns(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        vec![
            spawn(2, 100.0, SpawnKind::Player),
            spawn(3, 100.0, SpawnKind::Npc),
        ],
    );
    app.update();
    let marker = markers(&mut app)[&3];
    clicks::click(&mut app, marker);
    assert_eq!(app.world().resource::<MapView>().selected.unwrap().id, 3);
    clicks::click(&mut app, marker);
    assert_eq!(app.world().resource::<MapView>().selected.unwrap().id, 2);
    session::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Despawn(2)],
    );
    session::spawns(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        vec![spawn(2, 100.0, SpawnKind::Npc)],
    );
    app.update();
    assert!(app.world().resource::<MapView>().selected.is_none());
    clicks::click(&mut app, marker);
    let world = app.world_mut();
    let clear = world
        .query::<(Entity, &Action)>()
        .iter(world)
        .find(|(_, action)| matches!(action, Action::Clear))
        .unwrap()
        .0;
    clicks::click(&mut app, clear);
    assert!(app.world().resource::<MapView>().selected.is_none());
}

#[test]
fn unreported_values_are_not_invented() {
    let mut online = online();
    let mut npc = spawn(2, 10.0, SpawnKind::NpcCorpse);
    npc.class = None;
    npc.level = 0;
    session::spawns(&mut online, vec![npc]);
    let description = describe(
        entity(online.world(), 2).unwrap(),
        online.world(),
        &super::super::MapMotion::default(),
    );
    assert!(description.contains("NPC corpse"));
    assert!(description.contains("Level ?"));
    assert!(description.contains("HP not reported"));
    assert!(description.contains("last reported position"));
}

#[test]
fn pc_color_is_purple_and_npc_colors_follow_level_and_server_consider() {
    let mut online = online();
    let mut player = session::player(1);
    player.level = 20;
    session::admit(&mut online, 1, player);
    let mut npc = spawn(2, 10.0, SpawnKind::Npc);
    npc.level = 22;
    session::spawns(&mut online, vec![npc, spawn(3, 20.0, SpawnKind::Player)]);
    let world = online.world();
    assert_eq!(
        color(entity(world, 2).unwrap(), world, false),
        theme::con(Some(eq_client_core::combat::ConColor::Yellow))
    );
    assert_eq!(color(entity(world, 3).unwrap(), world, false), PLAYER);
    session::news(
        &mut online,
        [WorldEvent::Consideration(
            eq_client_core::combat::Consideration {
                target_id: 2,
                faction: 5,
                color: eq_client_core::combat::ConColor::Red,
                hit_points: None,
            },
        )],
    );
    let world = online.world();
    assert_eq!(
        color(entity(world, 2).unwrap(), world, false),
        theme::con(Some(eq_client_core::combat::ConColor::Red))
    );
}

#[test]
fn search_dims_nonmatches_but_watch_and_selection_keep_their_emphasis() {
    let mut online = online();
    let mut npc = spawn(2, 0.0, SpawnKind::Npc);
    npc.name = "A_test_guard001".into();
    session::spawns(&mut online, vec![npc]);
    let world = online.world();
    let npc = entity(world, 2).unwrap();
    let mut view = MapView {
        query: "spider".into(),
        ..default()
    };
    let mut watches = eq_client_core::maps::Watches::default();
    let dim = look(npc, world, &view, &watches, false, false);
    assert!(dim.fill.alpha() < 0.3);
    view.query = "GUARD".into();
    let matched = look(npc, world, &view, &watches, false, false);
    assert!(matched.fill.alpha() > 0.9 && matched.diameter > dim.diameter);
    view.query = "spider".into();
    watches.add("a test guard");
    let tracked = look(npc, world, &view, &watches, false, false);
    assert!(tracked.fill.alpha() > 0.9 && tracked.depth > dim.depth);
    assert_ne!(tracked.border, matched.border);
}
