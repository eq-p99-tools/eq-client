use super::*;

fn locked(online: &OnlineState, now: Instant) -> Assist {
    let mut assist = Assist::default();
    assist.observe(online.world(), true, Some(1), now);
    assert!(
        assist
            .steer(
                online.world(),
                &plane(),
                &online.regions,
                &body(online),
                OrbitInput::default(),
                now
            )
            .is_some()
    );
    assert_eq!(assist.status().0, "locked");
    assist
}

#[test]
fn melee_lock_release_requires_explicit_rearm_and_pending_choice_waits_for_authority() {
    let mut online = world(10.);
    let now = Instant::now();
    let mut assist = locked(&online, now);
    assist.release("forward_or_back");
    select(&mut online, 2);
    assist.observe(online.world(), true, Some(1), now);
    assert_eq!(assist.status(), ("released", "forward_or_back"));
    assert_eq!(assist.target_id(), Some(5));
    assert!(
        assist
            .steer(
                online.world(),
                &plane(),
                &online.regions,
                &body(&online),
                OrbitInput::default(),
                now
            )
            .is_none()
    );
    // An actual new player choice is a rearm, even choosing the same target.
    online.select_target(Some(5));
    assist.observe(online.world(), true, Some(1), now);
    assert_eq!(assist.status(), ("suspended", "target_pending"));
    assist.resume(now);
    assert!(
        assist
            .steer(
                online.world(),
                &plane(),
                &online.regions,
                &body(&online),
                OrbitInput::default(),
                now
            )
            .is_none()
    );
    assert_eq!(assist.status(), ("suspended", "target_pending"));
    select(&mut online, 3);
    assist.observe(online.world(), true, Some(1), now);
    assist.resume(now);
    assert!(
        assist
            .steer(
                online.world(),
                &plane(),
                &online.regions,
                &body(&online),
                OrbitInput::default(),
                now
            )
            .is_some()
    );
    assert_eq!(assist.status().0, "locked");
    assist.release("autorun");
    assist.observe(online.world(), true, Some(2), now);
    assert!(assist.active());
    assist.observe(online.world(), false, Some(2), now);
    assert_eq!(assist.status().0, "disabled");
    assist.observe(online.world(), true, Some(2), now);
    assert!(assist.active());
}

#[test]
fn melee_lock_invalidated_identity_never_reacquires_from_received_repetition() {
    let mut online = world(10.);
    let now = Instant::now();
    let mut assist = locked(&online, now);
    testing::spawn_entry(&mut online, 5, spawn(10.));
    assist.observe(online.world(), true, Some(1), now);
    assert_eq!(assist.status().0, "invalidated");
    assert!(!assist.owns_facing() && assist.target_id().is_none());
    select(&mut online, 2);
    assist.observe(online.world(), true, Some(1), now);
    assert_eq!(assist.status().0, "invalidated");
    assist.observe(online.world(), true, None, now);
    assert_eq!(assist.status().0, "invalidated");
    assist.observe(online.world(), true, Some(2), now);
    assert!(assist.active());
}

#[test]
fn melee_lock_suspension_keeps_scope_but_never_accumulates_motion_credit() {
    let online = world(10.);
    let now = Instant::now();
    let mut assist = locked(&online, now);
    assist.suspend("casting");
    assert!(!assist.owns_facing() && assist.camera_locked(online.world(), now));
    assert_eq!(assist.admission_id(), Some(11));
    assist.yield_input(now);
    assert!(
        assist
            .steer(
                online.world(),
                &plane(),
                &online.regions,
                &body(&online),
                OrbitInput::default(),
                now + Duration::from_millis(499)
            )
            .is_none()
    );
    let later = now + Duration::from_secs(10);
    assist.resume(later);
    let input = OrbitInput {
        side: 1.,
        strafe: 3.,
        ..OrbitInput::default()
    };
    let plan = assist
        .steer(
            online.world(),
            &plane(),
            &online.regions,
            &body(&online),
            input,
            later,
        )
        .unwrap()
        .orbit
        .unwrap();
    assert!(plan.motion.speed * body(&online).seconds <= 0.301);
    assert!(
        super::super::super::arc(body(&online).position.heading, plan.heading).abs()
            <= motion::TURN_RATE * body(&online).seconds
    );
}

#[test]
fn melee_orbit_uses_current_geometry_and_refuses_zone_or_carrier_transitions() {
    let now = Instant::now();
    let online = world(10.);
    let input = OrbitInput {
        side: 1.,
        strafe: 3.,
        ..OrbitInput::default()
    };
    let mut assist = locked(&online, now);
    let line = eq_client_assets::regions::ZoneRegions::test_zone_line_slab(
        -0.3,
        0.3,
        eq_client_assets::regions::ZoneLine::Reference(7),
    );
    assert!(
        assist
            .steer(online.world(), &plane(), &line, &body(&online), input, now)
            .is_none()
    );
    assert_eq!(assist.status().0, "suspended");
    let mut assist = locked(&online, now);
    assert!(
        assist
            .steer(
                online.world(),
                &transitions::deck(true),
                &online.regions,
                &body(&online),
                input,
                now
            )
            .is_none()
    );
    assert_eq!(assist.status().0, "suspended");
}

#[test]
fn melee_orbit_body_sweep_stops_a_lateral_wall_and_overlap_never_forces_backoff() {
    let now = Instant::now();
    let online = world(10.);
    let input = OrbitInput {
        side: 1.,
        strafe: 3.,
        ..OrbitInput::default()
    };
    let collision = eq_client_core::movement::CollisionWorld::new([
        [[-300., 0., -300.], [300., 0., 300.], [300., 0., -300.]],
        [[-300., 0., -300.], [-300., 0., 300.], [300., 0., 300.]],
        [[-20., 0., 0.6], [20., 20., 0.6], [20., 0., 0.6]],
        [[-20., 0., 0.6], [-20., 20., 0.6], [20., 20., 0.6]],
    ])
    .unwrap();
    let mut assist = locked(&online, now);
    assert!(
        assist
            .steer(
                online.world(),
                &collision,
                &online.regions,
                &body(&online),
                input,
                now
            )
            .is_none()
    );
    assert_eq!(assist.status(), ("suspended", "orbit_obstructed"));
    let overlapping = world(0.4);
    let mut assist = Assist::default();
    assist.observe(overlapping.world(), true, Some(1), now);
    assert!(
        assist
            .steer(
                overlapping.world(),
                &plane(),
                &overlapping.regions,
                &body(&overlapping),
                input,
                now
            )
            .is_none()
    );
    assert_eq!(assist.status(), ("suspended", "orbit_radius"));
}

#[test]
fn melee_orbit_larger_body_and_height_keep_the_current_reach_and_turn_budget() {
    let online = world_with_target(SpawnState {
        size: 10.,
        ..spawn(18.)
    });
    let mut body = body(&online);
    body.height = 10.;
    let now = Instant::now();
    let mut assist = Assist::default();
    assist.observe(online.world(), true, Some(1), now);
    let input = OrbitInput {
        side: -1.,
        strafe: 3.,
        turn_budget: Some(1.),
        ..OrbitInput::default()
    };
    let plan = assist
        .steer(online.world(), &plane(), &online.regions, &body, input, now)
        .unwrap()
        .orbit
        .unwrap();
    assert!(assist.radius().is_some_and(|radius| radius < 20.));
    assert!(plan.motion.speed <= 3.);
    assert!(motion::arc(body.position.heading, plan.heading).abs() <= 1.001);
}
