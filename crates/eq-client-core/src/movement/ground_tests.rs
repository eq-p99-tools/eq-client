use super::*;
use crate::movement::Liquids;

struct Dry;
impl Liquids for Dry {
    fn liquid_at(&self, _: Vec3) -> Option<Liquid> {
        None
    }
    fn passes_through(&self, _: Vec3, _: Vec3, _: Liquid) -> bool {
        false
    }
    fn segment_known(&self, _: Vec3, _: Vec3) -> bool {
        true
    }
    fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
        Some(Box::new(Self))
    }
}
fn terrain() -> Vec<[[f32; 3]; 3]> {
    vec![
        [
            [-2000., 0., -2000.],
            [-2000., 0., 2000.],
            [2000., 0., -2000.],
        ],
        [[2000., 0., -2000.], [-2000., 0., 2000.], [2000., 0., 2000.]],
    ]
}
fn world() -> CollisionWorld {
    CollisionWorld::new(terrain()).unwrap().with_liquids(Dry)
}
fn next(
    motion: GroundMotion,
    world: &CollisionWorld,
    speed: f32,
    held: bool,
    dt: Duration,
) -> GroundMotion {
    motion
        .frame(world, 6., Vec3::X, speed, held, dt)
        .expect("qualified static dry frame")
}

#[test]
fn native_ground_release_removes_input_at_each_frame_and_never_drains_a_horizon() {
    let world = world();
    for fps in [20_u32, 60, 100, 144] {
        let dt = Duration::from_secs_f64(1. / f64::from(fps));
        for speed in [6., 29.866_667] {
            for hold in [1_u32, 2, 13] {
                let mut motion = GroundMotion::new(Vec3::ZERO).unwrap();
                for _ in 0..hold {
                    motion = next(motion, &world, speed, true, dt);
                }
                assert!(motion.feet().x > 0., "no prompt first step");
                let mut prior_step = f32::INFINITY;
                for _ in 0..30 {
                    let before = motion;
                    motion = next(motion, &world, speed, false, dt);
                    assert!(motion.velocity().length() <= before.velocity().length());
                    let step = motion.feet().x - before.feet().x;
                    assert!(
                        step <= prior_step + 0.000_01,
                        "release accelerated the shown step"
                    );
                    prior_step = step;
                }
                assert_eq!(motion.velocity(), Vec3::ZERO);
                let maximum = speed * (dt * hold).as_secs_f32();
                assert!(
                    motion.feet().x <= maximum + 0.001,
                    "invented future held time"
                );
                let stopped = motion.feet();
                for _ in 0..30 {
                    motion = next(motion, &world, speed, false, dt);
                }
                assert_eq!(
                    motion.feet(),
                    stopped,
                    "a hidden endpoint was drained after rest"
                );
            }
        }
    }
}

#[test]
fn native_ground_repress_retains_only_the_owned_velocity_and_actual_held_frames() {
    let world = world();
    let dt = Duration::from_secs_f64(1. / 60.);
    for gap in [1, 3, 12] {
        let mut motion = GroundMotion::new(Vec3::ZERO).unwrap();
        for _ in 0..6 {
            motion = next(motion, &world, 6., true, dt);
        }
        for _ in 0..gap {
            motion = next(motion, &world, 6., false, dt);
        }
        let velocity = motion.velocity();
        motion = next(motion, &world, 6., true, dt);
        assert!((motion.velocity() - (velocity * 0.625 + Vec3::X * 2.25)).length() < 0.000_01);
        for _ in 0..6 {
            motion = next(motion, &world, 6., true, dt);
        }
        for _ in 0..30 {
            motion = next(motion, &world, 6., false, dt);
        }
        assert!(motion.feet().x <= 6. * (dt * 13).as_secs_f32() + 0.001);
        assert_eq!(motion.velocity(), Vec3::ZERO);
    }
}

#[test]
fn native_ground_ten_seconds_matches_the_unfitted_native_whole_ms_reference() {
    let world = world();
    for fps in [20_u32, 60, 100, 144] {
        let dt = Duration::from_secs_f64(1. / f64::from(fps));
        let frames = fps * 10;
        let speed = 29.866_667;
        let mut motion = GroundMotion::new(Vec3::ZERO).unwrap();
        let (mut native_velocity, mut native_position) = (0_f64, 0_f64);
        // Phase 0.5 ms gives the read's first 60 Hz frame its 17 ms. The
        // coefficient/clock choice is fixed before either outcome is measured.
        let mut last_ms = 0;
        for frame in 1..=frames {
            motion = next(motion, &world, speed, true, dt);
            let ms = (frame * 1000 + fps / 2) / fps;
            native_velocity = native_velocity * 0.625 + 0.8 * 0.7;
            native_position += native_velocity * (0.02 * f64::from(ms - last_ms)).min(3.);
            last_ms = ms;
        }
        let actual = f64::from(motion.feet().x);
        assert!(
            (actual - native_position).abs() / native_position < 0.04,
            "{fps}: native={native_position} current={actual}"
        );
        assert!(motion.velocity().length() <= speed);
        // An independent closed form also protects the precise-frame path
        // from losing a fraction on every collision-coordinate conversion.
        let n = f64::from(frames);
        let expected =
            f64::from(speed) * dt.as_secs_f64() * (n - 0.625 * (1. - 0.625_f64.powf(n)) / 0.375);
        assert!((actual - expected).abs() < 0.01);
    }
}

#[test]
fn native_ground_collision_uses_the_existing_sweep_and_never_crosses_the_wall() {
    let mut triangles = terrain();
    triangles.extend([
        [[3., 0., -10.], [3., 0., 10.], [3., 10., -10.]],
        [[3., 10., -10.], [3., 0., 10.], [3., 10., 10.]],
    ]);
    let world = CollisionWorld::new(triangles).unwrap().with_liquids(Dry);
    let dt = Duration::from_secs_f64(1. / 60.);
    let mut motion = GroundMotion::new(Vec3::ZERO).unwrap();
    for _ in 0..120 {
        motion = next(motion, &world, 6., true, dt);
        assert!(motion.feet().x < 2.58);
    }
    let at_wall = motion.feet();
    for _ in 0..30 {
        let before = motion.feet();
        motion = next(motion, &world, 6., false, dt);
        let ordinary = world.step(before, motion.velocity() * dt.as_secs_f32(), 6.);
        assert!(
            motion.feet().distance(ordinary) < 0.000_01,
            "native endpoint diverged from its collision owner"
        );
        assert!(motion.feet().x < 2.58);
    }
    // The ordinary sweep can approach its wall clearance differently as the
    // decaying stride gets shorter. Do not replace it with a new sticky-wall
    // rule or infer owned velocity from that resolved displacement.
    println!(
        "native ground wall: held_x={} stopped_x={} approach={}",
        at_wall.x,
        motion.feet().x,
        motion.feet().distance(at_wall)
    );
    assert_eq!(motion.velocity(), Vec3::ZERO);
}

#[test]
fn native_ground_unknown_liquid_edge_and_invalid_input_decline_without_state_change() {
    let plain = world();
    let unknown = CollisionWorld::new(terrain()).unwrap();
    let initial = GroundMotion::new(Vec3::ZERO).unwrap();
    let dt = Duration::from_secs_f64(1. / 60.);
    assert!(initial.frame(&unknown, 6., Vec3::X, 6., true, dt).is_none());
    for speed in [0., -1., f32::NAN, f32::INFINITY] {
        assert!(
            initial
                .frame(&plain, 6., Vec3::X, speed, true, dt)
                .is_none()
        );
    }
    assert!(
        initial
            .frame(&plain, 6., Vec3::ZERO, 6., true, dt)
            .is_none()
    );
    let edge = GroundMotion::new(Vec3::new(1999.59, 0., 0.)).unwrap();
    assert!(
        edge.frame(
            &plain,
            6.,
            Vec3::X,
            29.866_667,
            true,
            Duration::from_millis(50)
        )
        .is_none()
    );
    assert_eq!(initial.feet(), Vec3::ZERO);
    assert_eq!(initial.velocity(), Vec3::ZERO);
}

#[test]
fn native_ground_frame_cap_and_zero_input_do_not_invent_subframe_holds() {
    let world = world();
    let initial = GroundMotion::new(Vec3::ZERO).unwrap();
    let stopped = next(initial, &world, 6., false, Duration::from_secs(1));
    assert_eq!(stopped.feet(), initial.feet());
    let capped = next(initial, &world, 6., true, Duration::from_secs(1));
    let regular = next(initial, &world, 6., true, Duration::from_millis(150));
    assert_eq!(capped.feet(), regular.feet());
    assert_eq!(capped.velocity(), regular.velocity());
}

#[test]
fn native_ground_dry_endpoints_do_not_prove_a_dry_path_between_them() {
    struct ThinWater;
    impl Liquids for ThinWater {
        fn liquid_at(&self, point: Vec3) -> Option<Liquid> {
            (point.x > 0.02 && point.x < 0.03).then_some(Liquid::Water)
        }
        fn passes_through(&self, from: Vec3, to: Vec3, liquid: Liquid) -> bool {
            liquid == Liquid::Water && from.x.min(to.x) < 0.03 && from.x.max(to.x) > 0.02
        }
        fn segment_known(&self, _: Vec3, _: Vec3) -> bool {
            true
        }
        fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
            Some(Box::new(Self))
        }
    }
    let world = CollisionWorld::new(terrain())
        .unwrap()
        .with_liquids(ThinWater);
    let initial = GroundMotion::new(Vec3::ZERO).unwrap();
    assert!(
        initial
            .frame(
                &world,
                6.,
                Vec3::X,
                6.,
                true,
                Duration::from_secs_f64(1. / 60.)
            )
            .is_none()
    );
    assert_eq!(initial.feet(), Vec3::ZERO);
}
