//! Past completed held-input frames; no tentative XYZ enters presentation.
use super::{
    Controls, Duration, HeadingUpdate, Instant, MotionInput, MovementMode, MovementRequest, Pace,
    START_CYCLE_DURATION, StrafeSide, Vec3, online, owned_swim,
};
use bevy::prelude::trace;
use eq_client_core::{
    ClientCommand, WorldPosition,
    movement::{
        CollisionWorld, SwimBasis, SwimCalibration, SwimCollision, SwimControl, SwimControls,
        SwimInput, SwimSceneHandle, SwimSceneMotion,
        water::{WetInput, WetVelocity},
    },
};

#[derive(Clone)]
struct Frame {
    at: Instant,
    motion: SwimSceneMotion,
    position: WorldPosition,
    input: SwimInput,
    heading: f32,
    pitch: f32,
    seconds: f32,
    kind: eq_client_core::movement::water::WetKind,
}
struct Pending {
    request: MovementRequest,
    count: usize,
    frame: Frame,
    path_xy: f64,
    path_xyz: f64,
    seconds: f32,
}

#[derive(Default)]
pub(super) struct Control {
    reset: Option<(u64, WorldPosition)>,
    basis: Option<(u64, WorldPosition, SwimBasis)>,
    accepted: Option<Frame>,
    current: Option<Frame>,
    frames: Vec<Frame>,
    pending: Option<Pending>,
    collision: Option<SwimCollision>,
    rates: Option<(SwimCalibration, Option<f32>, Option<f32>)>,
    last_observed: Option<Instant>,
    epoch: Option<Instant>,
    result: Option<(u64, Instant, bool)>,
    invalidated: bool,
    awaiting_basis: bool,
}
impl Control {
    pub fn untouched_reset(&self, wet: WetVelocity) -> Self {
        if !self.active() && !self.pending() && wet == WetVelocity::default() {
            Self {
                reset: self.reset,
                basis: self.basis,
                ..Self::default()
            }
        } else {
            Self::default()
        }
    }
    pub fn invalidate_reset(&mut self) {
        if !self.active() {
            self.reset = None;
            self.basis = None;
        }
    }
    pub fn reset(&mut self, session: u64, at: WorldPosition) {
        *self = Self {
            reset: Some((session, at)),
            ..Self::default()
        };
    }
    pub fn receipt(
        &mut self,
        session: u64,
        at: WorldPosition,
        created: Option<Instant>,
        basis: Option<SwimBasis>,
        wet: WetVelocity,
    ) {
        if created.is_none()
            && self.reset == Some((session, at))
            && wet == WetVelocity::default()
            && basis.is_some_and(SwimBasis::is_reset)
        {
            self.basis = basis.map(|basis| (session, at, basis));
        } else if let Some(created) = created
            && self.result == Some((session, created, true))
        {
            self.result = None;
            self.awaiting_basis = false;
            self.basis = basis.map(|basis| (session, at, basis));
            if self.basis.is_none() {
                self.retire();
            }
        }
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn active(&self) -> bool {
        self.current.is_some()
    }
    pub fn heading(&self) -> Option<f32> {
        self.current.as_ref().map(|f| f.position.heading)
    }
    pub fn unavailable(&mut self, now: Instant) {
        self.retire();
        self.last_observed = Some(now);
    }
    pub fn retire(&mut self) {
        self.basis = None;
        self.current = None;
        self.accepted = None;
        self.frames.clear();
        self.collision = None;
        self.rates = None;
        self.reset = None;
        self.epoch = None;
        self.invalidated = false;
        self.awaiting_basis = false;
    }
    pub fn resolved(
        &mut self,
        session: Option<u64>,
        created: Instant,
        at: WorldPosition,
        refused: bool,
    ) -> Option<WetVelocity> {
        if !self
            .pending
            .as_ref()
            .is_some_and(|p| Some(p.request.session_id) == session && p.request.created == created)
        {
            return None;
        }
        let p = self.pending.take().expect("matching physical swim result");
        let accepted = !refused && at == p.request.position;
        self.result = Some((p.request.session_id, created, accepted));
        self.awaiting_basis = accepted;
        let velocity = if accepted {
            if self.frames.len() >= p.count {
                self.frames.drain(..p.count);
            } else {
                self.frames.clear();
                self.current = None;
            }
            self.accepted = Some(p.frame.clone());
            p.frame.motion.velocity().quarter_turn(true)
        } else {
            let velocity = self.accepted.as_ref().map_or(WetVelocity::default(), |f| {
                f.motion.velocity().quarter_turn(true)
            });
            self.retire();
            velocity
        };
        trace!(target:"eq_client_render::motion::swim", basis=?self.basis, proposed=?p.request.position,
            created=?created,dt=p.seconds,accepted_path_xy=if accepted {p.path_xy}else{0.0},
            accepted_path_xyz=if accepted {p.path_xyz}else{0.0},refused=!accepted,
            mode="physical_floor_swim", verdict=if accepted {"local_accepted"}else{"local_refused"},
            "Completed frame prefix resolved; no packet ACK implied");
        Some(velocity)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Context<'a> {
    pub world: &'a CollisionWorld,
    pub online: &'a online::OnlineState,
    pub outbox: &'a crate::outbox::Outbox,
    pub now: Instant,
    pub delta: Duration,
    pub height: f32,
    pub offset: f32,
    pub eligible: bool,
    pub facing: f32,
    pub pitch: f32,
    pub body_pitch: f32,
}

/// Keep camera-relative steering and unsupported control mappings with the
/// ordinary controller before consuming its reset receipt or any physics.
pub(super) fn supports(controls: &Controls, intent: &MotionInput, facing: f32, pitch: f32) -> bool {
    let Some((input, heading)) = owned_swim::observed(intent.mode, intent.direction, facing) else {
        return false;
    };
    if matches!(input, SwimInput::Forward(_) | SwimInput::ForwardStrafe(..))
        && !intent.preserve_facing
        && super::arc(facing, heading).abs() > 0.001
    {
        return false;
    }
    let Some(swim) = controls.swim else {
        return false;
    };
    let Some(native) = native_input(
        input,
        heading,
        pitch,
        swim,
        controls.strafe_speed,
        controls.strafe_walk_speed,
    ) else {
        return false;
    };
    // Pitched forward strokes and shore impulses retain their existing controller.
    if native.vertical != 0.0 {
        return false;
    }
    let applied = Vec3::new(native.horizontal[1], 0.0, -native.horizontal[0]).normalize_or_zero();
    let actual = Vec3::new(intent.direction.x, 0.0, intent.direction.z).normalize_or_zero();
    applied == Vec3::ZERO || (applied - actual).length_squared() <= 1e-10
}

/// Collects one actual physical frame before report/pending gates. A pending
/// prefix may have future completed frames, but only its matching acceptance
/// enters presentation. Refusal discards that entire unaccepted suffix.
#[allow(clippy::too_many_lines)]
pub(super) fn input(controls: &mut Controls, context: Context<'_>, intent: MotionInput) -> bool {
    let Context {
        world,
        online,
        outbox,
        now,
        delta,
        height,
        offset,
        eligible,
        facing,
        pitch,
        body_pitch,
    } = context;
    let session = online.world().session_id();
    let mut owner = std::mem::take(&mut controls.floor_swim);
    let handled = (|| {
        let observed = owned_swim::observed(intent.mode, intent.direction, facing);
        let rates = controls
            .swim
            .map(|swim| (swim, controls.strafe_speed, controls.strafe_walk_speed));
        let compatible = eligible
            && !delta.is_zero()
            && !owner.invalidated
            && observed.is_some()
            && owner.rates.is_none_or(|old| Some(old) == rates)
            && owner
                .collision
                .as_ref()
                .is_none_or(|scene| scene.matches(world, height, offset));
        if !compatible {
            owner.invalidated = owner.active();
            owner.last_observed = Some(now);
            if owner.pending() {
                return true;
            }
            if owner.active() {
                controls.wet = owner
                    .accepted
                    .as_ref()
                    .map_or(controls.wet, |f| f.motion.velocity().quarter_turn(true));
                controls.swim_owner.withdraw();
                owner.retire();
            }
            return false;
        }
        let Some((admission, at, basis)) = owner.basis else {
            // A result can precede its continuation receipt. Preserve observed
            // frames while waiting, never create a replacement basis.
            return owner.active() || owner.pending();
        };
        if session != Some(admission) {
            owner.retire();
            return false;
        }
        let Some((swim, strafe, walk)) = rates else {
            return false;
        };
        if owner.current.is_none() {
            if !basis.is_reset()
                || owner.reset != Some((admission, at))
                || controls.wet != WetVelocity::default()
                || controls.pending.is_some()
            {
                return false;
            }
            if online.world().player().is_none_or(|p| p.position != at) {
                owner.retire();
                return false;
            }
            let origin = Vec3::from_array(eq_client_core::render_position(at));
            let Some(scene) = SwimCollision::new(world, origin, height, offset) else {
                return false;
            };
            let handle = SwimSceneHandle::new(scene.clone());
            let Ok(motion) = SwimSceneMotion::at_reset(handle, [at.x, at.y, at.z]) else {
                return false;
            };
            let rest = Frame {
                at: basis.started_at(),
                motion,
                position: at,
                input: SwimInput::Idle,
                heading: at.heading,
                pitch: 0.0,
                seconds: 0.0,
                kind: swim_kind(world, origin, height, offset),
            };
            owner.reset = None;
            owner.current = Some(rest.clone());
            owner.accepted = Some(rest);
            owner.collision = Some(scene);
            owner.rates = Some((swim, strafe, walk));
            // Begin at this observed boundary; no call is invented before
            // this owner saw the matched reset, body and input. Epoch only
            // schedules publication and is not a new accepted-state stamp.
            owner.last_observed = Some(now);
            owner.epoch = Some(now);
            controls.swim_owner.withdraw();
            return true;
        }
        let (input, heading) = observed.expect("checked input");
        let last = owner.last_observed.replace(now).unwrap_or(now);
        let seconds = now.saturating_duration_since(last).as_secs_f32();
        // Freeze a stalled prefix. Never catch up the unobserved time later.
        let consumed: f32 = owner.frames.iter().map(|f| f.seconds).sum();
        if seconds > 1e-5
            && seconds <= 0.15
            && consumed + seconds <= 0.24
            && owner.frames.len() < 64
        {
            let current = owner.current.as_ref().expect("started swim");
            let Ok(kind) = current.motion.kind() else {
                if owner.pending() {
                    owner.invalidated = true;
                    return true;
                }
                owner.retire();
                return false;
            };
            let Some(native) = native_input(input, heading, pitch, swim, strafe, walk) else {
                if owner.pending() {
                    owner.invalidated = true;
                    return true;
                }
                owner.retire();
                return false;
            };
            let (motion, frame) = match current.motion.frame(swim.set_for(kind), native, seconds) {
                Ok(frame) => frame,
                Err(reason) => {
                    trace!(target:"eq_client_render::motion::swim",?reason,mode="physical_floor_swim",verdict="scene_declined",
                        "Physical swim keeps conservative fallback");
                    if owner.pending() {
                        owner.invalidated = true;
                        return true;
                    }
                    controls.wet = owner
                        .accepted
                        .as_ref()
                        .map_or(controls.wet, |f| f.motion.velocity().quarter_turn(true));
                    owner.retire();
                    return false;
                }
            };
            let from = Vec3::from_array(eq_client_core::render_position(current.position));
            let [x, y, z] = frame.origin;
            let to = Vec3::new(y, z, -x);
            if super::stop_at_zone_line(&online.regions, from, to) != to {
                if owner.pending() {
                    owner.invalidated = true;
                    return true;
                }
                controls.wet = owner
                    .accepted
                    .as_ref()
                    .map_or(controls.wet, |f| f.motion.velocity().quarter_turn(true));
                owner.retire();
                return false;
            }
            let [x, y, z] = frame.origin;
            let position = WorldPosition {
                x,
                y,
                z,
                heading: facing,
            };
            trace!(target:"eq_client_render::motion::swim", basis=?current.position, proposed=?position,
                dt=seconds,?kind,?input,heading,pitch,body_look_pitch=body_pitch,raw_delta=?frame.raw,floor_snap=frame.floor_snap,
                mode="physical_floor_swim",verdict="completed_unaccepted", "Actual held-input frame; no presentation credit");
            let next = Frame {
                at: now,
                motion,
                position,
                input,
                heading,
                pitch,
                seconds,
                kind,
            };
            owner.current = Some(next.clone());
            owner.frames.push(next);
        } else if seconds > 0.15 || consumed + seconds > 0.24 || owner.frames.len() >= 64 {
            owner.invalidated = true;
            if owner.pending() {
                return true;
            }
            owner.retire();
            return false;
        }
        if owner.pending()
            || owner.awaiting_basis
            || now.saturating_duration_since(
                controls
                    .last_accepted
                    .max(owner.epoch.unwrap_or(controls.last_accepted)),
            ) < START_CYCLE_DURATION
            || owner.frames.is_empty()
        {
            return true;
        }
        let frame = owner.current.as_ref().expect("current frame").clone();
        let mut until = Duration::ZERO;
        let pieces: Result<Vec<_>, _> = owner
            .frames
            .iter()
            .map(|f| {
                until += Duration::from_secs_f32(f.seconds);
                SwimControl::physical(until, f.heading, f.kind, f.input, f.seconds, f.pitch, f.at)
            })
            .collect();
        let Ok(pieces) = pieces else {
            owner.retire();
            return false;
        };
        let Ok(tape) = SwimControls::from_physical_frames(
            basis,
            pieces,
            frame.position.heading,
            frame.motion.scene().clone(),
        ) else {
            owner.retire();
            return false;
        };
        let mut request =
            MovementRequest::new(MovementMode::Swim, admission, frame.position, frame.at);
        request.heading = HeadingUpdate::Snapshot;
        let mut poses = super::pose::Path::default();
        let start = owner.accepted.as_ref().expect("accepted swim basis");
        let start_origin = Vec3::from_array(eq_client_core::render_position(start.position));
        poses.completed_position(0.0, start_origin);
        let mut elapsed = 0.0;
        let mut path_xy = 0.0_f64;
        let mut path_xyz = 0.0_f64;
        let mut previous = start.position;
        for frame in &owner.frames {
            let dx = f64::from(frame.position.x) - f64::from(previous.x);
            let dy = f64::from(frame.position.y) - f64::from(previous.y);
            let dz = f64::from(frame.position.z) - f64::from(previous.z);
            path_xy += dx.hypot(dy);
            path_xyz += dx.hypot(dy).hypot(dz);
            previous = frame.position;
            elapsed += frame.seconds;
            let ([x, z], y) = frame
                .motion
                .velocity()
                .quarter_turn(true)
                .units_per_second();
            poses.push(
                elapsed,
                super::pose::Snapshot {
                    horizontal: super::Vec2::new(x, z),
                    vertical: y,
                },
            );
            poses.completed_position(
                elapsed,
                Vec3::from_array(eq_client_core::render_position(frame.position)),
            );
        }
        poses.origins(
            0.0,
            start_origin,
            Vec3::from_array(eq_client_core::render_position(frame.position)),
            until.as_secs_f32(),
        );
        if let Err(reason) = outbox.post(online.world(), |_| ClientCommand::SwimFrame {
            request: request.clone(),
            controls: Box::new(tape),
        }) {
            controls.send_failed(reason);
            return true;
        }
        controls.sample_pose = Some(poses);
        controls.sent_yaw = if controls.mouse_heading.is_some() {
            super::SentYaw::MouseSnapshot
        } else {
            super::SentYaw::KeyboardRate
        };
        controls.last_sent = Some(frame.at);
        controls.sent_span = until.as_secs_f32();
        owner.pending = Some(Pending {
            request,
            count: owner.frames.len(),
            path_xy,
            path_xyz,
            seconds: until.as_secs_f32(),
            frame,
        });
        true
    })();
    controls.floor_swim = owner;
    handled
}

fn swim_kind(
    world: &CollisionWorld,
    origin: Vec3,
    height: f32,
    offset: f32,
) -> eq_client_core::movement::water::WetKind {
    world.wet_kind(origin - Vec3::Y * offset, height)
}
fn native_input(
    input: SwimInput,
    heading: f32,
    pitch: f32,
    swim: SwimCalibration,
    strafe: Option<f32>,
    walk: Option<f32>,
) -> Option<WetInput> {
    let rate = |pace| {
        if pace == Pace::Run {
            swim.forward
        } else {
            swim.walk
        }
    };
    let side_sign = |side| if side == StrafeSide::Left { 1.0 } else { -1.0 };
    let (across, along, vertical) = match input {
        SwimInput::Idle => (0.0, 0.0, 0.0),
        SwimInput::Backward => (0.0, -swim.backward, 0.0),
        SwimInput::Forward(pace) => (0.0, rate(pace), rate(pace) * pitch.sin()),
        SwimInput::Strafe(side, pace) => {
            let run = strafe?;
            let selected = if pace == Pace::Walk {
                walk.unwrap_or(run)
            } else {
                run
            };
            (
                side_sign(side) * swim.strafe * selected / run.max(f32::EPSILON),
                0.0,
                0.0,
            )
        }
        SwimInput::ForwardStrafe(side, pace) => {
            strafe?;
            let along = rate(pace) * std::f32::consts::FRAC_1_SQRT_2;
            (side_sign(side) * along, along, rate(pace) * pitch.sin())
        }
        _ => return None,
    };
    let (sin, cos) = (heading / 512.0 * std::f32::consts::TAU).sin_cos();
    Some(WetInput {
        horizontal: [sin * along + cos * across, cos * along - sin * across],
        vertical,
        jump: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reset_basis(now: Instant) -> SwimBasis {
        use eq_network_game::movement::{MotionSession, SpeedState, titanium_speeds};
        let calibration = titanium_speeds(&SpeedState {
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
        let mut session = MotionSession::new(11, 7, WorldPosition::default(), now)
            .unwrap()
            .with_swimming(true)
            .with_heading_snapshots(true);
        session.calibrate(calibration, now).unwrap();
        session.swim_basis().unwrap()
    }
    #[test]
    fn receipt_alone_or_a_consumed_reset_never_anchors_physical_swim() {
        let now = Instant::now();
        let basis = reset_basis(now);
        let at = WorldPosition::default();
        let mut owner = Control::default();
        owner.receipt(11, at, None, Some(basis), WetVelocity::default());
        assert!(owner.basis.is_none());
        owner.reset(11, at);
        owner.invalidate_reset();
        owner.receipt(11, at, None, Some(basis), WetVelocity::default());
        assert!(owner.basis.is_none());
    }
    #[test]
    fn only_an_untouched_matching_real_reset_survives_a_grant() {
        let now = Instant::now();
        let basis = reset_basis(now);
        let at = WorldPosition::default();
        let mut owner = Control::default();
        owner.reset(11, at);
        let mut preserved = owner.untouched_reset(WetVelocity::default());
        preserved.receipt(11, at, None, Some(basis), WetVelocity::default());
        assert!(preserved.basis.is_some());
        let carried = WetVelocity::from_units_per_second([1.0, 0.0], 0.0);
        let mut consumed = owner.untouched_reset(carried);
        consumed.receipt(11, at, None, Some(basis), WetVelocity::default());
        assert!(consumed.basis.is_none());
        let shifted = WorldPosition { x: 1.0, ..at };
        owner.receipt(11, shifted, None, Some(basis), WetVelocity::default());
        assert!(owner.basis.is_none());
    }
}
