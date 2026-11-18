use super::*;
use crate::boats::{Action, ControlBoat, ControlState, Vessel};

#[test]
fn a_raw_control_echo_is_only_an_observation_and_old_admission_results_are_ignored() {
    let mut world = admitted();
    let packet = ControlBoat {
        boat_id: 8,
        action: Action::Acquire,
        unknown: [1, 2, 3],
    };
    game(&mut world, WorldEvent::BoatControl(packet));
    assert_eq!(world.boat_control_notice(), Some(packet));
    assert_eq!(world.boat_control(), ControlState::Unknown);
    game(
        &mut world,
        WorldEvent::BoatControlState {
            session_id: 0,
            state: ControlState::Acquired(Vessel {
                spawn_id: 8,
                incarnation: 4,
            }),
        },
    );
    assert_eq!(world.boat_control(), ControlState::Unknown);
}

#[test]
fn replacement_disconnect_and_new_admission_withdraw_acquired_knowledge() {
    let mut world = admitted();
    let vessel = Vessel {
        spawn_id: 8,
        incarnation: 4,
    };
    game(&mut world, WorldEvent::Spawns(vec![spawn(8)]));
    game(
        &mut world,
        WorldEvent::BoatControlState {
            session_id: 1,
            state: ControlState::Acquired(vessel),
        },
    );
    assert_eq!(world.boat_control(), ControlState::Acquired(vessel));
    game(&mut world, WorldEvent::Spawns(vec![spawn(8)]));
    assert_eq!(world.boat_control(), ControlState::Unconfirmed(vessel));
    connection(&mut world, false, true);
    assert_eq!(world.boat_control(), ControlState::Unknown);
    game(&mut world, entered(2));
    assert_eq!(world.boat_control(), ControlState::Unknown);
    assert!(world.boat_control_notice().is_none());
}

#[test]
fn an_arrival_aboard_holds_until_placed_and_a_new_admission_forgets_it() {
    use crate::boats::riders::Arrival;
    let mut world = admitted();
    let waiting = Arrival::Waiting {
        name: "SirensBane00".into(),
    };
    // Another admission's word changes nothing.
    game(
        &mut world,
        WorldEvent::Arrival {
            session_id: 0,
            arrival: waiting.clone(),
        },
    );
    assert!(world.arrival().is_none() && !world.arrival_held());
    game(
        &mut world,
        WorldEvent::Arrival {
            session_id: 1,
            arrival: waiting.clone(),
        },
    );
    assert_eq!(world.arrival(), Some(&waiting));
    assert!(world.arrival_held());
    let placed = Arrival::SafePoint {
        position: WorldPosition::default(),
    };
    game(
        &mut world,
        WorldEvent::Arrival {
            session_id: 1,
            arrival: placed.clone(),
        },
    );
    assert_eq!(world.arrival(), Some(&placed));
    assert!(!world.arrival_held());
    game(
        &mut world,
        WorldEvent::Arrival {
            session_id: 1,
            arrival: waiting,
        },
    );
    game(&mut world, entered(2));
    assert!(world.arrival().is_none() && !world.arrival_held());
}
