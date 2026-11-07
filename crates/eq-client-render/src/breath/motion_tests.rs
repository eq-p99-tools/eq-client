use super::*;
use crate::{motion::Controls, online::testing, systems};
use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
use eq_client_assets::regions::{Liquid, ZoneRegions};
use eq_client_core::{
    BaseAttributes, ItemHitPoints, MovementRequest, WorldEvent, WorldUpdate,
    hazards::TitaniumEnvironment, inventory::InventoryUpdate,
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
            diagonal: default(),
            falls: false,
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
    crate::motion::tests::age_pending(&mut app);
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
            position: pending.position,
            refused: None,
        },
    );
    app.update();
    assert!(commands.try_recv().is_err());
    // A fresh observation restores the view of the same reserve and starts
    // a new interval. The unresolved gap neither refills nor replays ticks.
    assert_eq!(app.world().resource::<State>().air().unwrap().remaining, 0);
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
    testing::admit(&mut online, 11, player);
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
            WorldEvent::LiquidHazardRevision {
                session_id: 11,
                revision: 9,
            },
        ],
    );
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
