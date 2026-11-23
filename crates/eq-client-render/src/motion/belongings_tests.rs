//! A meal or coins arriving while the player runs or jumps (gap list 3.1).
//! The session keeps the grant in force while it asks for new inputs
//! (eq-network 4b3b974), so the events the host hears are the meal's
//! inventory news, the request for inputs and, once answered, the grant
//! again. None of them may drop autorun, a held key's tap, a crouch, a
//! queued jump or a jump under way (checklist 6.451).
use super::{Controls, Crouch, online};
use bevy::prelude::*;
use eq_client_core::{
    DiagonalRates, WorldEvent, WorldUpdate,
    inventory::{InventorySlot, InventoryUpdate},
    movement::AirborneController,
};
use std::{sync::mpsc, time::Instant};

fn grant(speed: f32) -> WorldEvent {
    WorldEvent::MotionState {
        session_id: 11,
        units_per_second: Some(speed),
        backward_units_per_second: None,
        walk_units_per_second: None,
        strafe_units_per_second: None,
        strafe_walk_units_per_second: None,
        swim_backward_units_per_second: None,
        diagonal: DiagonalRates::default(),
        falls: true,
        jump_factor: 1.0,
    }
}

#[test]
fn a_meal_or_coins_arriving_keep_autorun_and_a_jump_going() {
    let mut app = crate::testing::app();
    let mut online = online::OnlineState::new(true);
    online::testing::admit(&mut online, 11, online::testing::player(7));
    let (sender, receiver) = mpsc::sync_channel(1);
    app.insert_resource(online)
        .insert_resource(online::Updates(std::sync::Mutex::new(Some(receiver))))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .add_systems(Update, online::receive);
    sender.send(WorldUpdate::Game(grant(30.0))).unwrap();
    app.update();
    let now = Instant::now();
    {
        let mut controls = app.world_mut().resource_mut::<Controls>();
        assert_eq!(controls.speed, Some(30.0));
        controls.autorun = true;
        controls.moving = true;
        controls.crouch = Some(Crouch::Over);
        controls.jump = Some(now);
        controls.airborne = Some(AirborneController::default());
        controls.taps.0.push((KeyCode::KeyW, now));
    }
    let airborne = |controls: &Controls| {
        controls
            .airborne
            .as_ref()
            .map(|jump| (jump.velocity(), jump.peak()))
    };
    let under_way = airborne(app.world().resource::<Controls>());
    assert!(under_way.is_some());
    let mut revision = 1;
    for news in [
        // The session's own auto-eat and auto-drink, as it reports them.
        vec![
            WorldEvent::Inventory(InventoryUpdate::Deduct {
                slot: InventorySlot(22),
                quantity: 1,
            }),
            WorldEvent::Inventory(InventoryUpdate::Deduct {
                slot: InventorySlot(23),
                quantity: 1,
            }),
        ],
        // Titanium's server-reported use of a unit.
        vec![WorldEvent::Inventory(InventoryUpdate::Consume {
            slot: InventorySlot(24),
            charge: false,
        })],
        // A split's share arriving in the purse.
        vec![WorldEvent::Coins(eq_client_core::Coins {
            gold: 3,
            ..eq_client_core::Coins::default()
        })],
    ] {
        revision += 1;
        for event in news.into_iter().chain([
            WorldEvent::SpeedInputsRequired {
                session_id: 11,
                revision,
            },
            // The answer: the grant again, a little slower.
            grant(27.0),
        ]) {
            sender.send(WorldUpdate::Game(event)).unwrap();
            app.update();
            let controls = app.world().resource::<Controls>();
            assert!(controls.autorun, "{revision}");
            assert!(controls.moving, "{revision}");
            assert_eq!(controls.crouch, Some(Crouch::Over), "{revision}");
            assert_eq!(controls.jump, Some(now), "{revision}");
            assert_eq!(airborne(controls), under_way, "{revision}");
            assert_eq!(controls.taps.0.len(), 1, "{revision}");
        }
        // The answer's slower speed is in force.
        assert_eq!(
            app.world().resource::<Controls>().speed,
            Some(27.0),
            "{revision}"
        );
    }
}
