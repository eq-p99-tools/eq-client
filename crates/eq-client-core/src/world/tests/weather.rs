use super::*;
use crate::clock::Weather;

#[test]
fn weather_keeps_explicit_clear_and_unknown_distinct_from_no_report() {
    let mut world = ClientWorld::default();
    let rain = Weather::Rain { intensity: 2 };
    assert!(game(&mut world, WorldEvent::Weather(rain)).ignored);
    game(&mut world, entered(1));
    assert!(game(&mut world, WorldEvent::Weather(rain)).ignored);
    connection(&mut world, true, false);
    assert_eq!(world.weather(), None);

    for weather in [
        rain,
        Weather::Rain { intensity: 9 },
        Weather::Snow {
            intensity: u32::MAX,
        },
        Weather::Unknown {
            kind: 7,
            intensity: 12,
        },
        Weather::Clear,
    ] {
        assert!(!game(&mut world, WorldEvent::Weather(weather)).ignored);
        assert_eq!(world.weather(), Some(weather));
    }
}

#[test]
fn transfer_and_link_loss_forget_weather_even_when_the_same_admission_resumes() {
    let mut world = admitted();
    let rain = WorldEvent::Weather(Weather::Rain { intensity: 5 });
    game(&mut world, rain.clone());
    game(
        &mut world,
        WorldEvent::ZoneTransfer(ZoneOffer {
            zone_id: 2,
            instance_id: 0,
            position: WorldPosition::default(),
            reason: 0,
            to_bind: false,
            solicited: true,
        }),
    );
    assert_eq!(world.weather(), None);
    assert!(game(&mut world, rain.clone()).ignored);
    game(
        &mut world,
        WorldEvent::ZoneTransferRejected {
            session_id: 1,
            reason: ZoneRejection::Cancelled,
        },
    );
    assert!(world.in_world());
    assert_eq!(world.weather(), None);

    game(&mut world, rain.clone());
    connection(&mut world, false, false);
    assert!(game(&mut world, rain).ignored);
    connection(&mut world, true, false);
    assert_eq!(world.weather(), None);
}

#[test]
fn a_new_admission_camp_or_session_end_cannot_inherit_weather() {
    let mut world = admitted();
    let snow = WorldEvent::Weather(Weather::Snow { intensity: 3 });
    game(&mut world, snow.clone());
    // The new admission can have the same short name.
    game(&mut world, entered(2));
    assert_eq!(world.weather(), None);
    connection(&mut world, true, false);
    game(&mut world, snow.clone());
    game(&mut world, WorldEvent::Camp(CampStatus::Camped));
    assert_eq!(world.weather(), None);

    game(&mut world, entered(3));
    connection(&mut world, true, false);
    game(&mut world, snow.clone());
    connection(&mut world, false, true);
    assert!(game(&mut world, snow).ignored);
    assert_eq!(world.weather(), None);
}
