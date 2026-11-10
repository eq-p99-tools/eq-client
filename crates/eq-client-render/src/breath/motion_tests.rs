use super::*;
use crate::{Collision, OrbitCamera, motion::Controls, online::testing, systems};
use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
use eq_client_assets::regions::{Liquid, ZoneRegions};
use eq_client_core::{
    BaseAttributes, ItemHitPoints, MovementMode, MovementRequest, WorldEvent, WorldUpdate,
    hazards::TitaniumEnvironment,
    inventory::{InventoryUpdate, MoveRules},
    movement::CollisionWorld,
    world_position,
};
use std::{
    num::NonZeroU32,
    sync::{Mutex, mpsc},
    time::Duration,
};

#[test]
fn worker_result_and_revision_are_visible_to_breath_in_the_same_frame() {
    let (mut app, commands, updates) = scheduled();
    let placement = prepare_submerged(&mut app);
    app.update();
    let pending = next_move(&commands);
    due(&mut app, placement);
    // The worker publishes both the committed XYZ and its new liquid inputs
    // through the normal receive queue, while the drawn body is still at origin.
    push(
        &updates,
        WorldEvent::MotionSent {
            session_id: 11,
            created: pending.created,
            position: pending.position,
            refused: None,
        },
    );
    push(
        &updates,
        WorldEvent::LiquidHazardRevision {
            session_id: 11,
            revision: 10,
        },
    );
    app.update();
    assert_report(&commands, 10, pending.position);
}

#[test]
fn due_lava_heat_waits_for_the_matching_move_in_the_update_schedule() {
    let (mut app, commands, updates) = scheduled();
    prepare_submerged(&mut app);
    app.world_mut().resource_mut::<OnlineState>().regions =
        ZoneRegions::test_liquid_surface(4.0, Liquid::Lava);
    app.update();
    let pending = next_move(&commands);
    let started = Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
    app.world_mut()
        .resource_scope(|world, mut state: Mut<State>| {
            state.heat = lava::Heat::default();
            assert_eq!(
                state.heat.advance(
                    started,
                    world.resource::<OnlineState>(),
                    &world.resource::<OptionsState>().options.qol,
                    world.resource::<SpellNames>(),
                    LiquidBasis::Observed,
                ),
                None
            );
        });
    let created = crate::motion::tests::age_pending(&mut app);
    app.update();
    assert!(commands.try_recv().is_err());

    push(
        &updates,
        WorldEvent::MotionSent {
            session_id: 11,
            created,
            position: pending.position,
            refused: None,
        },
    );
    push(
        &updates,
        WorldEvent::LiquidHazardRevision {
            session_id: 11,
            revision: 10,
        },
    );
    app.update();
    let ClientCommand::LiquidDamage {
        session_id,
        hazard,
        amount,
        observation,
    } = commands.try_recv().unwrap()
    else {
        panic!("expected one lava report")
    };
    assert_eq!((session_id, hazard, amount), (11, LiquidHazard::Lava, 42));
    assert_eq!(observation.revision, 10);
    assert_eq!(observation.position, pending.position);
    app.update();
    assert!(
        commands
            .try_iter()
            .all(|command| !matches!(command, ClientCommand::LiquidDamage { .. }))
    );
}

#[test]
fn a_due_observation_goes_before_a_new_move_in_the_input_stage() {
    let (mut app, commands, _updates) = scheduled();
    let placement = prepare_submerged(&mut app);
    let origin = app
        .world()
        .resource::<OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    due(&mut app, placement);
    app.update();
    assert_report(&commands, 9, origin);
    assert_ne!(next_move(&commands).position, origin);
    assert!(commands.try_recv().is_err());
}

#[test]
fn a_full_outbox_never_puts_an_unsent_move_in_flight() {
    let (mut app, _commands, _updates) = scheduled();
    let placement = prepare_submerged(&mut app);
    let origin = app
        .world()
        .resource::<OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    let (sender, commands) = mpsc::sync_channel(1);
    sender
        .send(ClientCommand::SelectTarget {
            request_id: 1,
            session_id: 11,
            spawn_id: None,
            expected_pet: None,
        })
        .unwrap();
    app.insert_resource(Outbox::new(Some(sender)));
    app.update();
    assert!(matches!(
        commands.try_recv(),
        Ok(ClientCommand::SelectTarget { .. })
    ));
    assert!(commands.try_recv().is_err());

    due(&mut app, placement);
    app.update();
    assert_report(&commands, 9, origin);
}

#[test]
fn only_the_matching_worker_refusal_releases_a_move_after_a_grant_reset() {
    let (mut app, commands, updates) = scheduled();
    let placement = prepare_submerged(&mut app);
    let origin = app
        .world()
        .resource::<OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    app.update();
    let pending = next_move(&commands);
    due(&mut app, placement);

    push(
        &updates,
        WorldEvent::MotionState {
            session_id: 11,
            units_per_second: Some(6.0),
            backward_units_per_second: None,
            walk_units_per_second: None,
            strafe_units_per_second: None,
            strafe_walk_units_per_second: None,
            swim_backward_units_per_second: None,
            diagonal: default(),
            falls: false,
            jump_factor: 1.0,
        },
    );
    for (session_id, created) in [
        (10, pending.created),
        (11, pending.created + Duration::from_millis(1)),
    ] {
        push(
            &updates,
            WorldEvent::MotionRefused {
                session_id,
                created,
                reason: "Unrelated refusal".into(),
            },
        );
    }
    app.update();
    assert!(commands.try_recv().is_err());
    assert!(matches!(
        app.world().resource::<State>().probe(),
        Some((
            None,
            Err(Unavailable::MovementPending | Unavailable::MovementUnconfirmed)
        ))
    ));
    assert!(app.world().resource::<Controls>().refused.is_none());

    // Seed an eligible tick separately from the pending frame above, so the
    // refusal test is independent of wall-clock time spent running the fixture.
    due(&mut app, placement);
    push(
        &updates,
        WorldEvent::MotionRefused {
            session_id: 11,
            created: pending.created,
            reason: "The player cannot move yet".into(),
        },
    );
    app.update();
    assert_report(&commands, 9, origin);
    assert_eq!(
        app.world().resource::<Controls>().refused.as_deref(),
        Some("The player cannot move yet")
    );
}

#[test]
fn an_overdue_move_withholds_the_stale_gauge_and_recovers_on_a_late_result() {
    let (mut app, commands, updates) = scheduled();
    let placement = prepare_submerged(&mut app);
    app.update();
    let pending = next_move(&commands);
    due(&mut app, placement);
    let aged = crate::motion::tests::age_pending(&mut app);
    app.update();
    assert!(commands.try_recv().is_err());
    let state = app.world().resource::<State>();
    assert!(state.air().is_none());
    assert_eq!(
        state.probe(),
        Some((None, Err(Unavailable::MovementUnconfirmed)))
    );

    push(
        &updates,
        WorldEvent::MotionSent {
            session_id: 10,
            created: aged,
            position: pending.position,
            refused: None,
        },
    );
    app.update();
    assert!(commands.try_recv().is_err());
    assert!(app.world().resource::<State>().air().is_none());

    push(
        &updates,
        WorldEvent::MotionSent {
            session_id: 11,
            created: pending.created,
            position: pending.position,
            refused: None,
        },
    );
    app.update();
    assert!(commands.try_recv().is_err());
    assert!(app.world().resource::<Controls>().waiting());
    assert!(app.world().resource::<State>().air().is_none());

    push(
        &updates,
        WorldEvent::MotionSent {
            session_id: 11,
            created: aged,
            position: pending.position,
            refused: None,
        },
    );
    app.update();
    assert_report(&commands, 9, pending.position);
    assert!(commands.try_recv().is_err());
    // A fresh observation takes the due check once, using the same reserve.
    assert_eq!(app.world().resource::<State>().air().unwrap().remaining, 0);
    app.update();
    assert!(
        commands
            .try_iter()
            .all(|command| !matches!(command, ClientCommand::LiquidDamage { .. }))
    );
}

#[test]
fn p99_swim_samples_the_native_head_after_the_worker_result() {
    let (mut app, commands, updates) = scheduled();
    let (surface, placement) = prepare_p99_surface(&mut app);

    app.update();
    let dive = next_move(&commands);
    assert_eq!(dive.mode, MovementMode::Swim);
    assert!(dive.position.z < surface.z - 0.025);
    assert_eq!(
        app.world().resource::<State>().probe(),
        Some((Some(Medium::Air), Ok(Sample::CanBreathe))),
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.update();
    assert!(app.world().resource::<State>().air().is_none());
    assert_eq!(
        app.world().resource::<State>().probe(),
        Some((None, Err(Unavailable::MovementPending))),
    );
    // Subsequent sampling should not depend on how long it takes the test
    // runner to deliver the result. A withdrawn stroke grant still allows air.
    app.world_mut().resource_mut::<Controls>().speed = None;
    push(
        &updates,
        WorldEvent::MotionSent {
            session_id: 11,
            created: dive.created,
            position: dive.position,
            refused: None,
        },
    );
    app.update();
    let state = app.world().resource::<State>();
    assert!(matches!(
        state.probe(),
        Some((Some(Medium::Water), Ok(Sample::Submerged(_))))
    ));
    assert!(state.air().is_some());
    due(&mut app, placement);
    app.update();
    assert_eq!(app.world().resource::<State>().air().unwrap().remaining, 0);
    assert!(commands.try_recv().is_err());
    let online = app.world().resource::<OnlineState>();
    assert_eq!(online.server_placement().unwrap().position, surface);
    assert_eq!(online.world().hit_points(), Some((80, 80)));

    app.world_mut().resource_mut::<OnlineState>().regions = ZoneRegions::default();
    app.update();
    let state = app.world().resource::<State>();
    assert_eq!(
        state.probe(),
        Some((None, Err(Unavailable::UnresolvedMedium)))
    );
    assert!(state.air().is_none());

    app.world_mut().resource_mut::<OnlineState>().regions =
        ZoneRegions::test_liquid_surface(0.0, Liquid::Water);
    push(
        &updates,
        WorldEvent::Position {
            spawn_id: 7,
            position: surface,
            velocity: [0.0; 3],
            forward: 0.0,
        },
    );
    app.update();
    assert_eq!(
        app.world().resource::<State>().probe(),
        Some((Some(Medium::Air), Ok(Sample::CanBreathe))),
    );
    assert!(app.world().resource::<State>().air().is_none());
    assert!(commands.try_recv().is_err());
}

fn scheduled() -> (
    App,
    mpsc::Receiver<ClientCommand>,
    mpsc::Sender<WorldUpdate>,
) {
    let (mut app, commands) = crate::motion::tests::fixture();
    let (sender, updates) = mpsc::channel();
    app.insert_resource(crate::online::Updates(Mutex::new(Some(updates))))
        .init_resource::<State>()
        .init_resource::<Shown>()
        .init_resource::<SpellNames>()
        .init_resource::<crate::notices::Lines>()
        .init_resource::<crate::inventory::InventoryState>()
        .init_resource::<crate::combat::CombatState>()
        .init_resource::<crate::trade::TradeState>()
        .init_resource::<crate::items::ItemState>()
        .init_resource::<crate::sounds::Sounds>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>();
    systems::configure_stages(&mut app);
    app.add_systems(
        Update,
        (systems::liquid_and_movement(), systems::receive_world()),
    );
    // Fail if the production configuration leaves conflicting systems unordered,
    // even if the executor happens to pick the expected order in this run.
    app.edit_schedule(Update, |schedule| {
        schedule.set_build_settings(ScheduleBuildSettings {
            ambiguity_detection: LogLevel::Error,
            ..default()
        });
    });
    (app, commands, sender)
}

fn due(app: &mut App, placement: u64) {
    let started = Instant::now().checked_sub(Duration::from_secs(3)).unwrap();
    let wet = Sample::Submerged(NonZeroU32::MIN);
    let mut state = app.world_mut().resource_mut::<State>();
    state.air = AirState::default();
    state
        .air
        .observe(started, (11, 7), placement, wet, Some(80));
    state.air.observe(
        started + Duration::from_secs(1),
        (11, 7),
        placement,
        wet,
        Some(80),
    );
    assert_eq!(state.air().unwrap().remaining, 0);
}

fn push(updates: &mpsc::Sender<WorldUpdate>, event: WorldEvent) {
    updates.send(WorldUpdate::Game(event)).unwrap();
}

fn assert_report(
    commands: &mpsc::Receiver<ClientCommand>,
    revision: u64,
    position: eq_client_core::WorldPosition,
) {
    let ClientCommand::LiquidDamage {
        session_id,
        hazard,
        amount,
        observation,
    } = commands.try_recv().unwrap()
    else {
        panic!("expected a liquid report");
    };
    assert_eq!(
        (session_id, hazard, amount),
        (11, LiquidHazard::Drowning, 10)
    );
    assert_eq!(observation.revision, revision);
    assert_eq!(observation.position, position);
}

fn prepare_submerged(app: &mut App) -> u64 {
    prepare_liquid(app, None)
}

fn prepare_p99_surface(app: &mut App) -> (eq_client_core::WorldPosition, u64) {
    prepare_liquid(
        app,
        Some(vec![
            Capability::Inventory,
            Capability::Moving,
            Capability::Swimming,
            Capability::EnvironmentalDamage,
            Capability::GameplayDepartures,
        ]),
    );
    let regions = ZoneRegions::test_liquid_surface(0.0, Liquid::Water);
    let pool = CollisionWorld::new([[
        [-20.0, -20.0, -20.0],
        [20.0, -20.0, -20.0],
        [0.0, -20.0, 20.0],
    ]])
    .unwrap()
    .with_liquids(crate::scene_collision::ZoneLiquids(
        regions.clone().with_titanium_liquids(),
    ))
    .with_swim_origin(3.0);
    app.insert_resource(Collision(Some(pool)));
    let world = app.world_mut();
    world
        .query::<&mut OrbitCamera>()
        .single_mut(world)
        .unwrap()
        .pitch = -1.0;

    let basis = head::reference("[Example]", "Example").unwrap();
    let mut online = world.resource_mut::<OnlineState>();
    let player = online.world().player().unwrap();
    let eye = basis.for_player(online.world(), player).unwrap();
    let surface = world_position([0.0, 0.025 - eye, 0.0], 0.0);
    online.regions = regions;
    testing::place(&mut online, surface);
    (surface, online.placement_revision())
}

fn prepare_liquid(app: &mut App, capabilities: Option<Vec<Capability>>) -> u64 {
    let local_reports = capabilities.is_none();
    let mut online = app.world_mut().resource_mut::<OnlineState>();
    let mut player = online.world().player().unwrap().clone();
    player.base_attributes = Some(BaseAttributes {
        stamina: 40,
        ..default()
    });
    player.titanium_environment = Some(TitaniumEnvironment {
        zone_id: 27,
        base_fire_resistance: 25,
        has_alternate_advancement: false,
    });
    if let Some(capabilities) = capabilities {
        online.plan_inventory_as(Some(MoveRules::EqEmu));
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                session_id: 11,
                capabilities,
                choices: Vec::new(),
                zone: "example".to_owned(),
                player: Box::new(player),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        testing::connect(&mut online, true);
    } else {
        testing::admit(&mut online, 11, player);
    }
    testing::inventory(&mut online, InventoryUpdate::Snapshot(Vec::new()));
    testing::buffs(&mut online, default());
    let description = eq_client_core::zones::titanium_description(&[0; 700]).unwrap();
    testing::news(
        &mut online,
        [
            WorldEvent::ZoneDescription(description),
            WorldEvent::HitPoints {
                spawn_id: 7,
                current: 80,
                maximum: 80,
                items: ItemHitPoints::LeftOutOfCurrent,
            },
        ],
    );
    if local_reports {
        testing::news(
            &mut online,
            [WorldEvent::LiquidHazardRevision {
                session_id: 11,
                revision: 9,
            }],
        );
    }
    online.regions = ZoneRegions::test_liquid_surface(10.0, Liquid::Water);
    let placement = online.placement_revision();
    let world = app.world_mut();
    let entity = world
        .query_filtered::<Entity, With<Player>>()
        .single(world)
        .unwrap();
    world
        .entity_mut(entity)
        .insert(head::reference("[Example]", "Example").unwrap());
    world
        .resource_mut::<OptionsState>()
        .options
        .qol
        .set(Fix::TakeEnvironmentalDamage, true);
    placement
}

fn next_move(commands: &mpsc::Receiver<ClientCommand>) -> MovementRequest {
    let ClientCommand::Move(request) = commands.try_recv().unwrap() else {
        panic!("expected another movement command");
    };
    request
}

#[test]
fn titanium_backs_up_in_water_only_at_the_grants_rate() {
    use crate::keys::{Act, Chord};
    for (rate, moves) in [(Some(0.0), false), (Some(6.0), true), (None, true)] {
        let (mut app, commands, _updates) = scheduled();
        prepare_p99_surface(&mut app);
        crate::keys::testing::bind(&mut app, Act::Back, Chord::key(KeyCode::KeyB));
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::KeyB);
        }
        {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.backward_speed = Some(6.0);
            controls.swim_backward_speed = rate;
        }
        let origin = app
            .world()
            .resource::<OnlineState>()
            .world()
            .player()
            .unwrap()
            .position;
        app.update();
        let backing = next_move(&commands);
        assert_eq!(backing.mode, MovementMode::Swim, "{rate:?}");
        let moved = (backing.position.x - origin.x).hypot(backing.position.y - origin.y);
        assert_eq!(moved > 0.0, moves, "{rate:?}: {moved}");
    }
}
