//! Deterministic normal-input/local-result surface regression fixtures.
use super::*;
use eq_client_core::movement::{Liquid, Liquids, SurfaceRegion};
use eq_network_game::movement::{MotionSession, SpeedState, titanium_speeds};
use serde_json::{Value, json};
use std::sync::mpsc;

struct Water;
impl Liquids for Water {
    fn surface_water_identity(&self, point: Vec3) -> Option<usize> {
        (self.liquid_at(point) == Some(Liquid::Water)).then_some(0)
    }
    fn surface_water_below(
        &self,
        origin: Vec3,
        bottom: f32,
        water: SurfaceRegion,
        region: usize,
    ) -> Option<f32> {
        (origin.is_finite()
            && bottom.is_finite()
            && origin.y > 0.
            && bottom < 0.
            && water == SurfaceRegion::Water
            && region == 0)
            .then_some(0.)
    }
    fn surface_region(&self, p: Vec3) -> Option<SurfaceRegion> {
        if !p.is_finite() || p.y == 0. {
            None
        } else {
            Some(if p.y < 0. {
                SurfaceRegion::Water
            } else {
                SurfaceRegion::Air
            })
        }
    }
    fn surface_segment(&self, a: Vec3, b: Vec3, k: SurfaceRegion) -> bool {
        a.is_finite()
            && b.is_finite()
            && k == SurfaceRegion::Water
            && self.surface_region(a).is_some()
            && self.surface_region(b).is_some()
    }
    fn liquid_at(&self, p: Vec3) -> Option<Liquid> {
        (p.y < 0.).then_some(Liquid::Water)
    }
    fn passes_through(&self, a: Vec3, b: Vec3, k: Liquid) -> bool {
        k == Liquid::Water && (a.y < 0. || b.y < 0.)
    }
    fn region_known(&self, p: Vec3) -> bool {
        self.surface_region(p).is_some()
    }
    fn segment_known(&self, a: Vec3, b: Vec3) -> bool {
        a.is_finite() && b.is_finite()
    }
    fn first_outside(&self, a: Vec3, b: Vec3, _: Liquid) -> Option<std::ops::Range<f64>> {
        if a.y < 0. && b.y < 0. {
            None
        } else if a.y < 0. && b.y > 0. {
            Some(f64::from(-a.y / (b.y - a.y))..1.)
        } else {
            Some(0.0..1.0)
        }
    }
    fn water_surface_exit(&self, a: Vec3, b: Vec3) -> bool {
        a.y < 0. && b.y > 0.
    }
    fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
        None
    }
}
struct Native {
    app: App,
    receiver: mpsc::Receiver<ClientCommand>,
    session: MotionSession,
    time: Instant,
}
impl Native {
    fn new(pitch: f32, origin_y: f32, suppressed: bool, base: Instant) -> Self {
        Self::at(
            pitch,
            world_position([0., origin_y, 0.], 384.),
            suppressed,
            base,
        )
    }
    fn at(
        pitch: f32,
        placement: eq_client_core::WorldPosition,
        suppressed: bool,
        base: Instant,
    ) -> Self {
        let (mut app, receiver) = tests::fixture();
        crate::keys::testing::bind(
            &mut app,
            Act::CameraForward,
            crate::keys::Chord::key(KeyCode::KeyT),
        );
        crate::keys::testing::bind(
            &mut app,
            Act::Forward,
            crate::keys::Chord::key(KeyCode::KeyW),
        );
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(Fix::JesusMode, false);
        let world = CollisionWorld::new([
            [
                [-2000., -1000., -2000.],
                [2000., -1000., 2000.],
                [2000., -1000., -2000.],
            ],
            [
                [-2000., -1000., -2000.],
                [-2000., -1000., 2000.],
                [2000., -1000., 2000.],
            ],
        ])
        .unwrap()
        .with_liquids(Water)
        .with_swim_origin(3.);
        app.insert_resource(Collision(Some(world)));
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            placement,
        );
        let grant = titanium_speeds(&SpeedState {
            run: 0.7,
            walk: 0.46,
            level: 50,
            hit_points: Some((500, 500)),
            rate: 0,
            weight: 0,
            capacity: 100,
            sneaking: false,
            ducking: false,
            zone_id: 1,
            swimming_skill: Some(100),
        })
        .motion
        .unwrap();
        {
            let world = app.world_mut();
            let mut player = world.query_filtered::<&mut Transform, With<Player>>();
            player.single_mut(world).unwrap().translation =
                Vec3::from_array(eq_client_core::render_position(placement));
            let mut camera = world.query::<&mut OrbitCamera>();
            let mut camera = camera.single_mut(world).unwrap();
            camera.yaw = crate::camera::behind(eq_client_core::render_heading(placement.heading));
            camera.look = pitch;
            let mut controls = world.resource_mut::<Controls>();
            controls.speed = Some(grant.units_per_second);
            controls.strafe_speed = grant.strafe.map(|s| s.units_per_second);
            controls.strafe_walk_speed = grant.strafe.and_then(|s| s.walk_units_per_second);
            controls.walk_speed = grant.walk.map(|s| s.units_per_second);
            controls.backward_speed = grant.backward.map(|s| s.units_per_second);
            controls.diagonal_rates = grant.diagonal;
            controls.swim = grant.swim;
            controls.airborne = Some(AirborneController::default());
            controls.suppress_surface = suppressed;
            controls.test_now = Some(base);
            controls.last_accepted = base.checked_sub(Duration::from_millis(100)).unwrap();
        }
        app.add_systems(Update, input);
        let start = base.checked_sub(Duration::from_millis(100)).unwrap();
        let mut session = MotionSession::new(11, 7, placement, start)
            .unwrap()
            .with_swimming(true)
            .with_falls(true)
            .with_heading_snapshots(true)
            .with_jump(Some((44.55, 160.)));
        session.calibrate(grant, start).unwrap();
        Self {
            app,
            receiver,
            session,
            time: start,
        }
    }
    fn step(&mut self, ms: u64) -> (MovementRequest, Value) {
        self.time += Duration::from_millis(ms);
        {
            let mut c = self.app.world_mut().resource_mut::<Controls>();
            c.test_now = Some(self.time);
            c.sampling.span = Some(Duration::from_millis(ms).as_secs_f32());
            c.last_accepted = self.time.checked_sub(Duration::from_millis(100)).unwrap();
            c.last_sent = self.time.checked_sub(Duration::from_millis(ms));
        }
        self.app
            .world_mut()
            .resource_mut::<crate::outbox::Outbox>()
            .set_test_time(self.time);
        self.app.update();
        // This minimal App has no InputPlugin. Match its end-of-frame edge
        // clearing, while keeping physically held keys down.
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        let commands = self.receiver.try_iter().collect::<Vec<_>>();
        let mut requests = Vec::new();
        for command in commands {
            match command {
                ClientCommand::ClimbReach { per_step, .. } => self.session.set_climb(per_step),
                ClientCommand::Move(request) => requests.push(request),
                _ => (),
            }
        }
        assert_eq!(
            requests.len(),
            1,
            "surface report {}",
            self.app.world().resource::<Controls>().surface_report()
        );
        let request = requests.pop().unwrap();
        // Both input and the real outbox receive the same logical clock;
        // the unchanged session checks the actual request, without rewriting it.
        assert_eq!(request.created, self.time);
        let mut packet = Value::Null;
        self.session.send_move_sample(&request,self.time,|p| {
            packet=json!({"spawn":p.spawn_id,"sequence":p.sequence,
                "position":[p.position.x.to_bits(),p.position.y.to_bits(),p.position.z.to_bits(),p.position.heading.to_bits()],
                "delta":p.delta.map(f32::to_bits),"animation":p.animation,"turn":p.delta_heading});Ok(())
        }).unwrap_or_else(|e|panic!("surface guard {ms}ms: {e}; report {}",self.app.world().resource::<Controls>().surface_report()));
        (request, packet)
    }
    fn accept(&mut self, request: &MovementRequest) {
        tests::accept_now(&mut self.app, request);
    }
}

#[test]
fn surface_dispatch_physical_forward_uses_local_result_and_no_submitted_preview() {
    let mut f = Native::new(15_f32.to_radians(), -0.1, false, Instant::now());
    let initial = f.app.world().resource::<Controls>().wet;
    let before = {
        let world = f.app.world_mut();
        let mut q = world.query_filtered::<&Transform, With<Player>>();
        q.single(world).unwrap().translation
    };
    let (request, _) = f.step(100);
    assert_eq!(request.mode, MovementMode::Swim);
    let c = f.app.world().resource::<Controls>();
    assert!(c.surface.pending());
    assert!(c.preview.is_none());
    assert!(!c.release_sample);
    assert_eq!(c.wet, initial, "proposal must not commit wet state");
    let after = {
        let world = f.app.world_mut();
        let mut q = world.query_filtered::<&Transform, With<Player>>();
        q.single(world).unwrap().translation
    };
    assert_eq!(before, after);
    f.accept(&request);
    assert!(f.app.world().resource::<Controls>().surface.active());
    assert_ne!(f.app.world().resource::<Controls>().wet, initial);
    for ms in [117, 167] {
        let (r, _) = f.step(ms);
        f.accept(&r);
    }
    let c = f.app.world().resource::<Controls>();
    assert_eq!(c.surface.prior().unwrap().updates(), 24);
    let p = f
        .app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    let origin = Vec3::from_array(eq_client_core::render_position(p));
    let forward = Vec2::new(origin.x, origin.z).length();
    assert!(
        (forward - 0.850_186_6).abs() < 0.000_1,
        "384ms witness {forward}"
    );
}

#[test]
fn surface_whole_medium_commands_packets_and_phase_followups_keep_ordinary_bits() {
    for origin in [-20_f32, 20.] {
        for ms in [100_u64, 117, 150, 167, 200, 250] {
            let base = Instant::now();
            let mut a = Native::new(0., origin, false, base);
            let mut b = Native::new(0., origin, true, base);
            for dt in [100, ms, 117] {
                let (ar, ap) = a.step(dt);
                let (br, bp) = b.step(dt);
                assert_eq!(ar.mode, br.mode);
                assert_eq!(ar.position, br.position);
                assert_eq!(ar.vertical_speed, br.vertical_speed);
                assert_eq!(ap, bp, "wire bits differ at origin{origin}, {dt}ms");
                a.accept(&ar);
                b.accept(&br);
                let ac = a.app.world().resource::<Controls>();
                let bc = b.app.world().resource::<Controls>();
                assert_eq!(ac.wet, bc.wet);
                assert_eq!(
                    ac.airborne.as_ref().unwrap().velocity().to_bits(),
                    bc.airborne.as_ref().unwrap().velocity().to_bits()
                );
                assert!(!ac.surface.active());
            }
        }
    }
}

#[test]
fn surface_dispatch_busy_rolls_back_and_correction_fences_delayed_result() {
    let mut f = Native::new(15_f32.to_radians(), -0.1, false, Instant::now());
    let before = f.app.world().resource::<Controls>().wet;
    let (sender, _held) = mpsc::sync_channel(0);
    f.app
        .insert_resource(crate::outbox::Outbox::new(Some(sender)));
    f.time += Duration::from_millis(100);
    {
        let mut c = f.app.world_mut().resource_mut::<Controls>();
        c.test_now = Some(f.time);
        c.sampling.span = Some(0.1);
        c.last_accepted = f.time.checked_sub(Duration::from_millis(100)).unwrap();
    }
    f.app.update();
    let c = f.app.world().resource::<Controls>();
    assert_eq!(c.wet, before);
    assert!(!c.surface.pending());
    assert!(c.surface.prior().is_none());
    let mut f = Native::new(15_f32.to_radians(), -0.1, false, Instant::now());
    let (r, _) = f.step(100);
    assert!(f.app.world().resource::<Controls>().surface.pending());
    f.app.world_mut().resource_mut::<Controls>().rebase();
    f.app
        .world_mut()
        .resource_mut::<Controls>()
        .resolve_surface(Some(r.session_id), r.created, r.position, false);
    assert!(
        f.app
            .world()
            .resource::<Controls>()
            .surface
            .prior()
            .is_none()
    );
}

#[test]
fn surface_recalibration_cannot_commit_a_pre_change_velocity_snapshot() {
    let mut f = Native::new(15_f32.to_radians(), -0.1, false, Instant::now());
    let before = f.app.world().resource::<Controls>().wet;
    let (r, _) = f.step(100);
    let mut c = f.app.world_mut().resource_mut::<Controls>();
    assert!(c.surface.pending());
    let mut grant = c.speeds();
    grant.swim.as_mut().unwrap().strafe *= 0.5;
    c.set_speeds(grant);
    assert!(!c.surface.pending());
    c.resolve_surface(Some(r.session_id), r.created, r.position, false);
    assert!(c.surface.prior().is_none());
    assert_eq!(c.wet, before);
}

fn wet_handback_fixture() -> Native {
    let mut f = Native::new(15_f32.to_radians(), -0.1, false, Instant::now());
    let (r, _) = f.step(100);
    f.accept(&r);
    {
        let world = f.app.world_mut();
        world
            .query::<&mut OrbitCamera>()
            .single_mut(world)
            .unwrap()
            .look = -85_f32.to_radians();
    }
    for ms in [100, 101] {
        let (r, _) = f.step(ms);
        f.accept(&r);
    }
    f
}

fn ordinary_wet_partition(
    mut velocity: eq_client_core::movement::water::WetVelocity,
    set: eq_client_core::movement::water::WetCoefficients,
    input: eq_client_core::movement::water::WetInput,
    seconds: f32,
) -> (eq_client_core::movement::water::WetVelocity, Vec3) {
    let (mut left, mut travel) = (seconds, Vec3::ZERO);
    while left > 0. {
        let piece = left.min(eq_client_core::movement::water::WET_FRAME_SECONDS);
        let (across, up) = velocity.advance(set, input, piece);
        travel += Vec3::new(across[0], up, across[1]);
        left -= piece;
    }
    (velocity, travel)
}

#[test]
fn surface_wet_handback_copies_partial_phase_and_the_next_wet_call_exactly() {
    use eq_client_core::movement::{SurfaceClock, SurfaceStroke, water::WetInput};
    let mut f = wet_handback_fixture();
    let world = f.app.world().resource::<Collision>().0.as_ref().unwrap();
    let c = f.app.world().resource::<Controls>();
    let clock = c.surface.prior().expect("owned wet handback");
    assert_eq!(clock.medium(), SurfaceRegion::Water);
    let seed = clock.velocity();
    assert_eq!(
        c.wet, seed,
        "H1 transfers the object, not reconstructed units"
    );
    assert!(!seed.frame_phase().unwrap().at_boundary);
    let position = f
        .app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    let feet = Vec3::from_array(eq_client_core::render_position(position)) - Vec3::Y * 3.;
    let swim = c.swim.unwrap();
    let yaw = eq_client_core::render_heading(position.heading);
    let flat = Vec2::new(yaw.sin(), yaw.cos()).normalize_or_zero() * swim.forward;
    let input = WetInput {
        horizontal: [flat.x, flat.y],
        vertical: swim.forward * (-85_f32).to_radians().sin(),
        jump: false,
    };
    let stroke = |seconds| SurfaceStroke {
        set: swim.set,
        input,
        seconds,
        height: 6.,
    };
    let mut one_call = seed;
    let (across, up) = one_call.advance(swim.set, input, 0.001);
    let partial = SurfaceClock::plan(Some(clock), world, feet, seed, stroke(0.001)).unwrap();
    assert_eq!(
        partial.clock.velocity(),
        one_call,
        "H1 direct next wet API call keeps phase"
    );
    assert_eq!(partial.feet, feet + Vec3::new(across[0], up, across[1]));
    // Same ordinary outer partition, directly from the identical carried
    // object. Different partitions have the separate ten-second bound.
    let (expected, travel) = ordinary_wet_partition(seed, swim.set, input, 0.1);
    f.app
        .world_mut()
        .resource_mut::<Controls>()
        .suppress_surface = true;
    let (ordinary, _) = f.step(100);
    let c = f.app.world().resource::<Controls>();
    assert_eq!(
        c.wet, expected,
        "H1 actual ordinary wet call after handback"
    );
    assert_eq!(
        ordinary.position,
        world_position((feet + travel + Vec3::Y * 3.).to_array(), position.heading)
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "bounded ten-second ordinary/native comparison and independent phase ledger"
)]
fn surface_wet_partition_noise_over_ten_seconds_stays_inside_the_actual_clock_boundary() {
    use eq_client_core::movement::{
        SurfaceClock, SurfaceStroke,
        water::{WetFrameMedium, WetInput, WetVelocity},
    };
    let mut fixture = wet_handback_fixture();
    let world = fixture
        .app
        .world()
        .resource::<Collision>()
        .0
        .as_ref()
        .unwrap()
        .clone();
    let controls = fixture.app.world().resource::<Controls>();
    let mut clock = controls.surface.prior().unwrap().clone();
    let initial_updates = clock.updates();
    let swim = controls.swim.unwrap();
    let mut position = fixture
        .app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    let mut feet = Vec3::from_array(eq_client_core::render_position(position)) - Vec3::Y * 3.;
    let yaw = eq_client_core::render_heading(position.heading);
    let flat = Vec2::new(yaw.sin(), yaw.cos()).normalize_or_zero() * swim.forward;
    let input = WetInput {
        horizontal: [flat.x, flat.y],
        vertical: swim.forward * (-85_f32).to_radians().sin(),
        jump: false,
    };
    fixture
        .app
        .world_mut()
        .resource_mut::<Controls>()
        .suppress_surface = true;
    let (mut max_phase, mut max_position) = (0_f32, 0_f32);
    for _ in 0..100 {
        let before = fixture.app.world().resource::<Controls>().wet;
        let (same_partition, travel) = ordinary_wet_partition(before, swim.set, input, 0.1);
        let old_feet = Vec3::from_array(eq_client_core::render_position(position)) - Vec3::Y * 3.;
        let mut expected = SurfaceClock::plan(
            Some(&clock),
            &world,
            feet,
            clock.velocity(),
            SurfaceStroke {
                set: swim.set,
                input,
                seconds: 0.1,
                height: 6.,
            },
        )
        .unwrap();
        let (ordinary, _) = fixture.step(100);
        assert_eq!(
            ordinary.position,
            world_position(
                (old_feet + travel + Vec3::Y * 3.).to_array(),
                position.heading
            )
        );
        let actual = fixture.app.world().resource::<Controls>().wet;
        assert_eq!(actual, same_partition, "H1 same partition stays bit-exact");
        assert_eq!(
            actual.units_per_second(),
            expected.clock.velocity().units_per_second()
        );
        let difference = (actual.frame_phase().unwrap().remaining_seconds
            - expected
                .clock
                .velocity()
                .frame_phase()
                .unwrap()
                .remaining_seconds)
            .abs();
        max_phase = max_phase.max(difference);
        if difference > 0. {
            // Query the API's own window. No duplicate private constant and
            // no adjustment of either owner's carried state.
            let mut boundary = WetVelocity::default();
            let probe = boundary
                .advance_piece(WetFrameMedium::Water(swim.set), input, difference)
                .unwrap();
            assert!(
                !probe.starts_frame && !probe.completes_frame,
                "phase difference escaped clock boundary: {difference}"
            );
            assert!(boundary.frame_phase().unwrap().at_boundary);
        }
        let expected_position = world_position(
            (expected.feet + Vec3::Y * 3.).to_array(),
            ordinary.position.heading,
        );
        max_position = max_position.max(
            Vec3::from_array(eq_client_core::render_position(ordinary.position)).distance(
                Vec3::from_array(eq_client_core::render_position(expected_position)),
            ),
        );
        feet = Vec3::from_array(eq_client_core::render_position(expected_position)) - Vec3::Y * 3.;
        assert!(expected.clock.accepted_feet(feet));
        clock = expected.clock;
        position = ordinary.position;
        fixture.accept(&ordinary);
    }
    assert_eq!(clock.updates() - initial_updates, 600);
    println!(
        "H1 {}",
        json!({"elapsed_ms":10000,"ordinary_outer_ms":100,"same_partition_bit_equal":true,"velocity_bit_equal_across_partitions":true,"max_phase_seconds":max_phase,"max_position_units":max_position,"actual_api_boundary_pass":true,"reference_updates":600})
    );
}

#[test]
fn surface_takeover_does_not_silently_change_an_unclamped_snapshot() {
    use eq_client_core::movement::water::WetVelocity;
    let base = Instant::now();
    let mut ordinary = Native::new(0., -20., true, base);
    let mut candidate = Native::new(0., -20., false, base);
    let seed = WetVelocity::from_units_per_second([0., 20.], 50.);
    for f in [&mut ordinary, &mut candidate] {
        f.app.world_mut().resource_mut::<Controls>().wet = seed;
    }
    let (a, ap) = ordinary.step(100);
    let (b, bp) = candidate.step(100);
    assert_eq!(a.position, b.position);
    assert_eq!(ap, bp);
    assert_eq!(
        ordinary.app.world().resource::<Controls>().wet,
        candidate.app.world().resource::<Controls>().wet
    );
    assert!(
        !candidate
            .app
            .world()
            .resource::<Controls>()
            .surface
            .pending()
    );
}

#[test]
fn surface_normal_dispatch_retains_clock_across_pitches_and_cadences() {
    for pitch in [15_f32, 60., 85.] {
        for cadence in [
            &[100_u64][..],
            &[150][..],
            &[250][..],
            &[100, 117, 167, 200, 250][..],
        ] {
            let mut f = Native::new(pitch.to_radians(), -0.1, false, Instant::now());
            let mut elapsed = 0;
            for i in 0..20 {
                // Scene reconciliation refreshes these containers every
                // frame even when no door/carrier geometry changed.
                {
                    let mut collision = f.app.world_mut().resource_mut::<Collision>();
                    let world = collision.0.as_mut().unwrap();
                    world.set_door_obstacles(Vec::new());
                    world.set_spawn_instances(Vec::new());
                }
                let ms = if i == 0 {
                    100
                } else {
                    cadence[i % cadence.len()]
                };
                let (request, _) = f.step(ms);
                let c = f.app.world().resource::<Controls>();
                assert!(
                    c.surface.pending(),
                    "pitch{pitch}, row{i}: {}",
                    c.surface_report()
                );
                assert!(c.preview.is_none() && !c.release_sample);
                f.accept(&request);
                elapsed += ms;
                let c = f.app.world().resource::<Controls>();
                assert_eq!(
                    c.surface.prior().unwrap().updates(),
                    (elapsed * 60).div_ceil(1000)
                );
            }
        }
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "matched wet/dry handbacks, direct ordinary seed and retained jump diagnostic"
)]
fn surface_handbacks_match_the_ordinary_controller_seeded_from_the_same_carried_state() {
    use eq_client_core::movement::{SurfaceClock, SurfaceStroke, water::WetInput};
    let mut media = std::collections::BTreeSet::new();
    for pitch in [15_f32, 60., 85.] {
        for phase in [100_u64, 117, 167, 250] {
            let mut f = Native::new(pitch.to_radians(), -0.1, false, Instant::now());
            for ms in [100, phase] {
                let (r, _) = f.step(ms);
                f.accept(&r);
            }
            let world = f.app.world().resource::<Collision>().0.as_ref().unwrap();
            let c = f.app.world().resource::<Controls>();
            let position = f
                .app
                .world()
                .resource::<online::OnlineState>()
                .world()
                .player()
                .unwrap()
                .position;
            let feet = Vec3::from_array(eq_client_core::render_position(position)) - Vec3::Y * 3.;
            let clock = c.surface.prior().unwrap();
            media.insert(format!("{:?}", clock.medium()));
            let raw = clock.velocity();
            assert_eq!(c.wet, raw, "acceptance transfers velocity without scaling");
            if clock.medium() == SurfaceRegion::Air {
                let ([right, forward], up) = raw.units_per_second();
                let air = c.airborne.as_ref().unwrap();
                assert_eq!(air.velocity().to_bits(), up.to_bits());
                assert_eq!(air.drift(), Vec3::new(right, 0., forward));
            }
            let swim = c.swim.unwrap();
            let mut direct = Native::at(
                pitch.to_radians(),
                position,
                true,
                f.time + Duration::from_millis(100),
            );
            {
                let mut controls = direct.app.world_mut().resource_mut::<Controls>();
                controls.wet = raw;
                let air = controls.airborne.as_mut().unwrap();
                if clock.medium() == SurfaceRegion::Air {
                    let ([right, forward], up) = raw.units_per_second();
                    air.carry_surface_exit(
                        up,
                        Vec3::new(right, 0., forward),
                        feet.y,
                        clock.dry_peak().unwrap(),
                    );
                } else {
                    air.reset();
                }
            }
            let stored_medium = clock.medium();
            let yaw = eq_client_core::render_heading(position.heading);
            let flat = Vec2::new(yaw.sin(), yaw.cos()).normalize_or_zero() * swim.forward;
            let continuous = SurfaceClock::plan(
                Some(clock),
                world,
                feet,
                raw,
                SurfaceStroke {
                    set: swim.set_for(world.wet_kind(feet, 6.)),
                    input: WetInput {
                        horizontal: [flat.x, flat.y],
                        vertical: swim.forward * pitch.to_radians().sin(),
                        jump: false,
                    },
                    seconds: 0.1,
                    height: 6.,
                },
            )
            .unwrap();
            // Test-only selector hands the identical physical input/state
            // back to the unchanged ordinary path, without changing input.
            f.app
                .world_mut()
                .resource_mut::<Controls>()
                .suppress_surface = true;
            let (ordinary, _) = f.step(100);
            let (seeded, _) = direct.step(100);
            assert_eq!(
                ordinary.position, seeded.position,
                "H2 directly seeded ordinary endpoint at {pitch}/{phase}"
            );
            let actual_controls = f.app.world().resource::<Controls>();
            let direct_controls = direct.app.world().resource::<Controls>();
            assert_eq!(actual_controls.wet, direct_controls.wet);
            let actual_air = actual_controls.airborne.as_ref().unwrap();
            let direct_air = direct_controls.airborne.as_ref().unwrap();
            assert_eq!(
                actual_air.velocity().to_bits(),
                direct_air.velocity().to_bits()
            );
            assert_eq!(actual_air.drift(), direct_air.drift());
            assert_eq!(actual_air.peak(), direct_air.peak());
            let endpoint =
                Vec3::from_array(eq_client_core::render_position(ordinary.position)) - Vec3::Y * 3.;
            if stored_medium == SurfaceRegion::Water {
                assert!(
                    (endpoint - feet).with_y(0.).length()
                        <= (continuous.feet - feet).with_y(0.).length(),
                    "horizontal handback gain at pitch{pitch}/phase{phase}: ordinary={} continuous={}",
                    (endpoint - feet).with_y(0.).length(),
                    (continuous.feet - feet).with_y(0.).length()
                );
                assert!(
                    f64::from(endpoint.distance(feet)) <= continuous.path,
                    "3D handback gain at pitch{pitch}/phase{phase}: ordinary={} continuous={}",
                    endpoint.distance(feet),
                    continuous.path
                );
            }
            println!(
                "H5 {}",
                json!({"pitch":pitch,"prefix_second_ms":phase,"medium":format!("{stored_medium:?}"),"ordinary_endpoint_3d":endpoint.distance(feet),"continuous_owner_path":continuous.path,"diagnostic_3d_difference":f64::from(endpoint.distance(feet))-continuous.path,"direct_ordinary_equal":true,"diagnostic_row":"6.666; dry grouped clock differs from continuous; no bound on dry diagnostic"})
            );
        }
    }
    assert_eq!(
        media.len(),
        2,
        "exercise both actual medium ownership seams"
    );
}

#[test]
fn surface_present_follows_only_the_accepted_endpoint_without_extra_distance() {
    use bevy::ecs::system::RunSystemOnce;
    let mut f = Native::new(15_f32.to_radians(), -0.1, false, Instant::now());
    let (request, _) = f.step(100);
    let root = {
        let w = f.app.world_mut();
        let (entity, transform, body) = w
            .query_filtered::<(Entity, &Transform, &PlayerBody), With<Player>>()
            .single(w)
            .map(|(e, t, b)| (e, *t, *b))
            .unwrap();
        w.despawn(entity);
        crate::character::testing::stores(w);
        let asset = eq_client_assets::characters::CharacterAsset::testing_animations(&[
            "P06", "L09", "L01", "L02",
        ]);
        let prepared = crate::character::testing::prepared_from(w, asset);
        let root = crate::character::testing::drawn(w, &prepared);
        w.entity_mut(root).insert((Player, transform, body));
        root
    };
    f.app
        .init_resource::<Time>()
        .init_resource::<crate::spellbook::SpellNames>()
        .add_systems(
            Update,
            (
                interpolate.after(input),
                crate::character::animate.after(interpolate),
            ),
        );
    // Prime the normal 30 Hz picker without running input or accepting
    // the pending movement. Startup's default clip is not swim evidence.
    f.app
        .world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(50));
    f.app
        .world_mut()
        .run_system_once(crate::character::animate)
        .unwrap();
    assert_eq!(crate::character::testing::clip(f.app.world(), root), "L09");
    let start = Vec3::new(0., -0.1, 0.);
    let drawn = |app: &mut App| {
        let w = app.world_mut();
        w.query_filtered::<&Transform, With<Player>>()
            .single(w)
            .unwrap()
            .translation
    };
    f.app
        .world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(16));
    f.app.update();
    assert_eq!(
        drawn(&mut f.app),
        start,
        "pending cannot play unaccepted travel"
    );
    f.accept(&request);
    let end = Vec3::from_array(eq_client_core::render_position(request.position));
    let mut last = 0.;
    let mut last_swim_phase = None;
    let mut dry_swim = false;
    for _ in 0..12 {
        f.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(16));
        f.app.update();
        let at = drawn(&mut f.app);
        let distance = at.distance(start);
        assert!(distance >= last && distance <= end.distance(start) + 0.000_001);
        last = distance;
        let clip = crate::character::testing::clip(f.app.world(), root);
        assert!(matches!(clip, "P06" | "L09"));
        if matches!(clip, "P06" | "L09") {
            let phase = crate::character::testing::elapsed(f.app.world(), root);
            if let Some(previous) = last_swim_phase {
                assert!(phase >= previous, "dry origin restarted swim");
            }
            last_swim_phase = Some(phase);
            dry_swim |= at.y > 0.;
        }
    }
    assert!(dry_swim, "exercise the actual dry-origin/wet-soles picker");
    assert!(drawn(&mut f.app).distance(end) < 0.000_001);
    assert!(
        f.receiver
            .try_iter()
            .all(|c| !matches!(c, ClientCommand::Move(_))),
        "presentation adds no sample"
    );
}

#[test]
fn surface_dry_local_results_keep_peak_and_handback_keeps_it_on_the_descent() {
    let mut fixture = Native::new(85_f32.to_radians(), -0.1, false, Instant::now());
    for ms in [100, 117] {
        let (request, _) = fixture.step(ms);
        fixture.accept(&request);
    }
    let controls = fixture.app.world().resource::<Controls>();
    let peak = controls.surface.prior().unwrap().dry_peak().unwrap();
    assert_eq!(controls.airborne.as_ref().unwrap().peak(), Some(peak));
    fixture
        .app
        .world_mut()
        .resource_mut::<Controls>()
        .suppress_surface = true;
    let (ordinary, _) = fixture.step(100);
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<Controls>()
            .airborne
            .as_ref()
            .unwrap()
            .peak(),
        Some(peak)
    );
    fixture.accept(&ordinary);
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<Controls>()
            .airborne
            .as_ref()
            .unwrap()
            .peak(),
        Some(peak)
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "bounded matched H4 inputs, guarded requests and complete travel ledgers"
)]
fn surface_h4_redundant_forward_stays_eligible_without_compounding_gain() {
    use eq_client_core::movement::water::{LIQUID_5, WetFrameMedium, WetInput, WetVelocity};
    let mut failures = Vec::new();
    for pitch in [15_f32, 60., 85.] {
        for cadence in [
            &[100_u64][..],
            &[150][..],
            &[250][..],
            &[100, 117, 167, 200, 250][..],
        ] {
            let base = Instant::now();
            let mut alternating = Native::new(pitch.to_radians(), -0.1, false, base);
            let mut continuous = Native::new(pitch.to_radians(), -0.1, false, base);
            let mut deep = WetVelocity::default();
            let mut elapsed = 0_u64;
            let (
                mut index,
                mut actual_path,
                mut reference_path,
                mut actual_h,
                mut reference_h,
                mut deep_path,
                mut deep_h,
            ) = (0_usize, 0_f64, 0_f64, 0_f64, 0_f64, 0_f64, 0_f64);
            let (mut active_samples, mut handbacks, mut retakes, mut previously_active) =
                (0, 0, 0, false);
            let mut previous_actual = Vec3::new(0., -0.1, 0.);
            let mut previous_reference = previous_actual;
            let mut rows = Vec::new();
            while elapsed < 10000 {
                let ms = (if index == 0 {
                    100
                } else {
                    cadence[index % cadence.len()]
                })
                .min(10000 - elapsed);
                assert!(ms >= 100, "normal100ms sample gate; no extra final sample");
                let ineligible = index % 2 == 1;
                {
                    // This was the original H4 handback trigger. It now
                    // shares the ordinary stroke and must remain eligible.
                    let mut buttons = alternating
                        .app
                        .world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>();
                    if ineligible {
                        buttons.press(KeyCode::KeyT);
                    } else {
                        buttons.release(KeyCode::KeyT);
                    }
                }
                let (actual, _) = alternating.step(ms);
                let (reference, _) = continuous.step(ms);
                let actual_controls = alternating.app.world().resource::<Controls>();
                let reference_controls = continuous.app.world().resource::<Controls>();
                assert_eq!(
                    actual_controls.sampling.motion_intent,
                    reference_controls.sampling.motion_intent,
                    "physical eligibility pulse changed motion intent"
                );
                assert_eq!(actual.position.heading, reference.position.heading);
                actual_path += actual_controls.sampling.proposal_path.unwrap();
                reference_path += reference_controls.sampling.proposal_path.unwrap();
                let active = actual_controls.surface.active();
                assert!(active, "ordinary redundant forward lost continuation");
                active_samples += usize::from(active);
                handbacks += usize::from(previously_active && !active);
                retakes += usize::from(!previously_active && active && index > 0);
                previously_active = active;
                let point = Vec3::from_array(eq_client_core::render_position(actual.position));
                let reference_point =
                    Vec3::from_array(eq_client_core::render_position(reference.position));
                actual_h += f64::from((point - previous_actual).with_y(0.).length());
                reference_h +=
                    f64::from((reference_point - previous_reference).with_y(0.).length());
                previous_actual = point;
                previous_reference = reference_point;
                let mut left = Duration::from_millis(ms).as_secs_f32();
                while left > 0. {
                    let piece = deep
                        .advance_piece(
                            WetFrameMedium::Water(LIQUID_5),
                            WetInput {
                                horizontal: [1.7, 0.],
                                vertical: 1.7 * pitch.to_radians().sin(),
                                jump: false,
                            },
                            left,
                        )
                        .unwrap();
                    left -= piece.seconds;
                    let delta = Vec3::new(piece.horizontal[0], piece.vertical, piece.horizontal[1]);
                    deep_h += f64::from(delta.with_y(0.).length());
                    deep_path += delta.as_dvec3().length();
                }
                alternating.accept(&actual);
                continuous.accept(&reference);
                assert_eq!(actual.position, reference.position);
                elapsed += ms;
                index += 1;
                rows.push(json!({"sample":index,"ms":ms,"elapsed_ms":elapsed,"ineligible_physical_key":ineligible,
                    "active":active,"actual_mode":format!("{:?}",actual.mode),"actual":point.to_array(),
                    "actual_path":actual_path,"reference_path":reference_path,"deep_path":deep_path}));
            }
            let failed =
                actual_h > reference_h * 1.04 || actual_h > deep_h || actual_path > deep_path;
            let report = json!({"pitch":pitch,"cadence":cadence,"elapsed_ms":elapsed,"samples":index,
                "active_samples":active_samples,"handbacks":handbacks,"retakes":retakes,
                "horizontal":actual_h,"continuous_horizontal":reference_h,"deep_horizontal":deep_h,
                "resolved_path":actual_path,"continuous_resolved_path":reference_path,"deep_path":deep_path,
                "failed":failed,"rows":rows});
            println!("H4 {report}");
            if failed {
                failures.push(report);
            }
        }
    }
    assert!(
        failures.is_empty(),
        "H4 repeated-gain failures: {failures:?}"
    );
}

fn assert_ordinary_input(action: &str, controls: &Controls) {
    match action {
        "autorun" => assert!(controls.autorun, "ordinary physical toggle must latch"),
        "diagonal" => assert!(controls.diagonal_rates.run.is_some()),
        "strafe" => assert!(matches!(
            controls.sampling.motion_intent.unwrap().0,
            MovementMode::Strafe(_)
        )),
        "turn" => assert_ne!(controls.sampling.motion_intent.unwrap().3, 0.),
        _ => (),
    }
}

#[test]
fn surface_ordinary_turn_strafe_pitch_and_autorun_keep_the_clock() {
    for action in [
        "turn", "mouse", "strafe", "diagonal", "pitch", "autorun", "walk",
    ] {
        let mut f = Native::new(15_f32.to_radians(), -0.1, false, Instant::now());
        crate::keys::testing::bind(
            &mut f.app,
            Act::StrafeLeft,
            crate::keys::Chord::key(KeyCode::KeyZ),
        );
        crate::keys::testing::bind(
            &mut f.app,
            Act::Autorun,
            crate::keys::Chord::key(KeyCode::KeyR),
        );
        let (first, _) = f.step(100);
        f.accept(&first);
        let updates = f
            .app
            .world()
            .resource::<Controls>()
            .surface
            .prior()
            .unwrap()
            .updates();
        match action {
            "turn" => f
                .app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyQ),
            "mouse" => {
                f.app.insert_resource(MouseLook {
                    held: true,
                    left: false,
                });
                f.app.insert_resource(MouseAim {
                    horizontal: Some(0.1),
                    heading: Some(400.),
                    acquired: true,
                    interrupted: false,
                });
            }
            "strafe" | "diagonal" => {
                let mut keys = f.app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                if action == "strafe" {
                    keys.release(KeyCode::KeyW);
                }
                keys.press(KeyCode::KeyZ);
            }
            "pitch" => {
                let world = f.app.world_mut();
                let mut query = world.query::<&mut OrbitCamera>();
                query.single_mut(world).unwrap().look = 60_f32.to_radians();
            }
            "autorun" => {
                let mut keys = f.app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.release(KeyCode::KeyW);
                keys.press(KeyCode::KeyR);
            }
            "walk" => f.app.world_mut().resource_mut::<Controls>().walking = true,
            _ => unreachable!(),
        }
        // Whole physical-frame intervals isolate input eligibility. A
        // genuine short strafe partial keeps its owner veto below.
        for ms in [100, 150, 250] {
            let (request, _) = f.step(ms);
            assert_ordinary_input(action, f.app.world().resource::<Controls>());
            assert!(
                f.app.world().resource::<Controls>().surface.pending(),
                "{action}: {}",
                f.app.world().resource::<Controls>().surface_report()
            );
            f.accept(&request);
            assert!(
                f.app.world().resource::<Controls>().surface.active(),
                "{action}"
            );
        }
        assert!(
            f.app
                .world()
                .resource::<Controls>()
                .surface
                .prior()
                .unwrap()
                .updates()
                > updates
        );
    }
}

#[test]
fn surface_short_strafe_partial_keeps_the_ordinary_owner() {
    let base = Instant::now();
    let mut continued = Native::new(15_f32.to_radians(), -0.1, false, base);
    let mut ordinary = Native::new(15_f32.to_radians(), -0.1, false, base);
    for f in [&mut continued, &mut ordinary] {
        crate::keys::testing::bind(
            &mut f.app,
            Act::StrafeLeft,
            crate::keys::Chord::key(KeyCode::KeyZ),
        );
        let (first, _) = f.step(100);
        f.accept(&first);
        let mut keys = f.app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::KeyW);
        keys.press(KeyCode::KeyZ);
    }
    ordinary
        .app
        .world_mut()
        .resource_mut::<Controls>()
        .suppress_surface = true;
    let (attempted, attempted_packet) = continued.step(117);
    let (direct, direct_packet) = ordinary.step(117);
    let controls = continued.app.world().resource::<Controls>();
    assert_ordinary_input("strafe", controls);
    assert_eq!(
        controls.surface.last_decline,
        Some(eq_client_core::movement::SurfaceDecline::Shortened)
    );
    assert!(!controls.surface.pending());
    assert_eq!(attempted.position, direct.position);
    assert_eq!(attempted.mode, direct.mode);
    assert_eq!(attempted_packet, direct_packet);
    continued.accept(&attempted);
    ordinary.accept(&direct);
    assert_eq!(
        continued.app.world().resource::<Controls>().wet,
        ordinary.app.world().resource::<Controls>().wet
    );
}

#[test]
fn surface_retake_delay_after_one_ineligible_interval_is_reported() {
    for pitch in [15_f32, 60., 85.] {
        for cadence in [
            &[100_u64][..],
            &[150][..],
            &[250][..],
            &[100, 117, 167, 200, 250][..],
        ] {
            let mut f = Native::new(pitch.to_radians(), -0.1, false, Instant::now());
            for ms in [100, cadence[0]] {
                let (request, _) = f.step(ms);
                f.accept(&request);
            }
            f.app
                .world_mut()
                .resource_mut::<Controls>()
                .suppress_surface = true;
            let (ordinary, _) = f.step(cadence[0]);
            f.accept(&ordinary);
            f.app
                .world_mut()
                .resource_mut::<Controls>()
                .suppress_surface = false;
            let mut elapsed = 0_u64;
            let mut rows = Vec::new();
            let mut retake = None;
            for index in 0..100 {
                let ms = cadence[index % cadence.len()];
                let before = f.app.world().resource::<Controls>().wet.frame_phase();
                let (request, _) = f.step(ms);
                let c = f.app.world().resource::<Controls>();
                rows.push(json!({"elapsed_ms":elapsed,"span_ms":ms,"phase":format!("{before:?}"),"decline":format!("{:?}",c.surface.last_decline),"position":[request.position.x,request.position.y,request.position.z],"pending":c.surface.pending(),"boundary_delay_seconds":c.surface.boundary_delay}));
                if c.surface.pending() {
                    retake = Some(
                        (Duration::from_millis(elapsed).as_secs_f64()
                            + f64::from(c.surface.boundary_delay))
                            * 1000.,
                    );
                }
                f.accept(&request);
                elapsed += ms;
                if retake.is_some() || elapsed >= 10000 {
                    break;
                }
            }
            println!(
                "R3 {}",
                json!({"pitch":pitch,"cadence":cadence,"retake_ms":retake,"observed_ms":elapsed,"rows":rows})
            );
            // These measured rows all reach a proved boundary within a
            // second. Keep both the physics and local-result deadlines as
            // regressions; never reset phase to make the diagnostic pass.
            assert!(retake.is_some_and(|delay| (0. ..=1000.).contains(&delay)));
            assert!(
                elapsed <= 1000,
                "local acceptance missed the measured one-second bound"
            );
        }
    }
}

#[test]
fn surface_retake_uses_first_actual_wet_boundary_inside_the_request() {
    let base = Instant::now();
    let mut f = Native::new(0., -20., true, base);
    let mut ordinary = Native::new(0., -20., true, base);
    let (a, ap) = f.step(117);
    let (b, bp) = ordinary.step(117);
    assert_eq!(a.position, b.position);
    assert_eq!(ap, bp);
    f.accept(&a);
    ordinary.accept(&b);
    let phase = f
        .app
        .world()
        .resource::<Controls>()
        .wet
        .frame_phase()
        .unwrap();
    assert!(!phase.at_boundary);
    f.app
        .world_mut()
        .resource_mut::<Controls>()
        .suppress_surface = false;
    let (a, ap) = f.step(100);
    let (b, bp) = ordinary.step(100);
    assert_eq!(
        a.position, b.position,
        "passive recovery changes no ordinary travel"
    );
    assert_eq!(ap, bp);
    let c = f.app.world().resource::<Controls>();
    assert!(c.surface.pending(), "first actual wet boundary was missed");
    assert_eq!(c.surface.boundary_delay, phase.remaining_seconds);
    f.accept(&a);
    ordinary.accept(&b);
    assert!(
        f.app
            .world()
            .resource::<Controls>()
            .surface
            .prior()
            .is_some()
    );
    assert_eq!(
        f.app.world().resource::<Controls>().wet,
        ordinary.app.world().resource::<Controls>().wet
    );
}

#[test]
fn surface_clock_holds_at_a_slight_uptilt_while_a_mob_nearby_moves() {
    use eq_client_core::movement::{CollisionInstance, CollisionMesh};
    let mesh = std::sync::Arc::new(
        CollisionMesh::new([[[-2., -2., 0.], [2., -2., 0.], [-2., 2., 0.]]]).unwrap(),
    );
    let mut f = Native::new(10.7_f32.to_radians(), -0.1, false, Instant::now());
    let start = f.session.position();
    for i in 0..60_u16 {
        {
            // The mob swims a unit a frame, well clear of the swimmer.
            let mut collision = f.app.world_mut().resource_mut::<Collision>();
            let world = collision.0.as_mut().unwrap();
            let at = Vec3::new(60. - f32::from(i), -3., 60.);
            world.set_door_obstacles(Vec::new());
            world.set_spawn_instances(vec![(
                8,
                CollisionInstance::new(std::sync::Arc::clone(&mesh), at, Quat::IDENTITY).unwrap(),
            )]);
        }
        let ms = [100_u64, 117, 133, 150][usize::from(i) % 4];
        let (request, _) = f.step(ms);
        let c = f.app.world().resource::<Controls>();
        assert!(c.surface.pending(), "row {i}: {}", c.surface_report());
        f.accept(&request);
    }
    // About 7.5 s at the surface: the clock's travel, not the ordinary
    // path's crossing-cut strokes (about 3 units a second).
    let end = f.session.position();
    let travel = (end.x - start.x).hypot(end.y - start.y);
    assert!(travel > 70., "{travel}");
}
