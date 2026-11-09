//! The local collision half of Titanium's ordinary shoreline impulse.

use bevy::prelude::Vec3;
use eq_client_core::{
    MovementMode,
    movement::{
        CollisionWorld, SHORELINE_COOLDOWN, SHORELINE_FRAMES, ShorelineFrame, ShorelineStroke,
        ShorelineTrace, ShorelineVelocity, SwimDepth,
    },
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Arc {
    started: Instant,
    velocity: ShorelineVelocity,
    dry: bool,
}

#[derive(Clone, Copy)]
struct ObservedFrame {
    millis: u8,
    stroke: ShorelineStroke,
    forward_up: bool,
}

#[derive(Default)]
pub(super) struct Control {
    observed: Option<Instant>,
    sub_millis: Duration,
    frames: VecDeque<ObservedFrame>,
    previous_swim: bool,
    last_attempt: Option<Instant>,
    active: Option<Arc>,
    pending: Option<(MovementMode, Option<Arc>, Instant)>,
}

#[derive(Clone, Copy)]
pub(super) struct Body<'a> {
    pub world: &'a CollisionWorld,
    pub feet: Vec3,
    pub height: f32,
    pub feet_offset: f32,
    pub heading: f32,
    pub gravity: f32,
    pub grant: f32,
}

pub(super) struct Proposal {
    pub feet: Vec3,
    pub trace: ShorelineTrace,
    pub created: Instant,
    active: Option<Arc>,
}

impl Proposal {
    pub fn finish(&mut self) {
        self.trace.finish = true;
        self.active = None;
    }
}

enum End {
    Continue,
    Landed,
    Interrupted,
}

fn advance(
    body: Body<'_>,
    arc: &mut Arc,
    feet: &mut Vec3,
    observed: ObservedFrame,
) -> Option<(ShorelineFrame, End)> {
    let water = body.world.in_swimmable_water(*feet, body.height);
    if arc.dry && water {
        return None;
    }
    arc.dry |= !water;
    let frame = ShorelineFrame::new(observed.millis, water, false, observed.stroke)?;
    arc.velocity.advance(frame, body.grant, body.gravity);
    let ([x, y], vertical) = arc.velocity.travel(frame);
    let step = body
        .world
        .shoreline_step(*feet, Vec3::new(y, 0.0, -x), vertical, body.height)?;
    *feet = step.feet;
    // A wall clips travel, but does not grant more momentum. Preserve the
    // decaying impulse while a separately clear rise clears the lip.
    let frame = ShorelineFrame::new(observed.millis, water, step.stop_vertical, observed.stroke)?;
    frame.stop(&mut arc.velocity);
    let end = if step.supported {
        End::Landed
    } else if arc.dry && body.world.in_swimmable_water(*feet, body.height)
        || step.stop_vertical && vertical > 0.0
    {
        End::Interrupted
    } else {
        End::Continue
    };
    Some((frame, end))
}

/// Require a swept landing at the most recently observed cadence before
/// starting. Only actual frame durations are used for submitted positions.
fn has_landing(body: Body<'_>, mut arc: Arc, observed: ObservedFrame) -> bool {
    let mut feet = body.feet;
    // Bound even a trace from a very fast renderer. A shorter preview
    // can conservatively leave the ordinary swim in place.
    let frames = (SHORELINE_COOLDOWN.as_millis() / u128::from(observed.millis)).min(256);
    for _ in 0..frames {
        match advance(body, &mut arc, &mut feet, observed) {
            Some((_, End::Continue)) => (),
            Some((_, End::Landed)) => return true,
            _ => return false,
        }
    }
    false
}

impl Control {
    pub fn active(&self) -> bool {
        self.active.is_some()
    }

    pub fn reset(&mut self) {
        self.observed = None;
        self.sub_millis = Duration::ZERO;
        self.frames.clear();
        self.active = None;
        self.previous_swim = false;
        self.pending = None;
    }

    pub fn observe(&mut self, now: Instant, direction: Vec3, forward_up: bool) {
        let Some(previous) = self.observed.replace(now) else {
            return;
        };
        let elapsed = now.saturating_duration_since(previous) + self.sub_millis;
        let millis = elapsed.as_millis();
        self.sub_millis = Duration::from_nanos(u64::from(elapsed.subsec_nanos() % 1_000_000));
        if self.frames.len() == SHORELINE_FRAMES {
            if self.active.is_some()
                || self
                    .pending
                    .as_ref()
                    .is_some_and(|(mode, _, _)| matches!(mode, MovementMode::Shoreline(_)))
            {
                // This arc cannot skip an unaccounted frame, even during an
                // in-flight result. A late result must not restore it.
                self.reset();
                self.observed = Some(now);
                return;
            }
            self.frames.pop_front();
        }
        self.frames.push_back(ObservedFrame {
            millis: u8::try_from(millis.min(150)).unwrap_or(150),
            stroke: ShorelineStroke::new([-direction.z, direction.x, direction.y]),
            forward_up,
        });
    }

    pub fn plan(&mut self, body: Body<'_>, sampled: Instant, limit: Duration) -> Option<Proposal> {
        // Use the same whole-millisecond grid as observe(). Quantizing
        // absolute frame times preserves fractional time across samples.
        let now = self.observed?.checked_sub(self.sub_millis)?;
        let available = now
            .saturating_duration_since(sampled)
            .min(limit)
            .as_millis();
        let mut feet = body.feet;
        let start = self.active.is_none();
        let launch = if start {
            // An ordinary Swim stamp may be later than its observed frame.
            // A fresh impulse only spends the current frame's overlap with
            // the accepted sample window. No carried physics predates it.
            let mut frame = *self.frames.back()?;
            frame.millis = u8::try_from(u128::from(frame.millis).min(available)).ok()?;
            Some(frame)
        } else {
            None
        };
        let mut arc = if let Some(active) = self.active {
            if now.saturating_duration_since(active.started) >= SHORELINE_COOLDOWN {
                return None;
            }
            active
        } else {
            let (x, y) = (body.heading / 512.0 * std::f32::consts::TAU).sin_cos();
            let heading = Vec3::new(y, 0.0, -x);
            if !self.previous_swim
                || self
                    .last_attempt
                    .is_some_and(|last| now.saturating_duration_since(last) < SHORELINE_COOLDOWN)
                || body.world.swim_depth(feet, body.height) != Some(SwimDepth::Surface)
            {
                return None;
            }
            let observed = launch?;
            if observed.millis == 0 || !observed.forward_up || !observed.stroke.upward() {
                return None;
            }
            self.last_attempt = Some(now);
            if !body.world.shoreline_ahead(feet, heading, body.feet_offset) {
                return None;
            }
            let arc = Arc {
                started: now,
                velocity: ShorelineVelocity::start(body.heading),
                dry: false,
            };
            if !has_landing(body, arc, observed) {
                return None;
            }
            arc
        };
        // Only a fresh launch can be shortened. Continuations consume whole
        // frames within this sample, including the per-frame damping.
        let frames =
            launch.iter().chain(
                self.frames
                    .iter()
                    .take(if start { 0 } else { self.frames.len() }),
            );
        let millis: u128 = frames.clone().map(|frame| u128::from(frame.millis)).sum();
        if millis > available {
            self.reset();
            return None;
        }
        if millis == 0 {
            return None;
        }
        let mut trace = ShorelineTrace {
            start,
            finish: false,
            frames: [ShorelineFrame::EMPTY; SHORELINE_FRAMES],
        };
        for (slot, &observed) in trace.frames.iter_mut().zip(frames) {
            let (frame, end) = advance(body, &mut arc, &mut feet, observed)?;
            *slot = frame;
            trace.finish = !matches!(end, End::Continue);
            if trace.finish {
                break;
            }
        }
        Some(Proposal {
            feet,
            trace,
            created: now,
            active: (!trace.finish).then_some(arc),
        })
    }

    pub fn sent(&mut self, mode: MovementMode, plan: Option<Proposal>, stamp: Instant) {
        self.frames.clear();
        self.observed.get_or_insert(stamp);
        self.pending = Some((mode, plan.and_then(|plan| plan.active), stamp));
    }

    pub fn resolved(&mut self, refused: bool) {
        if let Some((mode, active, stamp)) = self.pending.take() {
            if refused {
                // The worker never spent the rejected trace. Its consumed
                // observations cannot be skipped by resuming the old arc.
                self.reset();
                return;
            }
            self.previous_swim = mode == MovementMode::Swim;
            self.active = active;
            if matches!(mode, MovementMode::Shoreline(trace) if trace.start) {
                self.last_attempt = Some(stamp);
                if let Some(active) = self.active.as_mut() {
                    active.started = stamp;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::movement::{Liquid, Liquids};

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Gap {
        KnownDry,
        Unresolved,
    }

    #[derive(Clone)]
    struct Canal {
        second_pool: bool,
        gap: Option<Gap>,
    }

    const GAP: std::ops::Range<f32> = -0.62..-0.61;
    const FRESH_LIMIT: Duration = Duration::from_millis(100);

    impl Liquids for Canal {
        fn liquid_at(&self, point: Vec3) -> Option<Liquid> {
            (point.y < 0.0
                && (point.x < 0.0 || self.second_pool && point.x > 0.25)
                && !(self.gap.is_some() && GAP.contains(&point.x)))
            .then_some(Liquid::Water)
        }

        fn region_known(&self, point: Vec3) -> bool {
            !(self.gap == Some(Gap::Unresolved) && GAP.contains(&point.x))
        }

        fn segment_known(&self, start: Vec3, end: Vec3) -> bool {
            self.gap != Some(Gap::Unresolved)
                || start.x.max(end.x) < GAP.start
                || start.x.min(end.x) >= GAP.end
        }

        fn passes_through(&self, _: Vec3, _: Vec3, _: Liquid) -> bool {
            false
        }

        fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
            Some(Box::new(self.clone()))
        }
    }

    fn world(bank: Option<f32>, ceiling: bool, second_pool: bool) -> CollisionWorld {
        world_with_gap(bank, ceiling, second_pool, None)
    }

    fn world_with_gap(
        bank: Option<f32>,
        ceiling: bool,
        second_pool: bool,
        gap: Option<Gap>,
    ) -> CollisionWorld {
        let quad = |a, b, c, d| [[a, b, c], [a, c, d]];
        let mut triangles = Vec::from(quad(
            [-10.0, -8.0, -10.0],
            [0.0, -8.0, -10.0],
            [0.0, -8.0, 10.0],
            [-10.0, -8.0, 10.0],
        ));
        if let Some(y) = bank {
            triangles.extend(quad(
                [0.0, y, -10.0],
                [10.0, y, -10.0],
                [10.0, y, 10.0],
                [0.0, y, 10.0],
            ));
            triangles.extend(quad(
                [0.0, -8.0, -10.0],
                [0.0, y, -10.0],
                [0.0, y, 10.0],
                [0.0, -8.0, 10.0],
            ));
        }
        if ceiling {
            triangles.extend(quad(
                [-2.0, 3.9, -10.0],
                [2.0, 3.9, -10.0],
                [2.0, 3.9, 10.0],
                [-2.0, 3.9, 10.0],
            ));
        }
        CollisionWorld::new(triangles)
            .unwrap()
            .with_liquids(Canal { second_pool, gap })
            .with_swim_origin(3.75)
    }

    fn body(world: &CollisionWorld) -> Body<'_> {
        Body {
            world,
            feet: Vec3::new(-0.7, -3.76, 0.0),
            height: 7.5,
            feet_offset: 3.75,
            heading: 0.0,
            gravity: 160.0,
            grant: 30.0,
        }
    }

    fn controls(start: Instant) -> Control {
        let mut controls = Control::default();
        controls.sent(MovementMode::Swim, None, start);
        controls.resolved(false);
        controls.observe(
            start + Duration::from_millis(16),
            Vec3::new(0.9, 0.4, 0.0),
            true,
        );
        controls
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "One fixture checks the renderer, matching feedback, and worker transaction"
    )]
    fn blocked_swim_from_fresh_admission_can_start_shoreline_with_fly_off() {
        use crate::{
            Collision, OrbitCamera, Player, PlayerBody, motion::Controls, online, options,
        };
        use bevy::prelude::{ButtonInput, IntoScheduleConfigs, KeyCode, Transform, Update, With};
        use eq_client_core::{ClientCommand, MotionCalibration, WorldEvent, world_position};
        use eq_network_game::movement::{DiagonalRates, MotionSession};

        let (mut app, commands) = super::super::tests::fixture();
        let collision = world(Some(-2.75), false, false);
        let feet = Vec3::new(-0.431, -3.76, 0.0);
        let origin = feet + Vec3::Y * 3.75;
        let placement = world_position(origin.to_array(), 0.0);
        assert_eq!(collision.swim_depth(feet, 7.5), Some(SwimDepth::Surface));
        assert_eq!(collision.swim_step(feet, Vec3::X * 0.46, 7.5), feet);
        app.insert_resource(Collision(Some(collision)))
            .init_resource::<options::MovementSync>()
            .add_systems(Update, (options::tell_choices, super::super::input).chain());
        let set_pitch = |world: &mut bevy::prelude::World, pitch| {
            let mut camera = world.query::<&mut OrbitCamera>();
            let mut camera = camera.single_mut(world).unwrap();
            camera.yaw = crate::camera::behind(eq_client_core::render_heading(0.0));
            camera.pitch = pitch;
        };
        {
            let world = app.world_mut();
            online::testing::place(&mut world.resource_mut::<online::OnlineState>(), placement);
            let mut player =
                world.query_filtered::<(&mut PlayerBody, &mut Transform), With<Player>>();
            let (mut body, mut transform) = player.single_mut(world).unwrap();
            body.feet_offset = 3.75;
            body.height = 7.5;
            transform.translation = origin;
            world.resource_mut::<Controls>().speed = Some(30.0);
            world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
            set_pitch(world, 0.0);
        }
        app.update();
        assert!(!app.world().resource::<Controls>().movement_options.fly);
        assert!(commands.try_iter().all(|command| !matches!(
            command,
            ClientCommand::Move(_) | ClientCommand::SetMovementOptions { .. }
        )));

        let start = Instant::now();
        let mut worker = MotionSession::new(11, 7, placement, start)
            .unwrap()
            .with_swimming(true)
            .with_jump(Some((44.55, 160.0)));
        worker
            .calibrate(
                MotionCalibration {
                    units_per_second: 30.0,
                    velocity_scale: 1.0,
                    animation: 1,
                    diagonal: DiagonalRates::default(),
                    strafe: None,
                    walk: None,
                    backward: None,
                },
                start,
            )
            .unwrap();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.update();
        let ClientCommand::Move(mut swim) = commands.try_recv().unwrap() else {
            panic!("expected the first ordinary swim sample");
        };
        assert_eq!(swim.mode, MovementMode::Swim);
        assert_eq!(
            (swim.position.x, swim.position.y, swim.position.z),
            (placement.x, placement.y, placement.z)
        );
        let queue_delay = Duration::from_micros(3_500);
        let stamp = swim.created;
        // The ordinary stroke was observed at 100 ms and queued at 103 ms.
        // The worker's processing clock is later than the request stamp.
        swim.created = start + Duration::from_millis(103);
        worker
            .send_move_sample(&swim, swim.created + queue_delay, |_| Ok(()))
            .unwrap();
        online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [WorldEvent::MotionSent {
                session_id: 11,
                created: stamp,
                position: swim.position,
                refused: None,
            }],
        );
        {
            let world = app.world_mut();
            set_pitch(world, 0.4);
            let mut controls = world.resource_mut::<Controls>();
            assert!(controls.moved(Some(11), stamp));
            controls.resolve_water_entry(false);
            // Set only the fixture clocks. The matching result establishes the
            // prior Swim mode in both the renderer and the worker.
            let now = Instant::now();
            controls.last_accepted = now.checked_sub(Duration::from_millis(100)).unwrap();
            controls.last_sent = now.checked_sub(Duration::from_millis(113));
            controls.shoreline.observed = now.checked_sub(Duration::from_micros(16_600));
        }
        app.update();
        let ClientCommand::Move(mut boost) = commands.try_recv().unwrap() else {
            panic!("expected a shoreline sample after the accepted blocked stroke");
        };
        assert!(
            matches!(boost.mode, MovementMode::Shoreline(trace) if trace.start && !trace.finish)
        );
        assert!(boost.position.z > swim.position.z);
        assert!(commands.try_recv().is_err());
        let boost_stamp = boost.created;
        boost.created = start + Duration::from_millis(216);
        worker
            .send_move_sample(&boost, boost.created + queue_delay, |_| Ok(()))
            .unwrap();
        online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [WorldEvent::MotionSent {
                session_id: 11,
                created: boost_stamp,
                position: boost.position,
                refused: None,
            }],
        );
        {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            assert_eq!(controls.last_sent, Some(boost_stamp));
            assert_eq!(
                controls
                    .shoreline
                    .observed
                    .and_then(|seen| seen.checked_sub(controls.shoreline.sub_millis)),
                Some(boost_stamp)
            );
            assert!(controls.moved(Some(11), boost_stamp));
            controls.resolve_water_entry(false);
        }

        // Age the clocks together before each renderer frame. Their relative
        // sample time and fractional frame remainder stay intact.
        let frame = Duration::from_micros(16_600);
        let mut aged = Duration::ZERO;
        let ClientCommand::Move(mut continued) = (0..8)
            .find_map(|_| {
                {
                    let mut controls = app.world_mut().resource_mut::<Controls>();
                    controls.last_accepted = controls.last_accepted.checked_sub(frame).unwrap();
                    controls.last_sent =
                        controls.last_sent.and_then(|sent| sent.checked_sub(frame));
                    controls.shoreline.observed = controls
                        .shoreline
                        .observed
                        .and_then(|seen| seen.checked_sub(frame));
                }
                aged += frame;
                app.update();
                commands.try_recv().ok()
            })
            .expect("next sampled shoreline frame")
        else {
            panic!("expected a continuation movement request");
        };
        assert!(matches!(continued.mode, MovementMode::Shoreline(trace) if !trace.start));
        let elapsed = continued
            .created
            .duration_since(boost_stamp.checked_sub(aged).unwrap());
        assert!(elapsed >= Duration::from_millis(100));
        continued.created = boost.created + elapsed;
        worker
            .send_move_sample(&continued, continued.created + queue_delay, |_| Ok(()))
            .unwrap();
    }

    #[test]
    fn shoreline_starts_on_the_observed_frame_and_held_input_does_not_retrigger() {
        let start = Instant::now();
        let now = start + Duration::from_millis(16);
        let world = world(Some(-2.75), false, false);
        let mut controls = controls(start);
        let body = body(&world);
        let proposal = controls
            .plan(body, start, FRESH_LIMIT)
            .expect("reachable low lip");
        assert!(proposal.trace.start);
        assert!((proposal.trace.frames[0].seconds() - 0.016).abs() < 1e-6);
        assert_eq!(proposal.trace.frames[1], ShorelineFrame::EMPTY);
        assert!(proposal.feet.y > body.feet.y && proposal.feet.y < body.feet.y + 1.0);
        controls.sent(MovementMode::Shoreline(proposal.trace), Some(proposal), now);
        controls.resolved(true);
        assert!(!controls.active());
        let next = now + Duration::from_millis(100);
        controls.observe(next, Vec3::new(0.9, 0.4, 0.0), true);
        assert!(controls.plan(body, start, FRESH_LIMIT).is_none());
    }

    #[test]
    fn shoreline_requires_a_clear_supported_landing() {
        let start = Instant::now();
        for world in [
            world(None, false, false),
            world(None, false, true),
            world(Some(6.0), false, false),
            world(Some(-2.75), true, false),
        ] {
            let mut controls = controls(start);
            assert!(controls.plan(body(&world), start, FRESH_LIMIT).is_none());
        }
    }

    #[test]
    fn shoreline_sweeps_thin_spans_between_wet_endpoints() {
        let start = Instant::now();
        for gap in [Gap::KnownDry, Gap::Unresolved] {
            let known = gap == Gap::KnownDry;
            let world = world_with_gap(Some(-2.75), false, false, Some(gap));
            let body = body(&world);
            let horizontal = Vec3::X * 0.2;
            assert!(world.in_swimmable_water(body.feet, body.height));
            assert!(world.in_swimmable_water(body.feet + horizontal, body.height));
            assert_eq!(
                world
                    .shoreline_step(body.feet, horizontal, 0.0, body.height)
                    .is_some(),
                known,
            );
            assert_eq!(
                controls(start).plan(body, start, FRESH_LIMIT).is_some(),
                known
            );
        }
    }

    #[test]
    fn shoreline_retires_when_observed_frames_cannot_all_be_accounted_for() {
        let start = Instant::now();
        let now = start + Duration::from_millis(16);
        let world = world(Some(-2.75), false, false);
        let body = body(&world);
        for overflow in [false, true] {
            let mut controls = controls(start);
            let proposal = controls
                .plan(body, start, FRESH_LIMIT)
                .expect("reachable low lip");
            let next_body = Body {
                feet: proposal.feet,
                ..body
            };
            controls.sent(MovementMode::Shoreline(proposal.trace), Some(proposal), now);
            if !overflow {
                controls.resolved(false);
                assert!(controls.active());
            }
            let frames = if overflow {
                SHORELINE_FRAMES as u64 + 1
            } else {
                2
            };
            for index in 1..=frames {
                let step_ms = if overflow { 3 } else { 60 };
                controls.observe(
                    now + Duration::from_millis(index * step_ms),
                    Vec3::ZERO,
                    false,
                );
            }
            controls.resolved(false);
            assert!(controls.plan(next_body, now, FRESH_LIMIT).is_none());
            assert!(!controls.active());
        }
    }

    #[test]
    fn shoreline_rejection_cannot_resume_the_last_accepted_arc() {
        let start = Instant::now();
        let now = start + Duration::from_millis(16);
        let world = world(Some(-2.75), false, false);
        let body = body(&world);
        let mut controls = controls(start);
        let proposal = controls
            .plan(body, start, FRESH_LIMIT)
            .expect("reachable low lip");
        let body = Body {
            feet: proposal.feet,
            ..body
        };
        controls.sent(MovementMode::Shoreline(proposal.trace), Some(proposal), now);
        controls.resolved(false);
        let then = now + Duration::from_millis(16);
        controls.observe(then, Vec3::ZERO, false);
        let next = controls.plan(body, now, FRESH_LIMIT).expect("continuation");
        assert!(!next.trace.start);
        controls.sent(MovementMode::Shoreline(next.trace), Some(next), then);
        controls.resolved(true);
        assert!(!controls.active());
    }

    #[test]
    fn shoreline_launch_and_continuation_share_the_observed_clock_despite_queue_delay() {
        let epoch = Instant::now();
        let world = world(Some(-2.75), false, false);
        let body = body(&world);
        let mut controls = Control::default();
        let swim_frame = epoch + Duration::from_millis(100);
        let swim_sent = epoch + Duration::from_millis(103);
        controls.observe(swim_frame, Vec3::ZERO, false);
        controls.sent(MovementMode::Swim, None, swim_sent);
        controls.resolved(false);

        let launch_frame = epoch + Duration::from_micros(116_600);
        controls.observe(launch_frame, Vec3::new(0.9, 0.4, 0.0), true);
        let launch = controls
            .plan(body, swim_sent, FRESH_LIMIT)
            .expect("overlapping launch");
        assert_eq!(launch.created, epoch + Duration::from_millis(116));
        assert_eq!(launch.trace.frames[0].millis(), 13);
        let sent = launch.created;
        let launch_queued = launch_frame + Duration::from_millis(3);
        let body = Body {
            feet: launch.feet,
            ..body
        };
        controls.sent(MovementMode::Shoreline(launch.trace), Some(launch), sent);
        controls.resolved(false);

        for index in 1..=7 {
            controls.observe(
                launch_frame + Duration::from_micros(index * 16_600),
                Vec3::ZERO,
                false,
            );
        }
        let continued = controls
            .plan(body, sent, Duration::from_millis(250))
            .expect("continuation after delayed queue");
        let millis = continued
            .trace
            .frames
            .into_iter()
            .map(|frame| u64::from(frame.millis()))
            .sum();
        assert_eq!(millis, 116_u64);
        assert_eq!(
            continued.created.duration_since(sent),
            Duration::from_millis(millis)
        );
        assert!(continued.created.duration_since(launch_queued) < Duration::from_millis(millis));
    }
}
