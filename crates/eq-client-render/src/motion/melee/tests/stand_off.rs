use super::*;

#[test]
fn melee_stand_off_reserves_granted_travel_and_observed_recession_inside_both_bodies() {
    let mut online = world(11.5);
    let mut body = Body {
        speeds: (29.866_667, 12.800_001),
        ..body(&online)
    };
    let mut assist = Assist::default();
    let now = Instant::now();
    let first = assist
        .step(online.world(), &plane(), &online.regions, &body, now)
        .unwrap();
    assert!(
        first.forward && first.run,
        "old in-range edge must close inward"
    );
    assert!((first.beyond - (11.5 - (12. - 29.866_667 * 0.1))).abs() < 0.004);
    // Re-reading the unchanged spawn at render cadence must not turn the
    // next network position into a two-unit jump over a single millisecond.
    for ms in [10, 50, 99] {
        assist
            .step(
                online.world(),
                &plane(),
                &online.regions,
                &body,
                now + Duration::from_millis(ms),
            )
            .unwrap();
    }
    move_target(&mut online, 13.5);
    let receding = assist
        .step(
            online.world(),
            &plane(),
            &online.regions,
            &body,
            now + Duration::from_millis(100),
        )
        .unwrap();
    // Twenty units/s of actual target displacement during the last100ms.
    let expected_stop = 12. - (29.866_667 + 20.) * 0.1;
    assert!((receding.beyond - (13.5 - expected_stop)).abs() < 0.004);
    assert!(expected_stop > 6. && expected_stop < 11.);
    let between_updates = assist
        .step(
            online.world(),
            &plane(),
            &online.regions,
            &body,
            now + Duration::from_millis(150),
        )
        .unwrap();
    assert!((between_updates.beyond - receding.beyond).abs() < 0.004);
    body.seconds = 0.25;
    let capped = assist
        .step(
            online.world(),
            &plane(),
            &online.regions,
            &body,
            now + Duration::from_millis(200),
        )
        .unwrap();
    assert!((capped.beyond - (13.5 - 6.)).abs() < 0.004);
    let small = abilities::Body {
        race: 1,
        size: 6.,
        x: 0.,
        y: 0.,
    };
    let larger = abilities::Body { size: 10., ..small };
    assert!((reach(larger, small) - 20.).abs() < 0.004);
    assert!((reach(small, larger) - 20.).abs() < 0.004);
}

#[test]
fn melee_moving_target_reopens_before_outer_reach_but_stationary_target_does_not() {
    let mut online = world(5.);
    let body = Body {
        speeds: (29.866_667, 12.800_001),
        ..body(&online)
    };
    let now = Instant::now();
    let mut assist = Assist::default();
    assert!(
        !assist
            .step(online.world(), &plane(), &online.regions, &body, now)
            .unwrap()
            .forward
    );
    move_target(&mut online, 10.8);
    assert!(
        assist
            .step(
                online.world(),
                &plane(),
                &online.regions,
                &body,
                now + Duration::from_millis(100)
            )
            .unwrap()
            .forward
    );
    let mut stationary = Assist::default();
    move_target(&mut online, 5.);
    assert!(
        !stationary
            .step(online.world(), &plane(), &online.regions, &body, now)
            .unwrap()
            .forward
    );
    move_target(&mut online, 10.8);
    // Same-time placement has no velocity observation; keep outer hysteresis.
    assert!(
        !stationary
            .step(online.world(), &plane(), &online.regions, &body, now)
            .unwrap()
            .forward
    );
}
