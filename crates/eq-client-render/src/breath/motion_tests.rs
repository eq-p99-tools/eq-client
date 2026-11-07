use super::*;
use crate::{motion::Controls, online::testing};
use eq_client_assets::regions::{Liquid, ZoneRegions};
use eq_client_core::{
    BaseAttributes, ItemHitPoints, MovementRequest, WorldEvent, hazards::TitaniumEnvironment,
    inventory::InventoryUpdate,
};
use std::{num::NonZeroU32, time::Duration};

#[test]
fn due_liquid_report_follows_delayed_move_result_and_precedes_the_next_move() {
    let (mut app, commands) = crate::motion::tests::fixture();
    app.init_resource::<State>()
        .init_resource::<Shown>()
        .init_resource::<SpellNames>()
        .add_systems(Update, crate::systems::liquid_and_movement());
    let placement = prepare_submerged(&mut app);
    app.update();
    let mut pending = next_move(&commands);
    for _ in 0..2 {
        app.update();
        assert!(commands.try_recv().is_err());
        receive_move(&mut app, &pending);
        app.update();
        let next = next_move(&commands);
        assert_ne!(next.position, pending.position);
        pending = next;
    }

    // Put the existing counter at zero with a tick already due. The latest
    // submitted movement still has no worker result.
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
    for _ in 0..3 {
        app.update();
        assert!(commands.try_recv().is_err());
        assert_eq!(
            app.world().resource::<State>().probe(),
            Some((None, Err(Unavailable::MovementPending)))
        );
    }

    // The worker's echo makes the due observation and another move eligible
    // in the same frame. The real schedule puts the observation first.
    receive_move(&mut app, &pending);
    app.update();
    let ClientCommand::LiquidDamage {
        session_id,
        hazard,
        amount,
        observation,
    } = commands.try_recv().unwrap()
    else {
        panic!("expected a liquid report before the next move");
    };
    assert_eq!(
        (session_id, hazard, amount),
        (11, LiquidHazard::Drowning, 10)
    );
    assert_eq!(observation.revision, 9);
    assert_eq!(observation.position, pending.position);
    assert_ne!(next_move(&commands).position, observation.position);
    assert!(commands.try_recv().is_err());
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

fn next_move(commands: &std::sync::mpsc::Receiver<ClientCommand>) -> MovementRequest {
    let ClientCommand::Move(request) = commands.try_recv().unwrap() else {
        panic!("expected another movement command");
    };
    request
}

fn receive_move(app: &mut App, request: &MovementRequest) {
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::MotionSent {
            session_id: request.session_id,
            position: request.position,
            refused: None,
        }],
    );
    let mut motion = app.world_mut().resource_mut::<Controls>();
    motion.accepted();
    motion.last_accepted = Instant::now()
        .checked_sub(Duration::from_millis(200))
        .unwrap();
}
