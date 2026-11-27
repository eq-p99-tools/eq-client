//! Direct pursuit stays direct on supported ground. Only an unsafe direct
//! path invokes the same route planner used by ordinary scripted walking.
use super::{
    Duration, Following, SEARCH_BUDGET, Script, Step, Vec2, Vec3, debug, drops, spawn_goal, walking,
};
use crate::{PlayerBody, online::OnlineState};
use eq_client_assets::ui::InstalledClient;
use eq_client_core::movement::{AirborneController, RouteStep};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Edges {
    Grounded,
    WithDrops,
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one direct-versus-routed chase transition with its observed inputs"
)]
pub(super) fn aim(
    script: &mut Script,
    online: &OnlineState,
    client: InstalledClient,
    body: PlayerBody,
    collision: &eq_client_core::movement::CollisionWorld,
    mesh: Option<&crate::zone_navmesh::ZoneNavMesh>,
    airborne: Option<&AirborneController>,
    carried: eq_client_core::WorldPosition,
    range: f32,
    now: Duration,
) -> Result<Option<eq_client_core::WorldPosition>, String> {
    let world = online.world();
    let id = script.awaited.ok_or("chase needs its original target")?;
    let spawn = world.spawn(id).ok_or("chase lost its original target")?;
    if script.chase_actor
        != world
            .session_id()
            .map(|admission| (admission, spawn.revision))
    {
        return Err("chase target or admission was replaced".into());
    }
    let player = world.player().ok_or("chase needs a current player")?;
    let height =
        crate::posture::movement_height(client, world, &body, world.posture(player.spawn_id));
    let pose_collision = collision.clone().with_collision_height(height);
    let collision = &pose_collision;
    let feet = Vec3::from_array(eq_client_core::render_position(player.position))
        - Vec3::Y * body.feet_offset;
    let target = &spawn.state;
    let goal = spawn_goal(
        Some(collision),
        Vec3::from_array(eq_client_core::render_position(carried)),
        crate::entities::Drawn::of(client, world, (target.race, target.gender, target.size)),
    );
    let distance = Vec2::new(goal.x - feet.x, goal.z - feet.z).length();
    if distance <= range && (goal.y - feet.y).abs() <= eq_client_core::movement::MAX_GROUNDED_STEP {
        script.route = None;
        return Ok(None);
    }
    let falling = airborne.is_some_and(|state| state.peak().is_some());
    if script.route.is_none() && !script.chase_needs_route {
        if falling {
            return Ok(Some(carried));
        }
        match drops::leg(
            collision,
            world,
            client,
            body,
            feet,
            goal,
            script.allow_drops,
        ) {
            Ok(end)
                if (end - goal).with_y(0.).length() <= range
                    && (collision.support(goal).is_none()
                        || (end.y - goal.y).abs()
                            <= eq_client_core::movement::MAX_GROUNDED_STEP) =>
            {
                return Ok(Some(carried));
            }
            Ok(_) => script.chase_route_reason = Some("chase target is on a different floor"),
            Err(reason) => script.chase_route_reason = Some(reason),
        }
    }
    if script.route.is_none() && script.walk_scope.is_none() {
        script
            .progress
            .event("Chase finding a safe route around a drop".to_owned());
        debug!(id, ?feet, ?goal, ?carried, raw_target = ?target.position,
            target_model = ?(target.race, target.gender, target.size),
            allow_damaging_drops = script.allow_drops,
            "Script chase direct path needs a safe route");
    }
    if !walking::prepare(
        script,
        &Step::Chase(range, Duration::ZERO),
        online,
        client,
        body,
        collision,
        mesh,
        airborne.is_some() && (script.allow_drops || script.chase_edges == Edges::WithDrops),
        range,
    )? {
        return Ok(None);
    }
    let route = script.route.as_mut().expect("prepared above");
    route.movement_height(height, feet, goal);
    let next = route.next_with(collision, (feet, falling), goal, SEARCH_BUDGET, now);
    // Prefer a continuous floor route. A weighted search with drop edges can
    // choose to climb a nearby ramp and jump off it to reach a ground target.
    // If that first search fails, try the ordinary fall-capable planner before
    // following a partial path. Per-leg damage checks still apply.
    if script.chase_edges == Edges::Grounded
        && !script.allow_drops
        && airborne.is_some()
        && (route.partial() || matches!(next, RouteStep::Unreachable | RouteStep::Stalled))
    {
        script.chase_edges = Edges::WithDrops;
        script.chase_needs_route = true;
        script.route = None;
        script.following = None;
        return Ok(None);
    }
    match next {
        RouteStep::Searching => Ok(None),
        RouteStep::Toward(toward) => {
            if !falling {
                drops::leg(
                    collision,
                    world,
                    client,
                    body,
                    feet,
                    toward,
                    script.allow_drops,
                )?;
            }
            if script.following.is_none_or(|last| last.toward != toward) {
                debug!(?feet, ?toward, ?goal, index = route.waypoints_reached(),
                    polygons = ?mesh.and_then(|meshes| meshes.for_zone_walker(world.zone(), meshes.walker_size(body, client)).ok())
                        .map(|mesh| (mesh.locate(feet), mesh.locate(toward), mesh.locate(goal))),
                    "Script chase following safe route");
            }
            script.following = Some(Following {
                toward,
                index: route.waypoints_reached(),
                ahead: route.remaining(),
                left: route.distance_left(feet, goal),
                traced: Some(now),
            });
            Ok(Some(eq_client_core::world_position(
                (toward + Vec3::Y * body.feet_offset).to_array(),
                0.0,
            )))
        }
        RouteStep::Arrived => {
            script.route = None;
            if distance <= range
                && (goal.y - feet.y).abs() <= eq_client_core::movement::MAX_GROUNDED_STEP
            {
                Ok(None)
            } else {
                Err(no_route(script))
            }
        }
        RouteStep::Unreachable | RouteStep::Stalled => Err(no_route(script)),
        RouteStep::OpenDoor(_) => Err("safe chase route is blocked by a door".into()),
    }
}

fn no_route(script: &Script) -> String {
    format!(
        "{}; no safe route to the chase target",
        script
            .chase_route_reason
            .unwrap_or("chase has no verified safe path")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::WorldEvent;
    use std::time::Instant;

    const BODY: PlayerBody = PlayerBody {
        feet_offset: 3.75,
        height: 6.5,
        size: 6.0,
    };

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, c, b], [a, d, c]]
    }

    fn village(ramp: bool) -> eq_client_core::movement::CollisionWorld {
        let mut faces = quad(
            [-140.0, 0.0, -40.0],
            [40.0, 0.0, -40.0],
            [40.0, 0.0, 40.0],
            [-140.0, 0.0, 40.0],
        )
        .to_vec();
        faces.extend(quad(
            [-10.0, 53.0, -10.0],
            [10.0, 53.0, -10.0],
            [10.0, 53.0, 10.0],
            [-10.0, 53.0, 10.0],
        ));
        if ramp {
            faces.extend(quad(
                [-116.0, 0.0, -3.0],
                [-10.0, 53.0, -3.0],
                [-10.0, 53.0, 3.0],
                [-116.0, 0.0, 3.0],
            ));
        }
        eq_client_core::movement::CollisionWorld::new(faces).unwrap()
    }

    fn setup(from: Vec3, goal: Vec3) -> (OnlineState, Script) {
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.position =
            eq_client_core::world_position((from + Vec3::Y * BODY.feet_offset).to_array(), 0.0);
        testing::admit(&mut online, 1, player);
        let mut target = testing::pet(9, 0);
        target.pet_owner = None;
        target.race = 1;
        target.gender = 0;
        target.size = 6.0;
        target.position =
            eq_client_core::world_position((goal + Vec3::Y * BODY.feet_offset).to_array(), 0.0);
        testing::spawns(&mut online, vec![target]);
        let mut script = Script::new(Vec::new());
        script.awaited = Some(9);
        script.chase_actor = Some((1, online.world().spawn(9).unwrap().revision));
        (online, script)
    }

    #[test]
    fn ordinary_moving_targets_keep_direct_pursuit_without_a_search() {
        let collision = village(false);
        let (mut online, mut script) =
            setup(Vec3::new(-130.0, 0.0, 20.0), Vec3::new(-110.0, 0.0, 20.0));
        for x in -110_i16..-105 {
            let point = eq_client_core::world_position([f32::from(x), BODY.feet_offset, 20.0], 0.0);
            testing::news(
                &mut online,
                [WorldEvent::Position {
                    spawn_id: 9,
                    position: point,
                    velocity: [0.0; 3],
                    forward: 0.0,
                }],
            );
            assert_eq!(
                aim(
                    &mut script,
                    &online,
                    InstalledClient::Titanium,
                    BODY,
                    &collision,
                    None,
                    Some(&AirborneController::default()),
                    point,
                    1.0,
                    Duration::ZERO
                )
                .unwrap(),
                Some(point)
            );
            assert!(script.route.is_none());
        }
    }

    /// Follow the actual production chase decision; use collision walking
    /// for the grounded fixture's samples, never teleport over a route leg.
    fn follow(from: Vec3, goal: Vec3, limit: usize) -> (Vec3, f32, bool) {
        let collision = village(true);
        let (mut online, mut script) = setup(from, goal);
        let target = online.world().spawn(9).unwrap().state.position;
        let mut feet = from;
        let (mut peak, mut routed) = (0.0_f32, false);
        let started = Instant::now();
        for frame in 0..limit {
            let toward = aim(
                &mut script,
                &online,
                InstalledClient::Titanium,
                BODY,
                &collision,
                None,
                Some(&AirborneController::default()),
                target,
                1.0,
                Duration::from_millis(u64::try_from(frame).unwrap()),
            )
            .unwrap_or_else(|error| {
                panic!("chase frame {frame}: {feet:?} to {goal:?}, peak {peak}: {error}")
            });
            routed |= script.route.is_some();
            if let Some(toward) = toward {
                let toward = Vec3::from_array(eq_client_core::render_position(toward));
                let delta = (toward - feet).with_y(0.0).clamp_length_max(1.0);
                feet = collision.step(feet, delta, BODY.height);
                assert!(
                    collision.support(feet).is_some(),
                    "fixture stepped off support: {feet:?}"
                );
                peak = peak.max(feet.y);
                testing::place(
                    &mut online,
                    eq_client_core::world_position(
                        (feet + Vec3::Y * BODY.feet_offset).to_array(),
                        0.0,
                    ),
                );
            }
            if (feet - goal).length() <= 1.1 {
                return (feet, peak, routed);
            }
            assert!(started.elapsed() < Duration::from_secs(30));
            if script
                .route
                .as_ref()
                .is_some_and(eq_client_core::movement::Route::is_searching)
            {
                // Let the real background planner finish between simulated frames.
                std::thread::sleep(Duration::from_millis(1));
            } else {
                std::thread::yield_now();
            }
        }
        panic!(
            "chase did not reach its known floor: {feet:?} -> {goal:?}; route={:?}, last waypoint={:?}, reason={:?}",
            script.route.as_ref().map(|r| (
                r.is_searching(),
                r.waypoints_reached(),
                r.remaining(),
                r.searched()
            )),
            script.following.as_ref().map(|f| f.toward),
            script.chase_route_reason
        );
    }

    #[test]
    fn a_ground_target_is_not_chased_up_the_platform_ramp() {
        let (at, peak, routed) = follow(Vec3::new(-130.0, 0.0, 0.0), Vec3::ZERO, 20_000);
        assert!(routed, "an unsafe direct floor choice was never routed");
        assert!(
            peak <= eq_client_core::movement::MAX_GROUNDED_STEP,
            "unneeded platform climb: {peak}"
        );
        assert!(at.y.abs() < 0.01);
    }

    #[test]
    fn a_chase_without_a_grounded_path_can_still_use_a_harmless_drop() {
        let mut faces = quad(
            [-30., 5., -30.],
            [0., 5., -30.],
            [0., 5., 30.],
            [-30., 5., 30.],
        )
        .to_vec();
        faces.extend(quad(
            [0., 0., -30.],
            [30., 0., -30.],
            [30., 0., 30.],
            [0., 0., 30.],
        ));
        faces.extend(quad(
            [0., 0., -30.],
            [0., 0., 30.],
            [0., 5., 30.],
            [0., 5., -30.],
        ));
        let collision = eq_client_core::movement::CollisionWorld::new(faces).unwrap();
        let (online, mut script) = setup(Vec3::new(-10., 5., 0.), Vec3::new(10., 0., 0.));
        script.chase_needs_route = true;
        let target = online.world().spawn(9).unwrap().state.position;
        let start = Instant::now();
        loop {
            let next = aim(
                &mut script,
                &online,
                InstalledClient::Titanium,
                BODY,
                &collision,
                None,
                Some(&AirborneController::default()),
                target,
                1.,
                start.elapsed(),
            )
            .unwrap();
            if next.is_some() {
                assert_eq!(script.chase_edges, Edges::WithDrops);
                assert!(!script.route.as_ref().unwrap().partial());
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_platform_target_is_reached_by_its_real_ramp() {
        let (at, peak, _) = follow(
            Vec3::new(-130.0, 0.0, 0.0),
            Vec3::new(0.0, 53.0, 0.0),
            20_000,
        );
        assert!(peak >= 52.9);
        assert!((at.y - 53.0).abs() < 0.01);
    }

    #[test]
    fn a_target_overhead_is_reached_via_the_ramp_instead_of_arriving_underneath() {
        let (at, peak, routed) = follow(Vec3::ZERO, Vec3::new(0.0, 53.0, 0.0), 20_000);
        assert!(routed);
        assert!(peak >= 52.9);
        assert!((at.y - 53.0).abs() < 0.01);
    }

    #[test]
    fn a_replacement_cannot_retarget_the_running_chase() {
        let collision = village(false);
        let (mut online, mut script) =
            setup(Vec3::new(-130.0, 0.0, 20.0), Vec3::new(-110.0, 0.0, 20.0));
        let target = online.world().spawn(9).unwrap().state.clone();
        testing::spawns(&mut online, vec![target.clone()]);
        let result = aim(
            &mut script,
            &online,
            InstalledClient::Titanium,
            BODY,
            &collision,
            None,
            Some(&AirborneController::default()),
            target.position,
            1.0,
            Duration::ZERO,
        );
        assert_eq!(
            result,
            Err("chase target or admission was replaced".to_owned())
        );
        assert!(script.route.is_none());
    }

    #[test]
    fn an_isolated_platform_refuses_before_an_unconsented_descent() {
        let collision = village(false);
        let (mut online, mut script) = setup(Vec3::new(9.0, 53.0, 0.0), Vec3::new(30.0, 0.0, 0.0));
        let target = online.world().spawn(9).unwrap().state.position;
        let mut feet = Vec3::new(9.0, 53.0, 0.0);
        let started = Instant::now();
        for frame in 0_u64..20_000 {
            let result = aim(
                &mut script,
                &online,
                InstalledClient::Titanium,
                BODY,
                &collision,
                None,
                Some(&AirborneController::default()),
                target,
                1.0,
                Duration::from_millis(frame),
            );
            match result {
                Err(reason) => {
                    assert!(reason.starts_with("drop too high"));
                    return;
                }
                Ok(Some(toward)) => {
                    let toward = Vec3::from_array(eq_client_core::render_position(toward));
                    feet = collision.step(
                        feet,
                        (toward - feet).with_y(0.0).clamp_length_max(1.0),
                        BODY.height,
                    );
                    assert!((feet.y - 53.0).abs() < 0.01, "unsafe route exit: {feet:?}");
                    assert!(collision.support(feet).is_some());
                    testing::place(
                        &mut online,
                        eq_client_core::world_position(
                            (feet + Vec3::Y * BODY.feet_offset).to_array(),
                            0.0,
                        ),
                    );
                }
                Ok(None) => (),
            }
            assert!(started.elapsed() < Duration::from_secs(30));
            std::thread::yield_now();
        }
        panic!("unsafe chase never reported its refusal");
    }

    #[test]
    fn explicit_consent_allows_a_chase_toward_a_known_damaging_landing() {
        let collision = village(false);
        let (online, mut script) = setup(Vec3::new(9.0, 53.0, 0.0), Vec3::new(30.0, 0.0, 0.0));
        script.allow_drops = true;
        let target = online.world().spawn(9).unwrap().state.position;
        assert_eq!(
            aim(
                &mut script,
                &online,
                InstalledClient::Titanium,
                BODY,
                &collision,
                None,
                Some(&AirborneController::default()),
                target,
                1.0,
                Duration::ZERO
            )
            .unwrap(),
            Some(target)
        );
        assert!(
            script.route.is_none(),
            "a verified consented direct descent was unnecessarily routed"
        );
    }
}
