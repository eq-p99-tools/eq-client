//! Calibrated P99 input; no movement is generated without a session grant.
mod follow;
mod placement;
mod shoreline;

use super::{
    Collision, OrbitCamera, Player, PlayerBody,
    camera::{MouseAim, MouseLook},
    camera_relative_direction,
    keys::{Act, KeyMap},
    online,
};
use bevy::prelude::*;
use eq_client_assets::{
    falls::{FallCost, FallLanding},
    regions::ZoneLine,
    ui::InstalledClient,
};
use eq_client_core::{
    Capability, ClientCommand, DiagonalAlong, DiagonalRates, GroundedDiagonal, HeadingUpdate,
    MovementMode, MovementRequest, Posture, StrafeSide, Support,
    hazards::Hazard,
    movement::{
        AirborneController, CollisionWorld, Landing, PROVISIONAL_PHYSICS, VerticalPhysics,
        swim_speed,
    },
    qol::{Fix, Settings},
    world::ClientWorld,
    world_position,
};
use std::time::{Duration, Instant};

/// `/follow` asked, for [`input`] to start following the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FollowAsked;

impl FollowAsked {
    /// Whether a typed line is `/follow`, by the official parser's prefix
    /// rule (program read R6: `/fo` and longer).
    pub(crate) fn typed(input: &str) -> bool {
        input
            .strip_prefix('/')
            .and_then(|command| command.split_whitespace().next())
            .is_some_and(|name| crate::contacts::abbreviates(name, "follow", 2))
    }
}

#[derive(Resource)]
#[allow(clippy::struct_excessive_bools)] // Independent movement states.
pub(super) struct Controls {
    pub speed: Option<f32>,
    pub backward_speed: Option<f32>,
    pub walk_speed: Option<f32>,
    pub strafe_speed: Option<f32>,
    /// The strafe speed with walking selected, where it is slower than
    /// [`Self::strafe_speed`].
    pub strafe_walk_speed: Option<f32>,
    /// Backing up in water, before the water's share applies; the session's
    /// grant (`MotionGrant::swim_backward_units_per_second`).
    pub swim_backward_speed: Option<f32>,
    diagonal_rates: DiagonalRates,
    /// The share of the jump speed a jump starts with, as the session
    /// grants it: the health and weight factor.
    jump_factor: f32,
    pub walking: bool,
    /// Running forward by itself: on and off by the autorun key; a forward
    /// or back key or a posture change turns it off, while turning,
    /// strafing, jumping and attacking leave it on (Titanium program read
    /// N6 of 11-01; the Mac-era client's is unread, and other ways the
    /// program sets it, such as following, are not in this client:
    /// recording checklist 6.359).
    autorun: bool,
    pending: Option<Pending>,
    pub last_accepted: Instant,
    moving: bool,
    /// Seconds of play the latest sample sent covers: the model shows it
    /// over that long once the session accepts it.
    sent_span: f32,
    /// The lift the last sample sent stood on, that sample's stamp and the
    /// height it put the model at: the next pass carries the feet by the
    /// lift's own rise since then, and the model is drawn on the floor.
    riding: Option<(u8, Instant, f32)>,
    /// Whether the last pass found no room for the lift to carry the body
    /// (recording checklist 6.427), so the model is not drawn rising.
    ride_blocked: bool,
    sent_yaw: SentYaw,
    boundary: BoundaryTracker,
    placement: placement::Check,
    taps: TapBuffer,
    visual: Option<VisualMotion>,
    /// Vertical momentum, present only when the session accepts falls; without it,
    /// stepping off a ledge stops at the edge.
    pub airborne: Option<eq_client_core::movement::AirborneController>,
    /// Dry-side momentum to keep if the session refuses a proposed water entry.
    water_entry_rollback: Option<AirborneController>,
    shoreline: shoreline::Control,
    /// Why the movement guard refused the latest sample, until one is sent.
    pub refused: Option<String>,
    /// When a jump was pressed since the last sample; only where falls are simulated.
    pub jump: Option<Instant>,
    /// When the latest jump started; it outlasts a reset, so a new
    /// admission cannot hurry the next.
    last_jump: Option<Instant>,
    /// What the jump that just took off costs in endurance, for
    /// [`spend_endurance`] to show.
    endurance_spent: Option<u32>,
    /// A jump from standing still crouching first, as both installed
    /// clients do.
    crouch: Option<Crouch>,
    /// The cost of a landing the session's queue had no room to tell; it
    /// goes before the next sample.
    unreported: Option<FallCost>,
    /// Where the player faces while the heading sent still turns toward it;
    /// None once the two match.
    facing: Option<f32>,
    /// Owned horizontal mouse input waiting for a movement sample, in heading units.
    mouse_turn: f32,
    /// Whether a turn key turns left (else right) and when it was first
    /// held, which the turn ramp counts from; None while neither or both
    /// are held.
    turn_held: Option<(bool, Instant)>,
    /// Latest immediately displayed Titanium mouse heading awaiting a sample.
    mouse_heading: Option<f32>,
    /// What the samples sent lately asked of the controller and moved.
    pub recent: RecentMotion,
    /// The latest sample's request stamp, and when the session last answered
    /// one; both outlast a reset, so a walk that stops can say how long ago.
    pub last_sent: Option<Instant>,
    pub last_answer: Option<Instant>,
    /// Why the latest frame sent no sample; None when it sent one.
    pub held: Option<Held>,
    /// The admission and the climb a step last told to the session
    /// (`ClimbReach`), so it goes again at each admission and whenever the
    /// body's climb changes.
    climb_sent: Option<(u64, f32)>,
    /// Admission and revision last used to withdraw voluntary input.
    stun_revision: Option<(u64, u64)>,
    /// Emission permission, separate from actual held-key/autorun intent.
    stunned: bool,
    /// The actual stun also blocks jump and flight's voluntary height changes.
    stun_active: bool,
    /// Explicit departures; the session checks the same choices independently.
    movement_options: eq_client_core::movement::MovementOptions,
    /// The group member the player follows on foot (`/follow`).
    following: Option<follow::Following>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PendingKind {
    Move,
    Crossing,
}

#[derive(Clone, Copy)]
struct Pending {
    session_id: u64,
    created: Instant,
    kind: PendingKind,
}

/// Whether local liquid checks have a worker position on which to base a sample.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum LiquidBasis {
    Observed,
    Pending,
    Unconfirmed,
}

const MOVE_RESULT_WAIT: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SentYaw {
    #[default]
    Ordinary,
    MouseRate,
    MouseSnapshot,
}

/// How far a jump from standing still has come: with no forward input,
/// Titanium (0x4acb42-0x4acb50) and the Mac-era client both crouch before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Crouch {
    /// Crouched; it stands up and jumps at this time.
    Until(Instant),
    /// Stood up: the jump waiting for the next sample goes as it is.
    Over,
}

/// Why a frame sent no movement sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Held {
    /// The session took movement away, or never granted it.
    Withdrawn,
    /// A sample is out and its answer has not come back.
    Waiting,
    /// Too soon after the last answer, or a landing's damage or a zone
    /// crossing went first.
    Gate,
    /// Nothing asked to move: no direction, turn, fall or jump.
    NoSpeed,
    /// The player's body, the camera or the zone's collision is missing.
    NoBody,
    /// A fresh server placement is below the installed collision floor.
    PlacementBlocked,
    /// The session's queue refused the sample.
    Refused,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            speed: None,
            backward_speed: None,
            walk_speed: None,
            strafe_speed: None,
            strafe_walk_speed: None,
            swim_backward_speed: None,
            diagonal_rates: DiagonalRates::default(),
            jump_factor: 1.0,
            walking: false,
            autorun: false,
            pending: None,
            last_accepted: Instant::now(),
            moving: false,
            sent_span: 0.1,
            riding: None,
            ride_blocked: false,
            sent_yaw: SentYaw::Ordinary,
            boundary: BoundaryTracker::default(),
            placement: placement::Check::default(),
            taps: TapBuffer::default(),
            visual: None,
            airborne: None,
            water_entry_rollback: None,
            shoreline: shoreline::Control::default(),
            refused: None,
            jump: None,
            last_jump: None,
            endurance_spent: None,
            crouch: None,
            unreported: None,
            facing: None,
            mouse_turn: 0.0,
            turn_held: None,
            mouse_heading: None,
            recent: RecentMotion::default(),
            last_sent: None,
            last_answer: None,
            held: None,
            climb_sent: None,
            stun_revision: None,
            stunned: false,
            stun_active: false,
            movement_options: eq_client_core::movement::MovementOptions::default(),
            following: None,
        }
    }
}

/// The level displacement each sample sent over the last second asked of the
/// controller, and what it moved, so a walk that stops can say which.
#[derive(Default)]
pub(super) struct RecentMotion(std::collections::VecDeque<(Instant, Vec3, Vec3)>);

/// What the samples sent over the last second asked and moved, as level
/// distances.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct MotionSummary {
    pub samples: usize,
    /// The distance asked, summed sample by sample.
    pub asked: f32,
    /// The distance moved, summed sample by sample.
    pub moved: f32,
    /// How far the samples moved altogether, start to end.
    pub net: f32,
}

impl RecentMotion {
    const SPAN: Duration = Duration::from_secs(1);

    fn record(&mut self, now: Instant, asked: Vec3, moved: Vec3) {
        self.0.push_back((now, asked, moved));
        while self
            .0
            .front()
            .is_some_and(|(at, ..)| now.duration_since(*at) > Self::SPAN)
        {
            self.0.pop_front();
        }
    }

    /// The samples sent within the second before `now`.
    pub fn last_second(&self, now: Instant) -> MotionSummary {
        let level = |v: Vec3| Vec2::new(v.x, v.z).length();
        let (mut summary, mut net) = (MotionSummary::default(), Vec3::ZERO);
        for (_, asked, moved) in self
            .0
            .iter()
            .filter(|(at, ..)| now.saturating_duration_since(*at) <= Self::SPAN)
        {
            summary.samples += 1;
            summary.asked += level(*asked);
            summary.moved += level(*moved);
            net += *moved;
        }
        summary.net = level(net);
        summary
    }
}

/// Trigger once on entry, never merely because admission places us on a line.
#[derive(Default)]
struct BoundaryTracker {
    initialized: bool,
    current: Option<ZoneLine>,
}
impl BoundaryTracker {
    fn observe(&mut self, next: Option<ZoneLine>) -> Option<eq_client_core::ZoneLineDestination> {
        let entered = self.initialized && next != self.current;
        self.initialized = true;
        self.current = next;
        if !entered {
            return None;
        }
        Some(match next? {
            ZoneLine::Reference(number) => eq_client_core::ZoneLineDestination::Reference(number),
            ZoneLine::Absolute {
                zone_id,
                position: [x, y, z],
                heading,
            } => eq_client_core::ZoneLineDestination::Absolute {
                zone_id,
                position: eq_client_core::WorldPosition { x, y, z, heading },
            },
        })
    }
}
impl Controls {
    /// Shows a locally accepted sample without changing the simulation
    /// position: the model plays the accepted samples back in order, each
    /// over the time of play it covers, so it passes at the pace sent and
    /// never ahead of the newest accepted position. A sample accepted while
    /// the last still plays waits its turn; when none is left the model
    /// stands at the newest until the next is accepted. Display only;
    /// inferred, recording checklist 6.340 (the official client moves its
    /// own player every frame).
    ///
    /// The model shows where the player faces, which the heading sent may
    /// still be turning toward. A facing change beyond what the turn keys
    /// turn in the sample's span is the player facing a new way at once, so
    /// it shows at once while the positions still play.
    pub fn display_sample(&mut self, from: Transform, position: eq_client_core::WorldPosition) {
        if self.refused.is_some()
            || self
                .mouse_heading
                .is_some_and(|heading| arc(heading, position.heading) == 0.0)
        {
            self.mouse_heading = None;
        }
        let rotation = Quat::from_rotation_y(eq_client_core::render_heading(
            self.facing.unwrap_or(position.heading),
        ));
        let to = Transform::from_translation(Vec3::from_array(eq_client_core::render_position(
            position,
        )))
        .with_rotation(rotation);
        let span = self.sent_span;
        // Measured from the facing the last sample asked for, which the
        // model shows once that sample has played: a turn key's sample can
        // arrive before then.
        let asked = self
            .visual
            .as_ref()
            .map_or(from.rotation, |visual| visual.last().rotation);
        let limit = TURN_RATE * span / 512.0 * std::f32::consts::TAU;
        // A turn key turns at most the limit in a span; the 1% margin keeps
        // rounding in the angle between two rotations from making that a snap.
        let snap = match self.sent_yaw {
            SentYaw::Ordinary => asked.angle_between(rotation) > limit * 1.01,
            SentYaw::MouseRate => false,
            SentYaw::MouseSnapshot => true,
        };
        let segment = Segment {
            to,
            span,
            snap,
            follow_heading: self.sent_yaw != SentYaw::Ordinary,
        };
        match self.visual.as_mut().filter(|visual| !visual.finished()) {
            Some(visual) => {
                // A new way shows at once, though the samples before it are
                // still playing their positions.
                if snap {
                    visual.face(rotation);
                }
                visual.queue.push_back(segment);
            }
            None => self.visual = Some(VisualMotion::start(from, segment)),
        }
    }
    /// Selects a granted locomotion mode without modifying its timing budget.
    fn intent(
        &self,
        keys: &ButtonInput<KeyCode>,
        map: &KeyMap,
        scripted: &[Act],
        focused: bool,
        (yaw, heading): (f32, f32),
        look: Option<&MouseLook>,
    ) -> MotionInput {
        let focused = focused && !self.stunned;
        let looking = look.filter(|look| focused && look.held);
        let looking_ahead = looking.is_some_and(|look| look.left);
        // A complete tap can arrive between render frames. Preserve its press edge
        // for this one sample without mutating Bevy's shared held-key state.
        let keys = sample_keys(keys, map);
        let held = map.holding(&keys, scripted);
        let turn = if focused {
            let turning =
                |act| scripted.contains(&act) || (looking.is_none() && map.held(&keys, act));
            f32::from(turning(Act::TurnLeft)) - f32::from(turning(Act::TurnRight))
        } else {
            0.0
        };
        let mut intent = movement_input(
            held,
            (focused, looking_ahead, self.autorun && !self.stunned),
            yaw,
            heading,
            self.speed.unwrap_or(0.0),
            self.backward_speed,
            turn,
        );
        if self.walking && intent.mode == MovementMode::Forward {
            intent.mode = MovementMode::Walk;
            intent.speed = self.walk_speed.unwrap_or(0.0);
        }
        // Only physical turn bindings change meaning while looking.
        let sideways = |replace_turn| {
            let strafing =
                |strafe, turn| held.held(strafe) || (replace_turn && map.held(&keys, turn));
            f32::from(strafing(Act::StrafeRight, Act::TurnRight))
                - f32::from(strafing(Act::StrafeLeft, Act::TurnLeft))
        };
        let diagonal_available =
            self.diagonal_rates.run.is_some() || self.diagonal_rates.walk.is_some();
        if diagonal_available
            && focused
            && turn == 0.0
            && !self.movement_options.fly
            && !self.stun_active
        {
            intent.diagonal =
                self.diagonal(&intent, heading, looking_ahead, sideways(looking.is_some()));
        }
        // Retain the existing cardinal choice as the collision/fall fallback.
        let sideways = sideways(looking.is_some() && !looking_ahead);
        let (ahead, behind) = (held.held(Act::Forward), held.held(Act::Back));
        let longitudinal_key = if diagonal_available {
            ahead != behind
        } else {
            ahead || behind
        };
        if focused && sideways != 0.0 && !longitudinal_key {
            let radians = eq_client_core::render_heading(heading);
            intent.preserve_facing = true;
            if let Some(speed) = self.strafe_speed {
                intent.mode = MovementMode::Strafe;
                // Titanium strafes on the selected base speed (the R5
                // audit's S1).
                intent.speed = if self.walking {
                    self.strafe_walk_speed.unwrap_or(speed)
                } else {
                    speed
                };
                let facing = Vec3::new(radians.sin(), 0.0, radians.cos());
                intent.direction = facing.cross(Vec3::Y) * sideways;
            } else {
                intent.mode = MovementMode::Forward;
                intent.direction = Vec3::ZERO;
            }
        }
        let run_factor = if intent.mode == MovementMode::Forward {
            self.movement_options.run_speed_multiplier.factor()
        } else {
            1.0
        };
        intent.speed *= run_factor;
        if self.movement_options.fly && focused {
            intent.direction.y = if self.stun_active {
                0.0
            } else {
                held.axis(Act::Duck, Act::Jump)
            };
            intent.direction = intent.direction.normalize_or_zero();
            // Run boost changes only horizontal speed, including during flight.
            intent.direction.y /= run_factor;
        }
        intent
    }

    /// Composes granted longitudinal and lateral ground motion as independent components.
    fn diagonal(
        &self,
        intent: &MotionInput,
        heading: f32,
        looking_ahead: bool,
        sideways: f32,
    ) -> Option<DiagonalIntent> {
        if sideways == 0.0 || intent.direction == Vec3::ZERO || intent.speed <= 0.0 {
            return None;
        }
        let radians = eq_client_core::render_heading(heading);
        let facing = Vec3::new(radians.sin(), 0.0, radians.cos());
        // The mouse's forward path follows the camera in this client. It
        // can use the body-relative budget once those headings agree.
        if !(intent.preserve_facing || looking_ahead && intent.direction.abs_diff_eq(facing, 1e-5))
        {
            return None;
        }
        let along = match intent.mode {
            MovementMode::Forward => DiagonalAlong::Run,
            MovementMode::Walk => DiagonalAlong::Walk,
            MovementMode::Backward => DiagonalAlong::Backward {
                walking: self.walking,
            },
            _ => return None,
        };
        let side = if sideways < 0.0 {
            StrafeSide::Left
        } else {
            StrafeSide::Right
        };
        let request = GroundedDiagonal { along, side };
        let lateral = self.diagonal_rates.for_mode(request)?;
        let longitudinal = if along == DiagonalAlong::Run {
            intent.speed * self.movement_options.run_speed_multiplier.factor()
        } else {
            intent.speed
        };
        let velocity = intent.direction * longitudinal + facing.cross(Vec3::Y) * sideways * lateral;
        Some(DiagonalIntent {
            request,
            longitudinal,
            lateral,
            velocity,
        })
    }
    /// Starts motion afresh, as an admission, a zone or a server's
    /// correction does. The walk toggle is the player's and stays as they
    /// left it, as walking is never faster than running (whether the
    /// program keeps it is unread, recording checklist 6.363).
    pub fn reset(&mut self, speed: Option<f32>) {
        self.shoreline.reset();
        *self = Self {
            speed,
            last_sent: self.last_sent,
            last_answer: self.last_answer,
            last_jump: self.last_jump,
            shoreline: std::mem::take(&mut self.shoreline),
            walking: self.walking,
            placement: self.placement,
            stun_revision: self.stun_revision,
            // A correction or a server's move keeps following, which then
            // ends by its own rules; a new admission ends it in `input`.
            following: self.following.take(),
            movement_options: self.movement_options,
            // A grant or correction resets prediction, but the same worker
            // can still consume a move that was already queued.
            pending: self
                .pending
                .filter(|pending| pending.kind == PendingKind::Move),
            ..Self::default()
        };
    }

    /// A same-zone landing ends the old move and probes collision afresh.
    pub(super) fn same_zone_landed(&mut self) {
        self.reset(None);
        self.pending = None;
        self.placement = placement::Check::default();
    }

    /// A control transition never revokes calibrated speed or existing
    /// vertical momentum. Remember revisions so start/expiry in one frame
    /// still discard pending taps, facing and a crouched jump. Native input
    /// collection (0x4a1d74) leaves actual autorun/held intent in place while
    /// permission is blocked; fresh samples may resume after expiry.
    fn observe_stun(&mut self, world: &ClientWorld) -> bool {
        let revision = world.session_id().zip(world.stun_revision());
        let changed = revision != self.stun_revision;
        self.stun_revision = revision;
        let stunned = world.is_stunned();
        self.stun_active = stunned;
        self.stunned = stunned && !self.movement_options.walk_while_stunned;
        if stunned || (changed && revision.is_some()) {
            self.shoreline.reset();
            self.taps = TapBuffer::default();
            self.jump = None;
            self.crouch = None;
            self.facing = None;
            self.mouse_turn = 0.0;
            self.mouse_heading = None;
        }
        if changed && revision.is_some() {
            self.moving = false;
        }
        self.stunned
    }

    /// A permission change cannot bank displacement or carry a jump/fall into flight.
    fn choose_movement(&mut self, options: eq_client_core::movement::MovementOptions) {
        let previous = std::mem::replace(&mut self.movement_options, options);
        if previous.same_permissions(options) {
            return;
        }
        self.shoreline.reset();
        if previous.fly != options.fly {
            self.airborne = self
                .airborne
                .as_ref()
                .map(|_| AirborneController::default());
        }
        self.taps = TapBuffer::default();
        self.jump = None;
        self.crouch = None;
        self.facing = None;
        self.mouse_turn = 0.0;
        self.mouse_heading = None;
        self.moving = false;
        // The session schedules its paced stop when the permission changes.
        // Resume with a fresh local sample window.
        self.last_accepted = Instant::now();
    }

    /// Stops prediction without revoking the session's calibration. A pending
    /// proposal still owns the movement slot until its result or its admission ends.
    fn pause_prediction(&mut self) {
        self.shoreline.reset();
        self.moving = false;
        self.visual = None;
        self.taps = TapBuffer::default();
        self.jump = None;
        self.mouse_turn = 0.0;
        if self.crouch == Some(Crouch::Over) {
            self.crouch = None;
        }
        self.last_accepted = Instant::now();
    }

    /// A full queue sent nothing and can be retried with a fresh bounded sample.
    /// Other refusals invalidate the admission/capability as before.
    fn send_failed(&mut self, reason: crate::outbox::Refusal) {
        if reason == crate::outbox::Refusal::Busy {
            self.pause_prediction();
        } else {
            self.reset(None);
        }
    }
    /// Continuous motion covers the time since the previous sample went
    /// out, its round trip included, up to the longest sample the session
    /// admits; a fresh start covers at most 0.1 s so an idle gap never
    /// becomes a jump.
    fn span(&self, elapsed: Duration) -> f32 {
        elapsed
            .as_secs_f32()
            .min(if self.moving { MAX_CYCLE } else { 0.1 })
    }
    /// Takes the session's word on how the player may move.
    pub fn grant(&mut self, grant: eq_client_core::world::MotionGrant) {
        // A changed positive speed keeps the current intent. Any actual
        // withdrawal/reset still clears it, including death and zoning.
        let autorun = self.autorun
            && self
                .speed
                .is_some_and(|value| value.is_finite() && value > 0.0)
            && grant
                .units_per_second
                .is_some_and(|value| value.is_finite() && value > 0.0);
        let in_flight = self
            .airborne
            .take()
            .filter(|airborne| airborne.peak().is_some());
        let unreported = self.unreported;
        // Following outlasts a change of speed, and a withdrawal while the
        // session places the player; a root ends it in `input`.
        self.reset(grant.units_per_second);
        self.autorun = autorun;
        self.backward_speed = grant.backward_units_per_second;
        self.walk_speed = grant.walk_units_per_second;
        self.strafe_speed = grant.strafe_units_per_second;
        self.strafe_walk_speed = grant.strafe_walk_units_per_second;
        self.swim_backward_speed = grant.swim_backward_units_per_second;
        self.diagonal_rates = grant.diagonal;
        self.jump_factor = if grant.jump_factor.is_finite() {
            grant.jump_factor.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if grant.falls {
            // A speed recalibration may withdraw input anywhere in the jump
            // or fall. Keep the vertical history and any unsent cost, and
            // release lateral drift. Placement and lifecycle events reset it.
            let mut airborne = in_flight.unwrap_or_default();
            airborne.limit_drift(0.0);
            self.airborne = Some(airborne);
            self.unreported = unreported;
        }
    }

    fn finish(&mut self) {
        let now = Instant::now();
        self.pending = None;
        self.last_accepted = now;
        self.last_answer = Some(now);
    }

    pub(super) fn moved(&mut self, session_id: Option<u64>, created: Instant) -> bool {
        if self.is_pending_move(session_id, created) {
            self.finish();
            return true;
        }
        false
    }

    pub(super) fn crossing_resolved(&mut self) {
        if self
            .pending
            .is_some_and(|pending| pending.kind == PendingKind::Crossing)
        {
            self.finish();
        }
    }

    pub(super) fn move_refused(&mut self, session_id: Option<u64>, created: Instant, reason: &str) {
        if self.is_pending_move(session_id, created) {
            self.finish();
            self.resolve_water_entry(true);
            self.refused = Some(reason.to_owned());
        }
    }

    fn is_pending_move(&self, session_id: Option<u64>, created: Instant) -> bool {
        self.pending.is_some_and(|pending| {
            pending.kind == PendingKind::Move
                && Some(pending.session_id) == session_id
                && pending.created == created
        })
    }

    /// A local result either keeps the water entry or restores the dry-side motion.
    pub(super) fn resolve_water_entry(&mut self, refused: bool) {
        self.shoreline.resolved(refused);
        if let Some(previous) = self.water_entry_rollback.take()
            && refused
        {
            self.airborne = Some(previous);
        }
    }

    fn wait_for(&mut self, session_id: Option<u64>, created: Instant, kind: PendingKind) {
        self.pending = session_id.map(|session_id| Pending {
            session_id,
            created,
            kind,
        });
    }

    /// Whether a movement or crossing request still owns the slot.
    pub const fn waiting(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn liquid_basis(&self, session_id: Option<u64>, now: Instant) -> LiquidBasis {
        match self
            .pending
            .filter(|pending| Some(pending.session_id) == session_id)
        {
            None => LiquidBasis::Observed,
            Some(pending) if now.saturating_duration_since(pending.created) >= MOVE_RESULT_WAIT => {
                LiquidBasis::Unconfirmed
            }
            Some(_) => LiquidBasis::Pending,
        }
    }
}

/// Longest span one proposal may cover; the session admits up to 0.25 s per sample.
const MAX_CYCLE: f32 = 0.25;

/// How fast the heading sent follows the way the player faces, in heading
/// units a second: the turn keys' fastest rate.
const TURN_RATE: f32 = 240.0;

/// What one step of the turn ramp adds to the turn keys' rate, in heading
/// units a second (0x4a1d74).
const TURN_STEP: f32 = 20.0;

/// How often the turn ramp steps. The program steps it once a frame, so
/// this is taken as 60 a second, which never ramps faster than the program
/// at 60 frames a second (inferred, recording checklist 6.416).
const TURN_STEPS_PER_SECOND: f32 = 60.0;

/// The most steps the turn ramp takes (0x4a1d74): 12 (240 units a second),
/// 8 while walking, and 3 for race 141.
fn turn_cap(walking: bool, race: u32) -> f32 {
    if race == 141 {
        3.0
    } else if walking {
        8.0
    } else {
        12.0
    }
}

/// How far the turn keys turn between `from` and `to` seconds after one was
/// first held, in heading units: the ramp's first step turns at 20 units a
/// second, each later step 20 more, up to the cap. It applies standing and
/// going forward or back, which keep the facing; camera-relative movement
/// faces its own direction and takes no ramp.
pub(crate) fn ramped_turn(from: f32, to: f32, cap: f32) -> f32 {
    let turned = |seconds: f32| {
        let seconds = seconds.max(0.0);
        let full = (cap - 1.0) / TURN_STEPS_PER_SECOND;
        let rising = seconds.min(full);
        let steps = (rising * TURN_STEPS_PER_SECOND).floor();
        let ramp = TURN_STEP * steps * (steps + 1.0) / (2.0 * TURN_STEPS_PER_SECOND)
            + TURN_STEP * (steps + 1.0) * (rising - steps / TURN_STEPS_PER_SECOND);
        ramp + TURN_STEP * cap * (seconds - full).max(0.0)
    };
    (turned(to) - turned(from)).max(0.0)
}

/// Where the wire has no heading snapshot intent, keep mouse turning inside
/// the existing rate representation.
const MOUSE_WIRE_RATE: f32 = 511.0;

/// Holds short press edges across the send throttle, never across focus loss or expiry.
#[derive(Default)]
struct TapBuffer(Vec<(KeyCode, Instant)>);

impl TapBuffer {
    fn observe(
        &mut self,
        keys: &ButtonInput<KeyCode>,
        movement: &[KeyCode],
        focused: bool,
        now: Instant,
    ) {
        if !focused {
            self.0.clear();
            return;
        }
        self.0
            .retain(|(_, time)| now.saturating_duration_since(*time) < Duration::from_millis(250));
        for &key in movement {
            if keys.just_pressed(key) {
                self.0.retain(|(pending, _)| *pending != key);
                self.0.push((key, now));
            }
        }
    }

    fn take(&mut self, keys: &ButtonInput<KeyCode>) -> ButtonInput<KeyCode> {
        let mut sampled = keys.clone();
        for (key, _) in self.0.drain(..) {
            sampled.press(key);
        }
        sampled
    }
}

/// Includes fresh tap edges without modifying the shared keyboard state.
fn sample_keys(keys: &ButtonInput<KeyCode>, map: &KeyMap) -> ButtonInput<KeyCode> {
    let mut sampled = keys.clone();
    for key in map.movement_keys() {
        if keys.just_pressed(key) {
            sampled.press(key);
        }
    }
    sampled
}

fn swim_stroke(
    direction: Vec3,
    mode: MovementMode,
    (map, keys, scripted): (&KeyMap, &ButtonInput<KeyCode>, &[Act]),
    (camera, look): (&OrbitCamera, Option<&MouseLook>),
    autorun: bool,
) -> (Vec3, bool) {
    let keys = sample_keys(keys, map);
    let held = map.holding(&keys, scripted);
    let camera_forward = held.held(Act::CameraForward);
    let camera_back = held.held(Act::CameraBack);
    let pitched = camera.is_first_person()
        || look.is_some_and(|look| look.held)
        || camera_forward != camera_back;
    let longitudinal = mode == MovementMode::Backward
        || held.held(Act::Forward) != held.held(Act::Back)
        || autorun
        || camera_forward != camera_back
        || look.is_some_and(|look| look.held && look.left);
    if !pitched || !longitudinal || mode == MovementMode::Strafe || direction == Vec3::ZERO {
        return (direction, false);
    }
    let backing = mode == MovementMode::Backward || camera_back && !camera_forward;
    let pitch = if backing {
        -camera.view_pitch()
    } else {
        camera.view_pitch()
    };
    (
        (direction * pitch.cos() + Vec3::Y * pitch.sin()).normalize_or_zero(),
        !backing && pitch > 0.0,
    )
}

/// The mouse's look and aim, and the spell names a root is read by.
type Pointing<'w> = (
    Option<Res<'w, MouseLook>>,
    Option<Res<'w, MouseAim>>,
    Option<Res<'w, crate::spellbook::SpellNames>>,
);

/// The input system's optional settings: the movement sync choice, the
/// session last told to wait, and the lifts a rider can be carried by.
type InputExtras<'w, 's> = (
    Option<Res<'w, crate::options::MovementSync>>,
    Local<'s, Option<u64>>,
    Option<Res<'w, crate::doors::LiftReach>>,
);

/// Produces at most one in-flight proposal; renderer stalls never accumulate distance.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
pub(super) fn input(
    keys: crate::keys::Keys,
    navigation: Res<super::navigation::NavigationKeys>,
    online: Res<online::OnlineState>,
    collision: Res<Collision>,
    outbox: Res<crate::outbox::Outbox>,
    options: Res<crate::options::OptionsState>,
    (movement_sync, mut told_wait, lifts): InputExtras,
    mut controls: ResMut<Controls>,
    players: Query<(&PlayerBody, &Transform), With<Player>>,
    cameras: Query<(&OrbitCamera, Has<crate::camera::FreeCamera>)>,
    (settings, messages, mut chat, hud): (
        Res<crate::ViewerSettings>,
        Res<crate::hud::messages::Messages>,
        ResMut<crate::chat::ChatState>,
        Option<Res<crate::hud::HudState>>,
    ),
    (look, aim, spells): Pointing,
) {
    let client = settings.0.installed_client;
    // The program's speed-modifier routine ends following once a root
    // stops the player (R6, 0x411076); a server's move does not.
    if let Some(following) = controls.following.as_ref()
        && spells.as_deref().is_some_and(|spells| {
            matches!(
                crate::speed::inputs(online.world(), spells),
                eq_client_core::SpeedInputState::Ready(inputs) if inputs.rate <= -1000
            )
        })
    {
        chat.history.push(crate::chat::system_line(follow::ended(
            follow::End::Stopped,
            following.name(),
            &messages,
        )));
        controls.following = None;
    }
    if chat.follow_asked.take().is_some() {
        let can_move = controls.speed.is_some_and(|speed| speed > 0.0);
        match follow::start(online.world(), can_move, &messages) {
            Ok(following) => {
                follow::started(&following, &messages, &mut chat);
                controls.autorun = false;
                controls.following = Some(following);
            }
            Err(Some(said)) => chat.history.push(crate::chat::system_line(said)),
            Err(None) => (),
        }
    }
    if controls
        .following
        .as_ref()
        .is_some_and(|following| !following.here(online.world()))
    {
        controls.following = None;
    }
    if !online.enabled || !online.world().connected() || online.world().is_dead() {
        controls.following = None;
        *told_wait = None;
        controls.reset(None);
        controls.held = Some(Held::Withdrawn);
        return;
    }
    let detached = cameras.single().is_ok_and(|(_, free)| free);
    let session_id = online.world().session_id();
    controls.pending = controls
        .pending
        .filter(|pending| Some(pending.session_id) == session_id);
    let wanted = eq_client_core::movement::offered_options(&options.options.qol, online.world());
    let Some(movement) = movement_sync
        .as_ref()
        .map_or(Some(wanted), |sync| sync.usable(session_id, wanted))
    else {
        // Say why once for this wait, including when the queue keeps retrying.
        if *told_wait != session_id {
            chat.history.push(crate::chat::system_line(
                "Movement paused while your movement settings are applied.",
            ));
            *told_wait = session_id;
        }
        controls.pause_prediction();
        controls.mouse_heading = None;
        controls.held = Some(Held::Gate);
        return;
    };
    *told_wait = None;
    controls.choose_movement(movement);
    let flying = controls.movement_options.fly;
    let stunned = controls.observe_stun(online.world());
    if detached {
        controls.autorun = false;
        controls.taps = TapBuffer::default();
        controls.jump = None;
        controls.facing = None;
    }
    if controls.speed.is_none() && controls.airborne.is_none() {
        controls.mouse_heading = None;
        controls.held = Some(Held::Withdrawn);
        return;
    }
    let focused = keys.focused() && !detached && !stunned && controls.speed.is_some();
    let scripted_facing = focused.then(|| keys.scripted_facing()).flatten();
    let scripted = keys.scripted();
    let scripted_turn = scripted.contains(&Act::TurnLeft) || scripted.contains(&Act::TurnRight);
    let heading_snapshots = online.world().generation().supports_heading_snapshots();
    // Following owns the heading, so a mouse turn meanwhile is not kept for
    // the sample after it ends.
    let mouse_allowed = focused
        && scripted_facing.is_none()
        && !scripted_turn
        && controls.following.is_none()
        && !aim.as_ref().is_some_and(|aim| aim.interrupted);
    let mouse_aim = aim
        .as_ref()
        .and_then(|aim| aim.horizontal)
        .filter(|_| mouse_allowed);
    if !heading_snapshots || !mouse_allowed {
        controls.mouse_heading = None;
    } else if let Some(yaw) = mouse_aim.filter(|yaw| *yaw != 0.0)
        && let Ok((_, player)) = players.single()
    {
        // Start from the current drawn actor, then apply every owned delta
        // while the next movement sample waits for its normal opportunity.
        let drawn_yaw = controls.mouse_heading.map_or_else(
            || player.rotation.to_euler(EulerRot::YXZ).0,
            eq_client_core::render_heading,
        );
        controls.mouse_heading = Some(eq_client_core::world_heading(drawn_yaw + yaw));
    }
    if let Some(yaw) = mouse_aim.filter(|_| !heading_snapshots) {
        let limit = MOUSE_WIRE_RATE * MAX_CYCLE;
        controls.mouse_turn =
            (controls.mouse_turn + yaw * (512.0 / std::f32::consts::TAU)).clamp(-limit, limit);
    } else {
        controls.mouse_turn = 0.0;
    }
    if focused && keys.pressed(Act::Walk) && controls.walk_speed.is_some() {
        controls.walking = !controls.walking;
    }
    let standing = online
        .world()
        .player()
        .and_then(|player| online.world().posture(player.spawn_id))
        .is_none_or(|posture| posture == eq_client_core::PostureState::Standing);
    if focused && keys.pressed(Act::Autorun) {
        controls.autorun = !controls.autorun;
    } else if keys.pressed(Act::Forward) || keys.pressed(Act::Back) || !standing {
        controls.autorun = false;
    }
    let now = Instant::now();
    let water_enabled =
        client == InstalledClient::Titanium && !flying && online.world().can(Capability::Swimming);
    if !water_enabled || !focused || controls.stun_active {
        controls.shoreline.reset();
    }
    let swimming = water_enabled
        && collision
            .0
            .as_ref()
            .zip(players.single().ok())
            .zip(online.world().player())
            .is_some_and(|((world, (body, _)), player)| {
                let origin = Vec3::from_array(eq_client_core::render_position(player.position));
                world.in_swimmable_water(origin - Vec3::Y * body.feet_offset, body.height)
            });
    let shoreline_active = controls.shoreline.active();
    if swimming || shoreline_active {
        controls.jump = None;
        controls.crouch = None;
    }
    // Held until the next sample; jumps rise and fall like falls, so only
    // where the session takes both, and no sooner after the last than the
    // installed client allows.
    if focused
        && !flying
        && !swimming
        && !shoreline_active
        && !online.world().is_stunned()
        && keys.pressed(Act::Jump)
        && controls.airborne.is_some()
        && controls.crouch.is_none()
        && online.world().can(Capability::Jumping)
    {
        if jump_too_soon(client, controls.last_jump, now) {
            debug!("Jump refused: too soon after the last");
        } else if too_tired_to_jump(
            online.world().vitals().endurance,
            hud.as_ref()
                .and_then(|hud| hud.resource_estimate)
                .map(|(_, endurance)| endurance),
        ) {
            chat.history.push(crate::chat::system_line(
                messages.said_text(TOO_TIRED_TO_JUMP, "You are too tired to jump."),
            ));
        } else {
            controls.jump = Some(now);
        }
    }
    // A crouched jump stands up when its time comes, and goes with the next
    // sample.
    if let Some(Crouch::Until(due)) = controls.crouch.filter(|crouch| match crouch {
        Crouch::Until(due) => now >= *due,
        Crouch::Over => false,
    }) {
        controls.crouch = None;
        match post_posture(&outbox, online.world(), Posture::Standing) {
            Ok(()) => {
                controls.jump = (!detached).then_some(due);
                controls.crouch = Some(Crouch::Over);
            }
            Err(reason) => {
                controls.send_failed(reason);
                controls.held = Some(Held::Refused);
                return;
            }
        }
    }
    let map = &*keys.map;
    let keyboard = navigation.sample(&keys.input);
    // The turn keys ramp up from the frame one is first held, and start
    // over when the way changes or both are let go.
    let turning = if focused && look.as_deref().is_none_or(|look| !look.held) {
        let held = |act| scripted.contains(&act) || map.held(&keyboard, act);
        f32::from(held(Act::TurnLeft)) - f32::from(held(Act::TurnRight))
    } else {
        0.0
    };
    controls.turn_held = match (turning, controls.turn_held) {
        (0.0, _) => None,
        (turning, Some((left, since))) if left == (turning > 0.0) => Some((left, since)),
        (turning, _) => Some((turning > 0.0, now)),
    };
    if focused
        && !controls.stun_active
        && (swimming || shoreline_active)
        && let (Some(player), Ok((camera, _))) = (online.world().player(), cameras.single())
    {
        let heading = controls
            .mouse_heading
            .or(scripted_facing)
            .or(controls.facing)
            .unwrap_or(player.position.heading);
        let yaw = if controls.mouse_heading.is_some() {
            crate::camera::behind(eq_client_core::render_heading(heading))
        } else {
            camera.yaw
        };
        let intent = controls.intent(
            &keyboard,
            map,
            scripted,
            focused,
            (yaw, heading),
            look.as_deref(),
        );
        let (stroke, forward_up) = swim_stroke(
            intent.direction,
            intent.mode,
            (map, &keyboard, scripted),
            (camera, look.as_deref()),
            controls.autorun,
        );
        controls.shoreline.observe(now, stroke, forward_up);
    }
    let mut movement_keys = map.movement_keys();
    if flying {
        movement_keys.extend(
            [Act::Jump, Act::Duck]
                .into_iter()
                .flat_map(|act| map.keys(act)),
        );
    }
    controls
        .taps
        .observe(&keyboard, &movement_keys, focused, now);
    if controls.pending.is_some() {
        if controls.liquid_basis(session_id, now) == LiquidBasis::Unconfirmed {
            controls.pause_prediction();
        }
        controls.held = Some(Held::Waiting);
        return;
    }
    let elapsed = now.duration_since(controls.last_accepted);
    if elapsed < Duration::from_millis(100) {
        controls.held = Some(Held::Gate);
        return;
    }
    // A landing's damage goes before the next sample; a queue with no room
    // for it has none for the sample either.
    if let Some(cost) = controls.unreported.take() {
        controls.unreported = report_fall(&outbox, online.world(), &options.options.qol, cost);
        if controls.unreported.is_some() {
            controls.pause_prediction();
            controls.held = Some(Held::Gate);
            return;
        }
    }
    let (Ok((body, _)), Ok((camera, _)), Some(world), Some(accepted)) = (
        players.single(),
        cameras.single(),
        collision.0.as_ref(),
        online.world().player(),
    ) else {
        controls.held = Some(Held::NoBody);
        return;
    };
    let placement_blocked = client == InstalledClient::Titanium
        && !flying
        && controls.airborne.is_some()
        && controls.placement.holds(
            online.server_placement(),
            accepted.position,
            world,
            body,
            &mut chat,
            now,
        );
    if placement_blocked && controls.speed.is_none() {
        // A fall grant still needs the geometry guard. Readiness reports
        // the withdrawn movement grant before the placement hold.
        controls.jump = None;
        controls.held = Some(Held::Withdrawn);
        return;
    }
    let origin = Vec3::from_array(eq_client_core::render_position(accepted.position));
    let feet = origin - Vec3::Y * body.feet_offset;
    // The lift the last sample stood on has carried the feet by its own rise
    // since, however long the frame took, as the program's floor sweep
    // stands its rider on it each frame; one that sank leaves them to fall
    // at once (read R1).
    let ride = if placement_blocked || flying || swimming {
        Ride::default()
    } else {
        ride(
            world,
            lifts.as_deref(),
            online.world().doors(),
            controls.riding.map(|(door, at, _)| (door, at)),
            (feet, body.height),
            now,
        )
    };
    controls.ride_blocked = ride.blocked;
    let (origin, feet) = (origin + Vec3::Y * ride.rise, feet + Vec3::Y * ride.rise);
    let surface_world = if !placement_blocked
        && !flying
        && !swimming
        && options.options.qol.on(Fix::JesusMode)
        && online.world().can(Capability::GameplayDepartures)
    {
        world.with_liquid_support(feet, body.height)
    } else {
        None
    };
    let world = surface_world.as_ref().unwrap_or(world);
    if !placement_blocked && let Some(player) = online.world().player() {
        let position = player.position;
        let boundary = online
            .regions
            .zone_line_at(eq_client_core::render_position(position));
        if let Some(destination) = controls.boundary.observe(boundary) {
            if let Err(reason) = outbox.post(online.world(), |stamp| ClientCommand::CrossZoneLine {
                session_id: stamp.session_id,
                destination,
                position,
                created: stamp.created,
            }) {
                controls.send_failed(reason);
                controls.held = Some(Held::Refused);
            } else {
                controls.wait_for(session_id, now, PendingKind::Crossing);
                controls.held = Some(Held::Gate);
            }
            return;
        }
    }
    let current_heading = accepted.position.heading;
    // The clock runs from the previous sample's request stamp (which is
    // never after its answer came), so each sample covers the time since
    // the last one, its round trip included, and never more than the time
    // between the two stamps; a fresh start covers at most a tenth of a
    // second.
    let since_sent = controls.last_sent.map_or(elapsed, |sent| {
        now.saturating_duration_since(sent).max(elapsed)
    });
    let span = controls.span(since_sent);
    let mouse_limit = MOUSE_WIRE_RATE * span;
    let mouse_step = controls.mouse_turn.clamp(-mouse_limit, mouse_limit);
    let mouse_snapshot = controls
        .mouse_heading
        .filter(|heading| arc(current_heading, *heading) != 0.0);
    let mouse_steering = mouse_aim.is_some() || mouse_snapshot.is_some();
    // The ground integrator suppresses new lateral acceleration during
    // active yaw. A completed turn may still await its position sample.
    let active_mouse_yaw =
        mouse_step != 0.0 || heading_snapshots && mouse_aim.is_some_and(|yaw| yaw != 0.0);
    let mut heading_update = if mouse_snapshot.is_some() {
        HeadingUpdate::Snapshot
    } else {
        HeadingUpdate::Rate
    };
    let steered_heading = mouse_snapshot.unwrap_or_else(|| {
        if mouse_step == 0.0 {
            current_heading
        } else {
            (current_heading + mouse_step).rem_euclid(512.0)
        }
    });
    if mouse_snapshot.is_none() {
        controls.mouse_heading = None;
    }
    // A script's `face` step turns the player as moving with the
    // camera-relative keys does: the facing it names at once, the heading
    // sent toward it at the turn keys' rate.
    if let Some(heading) = scripted_facing {
        controls.facing = Some(heading.rem_euclid(512.0));
    } else if mouse_steering {
        controls.facing = None;
    }
    let shown = controls.facing.unwrap_or(steered_heading);
    let yaw = if mouse_steering {
        crate::camera::behind(eq_client_core::render_heading(steered_heading))
    } else {
        camera.yaw
    };
    let sampled = controls.taps.take(&keyboard);
    let mut intent = controls.intent(
        &sampled,
        map,
        scripted,
        focused,
        (yaw, shown),
        look.as_deref(),
    );
    // Following turns and goes by itself until a movement key moves the
    // player; it then ends, and the keys move them this sample. A stun
    // holds the player where they are, and following goes on after it.
    let follow_heading = if controls.following.is_some() && !stunned {
        let speeds = (
            controls.speed.unwrap_or(0.0),
            controls.walk_speed.or(controls.speed).unwrap_or(0.0),
        );
        let bodies = follow::Bodies {
            own: accepted.position,
            own_height: body.feet_offset,
            speeds,
        };
        // Only an absolute heading on the wire takes the program's turn; a
        // paced one turns no faster than the turn keys.
        let turn_limit = (!heading_snapshots).then_some(TURN_RATE * span);
        let result = if focused && follow::moves(map, &sampled, scripted) {
            Err(follow::End::Stopped)
        } else {
            controls
                .following
                .as_mut()
                .map_or(Err(follow::End::Stopped), |following| {
                    following.step(online.world(), &bodies, (span, turn_limit, now))
                })
        };
        match result {
            Ok(step) => {
                let radians = eq_client_core::render_heading(step.heading);
                let ahead = Vec3::new(radians.sin(), 0.0, radians.cos());
                let (mode, speed) = match (step.run, controls.walk_speed) {
                    (false, Some(walk)) => (MovementMode::Walk, walk),
                    _ => (MovementMode::Forward, controls.speed.unwrap_or(0.0)),
                };
                // The program stops in the frame it comes within reach; a
                // sample covers many frames, so it goes no further than
                // that frame would.
                let speed = speed.min((step.beyond + speed / 60.0) / span.max(f32::EPSILON));
                intent = MotionInput {
                    mode,
                    speed,
                    direction: if step.forward { ahead } else { Vec3::ZERO },
                    turn: 0.0,
                    preserve_facing: true,
                    diagonal: None,
                };
                if heading_snapshots {
                    heading_update = HeadingUpdate::Snapshot;
                }
                Some(step.heading)
            }
            Err(end) => {
                if let Some(following) = controls.following.take() {
                    chat.history.push(crate::chat::system_line(follow::ended(
                        end,
                        following.name(),
                        &messages,
                    )));
                }
                // The program's end leaves the player running.
                controls.walking = false;
                None
            }
        }
    } else {
        None
    };
    if placement_blocked {
        // Keep the turn inputs and ordinary sample timing. Translation,
        // jumps and gravity cannot start from the suspect received XYZ.
        intent.direction = Vec3::ZERO;
        intent.diagonal = None;
        controls.jump = None;
    }
    // A sitting player stands to move only where Allow Auto-Stand is on; off,
    // the program does not move them (0x4a1d74), so they stay put.
    if !options.options.allow_auto_stand
        && online
            .world()
            .player()
            .and_then(|player| online.world().posture(player.spawn_id))
            == Some(eq_client_core::PostureState::Sitting)
    {
        intent.direction = Vec3::ZERO;
        intent.diagonal = None;
    }
    // Ducking halves a strafe (the Titanium speeds read; the R5 audit's S1),
    // which the session doesn't track.
    if intent.mode == MovementMode::Strafe
        && online
            .world()
            .player()
            .and_then(|player| online.world().posture(player.spawn_id))
            == Some(eq_client_core::PostureState::Ducking)
    {
        intent.speed *= 0.5;
    }
    let MotionInput {
        mode,
        mut speed,
        mut direction,
        mut turn,
        preserve_facing,
        diagonal,
    } = intent;
    if swimming {
        // Titanium backs up in water only by the buffs' modifier (0x4a2bd8;
        // read R13 b), so not at all without one; the grant carries it. A
        // ducked player is stood to move by the session.
        speed = swim_speed(
            controls
                .swim_backward_speed
                .filter(|_| mode == MovementMode::Backward)
                .or(controls.speed)
                .unwrap_or(0.0),
        );
        if !focused || controls.stun_active {
            direction = Vec3::ZERO;
            turn = 0.0;
        } else {
            direction = swim_stroke(
                direction,
                mode,
                (map, &sampled, scripted),
                (camera, look.as_deref()),
                controls.autorun,
            )
            .0;
        }
    }
    let physics = climbing(physics(client, online.world().zone_gravity()), client, body);
    // A hurt or burdened player jumps at the share of the speed the session
    // grants, as the program launches its jumps (0x4193c5).
    let physics = VerticalPhysics {
        jump_speed: physics.jump_speed * controls.jump_factor,
        ..physics
    };
    // The session's guard counts a grounded climb by the body's own reach a
    // step, where the program's is read; it hears it at each admission and
    // whenever it changes, before any sample that climbs by it.
    if !placement_blocked
        && controls.airborne.is_some()
        && client.falls().is_some()
        && let Some(session) = online.world().session_id()
        && controls.climb_sent != Some((session, physics.climb))
    {
        let per_step = physics.climb;
        if let Err(reason) = outbox.post(online.world(), |stamp| ClientCommand::ClimbReach {
            session_id: stamp.session_id,
            per_step,
        }) {
            controls.send_failed(reason);
            controls.held = Some(Held::Refused);
            return;
        }
        controls.climb_sent = Some((session, per_step));
    }
    // A jump or fall keeps going after the keys are released, and a player
    // who may fall starts to where no ground is under them, as the server
    // can leave them after a move it makes (a `#goto` into the air).
    let airborne = !placement_blocked
        && !flying
        && !swimming
        && controls.airborne.as_ref().is_some_and(|airborne| {
            airborne.velocity() != 0.0 || ride.sinking || unsupported(world, feet)
        });
    // So does a heading sent that still turns toward the facing.
    if direction == Vec3::ZERO
        && turn == 0.0
        && !controls.moving
        && !airborne
        && !shoreline_active
        && controls.jump.is_none()
        && controls.facing.is_none()
        && mouse_step == 0.0
        && mouse_snapshot.is_none()
        && diagonal.is_none()
        && follow_heading.is_none_or(|heading| arc(current_heading, heading) == 0.0)
    {
        // A lift rising under a still player stands them on its new floor, as
        // the program's forced floor sweep does (0x4368b3, read R1).
        if !placement_blocked
            && !flying
            && !swimming
            && let Some((door, raised)) = ride.carried.map(|door| (door, feet)).or_else(|| {
                lifts
                    .as_deref()
                    .and_then(|lifts| lift_under(world, lifts, feet, body.height))
            })
        {
            let position = world_position(
                (raised + Vec3::Y * body.feet_offset).to_array(),
                current_heading,
            );
            let mut created = now;
            let sent = outbox.post(online.world(), |stamp| {
                created = stamp.created;
                ClientCommand::Move(
                    MovementRequest::new(
                        MovementMode::Forward,
                        stamp.session_id,
                        position,
                        stamp.created,
                    )
                    .supported_by(Support::Door(door)),
                )
            });
            if let Err(reason) = sent {
                controls.send_failed(reason);
                controls.held = Some(Held::Refused);
                return;
            }
            controls.last_sent = Some(created);
            controls.sent_span = span;
            controls.riding = Some((door, created, raised.y + body.feet_offset));
            controls.held = None;
            controls.wait_for(session_id, created, PendingKind::Move);
            return;
        }
        controls.held = Some(if placement_blocked {
            Held::PlacementBlocked
        } else {
            Held::NoSpeed
        });
        return;
    }
    let mut delta = direction * speed * span;
    let jump = controls.jump.take().is_some();
    let crouched = controls.crouch == Some(Crouch::Over);
    if crouched {
        controls.crouch = None;
    }
    // Where the installed client crouches before a jump from standing still,
    // that jump ducks now and goes once the crouch is over; backing up
    // cancels it. The jump a finished crouch hands on goes as it is. A
    // strafe-only jump counts as moving and goes at once: the read covered
    // the forward input only (inferred, recording checklist 6.417).
    let jump = match client.standing_jump_crouch().filter(|_| !crouched) {
        Some(_) if jump && mode == MovementMode::Backward => {
            debug!("Jump cancelled: backing up");
            false
        }
        Some(crouch) if jump && direction == Vec3::ZERO && !airborne => {
            if let Err(reason) = post_posture(&outbox, online.world(), Posture::Ducking) {
                controls.send_failed(reason);
                controls.held = Some(Held::Refused);
                return;
            }
            controls.crouch = Some(Crouch::Until(now + crouch));
            controls.held = Some(Held::NoSpeed);
            return;
        }
        _ => jump,
    };
    let previous_airborne = controls.airborne.clone();
    let mut shore = if !placement_blocked
        && water_enabled
        && focused
        && let Some((grant, sampled)) = controls.speed.zip(controls.last_sent)
    {
        let limit = if controls.moving {
            Duration::from_secs_f32(MAX_CYCLE)
        } else {
            Duration::from_millis(100)
        };
        controls.shoreline.plan(
            shoreline::Body {
                world,
                feet,
                height: body.height,
                feet_offset: body.feet_offset,
                heading: current_heading,
                gravity: physics.gravity,
                grant,
            },
            sampled,
            limit,
        )
    } else {
        None
    };
    let diagonal_heading = mouse_snapshot.unwrap_or(current_heading);
    // A combined input keeps one body heading and both signed component
    // budgets. Run the usual sweep on a copy before selecting that sample.
    // A slide that spends one component on the other or a ledge that needs
    // a Fall sample uses the ordinary cardinal path from the original state.
    let combined = diagonal
        .filter(|_| {
            standing
                && !jump
                && !airborne
                && !swimming
                && shore.is_none()
                && controls.facing.is_none()
                && !active_mouse_yaw
                && world.support(feet).is_some()
        })
        .and_then(|intent| {
            let displacement = intent.velocity * span;
            let mut trial = previous_airborne.clone();
            let landing = if let Some(controller) = trial.as_mut() {
                let landed = fall_step_with_water(
                    controller,
                    world,
                    physics,
                    client.falls().map(|_| physics.climb),
                    feet,
                    displacement,
                    span,
                    body.height,
                    false,
                    water_enabled,
                );
                if water_enabled && world.in_swimmable_water(landed, body.height) {
                    return None;
                }
                if controller.velocity() != 0.0
                    || controller.peak().is_some()
                    || controller.take_landing().is_some()
                {
                    return None;
                }
                // Airborne combined momentum is outside this grounded
                // budget. Keep any subsequent Fall inside its normal grant.
                let fall_speed = controls.speed.unwrap_or(0.0)
                    * controls.movement_options.run_speed_multiplier.factor();
                controller.limit_drift(fall_speed);
                landed
            } else {
                world.step(feet, displacement, body.height)
            };
            let resolved = world_position(
                (landing + Vec3::Y * body.feet_offset).to_array(),
                diagonal_heading,
            );
            (world.support(landing).is_some()
                && intent.request.admits_with_heading(
                    accepted.position,
                    resolved,
                    heading_update,
                    intent.longitudinal,
                    intent.lateral,
                    span,
                ))
            .then_some((landing, trial, intent.request, displacement))
        });
    let (landing, mut mode, jumped) = if placement_blocked {
        (feet, MovementMode::Forward, false)
    } else if let Some(plan) = &shore {
        (plan.feet, MovementMode::Shoreline(plan.trace), false)
    } else if swimming {
        (
            world.swim_step(feet, delta, body.height),
            MovementMode::Swim,
            false,
        )
    } else if let Some((landing, trial, request, displacement)) = combined {
        controls.airborne = trial;
        delta = displacement;
        (landing, MovementMode::GroundedDiagonal(request), false)
    } else {
        match controls.airborne.as_mut() {
            _ if flying => (world.fly_step(feet, delta, body.height), mode, false),
            Some(airborne) => {
                let rising = airborne.velocity() > 0.0;
                let landing = fall_step_with_water(
                    airborne,
                    world,
                    physics,
                    client.falls().map(|_| physics.climb),
                    feet,
                    delta,
                    span,
                    body.height,
                    jump,
                    water_enabled,
                );
                let falling = airborne.velocity() < 0.0
                    || feet.y - landing.y > eq_client_core::movement::MAX_GROUNDED_STEP;
                let jumped = !rising && airborne.velocity() > 0.0;
                (
                    landing,
                    if falling { MovementMode::Fall } else { mode },
                    jumped,
                )
            }
            None => (world.step(feet, delta, body.height), mode, false),
        }
    };
    // The server charges the jump's endurance; the arc travels in position
    // updates.
    if jumped
        && let Err(reason) = outbox.post(online.world(), |stamp| ClientCommand::Jump {
            session_id: stamp.session_id,
            created: stamp.created,
        })
    {
        controls.airborne = previous_airborne;
        controls.send_failed(reason);
        controls.held = Some(Held::Refused);
        return;
    }
    if jumped {
        controls.last_jump = Some(now);
        // The program takes the jump's endurance off at take-off; the
        // server's next report replaces it.
        controls.endurance_spent = hud
            .as_ref()
            .and_then(|hud| hud.resource_estimate)
            .map(|(_, endurance)| jump_endurance(endurance));
    }
    let position = if placement_blocked {
        origin
    } else {
        stop_at_zone_line(
            &online.regions,
            origin,
            landing + Vec3::Y * body.feet_offset,
        )
    };
    if let Some(plan) = shore.as_mut()
        && position.distance_squared(landing + Vec3::Y * body.feet_offset) > 1e-8
    {
        plan.finish();
        mode = MovementMode::Shoreline(plan.trace);
    }
    let entering = !placement_blocked
        && water_enabled
        && !swimming
        && world.in_swimmable_water(position - Vec3::Y * body.feet_offset, body.height);
    // The turn keys turn at their own rate. Camera-relative movement faces
    // its direction unless mouse-look is steering the body. The sent heading
    // follows the facing within the same sample budget as the position.
    let facing = if let Some(heading) = follow_heading {
        heading
    } else if matches!(mode, MovementMode::GroundedDiagonal(_)) {
        diagonal_heading
    } else if mouse_steering {
        steered_heading
    } else if Vec2::new(direction.x, direction.z) == Vec2::ZERO || preserve_facing {
        let cap = turn_cap(
            controls.walking,
            online.world().player().map_or(0, |player| player.race),
        );
        // The frame a key goes down already turns at the first step.
        let held = controls.turn_held.map_or(0.0, |(_, since)| {
            now.saturating_duration_since(since).as_secs_f32() + 1.0 / TURN_STEPS_PER_SECOND
        });
        (shown + turn * ramped_turn(held - span, held, cap)).rem_euclid(512.0)
    } else {
        eq_client_core::world_heading(direction.x.atan2(direction.z))
    };
    let heading = if mouse_steering || follow_heading.is_some() {
        facing
    } else {
        turn_toward(current_heading, facing, TURN_RATE * span)
    };
    trace_proposal(mode, delta, position - origin, heading, facing);
    let position_sent = if placement_blocked {
        eq_client_core::WorldPosition {
            heading,
            ..accepted.position
        }
    } else {
        world_position(position.to_array(), heading)
    };
    // The sample names the lift that carried the origin, or the one under
    // the landing; only the one under the landing carries the next pass.
    let (support, under) = request_support(ride, world, lifts.as_deref(), landing);
    let mut created = now;
    let sent = outbox.post(online.world(), |stamp| {
        // Shoreline physics already ended at the observed frame. Its next
        // request must budget from that same tick, even if planning was slow.
        created = shore.as_ref().map_or(stamp.created, |plan| plan.created);
        ClientCommand::Move(MovementRequest {
            mode,
            session_id: stamp.session_id,
            position: position_sent,
            heading: heading_update,
            created,
            support: support.map(Support::Door),
        })
    });
    if let Err(reason) = sent {
        controls.airborne = previous_airborne;
        controls.send_failed(reason);
        controls.held = Some(Held::Refused);
        return;
    }
    controls.shoreline.sent(mode, shore, created);
    if entering {
        controls.water_entry_rollback = previous_airborne;
    }
    if !placement_blocked
        && (entering || swimming || matches!(mode, MovementMode::Shoreline(_)))
        && let Some(airborne) = controls.airborne.as_mut()
    {
        airborne.reset();
    }
    controls.recent.record(now, delta, position - origin);
    // The host's movement session budgets each sample from the previous
    // request's stamp to this one's, so the next span runs from the stamp.
    controls.last_sent = Some(created);
    controls.sent_span = span;
    controls.riding = under.map(|door| (door, created, position.y));
    controls.sent_yaw = if mouse_snapshot.is_some() {
        SentYaw::MouseSnapshot
    } else if mouse_aim.is_some() || follow_heading.is_some() && heading_snapshots {
        SentYaw::MouseRate
    } else {
        SentYaw::Ordinary
    };
    controls.mouse_turn -= mouse_step;
    controls.held = placement_blocked.then_some(Held::PlacementBlocked);
    // A landing's damage follows the sample that lands.
    if !placement_blocked
        && let Some(fall) = controls
            .airborne
            .as_mut()
            .and_then(AirborneController::take_landing)
    {
        let cost = landing_cost(
            client,
            world,
            position - Vec3::Y * body.feet_offset,
            fall,
            online.world(),
        );
        let reports_damage = options.options.qol.on(Fix::TakeEnvironmentalDamage)
            && online.world().can(Capability::EnvironmentalDamage);
        if let Some(cost) = cost {
            debug!(
                fall_distance = fall.fall_distance,
                impact_speed = fall.impact_speed,
                carried_weight = ?online.world().fall_weight(),
                damage = cost.damage,
                safe_fall = cost.safe_fall,
                "Landing"
            );
            if reports_damage {
                say_fall(client, cost, online.world(), &messages, &mut chat);
            }
            controls.unreported = report_fall(&outbox, online.world(), &options.options.qol, cost);
        } else if reports_damage {
            chat.history.push(crate::chat::system_line(
                "Fall damage could not be reported because carried weight is unknown.",
            ));
        }
    }
    controls.moving = position.distance_squared(origin) > 0.000_001
        || mouse_snapshot.is_none() && (heading - current_heading).abs() > f32::EPSILON;
    // Until the heading sent reaches the facing, the model shows the facing.
    controls.facing = (arc(heading, facing).abs() > 0.01).then_some(facing);
    controls.wait_for(session_id, created, PendingKind::Move);
}

/// How the lift the last sample stood on moved the feet since that sample.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Ride {
    /// The lift that carried them, when it rose.
    carried: Option<u8>,
    /// How far it raised them.
    rise: f32,
    /// Whether it sank away from under them, so they fall at once.
    sinking: bool,
    /// Whether it rose but the body had no room to be carried.
    blocked: bool,
}

/// How far the lift a rider stood on at `riding`'s stamp has carried their
/// `feet` by `now`: its own rise over the gap, exact however long the gap
/// ([`eq_client_core::doors::titanium::Lifts::rise`]), when the body fits
/// there (recording checklist 6.427); or that it sank.
fn ride(
    world: &eq_client_core::movement::CollisionWorld,
    reach: Option<&crate::doors::LiftReach>,
    doors: &eq_client_core::doors::DoorTable,
    riding: Option<(u8, Instant)>,
    (feet, height): (Vec3, f32),
    now: Instant,
) -> Ride {
    let Some((door, since)) =
        riding.filter(|(door, _)| reach.is_some_and(|reach| reach.0.contains_key(door)))
    else {
        return Ride::default();
    };
    let lifts = doors.lifts();
    let rise = lifts.rise(door, since, now);
    let raised = feet + Vec3::Y * rise;
    if rise > 0.0
        && world
            .clearance(raised, height, 0.0)
            .is_none_or(|clearance| clearance > 0.0)
    {
        Ride {
            carried: Some(door),
            rise,
            sinking: false,
            blocked: false,
        }
    } else {
        Ride {
            sinking: lifts.sinks(door, since, now),
            blocked: rise > 0.0,
            ..Ride::default()
        }
    }
}

/// The lift a sample names, and the one that carries the next pass. The
/// sample names the lift that carried its origin this pass, even when the
/// step lands off it, or else the one under its landing; the session still
/// checks the rider's reach. Only the lift under the landing carries the
/// next pass, so a rider who stepped off is not raised beside it.
fn request_support(
    ride: Ride,
    world: &eq_client_core::movement::CollisionWorld,
    lifts: Option<&crate::doors::LiftReach>,
    landing: Vec3,
) -> (Option<u8>, Option<u8>) {
    let under = lifts.and_then(|lifts| lift_support(world, lifts, landing));
    (ride.carried.or(under), under)
}

/// The rideable lift whose floor holds the feet up, when they stand within
/// its reach ([`crate::doors::LiftReach`]).
fn lift_support(
    world: &eq_client_core::movement::CollisionWorld,
    lifts: &crate::doors::LiftReach,
    feet: Vec3,
) -> Option<u8> {
    let door = world.ground_door(feet, 0.05, 0.05)?;
    let (center, reach) = lifts.0.get(&door)?;
    (center.distance(feet) <= *reach).then_some(door)
}

/// A rideable lift that rose into the still feet: the door, and the feet
/// stood on its new floor. The program's forced floor sweep stands a rider
/// on the risen floor; where the body would not fit there, what it does is
/// unread, and this leaves the player to the ordinary support and fall test
/// (recording checklist 6.427).
fn lift_under(
    world: &eq_client_core::movement::CollisionWorld,
    lifts: &crate::doors::LiftReach,
    feet: Vec3,
    height: f32,
) -> Option<(u8, Vec3)> {
    let rise = eq_client_core::movement::MAX_GROUNDED_STEP;
    let door = world.ground_door(feet, rise, 0.05)?;
    let (center, reach) = lifts.0.get(&door)?;
    if center.distance(feet) > *reach {
        return None;
    }
    let floor = world.ground(feet, rise, 0.05)?;
    if floor <= feet.y + 0.001 {
        return None;
    }
    let raised = Vec3::new(feet.x, floor, feet.z);
    world
        .clearance(raised, height, 0.0)
        // A capsule crossing a mesh seam can report touching (0) rather than
        // its depth, so touching counts as not fitting.
        .is_none_or(|clearance| clearance > 0.0)
        .then_some((door, raised))
}

/// Whether no ground is under the feet within the step the server's
/// movement guard takes as grounded: the player there falls.
fn unsupported(world: &eq_client_core::movement::CollisionWorld, feet: Vec3) -> bool {
    world.support(feet).is_none()
        && world
            .ground(feet, 0.01, eq_client_core::movement::MAX_GROUNDED_STEP)
            .is_none()
}

/// Walks like a grounded step but keeps going past ledges and falls under gravity,
/// in the 50 ms slices the airborne controller steps; `jump` starts a jump
/// in the first slice when the character stands on the ground. Each slice may
/// climb, so the sample stops before its total rise passes what the session's
/// movement guard accepts: `climb` (the body's reach a step, which the session
/// has heard) in each program step the sample has spanned, or one riser a
/// sample where none is told, or one riser for a sample that ends falling
/// (the guard's fall rule); and while a jump rises, as far as the jump's
/// speed less gravity carries it in the sample (the guard's own rule after
/// the session's jump notice). A larger climb takes several accepted
/// samples instead of one that would be refused and retried forever.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn fall_step(
    airborne: &mut eq_client_core::movement::AirborneController,
    world: &eq_client_core::movement::CollisionWorld,
    physics: VerticalPhysics,
    climb: Option<f32>,
    feet: Vec3,
    delta: Vec3,
    span: f32,
    height: f32,
    jump: bool,
) -> Vec3 {
    fall_step_with_water(
        airborne, world, physics, climb, feet, delta, span, height, jump, false,
    )
}

#[allow(clippy::too_many_arguments)]
fn fall_step_with_water(
    airborne: &mut AirborneController,
    world: &CollisionWorld,
    physics: VerticalPhysics,
    climb: Option<f32>,
    feet: Vec3,
    delta: Vec3,
    span: f32,
    height: f32,
    jump: bool,
    water_enabled: bool,
) -> Vec3 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // At most five slices.
    let slices = (span / 0.05).ceil().clamp(1.0, 5.0) as u8;
    let slice = f32::from(slices);
    let mut position = feet;
    let start = airborne.velocity().max(0.0);
    let mut rising = start;
    for index in 0..slices {
        let mut trial = airborne.clone();
        let step = eq_client_core::movement::MotionStep {
            horizontal: delta / slice,
            jump: jump && index == 0,
            seconds: span / slice,
            height,
        };
        let next = if water_enabled {
            trial.step_into_water(world, position, physics, step)
        } else {
            trial.step(world, position, physics, step)
        };
        // A jump that started in this slice rises from its own speed.
        if index == 0 && trial.velocity() > start {
            rising = physics.jump_speed.max(start);
        }
        let elapsed = span / slice * f32::from(index + 1);
        let riser = eq_client_core::movement::MAX_GROUNDED_STEP;
        let reach = match climb {
            Some(per_step) if trial.velocity() >= 0.0 => eq_client_core::movement::climb_reach(
                per_step,
                eq_client_core::movement::program_steps(elapsed),
            ),
            _ => riser,
        };
        let allowed = reach + jump_rise(rising, physics.gravity, elapsed);
        if next.y - feet.y > allowed {
            break;
        }
        *airborne = trial;
        position = next;
        if water_enabled && world.in_swimmable_water(position, height) {
            break;
        }
    }
    position
}

/// How far a body rising at `speed` climbs in `seconds` under `gravity`,
/// and no farther than the top: the rise the session's movement guard
/// grants after a jump notice. The controller's air factor only slows the
/// rise, so the body climbs no farther than this.
fn jump_rise(speed: f32, gravity: f32, seconds: f32) -> f32 {
    let seconds = if gravity > 0.0 {
        seconds.min(speed / gravity)
    } else {
        seconds
    };
    (speed * seconds - gravity * seconds * seconds / 2.0).max(0.0)
}

/// How the installed client's player falls: its program's physics under
/// the zone header's gravity once it comes, or the provisional ones until
/// they are read for it. A walking step keeps the feet on the floor only
/// within the program's on-ground tolerance and falls past it, so a steep
/// face goes down at the walk's own pace as a run of short falls, each
/// landing by the program's rule (R-s). This client steps once each 50 ms,
/// the program one step per update, so how often those short falls land
/// waits on the update-rate question for the owner. A body climbs a riser
/// a step until [`climbing`] gives it its own reach.
pub(super) fn physics(client: InstalledClient, zone_gravity: Option<f32>) -> VerticalPhysics {
    client.falls().map_or(PROVISIONAL_PHYSICS, |falls| {
        let gravity = zone_gravity.map_or(falls.gravity, |per_step| falls.zone_gravity(per_step));
        VerticalPhysics {
            gravity,
            terminal_speed: falls.terminal_under(gravity),
            jump_speed: falls.jump_speed,
            air_factor: falls.air_factor,
            ground_tolerance: falls.ground_tolerance,
            climb: eq_client_core::movement::MAX_GROUNDED_STEP,
            steers_in_air: false,
        }
    })
}

/// `physics` for the player's own body: a step climbs onto a floor up to
/// its reach, the program's floor sweep from the origin, which stands the
/// body's feet offset above the feet (read for TAKP; for Titanium that the
/// origin stands there is inferred, 6.347). Unchanged where the
/// generation's falls are unread.
pub(super) fn climbing(
    physics: VerticalPhysics,
    client: InstalledClient,
    body: &PlayerBody,
) -> VerticalPhysics {
    client.falls().map_or(physics, |falls| VerticalPhysics {
        climb: falls.climb_per_step(body.feet_offset, body.size),
        ..physics
    })
}

/// Asks the session to change the player's stance.
fn post_posture(
    outbox: &crate::outbox::Outbox,
    world: &ClientWorld,
    posture: Posture,
) -> Result<(), crate::outbox::Refusal> {
    let spawn_id = world.player().map(|player| player.spawn_id);
    let spawn_id = spawn_id.ok_or(crate::outbox::Refusal::NotAdmitted)?;
    outbox.post(world, |stamp| ClientCommand::SetPosture {
        session_id: stamp.session_id,
        spawn_id,
        posture,
        created: stamp.created,
    })
}

/// Whether a jump now comes no later after the last take-off than the
/// installed client's spacing, which the programs refuse (they take a jump
/// only when more than the spacing has passed).
/// The official line for a jump refused as too tired, by `eqstr_us.txt`
/// number.
const TOO_TIRED_TO_JUMP: u32 = 12362;

/// Whether the program refuses a jump as too tired: endurance at or under 50
/// and at or under a tenth of the maximum (Titanium 0x4a2796-0x4a27f4, after
/// the spacing check). The maximum is the client's own estimate; without one
/// the rule can't be worked out, so the jump goes as before (inferred,
/// recording checklist 6.415).
fn too_tired_to_jump(endurance: Option<u32>, maximum: Option<u32>) -> bool {
    endurance
        .zip(maximum)
        .is_some_and(|(now, maximum)| now <= 50 && now <= maximum / 10)
}

/// What a jump costs in endurance at take-off: a tenth of the maximum, at
/// most 50 (Titanium 0x4acb38). The maximum is the client's own estimate, as
/// for [`too_tired_to_jump`] (inferred, recording checklist 6.415); without
/// one, nothing is taken off and the server's report shows the cost.
fn jump_endurance(maximum: u32) -> u32 {
    (maximum / 10).min(50)
}

/// Shows the endurance a jump that just took off costs, as [`input`] left it
/// (see [`jump_endurance`]).
pub(super) fn spend_endurance(
    mut controls: ResMut<Controls>,
    mut online: ResMut<online::OnlineState>,
) {
    if let Some(cost) = controls.endurance_spent {
        controls.endurance_spent = None;
        online.spend_endurance(cost);
    }
}

fn jump_too_soon(client: InstalledClient, last: Option<Instant>, now: Instant) -> bool {
    client
        .jump_spacing()
        .zip(last)
        .is_some_and(|(spacing, last)| now.saturating_duration_since(last) <= spacing)
}

/// What a landing with the feet here costs the player: nothing in water or
/// lava (`EQEmu` ignores a fall reported there, `Client::Handle_OP_EnvDamage`,
/// and the Mac-era program's liquids slow a fall to nothing), else the
/// installed client's rule. Until a client's rule is read, its fall costs the
/// inferred [`CollisionWorld::landing_damage`], and Safe Fall is announced,
/// as both programs announce it, wherever the player has the skill and the
/// fall did damage for it to lessen (Titanium: recording checklist item
/// 6.148). The rank of the ability that raises Safe Fall is not in what the
/// sessions decode, so it counts as none (unsure).
fn landing_cost(
    client: InstalledClient,
    collision: &CollisionWorld,
    feet: Vec3,
    landing: Landing,
    world: &ClientWorld,
) -> Option<FallCost> {
    if collision.in_liquid(feet) {
        return Some(FallCost::default());
    }
    let skill = safe_fall(world);
    Some(match client.falls() {
        Some(falls) => {
            let zone = eq_client_core::zones::number(world.zone()).unwrap_or(0);
            let Some(carried_weight) = world.fall_weight() else {
                return falls.landing_cost_without_weight(landing.impact_speed, zone);
            };
            let landing = FallLanding {
                impact_speed: landing.impact_speed,
                carried_weight,
                safe_fall: skill,
                safe_fall_ranks: 0,
                zone,
            };
            let mut cost = falls.landing_cost(landing);
            if cost.safe_fall && world.worker_inventory().ground_drop_pending() {
                // Extra mass can make an otherwise harmless landing eligible
                // for Safe Fall. Award the notice only when even zero weight
                // would have used the skill and the upper bound still uses it.
                cost.safe_fall = falls
                    .landing_cost(FallLanding {
                        carried_weight: 0.0,
                        ..landing
                    })
                    .safe_fall;
            }
            cost
        }
        None => FallCost {
            damage: collision.landing_damage(feet, landing, skill),
            safe_fall: skill > 0 && collision.landing_damage(feet, landing, 0) > 0,
        },
    })
}

/// Says in the chat what a landing that hurt did, in the installed client's
/// lines where its program's are read: the damage, then, on a fall the
/// player lives through, the second line.
fn say_fall(
    client: InstalledClient,
    cost: FallCost,
    world: &ClientWorld,
    messages: &crate::hud::messages::Messages,
    chat: &mut crate::chat::ChatState,
) {
    let Some(falls) = client.falls().filter(|_| cost.damage > 0) else {
        return;
    };
    if let Some(line) = falls.damage_line {
        chat.history.push(crate::chat::system_line(
            messages.said(line, &[cost.damage.to_string()]),
        ));
    }
    let survived = world
        .hit_points()
        .is_some_and(|(current, _)| i64::from(current) - i64::from(cost.damage) > -10);
    if let Some(line) = falls.survived_line.filter(|_| survived) {
        chat.history
            .push(crate::chat::system_line(messages.said(line, &[])));
    }
}

/// Tells the server what a landing cost the player, where the session
/// reports the world's damage and the player takes it
/// ([`Fix::TakeEnvironmentalDamage`]): Safe Fall's use first, where it was
/// used, then the damage. A landing that cost nothing sends nothing. Gives
/// the cost back when the session's queue has no room for it, to tell again
/// before the next sample.
fn report_fall(
    outbox: &crate::outbox::Outbox,
    world: &ClientWorld,
    qol: &Settings,
    cost: FallCost,
) -> Option<FallCost> {
    let stamp = outbox.peek(world)?;
    let wanted = (cost.damage > 0 || cost.safe_fall)
        && qol.on(Fix::TakeEnvironmentalDamage)
        && world.can(Capability::EnvironmentalDamage);
    let told = wanted
        && outbox.tell(
            world,
            ClientCommand::EnvironmentalDamage {
                session_id: stamp.session_id,
                hazard: Hazard::Falling,
                amount: cost.damage,
                safe_fall: cost.safe_fall,
            },
        );
    debug!(
        damage = cost.damage,
        safe_fall = cost.safe_fall,
        told,
        "Fall damage"
    );
    (wanted && !told).then_some(cost)
}

/// The Safe Fall the player has learned; none where the profile says nothing
/// of it or marks it unlearned.
fn safe_fall(world: &ClientWorld) -> u32 {
    world
        .player()
        .and_then(|player| player.skills.as_deref())
        .and_then(|skills| skills.get(usize::try_from(eq_client_core::skills::SAFE_FALL).ok()?))
        .copied()
        .filter(|value| *value < crate::skills::RESET)
        .unwrap_or(0)
}

/// One accepted sample waiting to play: where it ends, how many seconds of
/// play it covers, and whether its facing shows at once.
#[derive(Clone, Copy, Debug)]
struct Segment {
    to: Transform,
    span: f32,
    snap: bool,
    follow_heading: bool,
}

/// The accepted samples the model plays back, one after another, each over
/// its own span; never past the newest accepted position.
struct VisualMotion {
    from: Transform,
    to: Transform,
    elapsed: f32,
    duration: f32,
    follow_heading: bool,
    /// Samples accepted while an earlier one still plays, oldest first.
    queue: std::collections::VecDeque<Segment>,
}

impl VisualMotion {
    /// Plays `segment` from where the model is shown now.
    fn start(from: Transform, segment: Segment) -> Self {
        Self {
            from: if segment.snap {
                from.with_rotation(segment.to.rotation)
            } else {
                from
            },
            to: segment.to,
            elapsed: 0.0,
            duration: segment.span,
            follow_heading: segment.follow_heading,
            queue: std::collections::VecDeque::new(),
        }
    }

    /// Faces the model `rotation` from now on, through every sample still
    /// to play.
    fn face(&mut self, rotation: Quat) {
        self.from.rotation = rotation;
        self.to.rotation = rotation;
        for segment in &mut self.queue {
            segment.to.rotation = rotation;
        }
    }

    /// Where the newest accepted sample ends.
    fn last(&self) -> Transform {
        self.queue.back().map_or(self.to, |segment| segment.to)
    }

    /// Whether everything accepted has played.
    fn finished(&self) -> bool {
        self.queue.is_empty() && self.elapsed >= self.duration
    }

    /// Plays on `seconds`, moving to the next accepted sample as each ends,
    /// with the time left over carried into it.
    fn advance(&mut self, seconds: f32, transform: &mut Transform) {
        self.elapsed += seconds.max(0.0);
        while self.elapsed >= self.duration
            && let Some(next) = self.queue.pop_front()
        {
            self.elapsed -= self.duration;
            let mut from = self.to;
            if next.snap {
                from.rotation = next.to.rotation;
            }
            self.from = from;
            self.to = next.to;
            self.duration = next.span;
            self.follow_heading = next.follow_heading;
        }
        let fraction = (self.elapsed / self.duration.max(0.01)).clamp(0.0, 1.0);
        transform.translation = self.from.translation.lerp(self.to.translation, fraction);
        transform.rotation = self.from.rotation.slerp(self.to.rotation, fraction);
    }
}

/// Advances the player and follow camera every rendered frame between network
/// samples. With `eq_client_render::motion::pace` logging at debug, each
/// frame says how fast the model was shown moving, for a sitting's numbers.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn interpolate(
    time: Res<Time>,
    online: Res<online::OnlineState>,
    mut controls: ResMut<Controls>,
    mut players: Query<(Entity, &mut Transform), With<Player>>,
    mut cameras: Query<(&mut OrbitCamera, Has<crate::camera::FreeCamera>)>,
    mut displayed_mouse: Local<Option<Entity>>,
) {
    let preview = controls.mouse_heading;
    if controls.visual.is_none()
        && preview.is_none()
        && displayed_mouse.is_none()
        && controls.riding.is_none()
    {
        return;
    }
    let Ok((entity, mut transform)) = players.single_mut() else {
        *displayed_mouse = None;
        return;
    };
    let had_preview = displayed_mouse.take() == Some(entity);
    let mut follow_heading = false;
    if let Some(visual) = controls.visual.as_mut() {
        let seconds = time.delta_secs();
        let before = transform.translation;
        visual.advance(seconds, &mut transform);
        if seconds > 0.0 {
            let moved = transform.translation - before;
            debug!(
                target: "eq_client_render::motion::pace",
                seconds,
                shown_speed = moved.with_y(0.0).length() / seconds,
                waiting = visual.queue.len(),
                "Shown pace"
            );
        }
        follow_heading = visual.follow_heading;
        if visual.finished() {
            controls.visual = None;
        }
    }
    // A rising lift carries the drawn model on its floor between samples,
    // as the program draws its rider; the samples sent are unchanged.
    if let Some((door, at, sent)) = controls.riding.filter(|_| !controls.ride_blocked) {
        let floor = sent
            + online
                .world()
                .doors()
                .lifts()
                .rise(door, at, std::time::Instant::now());
        transform.translation.y = transform.translation.y.max(floor);
    }
    if let Some(heading) = preview {
        // Position still plays only accepted samples. An older queued pose
        // never overwrites the latest local mouse orientation.
        transform.rotation = Quat::from_rotation_y(eq_client_core::render_heading(heading));
        *displayed_mouse = Some(entity);
    } else if had_preview && let Some(player) = online.world().player() {
        // A withdrawn or interrupted preview must not leave its pose behind.
        let rotation =
            Quat::from_rotation_y(eq_client_core::render_heading(player.position.heading));
        transform.rotation = rotation;
        if let Some(visual) = controls.visual.as_mut() {
            visual.face(rotation);
        }
    }
    for (mut camera, free) in &mut cameras {
        camera.focus = transform.translation;
        if (follow_heading || preview.is_some() || had_preview) && !free {
            crate::camera::start_behind(&mut camera, &transform);
        }
    }
}

/// Keeps a crossing inside its boundary until the accepted position triggers a handoff.
fn stop_at_zone_line(
    regions: &eq_client_assets::regions::ZoneRegions,
    start: Vec3,
    end: Vec3,
) -> Vec3 {
    regions
        .zone_line_entry(start.to_array(), end.to_array())
        .map_or(end, Vec3::from_array)
}

/// Logs displacement magnitudes, the heading sent and the facing, without
/// recording the character's world coordinates.
fn trace_proposal(mode: MovementMode, requested: Vec3, resolved: Vec3, heading: f32, facing: f32) {
    debug!(
        ?mode,
        requested_distance = requested.length(),
        resolved_distance = resolved.length(),
        heading,
        facing,
        "Movement proposal after local collision"
    );
}

/// Input intent remains separate from collision, timing and network submission.
struct MotionInput {
    mode: MovementMode,
    speed: f32,
    direction: Vec3,
    turn: f32,
    preserve_facing: bool,
    diagonal: Option<DiagonalIntent>,
}

#[derive(Clone, Copy)]
struct DiagonalIntent {
    request: GroundedDiagonal,
    longitudinal: f32,
    lateral: f32,
    velocity: Vec3,
}

/// Resolves character-relative arrows and camera-relative actions without mixing
/// modes. `looking_ahead` is the left button held
/// while the right one looks around, which runs the way the camera faces as
/// the camera-relative forward key does (Adam's observation of the official
/// client; inferred, recording checklist 6.341).
fn movement_input(
    held: crate::keys::Holding,
    (focused, looking_ahead, autorun): (bool, bool, bool),
    camera_yaw: f32,
    current_heading: f32,
    speed: f32,
    backward_speed: Option<f32>,
    turn: f32,
) -> MotionInput {
    let (ahead, behind) = (held.held(Act::Forward), held.held(Act::Back));
    let backing = focused && behind && !ahead;
    // Autorun runs on whatever has the keyboard, as held forward does.
    let forward = (focused && ahead && !behind) || (autorun && !backing);
    let heading_radians = eq_client_core::render_heading(current_heading);
    let facing = Vec3::new(heading_radians.sin(), 0.0, heading_radians.cos());
    let (mode, speed, direction) = if backing {
        (
            MovementMode::Backward,
            backward_speed.unwrap_or(0.0),
            -facing,
        )
    } else if forward {
        (MovementMode::Forward, speed, facing)
    } else if focused {
        (
            MovementMode::Forward,
            speed,
            camera_relative_direction(
                held.axis(Act::CameraLeft, Act::CameraRight),
                f32::from(looking_ahead || held.held(Act::CameraForward))
                    - f32::from(held.held(Act::CameraBack)),
                camera_yaw,
            ),
        )
    } else {
        (MovementMode::Forward, speed, Vec3::ZERO)
    };
    // An unavailable backward mode can stop previous motion, but cannot send
    // a backward proposal or inherit the forward speed.
    let (mode, direction) = if speed == 0.0 {
        (MovementMode::Forward, Vec3::ZERO)
    } else {
        (mode, direction)
    };
    MotionInput {
        mode,
        speed,
        direction,
        turn,
        preserve_facing: backing || forward,
        diagonal: None,
    }
}

/// Rotates through the shortest arc without snapping across the heading wrap.
fn turn_toward(current: f32, desired: f32, limit: f32) -> f32 {
    (current + arc(current, desired).clamp(-limit, limit)).rem_euclid(512.0)
}

/// The shortest signed turn from one heading to another, in heading units.
fn arc(from: f32, to: f32) -> f32 {
    (to - from + 256.0).rem_euclid(512.0) - 256.0
}

#[cfg(test)]
pub(super) mod tests {
    use eq_client_core::{
        movement::{Liquid, Liquids, RunSpeedMultiplier},
        options::Level,
    };

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one ride, its step-off and its sinking"
    )]
    fn a_ride_carries_the_feet_by_the_lifts_own_rise_across_any_frame_gap() {
        use eq_client_core::doors::{Door, DoorTable, DoorUpdate};
        use eq_client_core::movement::{CollisionMesh, CollisionWorld};
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let mut doors = DoorTable::default();
        let lift = Door {
            id: 4,
            model: "LIFT".into(),
            position: eq_client_core::WorldPosition::default(),
            incline: 0,
            size: 100,
            open_type: 59,
            state_at_spawn: 0,
            invert_state: 0,
            parameter: 90,
            action: None,
        };
        doors.apply(&DoorUpdate::Spawn(vec![lift]), start);
        doors.apply(&DoorUpdate::Move { id: 4, action: 2 }, start);
        let world = CollisionWorld::new([
            [[-50.0, 0.0, -50.0], [50.0, 0.0, -50.0], [50.0, 0.0, 50.0]],
            [[-50.0, 0.0, -50.0], [50.0, 0.0, 50.0], [-50.0, 0.0, 50.0]],
        ])
        .unwrap();
        let mut reach = crate::doors::LiftReach::default();
        reach.0.insert(4, (Vec3::ZERO, 310.0));
        // A still or walking rider: the last sample stood on the lift at
        // 1 s, at 9 up; the next pass comes 600 ms later, the lift 5.4
        // higher, well past any step the sweep would take.
        let since = start + Duration::from_secs(1);
        let feet = Vec3::new(2.0, 9.0, -1.0);
        let now = since + Duration::from_millis(600);
        let carried = super::ride(
            &world,
            Some(&reach),
            &doors,
            Some((4, since)),
            (feet, 6.0),
            now,
        );
        assert_eq!(carried.carried, Some(4));
        assert!((carried.rise - 5.4).abs() < 0.01, "{carried:?}");
        assert!(!carried.sinking);
        // Carried up and stepped off onto the zone in the same pass, the
        // sample still names the lift that raised it, but the lift no
        // longer carries the next pass.
        let off = Vec3::new(30.0, 14.4, -1.0);
        assert_eq!(super::lift_support(&world, &reach, off), None);
        let (named, under) = super::request_support(carried, &world, Some(&reach), off);
        assert_eq!((named, under), (Some(4), None));
        assert_eq!(
            super::request_support(super::Ride::default(), &world, Some(&reach), off),
            (None, None)
        );
        // So the next pass, with the lift still rising, neither raises the
        // rider beside it nor names it, and when it later sinks a rider
        // standing on the platform beside it is not falling.
        let next = now + Duration::from_millis(500);
        let riding = under.map(|door| (door, now));
        let stepped_off = super::ride(&world, Some(&reach), &doors, riding, (off, 6.0), next);
        assert_eq!(stepped_off, super::Ride::default());
        assert_eq!(
            super::request_support(stepped_off, &world, Some(&reach), off),
            (None, None)
        );
        let down = start + Duration::from_secs(31);
        assert!(!super::ride(&world, Some(&reach), &doors, riding, (off, 6.0), down).sinking);
        // Nothing rides a door that is not a listed lift, or nothing named.
        let unlisted = crate::doors::LiftReach::default();
        assert_eq!(
            super::ride(
                &world,
                Some(&unlisted),
                &doors,
                Some((4, since)),
                (feet, 6.0),
                now
            ),
            super::Ride::default()
        );
        assert_eq!(
            super::ride(&world, Some(&reach), &doors, None, (feet, 6.0), now),
            super::Ride::default()
        );
        // A ceiling in the way stops the carry (checklist 6.427).
        let mut capped = world.clone();
        capped.set_obstacles(vec![std::sync::Arc::new(
            CollisionMesh::new([
                [[-5.0, 18.0, -5.0], [5.0, 18.0, -5.0], [5.0, 18.0, 5.0]],
                [[-5.0, 18.0, -5.0], [5.0, 18.0, 5.0], [-5.0, 18.0, 5.0]],
            ])
            .unwrap(),
        )]);
        let blocked = super::ride(
            &capped,
            Some(&reach),
            &doors,
            Some((4, since)),
            (feet, 6.0),
            now,
        );
        assert_eq!(blocked.carried, None);
        assert!(!blocked.sinking);
        // Nor is the model drawn rising there.
        assert!(blocked.blocked && !carried.blocked);
        // On its way back down (from 30 s), the rider falls at once.
        let down = start + Duration::from_secs(31);
        let sinking = super::ride(
            &world,
            Some(&reach),
            &doors,
            Some((4, down)),
            (Vec3::new(2.0, 81.0, -1.0), 6.0),
            down + Duration::from_millis(100),
        );
        assert_eq!(sinking.carried, None);
        assert!(sinking.sinking);
    }

    #[test]
    fn a_still_rider_stands_on_a_risen_lift_only_inside_its_reach_and_with_room() {
        use eq_client_core::movement::{CollisionMesh, CollisionWorld};
        use std::sync::Arc;
        let deck = |y: f32| {
            Arc::new(
                CollisionMesh::new([
                    [[-5.0, y, -5.0], [5.0, y, -5.0], [5.0, y, 5.0]],
                    [[-5.0, y, -5.0], [5.0, y, 5.0], [-5.0, y, 5.0]],
                ])
                .unwrap(),
            )
        };
        let mut world = CollisionWorld::new([
            [[-50.0, 0.0, -50.0], [50.0, 0.0, -50.0], [50.0, 0.0, 50.0]],
            [[-50.0, 0.0, -50.0], [50.0, 0.0, 50.0], [-50.0, 0.0, 50.0]],
        ])
        .unwrap();
        let solid = eq_client_core::doors::Solidity::BOTH;
        world.set_door_obstacles_with(vec![(4, deck(0.5), solid)]);
        let mut lifts = crate::doors::LiftReach::default();
        lifts.0.insert(4, (Vec3::ZERO, 310.0));
        // Off the decks' diagonal seam.
        let feet = Vec3::new(2.0, 0.3, -1.0);
        let (door, raised) = super::lift_under(&world, &lifts, feet, 6.0).unwrap();
        assert_eq!(door, 4);
        assert!(raised.distance(Vec3::new(feet.x, 0.5, feet.z)) < 0.001);
        assert_eq!(super::lift_support(&world, &lifts, raised), Some(4));
        // Standing level with the floor already, nothing new is sent.
        assert!(super::lift_under(&world, &lifts, raised, 6.0).is_none());
        // Past the door's reach it carries no one.
        lifts.0.insert(4, (Vec3::new(400.0, 0.0, 0.0), 310.0));
        assert!(super::lift_under(&world, &lifts, feet, 6.0).is_none());
        assert_eq!(super::lift_support(&world, &lifts, raised), None);
        // A door that is not a listed lift carries no one either.
        lifts.0.clear();
        assert!(super::lift_under(&world, &lifts, feet, 6.0).is_none());
        // With a ceiling in the way the body would not fit; it stays put
        // (recording checklist 6.427).
        lifts.0.insert(4, (Vec3::ZERO, 310.0));
        world.set_door_obstacles_with(vec![(4, deck(0.5), solid), (5, deck(4.0), solid)]);
        assert!(super::lift_under(&world, &lifts, feet, 6.0).is_none());
        let seam = Vec3::new(1.0, 0.3, 1.0);
        assert!(super::lift_under(&world, &lifts, seam, 6.0).is_none());
    }

    fn movement_choice(app: &mut App, fix: Fix, on: bool) {
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(fix, on);
        let options = eq_client_core::movement::options(
            &app.world()
                .resource::<crate::options::OptionsState>()
                .options
                .qol,
        );
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.choose_movement(options);
        // These motion fixtures start after the policy's new local gate. Its
        // first-frame hold is exercised separately below.
        controls.last_accepted = Instant::now()
            .checked_sub(Duration::from_millis(100))
            .unwrap();
    }

    #[test]
    fn held_movement_and_prediction_continue_through_run_speed_changes() {
        let (mut app, receiver) = app();
        app.init_resource::<crate::options::MovementSync>()
            .add_systems(Update, crate::options::tell_choices.before(input));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        // Establish this admission's default options before starting to run.
        app.update();
        while receiver.try_recv().is_ok() {}
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.update();
        let move_sample = || match receiver.try_recv().unwrap() {
            ClientCommand::Move(sample) => sample,
            command => panic!("expected movement, got {command:?}"),
        };
        let change_speed = |app: &mut App, percent| {
            app.world_mut()
                .resource_mut::<crate::options::OptionsState>()
                .options
                .set_level(Level::RunSpeed, percent);
            app.update();
            let ClientCommand::SetMovementOptions {
                session_id: 11,
                request_id,
                options,
                ..
            } = receiver.try_recv().unwrap()
            else {
                panic!("expected speed change");
            };
            assert_eq!(options.run_speed_multiplier.percent(), percent);
            (request_id, options)
        };
        let check_speed = |app: &App, from: &MovementRequest, to: &MovementRequest, speed: f32| {
            let controls = app.world().resource::<Controls>();
            assert!(controls.visual.is_some());
            assert!((from.position.x - to.position.x - speed * controls.sent_span).abs() < 0.001);
        };
        let first = move_sample();
        accept(&mut app, &first);
        let (request_id, options) = change_speed(&mut app, 140);
        let pending = move_sample();
        check_speed(&app, &first, &pending, 6.0);

        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsApplied {
                session_id: 11,
                request_id,
                options,
            }],
        );
        app.update();
        assert!(receiver.try_recv().is_err());
        assert_eq!(app.world().resource::<Controls>().held, Some(Held::Waiting));
        accept(&mut app, &pending);
        app.update();
        let boosted = move_sample();
        check_speed(&app, &pending, &boosted, 8.4);
        accept(&mut app, &boosted);

        change_speed(&mut app, 100);
        let lowered = move_sample();
        check_speed(&app, &boosted, &lowered, 6.0);
    }

    #[test]
    fn the_run_multiplier_only_changes_granted_forward_speed() {
        let mut controls = Controls {
            speed: Some(6.0),
            backward_speed: Some(2.0),
            walk_speed: Some(1.5),
            strafe_speed: Some(3.0),
            ..Controls::default()
        };
        let map = KeyMap::for_client(InstalledClient::Titanium);
        let keys = ButtonInput::<KeyCode>::default();
        let intent =
            |controls: &Controls, act| controls.intent(&keys, &map, &[act], true, (0.0, 0.0), None);
        for (percent, speed) in [(100, 6.0), (110, 6.6), (120, 7.2), (140, 8.4)] {
            controls.movement_options.run_speed_multiplier =
                RunSpeedMultiplier::from_percent(percent).unwrap();
            controls.walking = false;
            assert!((intent(&controls, Act::Forward).speed - speed).abs() < 0.001);
            assert!((intent(&controls, Act::Back).speed - 2.0).abs() < 0.001);
            assert!((intent(&controls, Act::StrafeLeft).speed - 3.0).abs() < 0.001);
            controls.walking = true;
            assert!((intent(&controls, Act::Forward).speed - 1.5).abs() < 0.001);
        }
    }

    #[test]
    fn a_strafe_takes_the_walking_rate_with_walking_selected() {
        // Titanium strafes on the selected base speed (the R5 audit's S1):
        // the grant's strafe rate running, its walking rate walking.
        let mut controls = Controls {
            speed: Some(6.0),
            walk_speed: Some(4.0),
            strafe_speed: Some(6.0),
            strafe_walk_speed: Some(4.0),
            ..Controls::default()
        };
        let map = KeyMap::for_client(InstalledClient::Titanium);
        let keys = ButtonInput::<KeyCode>::default();
        let strafe = |controls: &Controls| {
            controls
                .intent(&keys, &map, &[Act::StrafeLeft], true, (0.0, 0.0), None)
                .speed
        };
        assert!((strafe(&controls) - 6.0).abs() < 0.001);
        controls.walking = true;
        assert!((strafe(&controls) - 4.0).abs() < 0.001);
        // Without a walking rate, one rate for both.
        controls.strafe_walk_speed = None;
        assert!((strafe(&controls) - 6.0).abs() < 0.001);
    }

    #[test]
    fn winded_opt_in_moves_during_stun_without_releasing_the_stun_or_jumping() {
        let (mut app, receiver) = app();
        movement_choice(&mut app, Fix::WalkWhileWinded, true);
        app.world_mut().resource_mut::<Controls>().airborne = Some(AirborneController::default());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [eq_client_core::WorldEvent::OwnStun {
                session_id: 11,
                revision: 1,
                state: eq_client_core::control::StunState::Stunned,
            }],
        );
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().expect("opted-in walking") else {
            panic!("unexpected command");
        };
        assert!(request.position.x < 0.0);
        assert!(request.position.x.hypot(request.position.y) <= 0.601);
        assert!(
            app.world()
                .resource::<online::OnlineState>()
                .world()
                .is_stunned()
        );
        assert!(
            receiver.try_recv().is_err(),
            "no jump or posture send while stunned"
        );
        accept(&mut app, &request);
        movement_choice(&mut app, Fix::WalkWhileWinded, false);
        app.update();
        assert!(
            receiver.try_recv().is_err(),
            "turning the option off restores the hold"
        );
    }

    #[test]
    fn flight_normalizes_diagonals_and_never_boosts_vertical_speed() {
        let mut controls = Controls {
            speed: Some(6.0),
            movement_options: eq_client_core::movement::MovementOptions {
                fly: true,
                ..Default::default()
            },
            ..Controls::default()
        };
        let map = KeyMap::for_client(InstalledClient::Titanium);
        let keys = ButtonInput::<KeyCode>::default();
        let intent = controls.intent(
            &keys,
            &map,
            &[Act::Forward, Act::Jump],
            true,
            (0.0, 128.0),
            None,
        );
        assert!((intent.direction.length() - 1.0).abs() < 0.001);
        assert!(intent.direction.y > 0.7);
        let opposite = controls.intent(
            &keys,
            &map,
            &[Act::Jump, Act::Duck],
            true,
            (0.0, 128.0),
            None,
        );
        assert!(opposite.direction.length() < f32::EPSILON);
        controls.movement_options.run_speed_multiplier = RunSpeedMultiplier::MAX;
        let up = controls.intent(&keys, &map, &[Act::Jump], true, (0.0, 128.0), None);
        assert!((up.direction.y * up.speed - 6.0).abs() < 0.001);
        let boosted = controls.intent(
            &keys,
            &map,
            &[Act::Forward, Act::Jump],
            true,
            (0.0, 128.0),
            None,
        );
        assert!(
            (boosted.direction.y * boosted.speed - intent.direction.y * intent.speed).abs() < 0.001
        );
    }

    #[test]
    fn flight_ascends_hovers_and_descends_without_jump_notices_then_resumes_gravity() {
        let (mut app, receiver) = app();
        movement_choice(&mut app, Fix::Fly, true);
        crate::keys::testing::bind(&mut app, Act::Duck, crate::keys::Chord::key(KeyCode::KeyC));
        {
            let mut online = app.world_mut().resource_mut::<online::OnlineState>();
            let mut player = online.world().player().unwrap().clone();
            player.position = world_position([0.0, 13.0, 0.0], 128.0);
            crate::online::testing::admit(&mut online, 11, player);
        }
        app.world_mut().resource_mut::<Controls>().airborne = Some(AirborneController::default());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert!(receiver.try_recv().is_err(), "an idle flyer hovers");
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        app.update();
        let ClientCommand::Move(up) = receiver.try_recv().expect("ascending sample") else {
            panic!("unexpected command");
        };
        assert!((up.position.z - 13.6).abs() < 0.001);
        assert!((up.position.heading - 128.0).abs() < 0.001);
        assert!(receiver.try_recv().is_err(), "flight sends no jump notice");
        accept(&mut app, &up);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        let ClientCommand::Move(stop) = receiver.try_recv().expect("hover stop") else {
            panic!("unexpected command");
        };
        assert!((stop.position.z - up.position.z).abs() < 0.001);
        accept(&mut app, &stop);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyC);
        app.update();
        let ClientCommand::Move(down) = receiver.try_recv().expect("descending sample") else {
            panic!("unexpected command");
        };
        assert!(down.position.z < stop.position.z && down.position.z >= stop.position.z - 0.601);
        assert!(receiver.try_recv().is_err());
        accept(&mut app, &down);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        movement_choice(&mut app, Fix::Fly, false);
        app.update();
        let ClientCommand::Move(fall) = receiver.try_recv().expect("gravity resumes") else {
            panic!("unexpected command");
        };
        assert_eq!(fall.mode, MovementMode::Fall);
        assert!(fall.position.z < down.position.z);
    }

    #[test]
    fn flight_requires_a_separate_stun_opt_in_and_never_supplies_unknown_speed() {
        let (mut app, receiver) = app();
        movement_choice(&mut app, Fix::Fly, true);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [eq_client_core::WorldEvent::OwnStun {
                session_id: 11,
                revision: 1,
                state: eq_client_core::control::StunState::Stunned,
            }],
        );
        app.update();
        assert!(receiver.try_recv().is_err());
        movement_choice(&mut app, Fix::WalkWhileWinded, true);
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().expect("separate stun opt-in")
        else {
            panic!("unexpected command");
        };
        assert!(
            (request.position.z - 3.0).abs() < 0.001,
            "winded never lifts the flight height hold"
        );
        assert!(request.position.x < 0.0);
        accept(&mut app, &request);
        app.world_mut().resource_mut::<Controls>().reset(None);
        app.update();
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn toggling_flight_at_floor_contact_keeps_height_and_restores_ground_motion() {
        let (mut app, receiver) = app();
        movement_choice(&mut app, Fix::Fly, true);
        app.update();
        let ClientCommand::Move(flying) = receiver.try_recv().unwrap() else {
            panic!("flight sample")
        };
        assert!(flying.position.x < 0.0);
        assert!((flying.position.z - 3.0).abs() < 0.001);
        accept(&mut app, &flying);
        movement_choice(&mut app, Fix::Fly, false);
        app.update();
        let ClientCommand::Move(grounded) = receiver.try_recv().unwrap() else {
            panic!("ground sample")
        };
        assert!(grounded.position.x < flying.position.x);
        assert!((grounded.position.z - 3.0).abs() < 0.001);
        assert!(
            receiver.try_recv().is_err(),
            "a contact transition invents no jump or damage"
        );
    }

    #[test]
    fn every_movement_opt_in_keeps_unknown_disconnected_and_dead_gates() {
        for state in 0..3 {
            let (mut app, receiver) = app();
            app.world_mut()
                .resource_mut::<crate::options::OptionsState>()
                .options
                .set_level(Level::RunSpeed, 140);
            for fix in [Fix::WalkWhileWinded, Fix::Fly, Fix::JesusMode] {
                movement_choice(&mut app, fix, true);
            }
            match state {
                0 => app.world_mut().resource_mut::<Controls>().reset(None),
                1 => crate::online::testing::connect(
                    &mut app.world_mut().resource_mut::<online::OnlineState>(),
                    false,
                ),
                _ => crate::online::testing::news(
                    &mut app.world_mut().resource_mut::<online::OnlineState>(),
                    [eq_client_core::WorldEvent::Death(eq_client_core::Death {
                        spawn_id: 7,
                        killer_id: 0,
                        corpse_id: 0,
                        bind_zone_id: 0,
                        corpse_name: None,
                    })],
                ),
            }
            app.update();
            assert!(receiver.try_recv().is_err(), "state {state}");
            assert!(app.world().resource::<Controls>().speed.is_none());
        }
    }

    #[test]
    fn stun_pauses_autorun_and_discards_queued_input_without_banking_distance() {
        let (mut app, receiver) = app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.autorun = true;
            controls.jump = Some(Instant::now());
            controls.crouch = Some(Crouch::Until(Instant::now()));
            controls.facing = Some(256.0);
            controls.taps.0.push((KeyCode::KeyW, Instant::now()));
        }
        let update = |app: &mut App, revision, state| {
            let mut online = app.world_mut().resource_mut::<online::OnlineState>();
            crate::online::testing::news(
                &mut online,
                [eq_client_core::WorldEvent::OwnStun {
                    session_id: 11,
                    revision,
                    state,
                }],
            );
        };
        update(&mut app, 1, eq_client_core::control::StunState::Stunned);
        app.update();
        assert!(receiver.try_recv().is_err());
        let controls = app.world().resource::<Controls>();
        assert!(controls.speed.is_some());
        assert!(controls.autorun && controls.jump.is_none() && controls.crouch.is_none());
        assert!(controls.taps.0.is_empty() && controls.facing.is_none());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        update(&mut app, 2, eq_client_core::control::StunState::Clear);
        app.update();
        let ClientCommand::Move(resumed) = receiver.try_recv().expect("autorun should resume")
        else {
            panic!("unexpected command");
        };
        assert!(resumed.position.x.hypot(resumed.position.y) <= 0.601);
        assert!(app.world().resource::<Controls>().autorun);
        accept(&mut app, &resumed);
        // A whole stun can pass between render frames. Its revision still
        // withdraws intent accumulated before the session's interruption.
        {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.jump = Some(Instant::now());
            controls.last_accepted = Instant::now()
                .checked_sub(Duration::from_millis(150))
                .unwrap();
        }
        update(&mut app, 3, eq_client_core::control::StunState::Stunned);
        update(&mut app, 4, eq_client_core::control::StunState::Clear);
        app.update();
        let controls = app.world().resource::<Controls>();
        assert!(controls.autorun && controls.jump.is_none());
        assert!(
            !receiver
                .try_iter()
                .any(|command| matches!(command, ClientCommand::Jump { .. }))
        );
    }

    #[test]
    fn a_physically_held_key_resumes_after_stun_without_release_and_repress() {
        let (mut app, receiver) = app();
        let update = |app: &mut App, revision, state| {
            let mut online = app.world_mut().resource_mut::<online::OnlineState>();
            crate::online::testing::news(
                &mut online,
                [eq_client_core::WorldEvent::OwnStun {
                    session_id: 11,
                    revision,
                    state,
                }],
            );
        };
        update(&mut app, 1, eq_client_core::control::StunState::Stunned);
        app.update();
        assert!(receiver.try_recv().is_err());
        update(&mut app, 2, eq_client_core::control::StunState::Clear);
        app.update();
        let ClientCommand::Move(resumed) = receiver.try_recv().expect("held key should resume")
        else {
            panic!("unexpected command");
        };
        assert!(resumed.position.x < 0.0);
        assert!(resumed.position.x.hypot(resumed.position.y) <= 0.601);
    }

    #[test]
    fn stun_keeps_passive_falling_and_blocks_held_horizontal_input() {
        let (mut app, receiver) = app();
        {
            let mut online = app.world_mut().resource_mut::<online::OnlineState>();
            let mut player = online.world().player().unwrap().clone();
            player.position = world_position([0.0, 53.0, 0.0], 0.0);
            crate::online::testing::admit(&mut online, 11, player);
            crate::online::testing::news(
                &mut online,
                [eq_client_core::WorldEvent::OwnStun {
                    session_id: 11,
                    revision: 1,
                    state: eq_client_core::control::StunState::Stunned,
                }],
            );
        }
        {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.airborne = Some(AirborneController::default());
            controls.autorun = true;
            controls.last_accepted = Instant::now()
                .checked_sub(Duration::from_millis(150))
                .unwrap();
        }
        app.update();
        let commands: Vec<_> = receiver.try_iter().collect();
        let request = commands
            .iter()
            .find_map(|command| match command {
                ClientCommand::Move(request) => Some(request),
                _ => None,
            })
            .expect("passive fall missing");
        assert_eq!(request.mode, MovementMode::Fall);
        assert!(request.position.x.abs() < f32::EPSILON && request.position.y.abs() < f32::EPSILON);
        assert!(request.position.z < 53.0);
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, ClientCommand::Jump { .. }))
        );
    }

    /// A size 6 human, standing 3.75 above its feet.
    const HUMAN: super::PlayerBody = super::PlayerBody {
        feet_offset: 3.75,
        height: 6.0,
        size: 6.0,
    };
    #[test]
    fn visual_motion_interpolates_without_extrapolation_and_preserves_scale() {
        let mut visual = super::VisualMotion::start(
            bevy::prelude::Transform::IDENTITY,
            super::Segment {
                to: bevy::prelude::Transform::from_xyz(10.0, 0.0, 0.0),
                span: 0.1,
                snap: false,
                follow_heading: false,
            },
        );
        let mut transform = bevy::prelude::Transform::from_scale(bevy::prelude::Vec3::splat(2.0));
        visual.advance(0.05, &mut transform);
        assert!((transform.translation.x - 5.0).abs() < 0.001);
        visual.advance(1.0, &mut transform);
        assert_eq!(transform.translation, visual.to.translation);
        assert_eq!(transform.scale, bevy::prelude::Vec3::splat(2.0));
    }

    #[test]
    fn each_sample_plays_over_the_span_it_covers_in_turn() {
        use bevy::prelude::{Transform, Vec3};
        let mut controls = super::Controls {
            sent_span: super::MAX_CYCLE,
            ..super::Controls::default()
        };
        controls.display_sample(
            Transform::IDENTITY,
            eq_client_core::world_position([10.0, 0.0, 0.0], 0.0),
        );
        // The next is accepted while the first still plays: it waits.
        controls.sent_span = 0.1;
        controls.display_sample(
            Transform::from_xyz(4.0, 0.0, 0.0),
            eq_client_core::world_position([14.0, 0.0, 0.0], 0.0),
        );
        let visual = controls.visual.as_mut().unwrap();
        assert!((visual.duration - super::MAX_CYCLE).abs() < f32::EPSILON);
        let mut shown = Transform::IDENTITY;
        visual.advance(0.2, &mut shown);
        assert!((shown.translation - Vec3::new(8.0, 0.0, 0.0)).length() < 0.001);
        // Past the first's end, the time left over plays the second.
        visual.advance(0.1, &mut shown);
        assert!((shown.translation - Vec3::new(12.0, 0.0, 0.0)).length() < 0.001);
        // Never past the newest accepted position.
        visual.advance(1.0, &mut shown);
        assert!((shown.translation - Vec3::new(14.0, 0.0, 0.0)).length() < 0.001);
        assert!(visual.finished());
    }

    /// Runs a held run for `seconds` at 60 frames a second: a sample goes
    /// out at the first frame 100 ms after the last answer, covering the
    /// time since the last went out, and its answer comes back after the
    /// round trip `trips` gives in frames. Each frame's shown speed comes
    /// back, and `show` is told of each answer: the sample's end, the time
    /// it covers and the time between the last two answers.
    fn held_run(
        seconds: f32,
        trips: &[u32],
        mut show: impl FnMut(f32, f32, f32),
        mut play: impl FnMut(f32) -> f32,
    ) -> Vec<f32> {
        const FRAME: f32 = 1.0 / 60.0;
        const SPEED: f32 = 30.0;
        let mut speeds = Vec::new();
        let (mut sent_at, mut answered_at, mut position) = (0.0_f32, -1.0_f32, 0.0_f32);
        let mut out: Option<(u32, f32, f32)> = None;
        let mut sent = 0_usize;
        let mut shown = 0.0_f32;
        let frames = std::iter::successors(Some(0_u16), |frame| frame.checked_add(1))
            .map(|frame| (u32::from(frame), f32::from(frame) * FRAME))
            .take_while(|(_, now)| *now < seconds);
        for (frame, now) in frames {
            if let Some((due, end, span)) = out
                && frame >= due
            {
                out = None;
                show(end, span, now - answered_at);
                answered_at = now;
            }
            if out.is_none() && now - answered_at >= 0.1 - 1e-4 {
                let span = (now - sent_at).min(super::MAX_CYCLE);
                position += SPEED * span;
                sent_at = now;
                out = Some((frame + trips[sent % trips.len()], position, span));
                sent += 1;
            }
            let next = play(FRAME);
            speeds.push((next - shown) / FRAME);
            shown = next;
        }
        speeds
    }

    #[test]
    fn the_shown_pace_holds_steady_through_jittered_round_trips() {
        use bevy::prelude::Transform;
        use std::cell::RefCell;
        // Round trips of none to two frames, as the session's answers come.
        let trips = [1, 0, 2, 1, 1, 0, 2, 2, 0, 1];
        // The rule before 6.340, kept here as the hypothesis's check: each
        // answer eased from where the model stood over the time between the
        // last two answers. The pace swings with the round trips.
        let eased = RefCell::new((0.0_f32, 0.0_f32, 0.0_f32, 0.1_f32));
        let before = held_run(
            4.0,
            &trips,
            |end, _, between| {
                let (from, to, elapsed, duration) = *eased.borrow();
                let shown = from + (to - from) * (elapsed / duration).min(1.0);
                *eased.borrow_mut() = (shown, end, 0.0, between.clamp(0.05, super::MAX_CYCLE));
            },
            |seconds| {
                let mut state = eased.borrow_mut();
                state.2 += seconds;
                state.0 + (state.1 - state.0) * (state.2 / state.3).min(1.0)
            },
        );
        let swing = |speeds: &[f32]| {
            speeds[60..]
                .iter()
                .map(|speed| (speed / 30.0 - 1.0).abs())
                .fold(0.0_f32, f32::max)
        };
        assert!(swing(&before) > 0.1, "{}", swing(&before));
        // Played back over the time each covers, the pace stays the run's.
        let controls = RefCell::new(super::Controls::default());
        let shown = RefCell::new(Transform::IDENTITY);
        let after = held_run(
            4.0,
            &trips,
            |end, span, _| {
                let mut controls = controls.borrow_mut();
                controls.sent_span = span;
                let from = *shown.borrow();
                controls.display_sample(from, eq_client_core::world_position([end, 0.0, 0.0], 0.0));
            },
            |seconds| {
                let mut controls = controls.borrow_mut();
                let mut shown = shown.borrow_mut();
                if let Some(visual) = controls.visual.as_mut() {
                    visual.advance(seconds, &mut shown);
                    if visual.finished() {
                        controls.visual = None;
                    }
                }
                shown.translation.x
            },
        );
        assert!(swing(&after) < 0.02, "{}", swing(&after));
    }

    #[test]
    fn one_sample_never_climbs_more_than_the_movement_guard_accepts() {
        use eq_client_core::movement::{AirborneController, CollisionWorld, MAX_GROUNDED_STEP};
        // Steep stairs: each slice of a fast sample can climb a whole step.
        let mut triangles = vec![
            [[-20.0, 0.0, -10.0], [0.0, 0.0, -10.0], [0.0, 0.0, 10.0]],
            [[-20.0, 0.0, -10.0], [0.0, 0.0, 10.0], [-20.0, 0.0, 10.0]],
        ];
        for step in 0..10_u8 {
            let (x0, x1) = (f32::from(step) * 0.6, f32::from(step + 1) * 0.6);
            let (low, top) = (f32::from(step) * 1.5, f32::from(step + 1) * 1.5);
            triangles.extend([
                [[x0, top, -10.0], [x1, top, -10.0], [x1, top, 10.0]],
                [[x0, top, -10.0], [x1, top, 10.0], [x0, top, 10.0]],
                [[x0, low, -10.0], [x0, top, -10.0], [x0, top, 10.0]],
                [[x0, low, -10.0], [x0, top, 10.0], [x0, low, 10.0]],
            ]);
        }
        let world = CollisionWorld::new(triangles).unwrap();
        let mut airborne = AirborneController::default();
        let start = Vec3::new(-0.5, 0.0, 0.0);
        let end = super::fall_step(
            &mut airborne,
            &world,
            PROVISIONAL_PHYSICS,
            None,
            start,
            Vec3::X * 3.0,
            0.25,
            6.0,
            false,
        );
        assert!(end.x > start.x, "{end:?}");
        assert!(end.y - start.y <= MAX_GROUNDED_STEP, "{end:?}");
        let next = super::fall_step(
            &mut airborne,
            &world,
            PROVISIONAL_PHYSICS,
            None,
            end,
            Vec3::X * 3.0,
            0.25,
            6.0,
            false,
        );
        assert!(
            next.y > end.y && next.y - end.y <= MAX_GROUNDED_STEP,
            "{next:?}"
        );
    }

    #[test]
    fn a_jump_starts_in_the_first_slice_and_keeps_its_arc_across_samples() {
        use eq_client_core::movement::{AirborneController, CollisionWorld};
        let world = CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ])
        .unwrap();
        let mut airborne = AirborneController::default();
        let still = super::fall_step(
            &mut airborne,
            &world,
            PROVISIONAL_PHYSICS,
            None,
            Vec3::ZERO,
            Vec3::ZERO,
            0.15,
            6.0,
            false,
        );
        assert!(still.y.abs() < 0.001 && airborne.velocity() == 0.0);
        let rising = super::fall_step(
            &mut airborne,
            &world,
            PROVISIONAL_PHYSICS,
            None,
            still,
            Vec3::X,
            0.15,
            6.0,
            true,
        );
        assert!(rising.y > 0.5 && rising.x > 0.9, "{rising:?}");
        assert!(airborne.velocity() > 0.0);
        // Released keys: the arc continues and lands back on the ground.
        let mut feet = rising;
        for _ in 0..10 {
            feet = super::fall_step(
                &mut airborne,
                &world,
                PROVISIONAL_PHYSICS,
                None,
                feet,
                Vec3::ZERO,
                0.15,
                6.0,
                false,
            );
        }
        assert!(
            feet.y.abs() < 0.001 && airborne.velocity() == 0.0,
            "{feet:?}"
        );
    }

    #[test]
    fn a_goto_just_over_a_steep_face_stands_up_and_climbs_it() {
        // Live's look (11-01, 85b9d33d): a goto 1 u over a 50° face, then
        // a run and a walk up it, moved 0.0 on every sample. Each sample
        // now stands up and climbs, and never rises past the reach the
        // session's guard allows it.
        use eq_client_core::movement::{
            AirborneController, CollisionWorld, climb_reach, program_steps,
        };
        let titanium = climbing(
            physics(InstalledClient::Titanium, None),
            InstalledClient::Titanium,
            &HUMAN,
        );
        for degrees in [50.0_f32, 55.0] {
            let slope = degrees.to_radians().tan();
            let world = CollisionWorld::new([
                [
                    [40.0, -40.0 * slope, -40.0],
                    [-40.0, 40.0 * slope, -40.0],
                    [-40.0, 40.0 * slope, 40.0],
                ],
                [
                    [40.0, -40.0 * slope, -40.0],
                    [-40.0, 40.0 * slope, 40.0],
                    [40.0, -40.0 * slope, 40.0],
                ],
            ])
            .unwrap();
            for speed in [32.0_f32, 12.8] {
                let mut feet = Vec3::new(0.0, 1.0 - HUMAN.feet_offset, 0.0);
                let mut airborne = AirborneController::default();
                let mut moved = 0.0;
                for _ in 0..10 {
                    let next = fall_step(
                        &mut airborne,
                        &world,
                        titanium,
                        Some(titanium.climb),
                        feet,
                        -Vec3::X * speed * 0.1,
                        0.1,
                        HUMAN.height,
                        false,
                    );
                    let case = format!("{degrees}° at {speed}: {feet} to {next}");
                    assert!(
                        next.y - feet.y <= climb_reach(titanium.climb, program_steps(0.1)) + 0.001,
                        "{case}"
                    );
                    assert!(next.y >= -next.x * slope - 0.01, "{case}");
                    moved += feet.x - next.x;
                    feet = next;
                }
                // A walk keeps its pace up both; a run up them goes as far as the reach lets it.
                assert!(moved > 0.5 * speed, "{degrees}° at {speed}: moved {moved}");
                if speed < 20.0 {
                    assert!(
                        moved > 0.95 * speed - 0.7,
                        "{degrees}° at {speed}: moved {moved}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_titanium_jump_rises_at_its_own_speed_within_the_guards_allowance() {
        use eq_client_core::movement::{AirborneController, CollisionWorld, MAX_GROUNDED_STEP};
        let world = CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ])
        .unwrap();
        let titanium = climbing(
            physics(InstalledClient::Titanium, None),
            InstalledClient::Titanium,
            &HUMAN,
        );
        let mut airborne = AirborneController::default();
        let rising = super::fall_step(
            &mut airborne,
            &world,
            titanium,
            None,
            Vec3::ZERO,
            Vec3::ZERO,
            0.15,
            6.0,
            true,
        );
        // At 44.55 u/s under 160 u/s², three 50 ms steps climb well past a
        // riser (1.81 + 1.40 + 0.99), and no farther than the guard grants
        // after the notice.
        let granted = MAX_GROUNDED_STEP + super::jump_rise(44.55, titanium.gravity, 0.15);
        assert!(rising.y > MAX_GROUNDED_STEP, "{rising:?}");
        assert!(rising.y <= granted, "{rising:?} over {granted}");
        // The whole climb went: the sample was not cut short.
        let mut free = AirborneController::default();
        let mut feet = Vec3::ZERO;
        for index in 0..3 {
            feet = free.step(
                &world,
                feet,
                titanium,
                eq_client_core::movement::MotionStep {
                    horizontal: Vec3::ZERO,
                    jump: index == 0,
                    seconds: 0.05,
                    height: 6.0,
                },
            );
        }
        assert!((rising.y - feet.y).abs() < 0.001, "{rising:?} {feet:?}");
        // The rise stops at the top of the arc.
        assert!((super::jump_rise(44.55, 400.0, 1.0) - 44.55 * 44.55 / 800.0).abs() < 0.001);
    }

    #[test]
    fn motion_reset_discards_pending_visual_transition() {
        let mut controls = super::Controls::default();
        controls.display_sample(
            bevy::prelude::Transform::IDENTITY,
            eq_client_core::world_position([10.0, 0.0, 0.0], 0.0),
        );
        assert!(controls.visual.is_some());
        controls.reset(None);
        assert!(controls.visual.is_none());
    }

    #[test]
    fn a_new_facing_shows_at_once_while_a_key_turn_eases() {
        use bevy::prelude::{Quat, Transform};
        let facing = |heading| Quat::from_rotation_y(eq_client_core::render_heading(heading));
        let start = Transform::from_rotation(facing(0.0));
        // In a tenth of a second the turn keys turn 24 heading units. A
        // quarter turn shows at once, and the position still eases.
        let mut controls = super::Controls::default();
        controls.display_sample(
            start,
            eq_client_core::world_position([10.0, 0.0, 0.0], 128.0),
        );
        let visual = controls.visual.as_mut().unwrap();
        let mut shown = start;
        visual.advance(0.0, &mut shown);
        assert!(shown.rotation.angle_between(facing(128.0)) < 0.001);
        assert!(shown.translation.length() < 0.001);
        visual.advance(0.05, &mut shown);
        assert!(shown.rotation.angle_between(facing(128.0)) < 0.001);
        assert!((shown.translation - Vec3::new(5.0, 0.0, 0.0)).length() < 0.001);
        // A turn the keys make in a cycle eases with the position.
        let mut controls = super::Controls::default();
        controls.display_sample(
            start,
            eq_client_core::world_position([10.0, 0.0, 0.0], 500.0),
        );
        let visual = controls.visual.as_mut().unwrap();
        let mut shown = start;
        visual.advance(0.05, &mut shown);
        assert!(shown.rotation.angle_between(facing(506.0)) < 0.001);
        assert!((shown.translation - Vec3::new(5.0, 0.0, 0.0)).length() < 0.001);
        // A turn key's next sample, before the last one's easing ends, is
        // measured from where that sample asked to face (20 units), not from
        // the facing shown (26 units), so it still eases.
        controls.display_sample(
            shown,
            eq_client_core::world_position([10.0, 0.0, 0.0], 480.0),
        );
        let visual = controls.visual.as_mut().unwrap();
        let mut next = shown;
        visual.advance(0.0, &mut next);
        assert!(next.rotation.angle_between(facing(506.0)) < 0.001);
    }
    #[test]
    fn throttled_taps_survive_until_one_sample_but_not_focus_loss_or_expiry() {
        let start = Instant::now();
        let mut keys = ButtonInput::default();
        let mut taps = TapBuffer::default();
        keys.press(KeyCode::ArrowDown);
        keys.release(KeyCode::ArrowDown);
        taps.observe(&keys, &KeyMap::default().movement_keys(), true, start);
        keys.clear();
        taps.observe(
            &keys,
            &KeyMap::default().movement_keys(),
            true,
            start + Duration::from_millis(100),
        );
        assert!(taps.take(&keys).pressed(KeyCode::ArrowDown));
        assert!(!taps.take(&keys).pressed(KeyCode::ArrowDown));

        keys.press(KeyCode::ArrowDown);
        keys.release(KeyCode::ArrowDown);
        taps.observe(&keys, &KeyMap::default().movement_keys(), true, start);
        keys.clear();
        taps.observe(
            &keys,
            &KeyMap::default().movement_keys(),
            false,
            start + Duration::from_millis(1),
        );
        assert!(!taps.take(&keys).pressed(KeyCode::ArrowDown));

        keys.press(KeyCode::ArrowDown);
        keys.release(KeyCode::ArrowDown);
        taps.observe(&keys, &KeyMap::default().movement_keys(), true, start);
        keys.clear();
        taps.observe(
            &keys,
            &KeyMap::default().movement_keys(),
            true,
            start + Duration::from_millis(250),
        );
        assert!(!taps.take(&keys).pressed(KeyCode::ArrowDown));
    }

    use super::*;

    #[test]
    #[allow(clippy::float_cmp, reason = "The constants are copied, not computed")]
    fn each_client_falls_by_its_own_physics_and_spaces_its_jumps_its_own_way() {
        // Titanium reads the same header field with the same step and air.
        let titanium = climbing(
            physics(InstalledClient::Titanium, Some(1.0)),
            InstalledClient::Titanium,
            &HUMAN,
        );
        assert!(
            (titanium.gravity - 400.0).abs() < 0.01,
            "{}",
            titanium.gravity
        );
        // The program's own 44.55: the movement guard takes a jump's rise.
        assert!((titanium.jump_speed - 44.55).abs() < 0.001);
        // Titanium sweeps from the origin with radius 0.1 times the scaled
        // model reference. EQMac starts 0.5 above it with radius size / 4.
        let (ti, eqmac) = (
            climbing(
                physics(InstalledClient::Titanium, Some(0.4)),
                InstalledClient::Titanium,
                &HUMAN,
            ),
            climbing(
                physics(InstalledClient::EqMac, Some(0.4)),
                InstalledClient::EqMac,
                &HUMAN,
            ),
        );
        assert_eq!(
            VerticalPhysics { climb: 0.0, ..ti },
            VerticalPhysics {
                climb: 0.0,
                ..eqmac
            }
        );
        assert!((ti.climb - 3.375).abs() < 0.001, "{}", ti.climb);
        assert!((eqmac.climb - 2.75).abs() < 0.001, "{}", eqmac.climb);
        assert!(!ti.steers_in_air && !eqmac.steers_in_air);
        let mac = climbing(
            physics(InstalledClient::EqMac, None),
            InstalledClient::EqMac,
            &HUMAN,
        );
        assert!((mac.gravity - 160.0).abs() < 0.001, "{}", mac.gravity);
        // The session's motion guard lets the program's fastest fall through.
        assert!(
            (mac.terminal_speed - eq_client_core::movement::EQMAC_FALL_SPEED).abs() < 0.01,
            "{}",
            mac.terminal_speed
        );
        // The zone header's gravity, a step at a time; none pulls nothing.
        let heavy = climbing(
            physics(InstalledClient::EqMac, Some(0.8)),
            InstalledClient::EqMac,
            &HUMAN,
        );
        assert!((heavy.gravity - 320.0).abs() < 0.01, "{}", heavy.gravity);
        assert!((heavy.terminal_speed - mac.terminal_speed).abs() < 0.01);
        let light = climbing(
            physics(InstalledClient::EqMac, Some(0.2)),
            InstalledClient::EqMac,
            &HUMAN,
        );
        assert!(
            (light.terminal_speed - 396.0).abs() < 0.01,
            "{}",
            light.terminal_speed
        );
        assert!(
            climbing(
                physics(InstalledClient::EqMac, Some(0.0)),
                InstalledClient::EqMac,
                &HUMAN
            )
            .gravity
            .abs()
                < f32::EPSILON
        );
        let now = Instant::now();
        let ago = |ms| now.checked_sub(Duration::from_millis(ms));
        assert!(jump_too_soon(InstalledClient::Titanium, ago(1500), now));
        assert!(!jump_too_soon(InstalledClient::Titanium, ago(1501), now));
        assert!(!jump_too_soon(InstalledClient::Titanium, None, now));
        // The Mac-era client waits 1.5 s.
        assert!(jump_too_soon(InstalledClient::EqMac, ago(1500), now));
        assert!(!jump_too_soon(InstalledClient::EqMac, ago(1501), now));
    }

    #[test]
    fn the_turn_keys_ramp_up_to_their_cap() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        // The first step turns at 20 a second, the second at 40.
        assert!(close(ramped_turn(0.0, 1.0 / 60.0, 12.0), 20.0 / 60.0));
        assert!(close(
            ramped_turn(1.0 / 60.0, 2.0 / 60.0, 12.0),
            40.0 / 60.0
        ));
        // Running reaches 240 a second after 11 steps, walking 160 after 7.
        assert!(close(ramped_turn(1.0, 2.0, 12.0), 240.0));
        assert!(close(ramped_turn(1.0, 2.0, turn_cap(true, 1)), 160.0));
        assert!(close(ramped_turn(1.0, 2.0, turn_cap(false, 141)), 60.0));
        // The whole ramp to full speed turns 20 x (1 + ... + 11) / 60.
        assert!(close(ramped_turn(0.0, 11.0 / 60.0, 12.0), 22.0));
        // A span that starts before the key was held counts from the press.
        assert!(close(
            ramped_turn(-0.5, 0.1, 12.0),
            ramped_turn(0.0, 0.1, 12.0)
        ));
        // Never faster than the cap over any span.
        for start in [0.0, 0.05, 0.13, 0.4] {
            assert!(ramped_turn(start, start + 0.25, 12.0) <= TURN_RATE * 0.25 + 1e-3);
        }
    }

    #[test]
    fn a_tired_jump_is_refused_only_under_50_and_a_tenth_of_the_maximum() {
        assert!(too_tired_to_jump(Some(50), Some(500)));
        assert!(!too_tired_to_jump(Some(51), Some(1000)));
        assert!(!too_tired_to_jump(Some(31), Some(300)));
        assert!(too_tired_to_jump(Some(30), Some(300)));
        assert!(too_tired_to_jump(Some(0), Some(20)));
        assert!(!too_tired_to_jump(Some(3), Some(20)));
        // Without a maximum (or a reported endurance) the jump goes.
        assert!(!too_tired_to_jump(Some(0), None));
        assert!(!too_tired_to_jump(None, Some(500)));
    }

    #[test]
    fn a_tired_jump_says_so_and_does_not_crouch() {
        let (mut app, receiver) = standing_jump(InstalledClient::Titanium);
        app.insert_resource(crate::hud::HudState {
            resource_estimate: Some((0, 500)),
        });
        app.update();
        assert_eq!(stances_and_jump(&receiver), (Vec::new(), false));
        assert_eq!(app.world().resource::<Controls>().crouch, None);
        let said = app
            .world()
            .resource::<crate::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::System)
            .into_iter()
            .any(|(_, line)| line.message.text.contains("too tired to jump"));
        assert!(said);
    }

    #[test]
    fn a_jump_takes_a_tenth_of_the_maximum_endurance_at_most_50_at_take_off() {
        assert_eq!(jump_endurance(300), 30);
        assert_eq!(jump_endurance(500), 50);
        assert_eq!(jump_endurance(2_000), 50);
        assert_eq!(jump_endurance(9), 0);
        let (mut app, receiver) = standing_jump(InstalledClient::Titanium);
        app.insert_resource(crate::hud::HudState {
            resource_estimate: Some((0, 300)),
        });
        crate::online::testing::resources(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            0,
            200,
        );
        app.update();
        let earlier = Instant::now()
            .checked_sub(Duration::from_millis(1))
            .unwrap();
        app.world_mut().resource_mut::<Controls>().crouch = Some(Crouch::Until(earlier));
        app.update();
        assert_eq!(
            stances_and_jump(&receiver),
            (vec![Posture::Ducking, Posture::Standing], true)
        );
        let shown = |app: &App| {
            app.world()
                .resource::<online::OnlineState>()
                .world()
                .vitals()
                .endurance
        };
        assert_eq!(shown(&app), Some(170));
        // The server's next report replaces it.
        crate::online::testing::resources(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            0,
            175,
        );
        assert_eq!(shown(&app), Some(175));
    }

    /// The app of [`app`] for this installed client, the player standing
    /// still on its floor where the session takes jumps, holding Jump.
    fn standing_jump(client: InstalledClient) -> (App, mpsc::Receiver<ClientCommand>) {
        let (mut app, _) = app();
        let (sender, receiver) = mpsc::sync_channel(16);
        app.insert_resource(crate::outbox::Outbox::new(Some(sender)));
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = client;
        app.world_mut().resource_mut::<Controls>().airborne = Some(AirborneController::default());
        let mut state = app.world_mut().resource_mut::<online::OnlineState>();
        let player = state.world().player().unwrap().clone();
        crate::online::testing::news(
            &mut state,
            [eq_client_core::WorldEvent::Entered {
                capabilities: eq_client_core::Capability::ALL.to_vec(),
                choices: Vec::new(),
                session_id: 11,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        crate::online::testing::connect(&mut state, true);
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::Space);
        (app, receiver)
    }

    /// The stances the commands asked for, and whether a jump went.
    fn stances_and_jump(receiver: &mpsc::Receiver<ClientCommand>) -> (Vec<Posture>, bool) {
        let sent: Vec<ClientCommand> = receiver.try_iter().collect();
        let stances = sent
            .iter()
            .filter_map(|command| match command {
                ClientCommand::SetPosture { posture, .. } => Some(*posture),
                _ => None,
            })
            .collect();
        let jumped = sent
            .iter()
            .any(|command| matches!(command, ClientCommand::Jump { .. }));
        (stances, jumped)
    }

    #[test]
    fn a_mac_era_jump_from_standing_still_crouches_once_then_goes() {
        let (mut app, receiver) = standing_jump(InstalledClient::EqMac);
        app.update();
        assert_eq!(stances_and_jump(&receiver), (vec![Posture::Ducking], false));
        // Jump held through the crouch asks nothing more.
        app.update();
        assert_eq!(stances_and_jump(&receiver), (Vec::new(), false));
        // Once the crouch is over: stand, and the sample carries the jump.
        let earlier = Instant::now()
            .checked_sub(Duration::from_millis(1))
            .unwrap();
        app.world_mut().resource_mut::<Controls>().crouch = Some(Crouch::Until(earlier));
        app.update();
        assert_eq!(stances_and_jump(&receiver), (vec![Posture::Standing], true));
        let controls = app.world().resource::<Controls>();
        assert!(controls.airborne.as_ref().unwrap().velocity() > 0.0);
        assert_eq!(controls.crouch, None);
    }

    #[test]
    fn a_titanium_jump_from_standing_still_crouches_first_too() {
        // Titanium crouches with no forward input as well (0x4acb42-0x4acb50).
        let (mut app, receiver) = standing_jump(InstalledClient::Titanium);
        app.update();
        assert_eq!(stances_and_jump(&receiver), (vec![Posture::Ducking], false));
        assert!(matches!(
            app.world().resource::<Controls>().crouch,
            Some(Crouch::Until(_))
        ));
    }

    #[test]
    fn a_running_drop_off_a_ledge_at_the_live_cadence_lands_at_the_read_steps_speed() {
        use eq_client_assets::falls::{FallLanding, TITANIUM};
        // Live's local EQEmu drops (11-01): a run off a ledge this high,
        // the impact the client logged, and the damage for a carried weight
        // of 0, of 16.5 (what the four drops fit) and of 18 (the inventory
        // window two hours earlier): T = whole weight / 100 + v - 4.
        for (height, impact, damages) in [
            (30.2, 90.0, [2, 4, 4]),
            (50.2, 110.8, [23, 28, 29]),
            (80.2, 144.2, [103, 113, 114]),
            (100.2, 157.1, [148, 161, 162]),
        ] {
            let ledge = |x0: f32, x1: f32, y: f32| {
                [
                    [[x0, y, -20.0], [x1, y, -20.0], [x1, y, 20.0]],
                    [[x0, y, -20.0], [x1, y, 20.0], [x0, y, 20.0]],
                ]
            };
            let [a, b] = ledge(-40.0, 0.0, height);
            let [c, d] = ledge(-40.0, 400.0, 0.0);
            let world = CollisionWorld::new([a, b, c, d]).unwrap();
            let physics = climbing(
                physics(InstalledClient::Titanium, None),
                InstalledClient::Titanium,
                &HUMAN,
            );
            let mut airborne = AirborneController::default();
            let mut feet = Vec3::new(-20.0, height, 0.0);
            // About 100 frames a second: each sample waits out the 100 ms
            // gate, then the frame that sends it, up to 21 ms.
            let spans = [0.10, 0.11, 0.12, 0.105, 0.121];
            let landing = (0..200)
                .find_map(|sample| {
                    let span = spans[sample % spans.len()];
                    feet = fall_step(
                        &mut airborne,
                        &world,
                        physics,
                        Some(physics.climb),
                        feet,
                        Vec3::X * 43.0 * span,
                        span,
                        HUMAN.height,
                        false,
                    );
                    airborne.take_landing()
                })
                .expect("the run lands");
            assert!(feet.y.abs() < 0.01, "{height}: {feet}");
            assert!(
                (landing.impact_speed - impact).abs() < 0.06,
                "{height}: {landing:?}"
            );
            for (weight, damage) in [0.0, 16.5, 18.0].into_iter().zip(damages) {
                let cost = TITANIUM.landing_cost(FallLanding {
                    impact_speed: landing.impact_speed,
                    carried_weight: weight,
                    safe_fall: 0,
                    safe_fall_ranks: 0,
                    zone: 1,
                });
                assert_eq!(cost.damage, damage, "{height} at {weight}: {landing:?}");
            }
        }
    }

    #[test]
    fn a_landing_costs_what_the_installed_clients_rule_says() {
        let floor = CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ])
        .unwrap();
        let world = ClientWorld::default();
        // 100 units down, landing at 6 units a step.
        let landing = Landing {
            fall_distance: 100.0,
            impact_speed: 120.0,
        };
        let cost = |client| landing_cost(client, &floor, Vec3::ZERO, landing, &world).unwrap();
        // Both programs': T = 6 - 2.5 - 1.5 = 2, so 40.
        assert_eq!(
            cost(InstalledClient::Titanium),
            FallCost {
                damage: 40,
                safe_fall: false
            }
        );
        assert_eq!(
            cost(InstalledClient::EqMac),
            FallCost {
                damage: 40,
                safe_fall: false
            }
        );
    }

    #[test]
    fn recent_motion_tells_asked_moved_and_net_over_the_last_second() {
        let start = Instant::now();
        let mut recent = RecentMotion::default();
        // An old sample falls out of the window.
        recent.record(start, Vec3::X * 9.0, Vec3::X * 9.0);
        let at = |ms| start + Duration::from_millis(ms);
        // Asked 2 a sample; moved 2 one way then back.
        recent.record(at(1200), Vec3::X * 2.0, Vec3::X * 2.0);
        recent.record(at(1400), Vec3::Z * 2.0, Vec3::X * -2.0);
        // Asked, and moved nothing.
        recent.record(at(1600), Vec3::X * 2.0, Vec3::ZERO);
        let summary = recent.last_second(at(1700));
        assert_eq!(summary.samples, 3);
        assert!((summary.asked - 6.0).abs() < 1e-5);
        assert!((summary.moved - 4.0).abs() < 1e-5);
        assert!(summary.net < 1e-5);
    }
    use std::sync::mpsc;

    #[test]
    fn autorun_runs_forward_with_no_key_held_until_back_is() {
        let controls = Controls {
            speed: Some(30.0),
            backward_speed: Some(12.0),
            autorun: true,
            ..default()
        };
        let map = KeyMap::for_client(eq_client_assets::ui::InstalledClient::Titanium);
        let mut keys = ButtonInput::default();
        // On, with nothing held, and with the keyboard elsewhere.
        for focused in [true, false] {
            let intent = controls.intent(&keys, &map, &[], focused, (0.0, 0.0), None);
            assert_eq!(intent.mode, MovementMode::Forward);
            // Along the player's heading.
            let radians = eq_client_core::render_heading(0.0);
            let facing = Vec3::new(radians.sin(), 0.0, radians.cos());
            assert!(
                (intent.direction - facing).length() < 1e-5,
                "{}",
                intent.direction
            );
        }
        // Turning keeps it running.
        keys.press(KeyCode::ArrowLeft);
        let intent = controls.intent(&keys, &map, &[], true, (0.0, 0.0), None);
        assert!(intent.direction != Vec3::ZERO && intent.turn != 0.0);
        // Back wins while held.
        keys.press(KeyCode::ArrowDown);
        let intent = controls.intent(&keys, &map, &[], true, (0.0, 0.0), None);
        assert_eq!(intent.mode, MovementMode::Backward);
    }

    #[test]
    fn between_frame_taps_move_once_without_sticking_or_ignoring_focus() {
        let controls = Controls {
            speed: Some(30.0),
            backward_speed: Some(12.0),
            ..default()
        };
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::ArrowUp);
        keys.release(KeyCode::ArrowUp);
        assert!(!keys.pressed(KeyCode::ArrowUp));
        assert_ne!(
            controls
                .intent(&keys, &KeyMap::default(), &[], true, (0.0, 0.0), None)
                .direction,
            Vec3::ZERO
        );
        assert_eq!(
            controls
                .intent(&keys, &KeyMap::default(), &[], false, (0.0, 0.0), None)
                .direction,
            Vec3::ZERO
        );
        keys.clear();
        assert_eq!(
            controls
                .intent(&keys, &KeyMap::default(), &[], true, (0.0, 0.0), None)
                .direction,
            Vec3::ZERO
        );
        keys.press(KeyCode::ArrowDown);
        keys.release(KeyCode::ArrowDown);
        assert_eq!(
            controls
                .intent(&keys, &KeyMap::default(), &[], true, (0.0, 0.0), None)
                .mode,
            MovementMode::Backward
        );
        assert!(!keys.pressed(KeyCode::ArrowDown));
    }

    #[test]
    fn boundary_crossing_is_edge_triggered_and_absolute_destinations_pass_through() {
        let mut tracker = BoundaryTracker::default();
        assert!(tracker.observe(Some(ZoneLine::Reference(7))).is_none());
        assert!(tracker.observe(Some(ZoneLine::Reference(7))).is_none());
        assert!(tracker.observe(None).is_none());
        assert_eq!(
            tracker.observe(Some(ZoneLine::Reference(7))),
            Some(eq_client_core::ZoneLineDestination::Reference(7))
        );
        assert!(tracker.observe(Some(ZoneLine::Reference(7))).is_none());
        let destination = tracker.observe(Some(ZoneLine::Absolute {
            zone_id: 42,
            position: [12.0, 34.0, 56.0],
            heading: 64.0,
        }));
        assert_eq!(
            destination,
            Some(eq_client_core::ZoneLineDestination::Absolute {
                zone_id: 42,
                position: eq_client_core::WorldPosition {
                    x: 12.0,
                    y: 34.0,
                    z: 56.0,
                    heading: 64.0
                }
            })
        );
    }

    #[test]
    fn landing_on_a_boundary_does_not_trigger_it_again() {
        let mut controls = Controls::default();
        let source = Some(ZoneLine::Reference(7));
        let destination = Some(ZoneLine::Reference(8));
        assert!(controls.boundary.observe(None).is_none());
        assert!(controls.boundary.observe(source).is_some());

        controls.wait_for(Some(5), Instant::now(), PendingKind::Move);
        controls.same_zone_landed();
        assert!(controls.pending.is_none());
        assert!(controls.boundary.observe(destination).is_none());
        assert!(controls.boundary.observe(destination).is_none());
        assert!(controls.boundary.observe(None).is_none());
        assert!(controls.boundary.observe(destination).is_some());
    }

    fn flat_surface(y: f32) -> [[[f32; 3]; 3]; 2] {
        [
            [[-20.0, y, -20.0], [20.0, y, -20.0], [20.0, y, 20.0]],
            [[-20.0, y, -20.0], [20.0, y, 20.0], [-20.0, y, 20.0]],
        ]
    }

    fn app() -> (App, mpsc::Receiver<ClientCommand>) {
        let (mut app, receiver) = fixture();
        app.add_systems(Update, (input, spend_endurance).chain());
        (app, receiver)
    }

    pub(crate) fn age_pending(app: &mut App) -> Instant {
        let created = Instant::now().checked_sub(MOVE_RESULT_WAIT).unwrap();
        app.world_mut()
            .resource_mut::<Controls>()
            .pending
            .as_mut()
            .unwrap()
            .created = created;
        created
    }

    pub(crate) fn fixture() -> (App, mpsc::Receiver<ClientCommand>) {
        let (sender, receiver) = mpsc::sync_channel(2);
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        // These camera-relative motion cases choose their own controls;
        // W/A/S/D and Q/E are not the installed client's default bindings.
        for (act, key) in [
            (Act::FreeCamera, KeyCode::F9),
            (Act::CameraForward, KeyCode::KeyW),
            (Act::CameraBack, KeyCode::KeyS),
            (Act::CameraLeft, KeyCode::KeyA),
            (Act::CameraRight, KeyCode::KeyD),
            (Act::TurnLeft, KeyCode::KeyQ),
            (Act::TurnRight, KeyCode::KeyE),
        ] {
            crate::keys::testing::bind(&mut app, act, crate::keys::Chord::key(key));
        }
        let mut state = online::OnlineState::new(true);
        crate::online::testing::admit(
            &mut state,
            11,
            eq_client_core::PlayerState {
                name: "Example".into(),
                titanium_environment: None,
                base_attributes: None,
                deity: None,
                class: Some(1),
                spawn_id: 7,
                race: 1,
                gender: 0,
                level: 1,
                position: world_position([0.0, 3.0, 0.0], 0.0),
                mana: 0,
                endurance: Some(0),
                skills: None,
                practice_points: None,
                languages: None,
                disciplines: None,
                spell_refresh_ms: None,
                memorized_spells: [None; 8],
                size: 0.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                hp_percent: Some(100),
                appearance: eq_client_core::outfit::Appearance::default(),
                listing: eq_client_core::listing::Listing::default(),
                name_parts: eq_client_core::names::NameParts::default(),
            },
        );
        let floor = CollisionWorld::new(flat_surface(0.0)).unwrap();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        app.insert_resource(state)
            .init_resource::<crate::chat::ChatState>()
            .init_resource::<crate::hud::messages::Messages>()
            .init_resource::<crate::ViewerSettings>()
            .init_resource::<crate::navigation::NavigationKeys>()
            .init_resource::<crate::options::OptionsState>()
            .insert_resource(Collision(Some(floor)))
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .insert_resource(keys)
            .insert_resource(Controls {
                speed: Some(6.0),
                last_accepted: Instant::now().checked_sub(Duration::from_secs(2)).unwrap(),
                // The session has heard this body's Titanium climb, 3 − 0.3
                // a step, so the samples are the only commands these tests
                // see ([`the_session_hears_the_bodys_climb_at_each_admission_and_change`]).
                climb_sent: Some((
                    11,
                    eq_client_assets::falls::TITANIUM.climb_per_step(3.0, 6.0),
                )),
                ..Controls::default()
            });
        app.world_mut().spawn((
            Player,
            Transform::from_xyz(0.0, 3.0, 0.0),
            PlayerBody {
                feet_offset: 3.0,
                height: 6.0,
                size: 6.0,
            },
        ));
        app.world_mut().spawn(OrbitCamera {
            focus: Vec3::ZERO,
            radius: 30.0,
            // Looking east, heading 384, so W walks along EQ's -X.
            yaw: eq_client_core::render_heading(384.0) + std::f32::consts::PI,
            pitch: 1.0,
            pulled: None,
            view: default(),
        });
        app.world_mut().spawn((
            Window {
                focused: true,
                ..Window::default()
            },
            bevy::window::PrimaryWindow,
        ));
        (app, receiver)
    }

    fn stationary_airborne_app() -> (App, mpsc::Receiver<ClientCommand>) {
        let (mut app, receiver) = app();
        app.world_mut().resource_mut::<Controls>().airborne = Some(AirborneController::default());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        (app, receiver)
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "The move keeps the received horizontal coordinates"
    )]
    fn titanium_recovers_a_floor_the_size_based_probe_missed_within_one_step() {
        use eq_client_core::movement::CollisionWorld;
        use eq_network_game::movement::MovementGuard;

        // Synthetic planes with the measured height differences. The lower
        // surface is far below the feet and the upper one leaves standing room.
        const FLOOR: f32 = 20.406_25;
        let body = PlayerBody {
            height: 6.506_872,
            ..HUMAN
        };
        let admitted_position = world_position([0.0, 21.0, 0.0], 0.0);
        let feet = Vec3::Y * (admitted_position.z - body.feet_offset);
        let terrain = CollisionWorld::new(
            flat_surface(FLOOR)
                .into_iter()
                .chain(flat_surface(-98.531_25))
                .chain(flat_surface(48.375)),
        )
        .unwrap();
        let corrected = climbing(
            physics(InstalledClient::Titanium, None),
            InstalledClient::Titanium,
            &body,
        );
        let step = |physics: VerticalPhysics| {
            fall_step(
                &mut AirborneController::default(),
                &terrain,
                physics,
                Some(physics.climb),
                feet,
                Vec3::ZERO,
                0.05,
                body.height,
                false,
            )
        };
        assert!(terrain.support(feet).is_none());
        assert!(
            step(VerticalPhysics {
                climb: 3.15,
                ..corrected
            })
            .y < FLOOR
        );
        let stood = step(corrected);
        assert!((stood.y - FLOOR).abs() < 0.001, "{stood:?}");
        assert!(terrain.support(stood).is_some());
        assert!(terrain.clearance(stood, body.height, 0.03).is_none());

        let (mut app, receiver) = stationary_airborne_app();
        app.world_mut().insert_resource(Collision(Some(terrain)));
        let mut bodies = app.world_mut().query::<&mut PlayerBody>();
        *bodies.single_mut(app.world_mut()).unwrap() = body;
        let mut player = online::testing::player(7);
        player.position = admitted_position;
        online::testing::admit(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            12,
            player,
        );
        app.update();
        let ClientCommand::ClimbReach {
            session_id: 12,
            per_step,
        } = receiver.try_recv().unwrap()
        else {
            panic!("expected reach for the new admission");
        };
        assert!((per_step - 3.375).abs() < 0.001);
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected an ordinary grounded move");
        };
        assert_eq!(request.mode, MovementMode::Forward);
        assert_eq!(
            (
                request.position.x,
                request.position.y,
                request.position.heading
            ),
            (
                admitted_position.x,
                admitted_position.y,
                admitted_position.heading
            )
        );
        assert!((request.position.z - (FLOOR + body.feet_offset)).abs() < 0.001);
        let start = request
            .created
            .checked_sub(Duration::from_millis(50))
            .unwrap();
        let mut guard = MovementGuard::new(12, admitted_position, start);
        guard.set_speed(Some(6.0), start);
        guard.set_climb(3.15);
        assert!(guard.accept(&request, request.created).is_err());
        guard.set_climb(per_step);
        assert!(guard.accept(&request, request.created).is_ok());

        let online = app.world().resource::<online::OnlineState>();
        assert_eq!(online.world().player().unwrap().position, admitted_position);
        assert_eq!(
            online.server_placement().unwrap().position,
            admitted_position
        );
    }

    #[test]
    fn an_unsupported_server_placement_holds_fall_until_a_fresh_correction() {
        let (mut app, receiver) = stationary_airborne_app();
        let mut player = online::testing::player(7);
        player.position = world_position([0.0, -5.25, 0.0], 0.0);
        online::testing::admit(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            12,
            player,
        );
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().held,
            Some(Held::PlacementBlocked)
        );
        assert!(receiver.try_recv().is_err());

        // A new authoritative sample still on the wrong side of the floor
        // holds independently and remains the visible server evidence.
        let below = world_position([0.0, -5.0, 0.0], 0.0);
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            below,
        );
        app.update();
        let online = app.world().resource::<online::OnlineState>();
        assert_eq!(online.server_placement().unwrap().position, below);
        assert_eq!(online.world().player().unwrap().position, below);
        assert_eq!(
            app.world().resource::<Controls>().held,
            Some(Held::PlacementBlocked)
        );
        assert!(receiver.try_recv().is_err());

        // The same admitted player is subsequently placed above the floor.
        // A high valid placement must still emit the ordinary fall.
        let above = world_position([0.0, 12.0, 0.0], 0.0);
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            above,
        );
        app.world_mut().resource_mut::<Controls>().climb_sent = Some((
            12,
            eq_client_assets::falls::TITANIUM.climb_per_step(3.0, 6.0),
        ));
        app.update();
        let ClientCommand::Move(fall) = receiver.try_recv().unwrap() else {
            panic!("expected a fall from the corrected placement");
        };
        assert_eq!(fall.session_id, 12);
        assert_eq!(fall.mode, MovementMode::Fall);
        assert!(fall.position.z < above.z);
        assert_eq!(
            app.world()
                .resource::<online::OnlineState>()
                .server_placement()
                .unwrap()
                .position,
            above
        );
    }

    #[test]
    fn a_held_placement_turns_at_the_received_xyz_without_repeating_its_warning() {
        let (mut app, receiver) = stationary_airborne_app();
        let mut player = online::testing::player(7);
        let initial_placement = world_position([2.375, -5.25, -3.625], 0.0);
        player.position = initial_placement;
        online::testing::generation(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            eq_client_core::GameDialect::Titanium,
        );
        online::testing::admit(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            12,
            player,
        );
        let warning_count = |app: &App| {
            app.world()
                .resource::<crate::chat::ChatState>()
                .history
                .lines(eq_client_core::chat::ChatTab::System)
                .into_iter()
                .filter(|(_, line)| line.message.text.contains("below the installed floor"))
                .count()
        };
        app.update();
        assert_eq!(warning_count(&app), 1);
        assert!(receiver.try_recv().is_err());

        let correction = eq_client_core::WorldPosition {
            heading: 128.0,
            ..initial_placement
        };
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            correction,
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::KeyW);
            keys.press(KeyCode::Space);
        }
        app.update();
        assert_eq!(warning_count(&app), 1);
        assert!(receiver.try_recv().is_err(), "travel and jump remain held");

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        let mut heading = correction.heading;
        for _ in 0..2 {
            app.update();
            let ClientCommand::Move(turn) = receiver.try_recv().unwrap() else {
                panic!("expected the stationary turn");
            };
            assert_eq!(turn.mode, MovementMode::Forward);
            assert_eq!(turn.heading, HeadingUpdate::Rate);
            assert!(arc(heading, turn.position.heading) < 0.0);
            assert_eq!(
                turn.position,
                eq_client_core::WorldPosition {
                    heading: turn.position.heading,
                    ..correction
                }
            );
            assert!(receiver.try_recv().is_err());
            heading = turn.position.heading;
            accept(&mut app, &turn);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut().resource_mut::<Controls>().mouse_heading = Some(256.0);
        app.update();
        let ClientCommand::Move(turn) = receiver.try_recv().unwrap() else {
            panic!("expected the stationary mouse turn");
        };
        assert_eq!(turn.heading, HeadingUpdate::Snapshot);
        assert_eq!(
            turn.position,
            eq_client_core::WorldPosition {
                heading: 256.0,
                ..correction
            }
        );
        assert!(receiver.try_recv().is_err());
        let online = app.world().resource::<online::OnlineState>();
        assert_eq!(online.server_placement().unwrap().position, correction);
        assert_eq!(warning_count(&app), 1);
    }

    #[test]
    fn a_held_placement_rechecks_for_a_new_landing_surface() {
        use eq_client_core::movement::CollisionMesh;

        let (mut app, receiver) = stationary_airborne_app();
        let placed = world_position([0.0, -5.0, 0.0], 0.0);
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            placed,
        );
        let world = app.world_mut();
        let body = *world.query::<&PlayerBody>().single(world).unwrap();
        let server = world.resource::<online::OnlineState>().server_placement();
        let mut check = placement::Check::default();
        // Seed the hold using the production check with a due timestamp so
        // the next input frame can retry without a sleep in the fixture.
        let held_at = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        world.resource_scope(|world, mut chat: Mut<crate::chat::ChatState>| {
            let collision = world.resource::<Collision>().0.as_ref().unwrap();
            assert!(check.holds(server, placed, collision, &body, &mut chat, held_at));
            assert!(check.holds(
                server,
                placed,
                collision,
                &body,
                &mut chat,
                held_at + Duration::from_millis(500),
            ));
        });
        world.resource_mut::<Controls>().placement = check;
        world
            .resource_mut::<Collision>()
            .0
            .as_mut()
            .unwrap()
            .set_door_obstacles(vec![(
                7,
                std::sync::Arc::new(CollisionMesh::new(flat_surface(-15.0)).unwrap()),
            )]);
        app.update();
        let ClientCommand::Move(fall) = receiver.try_recv().unwrap() else {
            panic!("expected an ordinary fall toward the new collider");
        };
        assert_eq!(fall.mode, MovementMode::Fall);
        assert!(fall.position.z < placed.z);
        let online = app.world().resource::<online::OnlineState>();
        assert_eq!(online.server_placement().unwrap().position, placed);
    }

    #[test]
    fn a_withdrawn_movement_grant_takes_precedence_over_a_placement_hold() {
        let (mut app, receiver) = stationary_airborne_app();
        let placed = world_position([0.0, -5.0, 0.0], 0.0);
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            placed,
        );
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().held,
            Some(Held::PlacementBlocked)
        );
        {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            let last_accepted = controls.last_accepted;
            controls.grant(eq_client_core::world::MotionGrant {
                units_per_second: None,
                backward_units_per_second: None,
                walk_units_per_second: None,
                strafe_units_per_second: None,
                strafe_walk_units_per_second: None,
                swim_backward_units_per_second: None,
                diagonal: eq_client_core::DiagonalRates::default(),
                falls: true,
                jump_factor: 1.0,
            });
            controls.last_accepted = last_accepted;
        }
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().held,
            Some(Held::Withdrawn)
        );
        assert!(receiver.try_recv().is_err());
        let online = app.world().resource::<online::OnlineState>();
        assert_eq!(online.world().player().unwrap().position, placed);
    }

    struct Pool {
        top: f32,
        liquid: Liquid,
    }

    impl Liquids for Pool {
        fn liquid_at(&self, point: Vec3) -> Option<Liquid> {
            (point.y <= self.top).then_some(self.liquid)
        }
        fn passes_through(&self, from: Vec3, to: Vec3, liquid: Liquid) -> bool {
            liquid == self.liquid && from.y.min(to.y) <= self.top
        }
        fn first_outside(
            &self,
            from: Vec3,
            to: Vec3,
            liquid: Liquid,
        ) -> Option<std::ops::Range<f64>> {
            if liquid != self.liquid || from.y > self.top {
                return Some(0.0..1.0);
            }
            let (from, to, top) = (f64::from(from.y), f64::from(to.y), f64::from(self.top));
            (to > top).then_some(((from - top) / (from - to))..1.0)
        }
        fn liquid_surface(&self, top: Vec3, bottom: f32) -> Option<(f32, Liquid)> {
            (top.y > self.top && bottom < self.top).then_some((self.top, self.liquid))
        }
        fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
            None
        }
    }

    fn liquid_pool(liquid: Liquid) -> CollisionWorld {
        CollisionWorld::new(flat_surface(-12.0))
            .unwrap()
            .with_liquids(Pool { top: 0.0, liquid })
    }

    #[test]
    fn fly_switch_ascends_from_unsupported_surface_water_after_worker_acceptance() {
        let (mut app, receiver) = stationary_airborne_app();
        let water = liquid_pool(Liquid::Water).with_swim_origin(3.0);
        let feet = Vec3::new(0.0, -3.5, 0.0);
        assert!(water.support(feet).is_none());
        assert_eq!(
            water.swim_depth(feet, 6.0),
            Some(eq_client_core::movement::SwimDepth::Surface)
        );
        app.insert_resource(Collision(Some(water)))
            .init_resource::<crate::options::MovementSync>()
            .add_systems(Update, crate::options::tell_choices.before(input));
        let surface = world_position([0.0, -0.5, 0.0], 0.0);
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            surface,
        );
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(Fix::Fly, true);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        app.update();
        let ClientCommand::SetMovementOptions {
            request_id,
            options,
            ..
        } = receiver.try_recv().unwrap()
        else {
            panic!("flight policy request");
        };
        assert!(options.fly);
        let only_settings =
            |command| !matches!(command, ClientCommand::Move(_) | ClientCommand::Jump { .. });
        assert!(receiver.try_iter().all(only_settings));
        online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsApplied {
                session_id: 11,
                request_id,
                options,
            }],
        );
        app.update();
        assert!(receiver.try_iter().all(only_settings));
        app.world_mut().resource_mut::<Controls>().last_accepted = Instant::now()
            .checked_sub(Duration::from_millis(100))
            .unwrap();

        let mut previous = surface.z;
        for _ in 0..2 {
            app.update();
            let ClientCommand::Move(up) = receiver.try_recv().expect("ascending flight sample")
            else {
                panic!("expected flight without a jump command");
            };
            assert_eq!(up.mode, MovementMode::Forward);
            assert!(up.position.z > previous + 0.5);
            assert!(receiver.try_recv().is_err());
            accept(&mut app, &up);
            previous = up.position.z;
        }
    }

    #[test]
    fn a_fresh_placement_between_a_bridge_and_mapped_liquid_can_fall() {
        for liquid in [Liquid::Water, Liquid::Lava] {
            let (mut app, receiver) = stationary_airborne_app();
            let bridge = CollisionWorld::new(flat_surface(0.0))
                .unwrap()
                .with_liquids(Pool { top: -9.0, liquid });
            app.insert_resource(Collision(Some(bridge)));
            let placed = world_position([0.0, -5.0, 0.0], 0.0);
            online::testing::place(
                &mut app.world_mut().resource_mut::<online::OnlineState>(),
                placed,
            );
            app.update();
            let ClientCommand::Move(fall) = receiver.try_recv().unwrap() else {
                panic!("expected an ordinary fall above {liquid:?}");
            };
            assert_eq!(fall.mode, MovementMode::Fall);
            assert!(fall.position.z < placed.z);
        }
    }

    #[test]
    fn a_water_correction_uses_the_swim_grant_and_a_dry_correction_leaves_it() {
        let (mut app, receiver) = app();
        app.insert_resource(Collision(Some(liquid_pool(Liquid::Water))));
        let placed = world_position([0.0, -4.0, 0.0], 0.0);
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            placed,
        );
        app.update();
        let ClientCommand::Move(swim) = receiver.try_recv().unwrap() else {
            panic!("expected a bounded swim proposal");
        };
        assert_eq!(swim.mode, MovementMode::Swim);
        assert!(
            swim.position.z > placed.z,
            "camera pitch should raise a forward stroke"
        );
        assert!(swim.position.z - placed.z < 0.1);
        assert!(receiver.try_recv().is_err());
        accept(&mut app, &swim);

        let bank = world_position([0.0, 3.0, 0.0], 0.0);
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            bank,
        );
        {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.reset(Some(6.0));
            controls.last_accepted = Instant::now()
                .checked_sub(Duration::from_millis(100))
                .unwrap();
        }
        app.update();
        let ClientCommand::Move(ordinary) = receiver.try_recv().unwrap() else {
            panic!("expected ordinary motion after the new placement");
        };
        assert_eq!(ordinary.mode, MovementMode::Forward);
    }

    #[test]
    fn jesus_mode_uses_ground_motion_and_respects_corrections_and_admission() {
        let (mut app, receiver) = app();
        let arm = |app: &mut App, session| {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.reset(Some(6.0));
            controls.airborne = Some(AirborneController::default());
            controls.last_accepted = Instant::now()
                .checked_sub(Duration::from_millis(100))
                .unwrap();
            controls.climb_sent = Some((
                session,
                eq_client_assets::falls::TITANIUM.climb_per_step(3.0, 6.0),
            ));
        };
        let next_move = |app: &mut App, session| {
            app.update();
            let ClientCommand::Move(proposal) = receiver.try_recv().unwrap() else {
                panic!("expected a movement proposal");
            };
            assert_eq!(proposal.session_id, session);
            proposal
        };
        app.insert_resource(Collision(Some(liquid_pool(
            eq_client_core::movement::Liquid::Water,
        ))));
        arm(&mut app, 11);
        movement_choice(&mut app, Fix::JesusMode, true);
        let walking = next_move(&mut app, 11);
        assert_eq!(walking.mode, MovementMode::Forward);
        assert!((walking.position.z - 3.0).abs() < 0.001);
        assert!(walking.position.x < -0.01);
        assert!(walking.position.x.hypot(walking.position.y) < 0.61);
        accept(&mut app, &walking);

        movement_choice(&mut app, Fix::JesusMode, false);
        let falling = next_move(&mut app, 11);
        assert_eq!(falling.mode, MovementMode::Fall);
        assert!(falling.position.z < walking.position.z);
        accept(&mut app, &falling);

        movement_choice(&mut app, Fix::JesusMode, true);
        let submerged = world_position([0.0, -4.0, 0.0], 0.0);
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            submerged,
        );
        arm(&mut app, 11);
        let swimming = next_move(&mut app, 11);
        assert_eq!(swimming.mode, MovementMode::Swim);
        assert!(swimming.position.z - submerged.z < 0.1);

        let mut online = app.world_mut().resource_mut::<online::OnlineState>();
        let mut player = online.world().player().unwrap().clone();
        player.position = world_position([0.0, 3.0, 0.0], 0.0);
        online::testing::admit(&mut online, 12, player);
        // The new zone has a floor but no mapped water at the old surface.
        let dry = CollisionWorld::new([[
            [-20.0, -12.0, -20.0],
            [20.0, -12.0, -20.0],
            [0.0, -12.0, 20.0],
        ]])
        .unwrap();
        app.insert_resource(Collision(Some(dry)));
        arm(&mut app, 12);
        let falling = next_move(&mut app, 12);
        assert_eq!(falling.mode, MovementMode::Fall);
        assert!(falling.position.z < walking.position.z);
    }

    #[test]
    fn jesus_mode_moves_over_lava_with_an_ordinary_sample() {
        let (mut app, receiver) = app();
        app.insert_resource(Collision(Some(liquid_pool(
            eq_client_core::movement::Liquid::Lava,
        ))));
        app.world_mut().resource_mut::<Controls>().airborne = Some(AirborneController::default());
        movement_choice(&mut app, Fix::JesusMode, true);
        app.update();
        let ClientCommand::Move(walking) = receiver.try_recv().unwrap() else {
            panic!("expected a grounded movement proposal");
        };
        assert_eq!(walking.mode, MovementMode::Forward);
        assert!((walking.position.z - 3.0).abs() < 0.001);
        assert!(walking.position.x < -0.01);
        assert!(walking.position.x.hypot(walking.position.y) < 0.61);
        assert!(receiver.try_recv().is_err());
        accept(&mut app, &walking);

        movement_choice(&mut app, Fix::JesusMode, false);
        app.update();
        let ClientCommand::Move(falling) = receiver.try_recv().unwrap() else {
            panic!("expected ordinary gravity after disabling the option");
        };
        assert_eq!(falling.mode, MovementMode::Fall);
        assert!(falling.position.z < walking.position.z);
    }

    #[test]
    fn jesus_mode_is_inert_where_the_session_withholds_gameplay_departures() {
        let (mut app, receiver) = app();
        {
            let mut state = app.world_mut().resource_mut::<online::OnlineState>();
            let capabilities: Vec<_> = eq_client_core::Capability::ALL
                .into_iter()
                .filter(|capability| *capability != eq_client_core::Capability::GameplayDepartures)
                .collect();
            let mut player = online::testing::player(7);
            player.position = world_position([0.0, 3.0, 0.0], 0.0);
            online::testing::news(
                &mut state,
                [eq_client_core::WorldEvent::Entered {
                    capabilities,
                    choices: Vec::new(),
                    session_id: 12,
                    zone: "qeytoqrg".into(),
                    player: Box::new(player),
                    far_clip: None,
                    spell_landing_flag: None,
                    zone_id: 0,
                }],
            );
            online::testing::connect(&mut state, true);
        }
        app.insert_resource(Collision(Some(liquid_pool(
            eq_client_core::movement::Liquid::Water,
        ))));
        {
            // The session has heard this body's climb for the new admission,
            // as the fixture arranges for session 11.
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.airborne = Some(AirborneController::default());
            controls.climb_sent = Some((
                12,
                eq_client_assets::falls::TITANIUM.climb_per_step(3.0, 6.0),
            ));
        }
        movement_choice(&mut app, Fix::JesusMode, true);
        app.update();
        let ClientCommand::Move(falling) = receiver.try_recv().unwrap() else {
            panic!("expected ordinary gravity where the departure is withheld");
        };
        assert_eq!(falling.session_id, 12);
        assert_eq!(falling.mode, MovementMode::Fall);
        assert!(falling.position.z < 3.0);
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies unchanged or explicitly assigned state"
    )]
    fn strafing_is_perpendicular_to_facing_and_never_borrows_forward_speed() {
        let map = crate::keys::testing::map(&[
            (Act::StrafeLeft, crate::keys::Chord::key(KeyCode::ArrowLeft)),
            (
                Act::StrafeRight,
                crate::keys::Chord::key(KeyCode::ArrowRight),
            ),
        ]);
        for heading in [0.0, 128.0, 256.0, 384.0] {
            let radians = eq_client_core::render_heading(heading);
            let facing = Vec3::new(radians.sin(), 0.0, radians.cos());
            let mut controls = Controls {
                speed: Some(6.0),
                ..default()
            };
            let mut keys = ButtonInput::default();
            keys.press(KeyCode::ArrowRight);
            assert_eq!(
                controls
                    .intent(&keys, &map, &[], true, (0.0, heading), None)
                    .direction,
                Vec3::ZERO
            );
            controls.strafe_speed = Some(3.0);
            let right = controls.intent(&keys, &map, &[], true, (0.0, heading), None);
            assert_eq!(right.mode, MovementMode::Strafe);
            assert!(right.preserve_facing);
            assert_eq!(right.speed, 3.0);
            assert!(right.direction.dot(facing).abs() < 0.0001);
            assert!(right.direction.dot(facing.cross(Vec3::Y)) > 0.999);
            keys.reset_all();
            keys.press(KeyCode::ArrowLeft);
            let left = controls.intent(&keys, &map, &[], true, (0.0, heading), None);
            assert!((left.direction + right.direction).length() < 0.0001);
            assert_eq!(
                controls
                    .intent(&keys, &map, &[], false, (0.0, heading), None)
                    .direction,
                Vec3::ZERO
            );
            keys.press(KeyCode::ArrowRight);
            assert_eq!(
                controls
                    .intent(&keys, &map, &[], true, (0.0, heading), None)
                    .direction,
                Vec3::ZERO
            );
        }
    }

    #[test]
    fn a_scripts_held_command_moves_whatever_keys_the_table_gives_it() {
        let controls = Controls {
            speed: Some(6.0),
            ..Controls::default()
        };
        let keys = ButtonInput::<KeyCode>::default();
        let titanium = KeyMap::for_client(InstalledClient::Titanium);
        // The Titanium table gives the camera-relative forward no key, and
        // a script holding it still moves.
        assert_eq!(titanium.keys(Act::CameraForward).count(), 0);
        let held = |scripted: &[Act], focused, heading| {
            controls
                .intent(&keys, &titanium, scripted, focused, (0.0, heading), None)
                .direction
        };
        assert!((held(&[Act::CameraForward], true, 0.0).length() - 1.0).abs() < 1e-4);
        // Forward goes the way the player faces.
        let ahead = held(&[Act::Forward], true, 128.0);
        let radians = eq_client_core::render_heading(128.0);
        assert!((ahead - Vec3::new(radians.sin(), 0.0, radians.cos())).length() < 1e-4);
        // Without the keyboard, as without a key, nothing moves.
        assert_eq!(held(&[Act::Forward], false, 128.0), Vec3::ZERO);
        assert_eq!(held(&[], true, 128.0), Vec3::ZERO);
    }

    #[test]
    fn walk_toggle_uses_its_grant_and_is_ignored_while_typing() {
        let (mut app, receiver) = app();
        app.world_mut().resource_mut::<Controls>().walk_speed = Some(2.0);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyR);
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected walking movement")
        };
        assert_eq!(request.mode, MovementMode::Walk);
        assert!((request.position.x.hypot(request.position.y) - 0.2).abs() < 0.001);
        accept(&mut app, &request);
        // A server's correction, as after a gm goto, starts motion afresh
        // but leaves the toggle as the player left it (6.363), so the next
        // grant walks again.
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.reset(None);
        assert!(controls.walking);
        assert!(controls.walk_speed.is_none());
        controls.grant(eq_client_core::world::MotionGrant {
            diagonal: eq_client_core::DiagonalRates::default(),
            units_per_second: Some(6.0),
            backward_units_per_second: None,
            walk_units_per_second: Some(2.0),
            strafe_units_per_second: None,
            strafe_walk_units_per_second: None,
            swim_backward_units_per_second: None,
            falls: false,
            jump_factor: 1.0,
        });
        controls.last_accepted = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected walking movement after the correction")
        };
        assert_eq!(request.mode, MovementMode::Walk);
        let (mut app, _) = self::app();
        app.world_mut().resource_mut::<Controls>().walk_speed = Some(2.0);
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyR);
        app.update();
        assert!(!app.world().resource::<Controls>().walking);
    }

    #[test]
    fn backing_up_preserves_facing_and_requires_its_own_grant() {
        let (mut app, receiver) = app();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::ArrowDown);
        app.update();
        assert!(receiver.try_recv().is_err());
        app.world_mut().resource_mut::<Controls>().backward_speed = Some(2.0);
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected backward movement")
        };
        assert_eq!(request.mode, MovementMode::Backward);
        assert!(request.position.heading.abs() < 0.001);
        assert!((request.position.y + 0.2).abs() < 0.001);
        assert!(request.position.x.abs() < 0.001);
        app.world_mut().resource_mut::<Controls>().reset(None);
        assert!(app.world().resource::<Controls>().backward_speed.is_none());
    }

    #[test]
    fn a_sitting_player_stays_put_with_auto_stand_off() {
        let (mut app, receiver) = app();
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [eq_client_core::WorldEvent::Posture {
                spawn_id: 7,
                posture: eq_client_core::PostureState::Sitting,
            }],
        );
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .allow_auto_stand = false;
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::ArrowUp);
        app.update();
        assert!(receiver.try_recv().is_err());
        // On, as by default, the same press moves them (and the session
        // stands them first).
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .allow_auto_stand = true;
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected movement")
        };
        assert_eq!(request.mode, MovementMode::Forward);
    }

    #[test]
    fn ducking_halves_a_strafe() {
        // The Titanium speeds read (the R5 audit's S1); the session doesn't
        // track the stance, so the player's side halves it.
        for (posture, moved) in [
            (eq_client_core::PostureState::Standing, 0.4),
            (eq_client_core::PostureState::Ducking, 0.2),
        ] {
            let (mut app, receiver) = app();
            crate::keys::testing::bind(
                &mut app,
                Act::StrafeRight,
                crate::keys::Chord::key(KeyCode::KeyL),
            );
            app.world_mut().resource_mut::<Controls>().strafe_speed = Some(4.0);
            crate::online::testing::news(
                &mut app.world_mut().resource_mut::<online::OnlineState>(),
                [eq_client_core::WorldEvent::Posture {
                    spawn_id: 7,
                    posture,
                }],
            );
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::KeyL);
            app.update();
            let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
                panic!("expected a strafe")
            };
            assert_eq!(request.mode, MovementMode::Strafe);
            let across = request.position.x.hypot(request.position.y);
            assert!((across - moved).abs() < 0.001, "{posture:?}: {across}");
        }
    }

    #[test]
    fn turning_in_place_changes_heading_without_displacing_the_player() {
        let (mut app, receiver) = app();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::KeyE);
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected turn")
        };
        assert_eq!(request.heading, HeadingUpdate::Rate);
        // The turn ramp's first step: 20 units a second for a frame.
        assert!((request.position.heading - (512.0 - 20.0 / 60.0)).abs() < 0.001);
        assert!(request.position.x.abs() < 0.001 && request.position.y.abs() < 0.001);
        assert!((turn_toward(508.0, 4.0, 3.0) - 511.0).abs() < 0.001);
        assert!((turn_toward(4.0, 508.0, 3.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn departure_motion_waits_for_matching_local_policy_application() {
        let (mut app, receiver) = app();
        let set_fix = |app: &mut App, fix, enabled| {
            app.world_mut()
                .resource_mut::<crate::options::OptionsState>()
                .options
                .qol
                .set(fix, enabled);
        };
        let wait_count = |app: &App| {
            app.world()
                .resource::<crate::chat::ChatState>()
                .history
                .lines(eq_client_core::chat::ChatTab::System)
                .into_iter()
                .filter(|(_, line)| {
                    line.message.text == "Movement paused while your movement settings are applied."
                })
                .count()
        };
        app.init_resource::<crate::options::MovementSync>()
            .add_systems(Update, crate::options::tell_choices.before(input));
        set_fix(&mut app, Fix::Fly, true);
        app.update();
        assert_eq!(wait_count(&app), 1);
        let commands: Vec<_> = receiver.try_iter().collect();
        let (request_id, options) = commands
            .iter()
            .find_map(|command| match command {
                ClientCommand::SetMovementOptions {
                    request_id,
                    options,
                    ..
                } => Some((*request_id, *options)),
                _ => None,
            })
            .unwrap();
        assert!(options.fly);
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, ClientCommand::Move(_)))
        );
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsApplied {
                session_id: 11,
                request_id: request_id + 1,
                options,
            }],
        );
        app.update();
        assert_eq!(wait_count(&app), 1);
        assert!(
            !receiver
                .try_iter()
                .any(|command| matches!(command, ClientCommand::Move(_)))
        );
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsApplied {
                session_id: 11,
                request_id,
                options,
            }],
        );
        app.update();
        assert_eq!(wait_count(&app), 1);
        assert!(
            !receiver
                .try_iter()
                .any(|command| matches!(command, ClientCommand::Move(_)))
        );
        app.world_mut().resource_mut::<Controls>().last_accepted = Instant::now()
            .checked_sub(Duration::from_millis(100))
            .unwrap();
        app.update();
        let sample = receiver
            .try_iter()
            .find_map(|command| match command {
                ClientCommand::Move(sample) => Some(sample),
                _ => None,
            })
            .expect("movement with flight applied");
        assert!((sample.position.x + 0.6).abs() < 0.001);
        set_fix(&mut app, Fix::Fly, false);
        app.update();
        app.update();
        assert_eq!(wait_count(&app), 2);

        // A new admission has its own wait even if the last one never cleared.
        while receiver.try_recv().is_ok() {}
        crate::online::testing::admit(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            12,
            crate::online::testing::player(7),
        );
        set_fix(&mut app, Fix::WalkWhileWinded, true);
        app.update();
        app.update();
        assert_eq!(wait_count(&app), 3);
    }

    /// The model's rotation when it faces this heading.
    fn turned_to(heading: f32) -> Quat {
        Quat::from_rotation_y(eq_client_core::render_heading(heading))
    }

    /// Takes the sample as the session would and draws it, then lets the next
    /// sample go after a tenth of a second; returns when that wait began.
    fn accept(app: &mut App, request: &MovementRequest) -> Instant {
        // Some guard fixtures replace the request's time with a simulated
        // clock. The host still waits on the stamp that it put in the queue.
        let created = app.world().resource::<Controls>().pending.unwrap().created;
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [eq_client_core::WorldEvent::MotionSent {
                session_id: request.session_id,
                created,
                position: request.position,
                refused: None,
            }],
        );
        let world = app.world_mut();
        let mut players = world.query_filtered::<&Transform, With<Player>>();
        let drawn = *players.single(world).unwrap();
        let mut controls = app.world_mut().resource_mut::<Controls>();
        assert!(controls.moved(Some(request.session_id), created));
        controls.display_sample(drawn, request.position);
        controls.last_accepted = Instant::now()
            .checked_sub(Duration::from_millis(100))
            .unwrap();
        controls.last_accepted
    }

    #[test]
    fn camera_relative_keys_show_their_way_at_once_while_the_heading_sent_turns() {
        // The player faces heading 0 and the camera looks along 384. Each
        // camera-relative key shows the player facing its way from the first
        // sample, however far that is, while the heading sent turns toward it
        // at the turn keys' rate: 24 units in a first sample's tenth of a
        // second. The character-relative forward arrow keeps the facing.
        for (key, way, sent) in [
            (KeyCode::KeyW, 384.0, 488.0),
            (KeyCode::KeyS, 128.0, 24.0),
            (KeyCode::KeyA, 0.0, 0.0),
            (KeyCode::KeyD, 256.0, 488.0),
            (KeyCode::ArrowUp, 0.0, 0.0),
        ] {
            let (mut app, receiver) = app();
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Transform, With<Player>>();
            players.single_mut(world).unwrap().rotation = turned_to(0.0);
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
            let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
                panic!("expected movement for {key:?}")
            };
            let heading = request.position.heading;
            assert!(arc(sent, heading).abs() < 0.001, "{key:?} sent {heading}");
            let moved = Vec3::new(request.position.x, request.position.y, 0.0);
            assert!(moved.length() > 0.1, "{key:?} did not move");
            accept(&mut app, &request);
            let visual = app.world().resource::<Controls>().visual.as_ref().unwrap();
            assert!(
                visual.from.rotation.angle_between(turned_to(way)) < 0.001,
                "{key:?} showed {}",
                visual.from.rotation
            );
        }
    }

    #[test]
    fn physical_turn_keys_strafe_only_while_the_world_owns_mouse_look() {
        let heading = eq_client_core::render_heading(0.0);
        let right = Vec3::new(heading.sin(), 0.0, heading.cos()).cross(Vec3::Y);
        let origin = Vec3::new(0.0, 3.0, 0.0);
        for (act, key, side) in [
            (Act::TurnLeft, KeyCode::KeyA, -1.0),
            (Act::TurnRight, KeyCode::KeyD, 1.0),
        ] {
            for world_owns_look in [false, true] {
                let (mut app, receiver) = app();
                crate::camera::testing::install_look(&mut app);
                app.init_resource::<crate::items::ItemState>()
                    .init_resource::<crate::inventory::InventoryState>();
                crate::keys::testing::bind(&mut app, act, crate::keys::Chord::key(key));
                app.world_mut().resource_mut::<Controls>().strafe_speed = Some(3.0);
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .reset_all();
                app.world_mut()
                    .resource_mut::<crate::chat::ChatState>()
                    .hovered = !world_owns_look;
                crate::camera::testing::button(&mut app, MouseButton::Right, true);
                app.update();
                assert!(receiver.try_recv().is_err());
                assert_eq!(app.world().resource::<MouseLook>().held, world_owns_look);

                // Leaving the chat panel with the button held cannot start a look.
                app.world_mut()
                    .resource_mut::<crate::chat::ChatState>()
                    .hovered = false;
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .clear();
                app.world_mut()
                    .resource_mut::<crate::keys::Typing>()
                    .composing = true;
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(key);
                app.update();
                assert!(receiver.try_recv().is_err());
                app.world_mut()
                    .resource_mut::<crate::keys::Typing>()
                    .composing = false;
                app.update();
                let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
                    panic!("expected a movement proposal");
                };
                let position = Vec3::from_array(eq_client_core::render_position(request.position));
                if world_owns_look {
                    assert_eq!(request.mode, MovementMode::Strafe);
                    assert!((position - origin - right * side * 0.3).length() < 0.001);
                    assert!(arc(0.0, request.position.heading).abs() < 0.001);
                    accept(&mut app, &request);
                    crate::camera::testing::button(&mut app, MouseButton::Right, false);
                    app.update();
                    let ClientCommand::Move(turn) = receiver.try_recv().unwrap() else {
                        panic!("expected turning on release");
                    };
                    assert!(!app.world().resource::<MouseLook>().held);
                    assert_eq!(turn.mode, MovementMode::Forward);
                    assert!(
                        Vec3::from_array(eq_client_core::render_position(turn.position))
                            .abs_diff_eq(position, 0.001)
                    );
                    assert!(arc(request.position.heading, turn.position.heading) * side < 0.0);
                } else {
                    assert_eq!(request.mode, MovementMode::Forward);
                    assert!(position.abs_diff_eq(origin, 0.001));
                    assert!(arc(0.0, request.position.heading) * side < 0.0);
                }
            }
        }
    }

    #[test]
    fn remapped_turn_keys_respect_strafe_grants_focus_and_semantic_actions() {
        let map = crate::keys::testing::map(&[
            (Act::TurnLeft, crate::keys::Chord::key(KeyCode::KeyA)),
            (Act::StrafeRight, crate::keys::Chord::key(KeyCode::KeyE)),
        ]);
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyA);
        let looking = MouseLook {
            held: true,
            left: false,
        };
        let both = MouseLook {
            held: true,
            left: true,
        };
        let mut controls = Controls {
            speed: Some(6.0),
            ..default()
        };
        let intent =
            |controls: &Controls, keys: &ButtonInput<KeyCode>, scripted: &[Act], focused, look| {
                controls.intent(keys, &map, scripted, focused, (0.0, 0.0), Some(look))
            };

        let no_grant = intent(&controls, &keys, &[], true, &looking);
        assert_eq!(no_grant.direction, Vec3::ZERO);
        assert!(no_grant.turn.abs() < f32::EPSILON);
        controls.strafe_speed = Some(3.0);
        let unfocused = intent(&controls, &keys, &[], false, &looking);
        assert_eq!(unfocused.direction, Vec3::ZERO);
        assert!(unfocused.turn.abs() < f32::EPSILON);
        controls.stunned = true;
        let stunned = intent(&controls, &keys, &[], true, &looking);
        assert_eq!(stunned.direction, Vec3::ZERO);
        assert!(stunned.turn.abs() < f32::EPSILON);
        controls.stunned = false;

        let forward = intent(&controls, &keys, &[], true, &both);
        assert_eq!(forward.mode, MovementMode::Forward);
        assert!(forward.direction.abs_diff_eq(Vec3::NEG_Z, 0.001));
        assert!(forward.turn.abs() < f32::EPSILON);
        keys.press(KeyCode::ArrowUp);
        let forward = intent(&controls, &keys, &[], true, &looking);
        let heading = eq_client_core::render_heading(0.0);
        assert_eq!(forward.mode, MovementMode::Forward);
        assert!(
            forward
                .direction
                .abs_diff_eq(Vec3::new(heading.sin(), 0.0, heading.cos()), 0.001)
        );
        assert!(forward.turn.abs() < f32::EPSILON);
        let turns = intent(
            &controls,
            &ButtonInput::default(),
            &[Act::TurnLeft],
            true,
            &looking,
        );
        assert_eq!(turns.direction, Vec3::ZERO);
        assert!((turns.turn - 1.0).abs() < f32::EPSILON);

        keys.reset_all();
        keys.press(KeyCode::KeyE);
        let explicit = intent(&controls, &keys, &[], true, &looking);
        assert_eq!(explicit.mode, MovementMode::Strafe);
        assert!((explicit.speed - 3.0).abs() < f32::EPSILON);
        assert!(explicit.turn.abs() < f32::EPSILON);
    }

    #[test]
    fn grounded_mouse_look_diagonals_keep_both_components_and_release_independently() {
        for (key, along, advance, walking) in [
            (KeyCode::KeyW, DiagonalAlong::Run, 0.6, false),
            (KeyCode::KeyW, DiagonalAlong::Walk, 0.4, true),
            (
                KeyCode::KeyS,
                DiagonalAlong::Backward { walking: false },
                -0.2,
                false,
            ),
            (
                KeyCode::KeyS,
                DiagonalAlong::Backward { walking: true },
                -0.2,
                true,
            ),
        ] {
            for (turn, side_key, side, sign) in [
                (Act::TurnLeft, KeyCode::KeyA, StrafeSide::Left, 1.0),
                (Act::TurnRight, KeyCode::KeyD, StrafeSide::Right, -1.0),
            ] {
                let (mut app, receiver) = app();
                for (act, key) in [
                    (Act::Forward, KeyCode::KeyW),
                    (Act::Back, KeyCode::KeyS),
                    (turn, side_key),
                ] {
                    crate::keys::testing::bind(&mut app, act, crate::keys::Chord::key(key));
                }
                app.insert_resource(MouseLook {
                    held: true,
                    left: false,
                });
                let mut controls = app.world_mut().resource_mut::<Controls>();
                controls.backward_speed = Some(2.0);
                controls.walk_speed = Some(4.0);
                // A combined movement has its own lateral allowance.
                controls.strafe_speed = Some(5.0);
                controls.diagonal_rates = DiagonalRates {
                    run: Some(3.0),
                    walk: Some(1.5),
                };
                controls.walking = walking;
                let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keyboard.reset_all();
                keyboard.press(key);
                keyboard.press(side_key);

                app.update();
                let ClientCommand::Move(combined) = receiver.try_recv().unwrap() else {
                    panic!("expected a diagonal sample")
                };
                assert_eq!(
                    combined.mode,
                    MovementMode::GroundedDiagonal(GroundedDiagonal { along, side })
                );
                let lateral = if walking { 0.15 } else { 0.3 };
                assert!((combined.position.x - sign * lateral).abs() < 0.001);
                assert!((combined.position.y - advance).abs() < 0.001);
                assert!(combined.position.heading.abs() < 0.001);
                accept(&mut app, &combined);

                let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keyboard.clear();
                keyboard.release(side_key);
                app.update();
                let ClientCommand::Move(straight) = receiver.try_recv().unwrap() else {
                    panic!("expected the remaining longitudinal sample")
                };
                assert!((straight.position.x - combined.position.x).abs() < 0.001);
                assert!((straight.position.y - combined.position.y) * advance > 0.0);
                accept(&mut app, &straight);

                let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keyboard.clear();
                keyboard.release(key);
                app.update();
                let ClientCommand::Move(stop) = receiver.try_recv().unwrap() else {
                    panic!("expected the ordinary stop sample")
                };
                assert_eq!(stop.position, straight.position);
                accept(&mut app, &stop);
                app.update();
                assert!(receiver.try_recv().is_err());
            }
        }
    }

    #[test]
    fn a_diagonal_trial_that_leaves_support_uses_the_ordinary_ledge_path() {
        let (mut app, receiver) = app();
        let edge = CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [0.0, 0.0, -20.0], [0.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [0.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ])
        .unwrap();
        app.insert_resource(Collision(Some(edge)))
            .insert_resource(MouseLook {
                held: true,
                left: false,
            });
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [eq_client_core::WorldEvent::MotionSent {
                session_id: 11,
                created: Instant::now(),
                position: world_position([-0.05, 3.0, 0.0], 0.0),
                refused: None,
            }],
        );
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.airborne = Some(AirborneController::default());
        controls.diagonal_rates = DiagonalRates {
            run: Some(3.0),
            walk: None,
        };
        crate::keys::testing::bind(
            &mut app,
            Act::Forward,
            crate::keys::Chord::key(KeyCode::KeyW),
        );
        crate::keys::testing::bind(
            &mut app,
            Act::TurnRight,
            crate::keys::Chord::key(KeyCode::KeyD),
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyD);
        app.update();
        let ClientCommand::Move(crossing) = receiver.try_recv().unwrap() else {
            panic!("expected the ordinary ledge proposal");
        };
        assert!(!matches!(crossing.mode, MovementMode::GroundedDiagonal(_)));
        assert!(crossing.position.x.abs() < 0.001);
        accept(&mut app, &crossing);
        app.update();
        let ClientCommand::Move(falling) = receiver.try_recv().unwrap() else {
            panic!("expected the ordinary fall");
        };
        assert_eq!(falling.mode, MovementMode::Fall);
        assert!(falling.position.x.abs() < 0.001);
    }

    #[test]
    fn diagonal_intent_keeps_axis_cancellation_and_mouse_forward_heading() {
        let map = crate::keys::testing::map(&[
            (Act::Forward, crate::keys::Chord::key(KeyCode::KeyW)),
            (Act::Back, crate::keys::Chord::key(KeyCode::KeyS)),
            (Act::StrafeLeft, crate::keys::Chord::key(KeyCode::KeyQ)),
            (Act::StrafeRight, crate::keys::Chord::key(KeyCode::KeyE)),
        ]);
        let mut controls = Controls {
            speed: Some(6.0),
            backward_speed: Some(2.0),
            strafe_speed: Some(5.0),
            diagonal_rates: DiagonalRates {
                run: Some(3.0),
                walk: None,
            },
            ..default()
        };
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::KeyQ);
        keys.press(KeyCode::KeyE);
        let only_forward = controls.intent(&keys, &map, &[], true, (0.0, 0.0), None);
        assert!(only_forward.diagonal.is_none());
        assert_eq!(only_forward.mode, MovementMode::Forward);
        keys.clear();
        keys.release(KeyCode::KeyQ);
        keys.press(KeyCode::KeyS);
        let only_side = controls.intent(&keys, &map, &[], true, (0.0, 0.0), None);
        assert!(only_side.diagonal.is_none());
        assert_eq!(only_side.mode, MovementMode::Strafe);

        keys.reset_all();
        keys.press(KeyCode::KeyE);
        controls.autorun = true;
        controls.movement_options.run_speed_multiplier =
            RunSpeedMultiplier::from_percent(120).unwrap();
        let autorun = controls.intent(&keys, &map, &[], true, (0.0, 0.0), None);
        let diagonal = autorun.diagonal.expect("forward and lateral movement");
        assert!((diagonal.longitudinal - 7.2).abs() < 0.001);
        assert!((diagonal.lateral - 3.0).abs() < 0.001);
        controls.autorun = false;
        let both = MouseLook {
            held: true,
            left: true,
        };
        let yaw = eq_client_core::render_heading(0.0) + std::f32::consts::PI;
        let mouse_forward = controls.intent(&keys, &map, &[], true, (yaw, 0.0), Some(&both));
        assert!(mouse_forward.diagonal.is_some());
        let mismatched = controls.intent(&keys, &map, &[], true, (0.0, 0.0), Some(&both));
        assert!(mismatched.diagonal.is_none());
        let turn = controls.intent(
            &keys,
            &map,
            &[Act::TurnLeft, Act::Forward],
            true,
            (yaw, 0.0),
            None,
        );
        assert!(turn.diagonal.is_none());
        controls.diagonal_rates = DiagonalRates::default();
        let ungranted = controls.intent(&keys, &map, &[Act::Forward], true, (yaw, 0.0), None);
        assert!(ungranted.diagonal.is_none());
        assert_eq!(ungranted.mode, MovementMode::Forward);
    }

    #[test]
    fn the_left_button_while_looking_around_runs_the_way_the_camera_faces() {
        // The camera looks along 384; with no key held, the left button
        // alone moves nothing, and held while the right button looks
        // around (the look takes it, `MouseLook::left`) it runs that way,
        // as W does.
        for (looking, left, moves) in [
            (false, true, false),
            (true, false, false),
            (true, true, true),
        ] {
            let (mut app, receiver) = app();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.insert_resource(crate::camera::MouseLook {
                held: looking,
                left: looking && left,
            });
            app.update();
            let sent = receiver.try_recv().ok();
            assert_eq!(sent.is_some(), moves, "looking {looking}, left {left}");
            if let Some(ClientCommand::Move(request)) = sent {
                assert_eq!(request.mode, MovementMode::Forward);
                assert!(request.position.x < -0.1, "{:?}", request.position);
                assert!(request.position.y.abs() < 0.01, "{:?}", request.position);
            }
        }
    }

    fn mouse_look_app() -> (App, mpsc::Receiver<ClientCommand>, Entity, Entity) {
        let (mut app, receiver) = app();
        crate::camera::testing::install_look(&mut app);
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(250));
        app.insert_resource(time)
            .init_resource::<crate::items::ItemState>()
            .init_resource::<crate::inventory::InventoryState>()
            .init_resource::<crate::windows::pointer::Wheel>()
            .add_systems(Update, crate::camera::turn.before(input))
            .add_systems(
                Update,
                (interpolate, crate::camera::orbit_camera)
                    .chain()
                    .after(input),
            );
        let world = app.world_mut();
        world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
        let camera = world
            .query_filtered::<Entity, With<OrbitCamera>>()
            .single(world)
            .unwrap();
        world.entity_mut(camera).insert(Transform::default());
        let player = world
            .query_filtered::<Entity, With<Player>>()
            .single(world)
            .unwrap();
        world.get_mut::<Transform>(player).unwrap().rotation = turned_to(0.0);
        app.update();
        (app, receiver, camera, player)
    }

    fn mouse_drag(app: &mut App, x: f32, y: f32) {
        app.world_mut()
            .write_message(bevy::input::mouse::MouseMotion {
                delta: Vec2::new(x, y),
            });
    }

    fn assert_attached_heading(app: &App, camera: Entity, player: Entity, heading: f32) {
        let world = app.world();
        let actor_forward = world.get::<Transform>(player).unwrap().rotation * Vec3::Z;
        let view_forward = world.get::<Transform>(camera).unwrap().forward();
        assert!(actor_forward.abs_diff_eq(turned_to(heading) * Vec3::Z, 0.001));
        assert!(actor_forward.abs_diff_eq(view_forward.with_y(0.0).normalize(), 0.001));
    }

    #[test]
    fn attached_mouse_half_turn_is_immediate_and_uses_the_normal_send_opportunity() {
        let (mut app, receiver, camera, player) = mouse_look_app();
        crate::camera::testing::button(&mut app, MouseButton::Right, true);
        mouse_drag(&mut app, 0.0, 20.0);
        app.update();
        assert!(receiver.try_recv().is_err());
        assert_attached_heading(&app, camera, player, 0.0);

        let start = Instant::now();
        let origin = world_position([0.0, 3.0, 0.0], 0.0);
        let mut session = eq_network_game::movement::MotionSession::new(11, 7, origin, start)
            .unwrap()
            .with_heading_snapshots(true);
        session
            .calibrate(
                eq_client_core::MotionCalibration {
                    swim_backward_units_per_second: None,
                    units_per_second: 6.0,
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
        let pixels = std::f32::consts::PI
            / app
                .world()
                .resource::<crate::options::OptionsState>()
                .options
                .turn_per_pixel(crate::camera::screen_width(None));
        app.world_mut().resource_mut::<Controls>().last_accepted = Instant::now();
        mouse_drag(&mut app, pixels, 0.0);
        app.update();
        assert!(receiver.try_recv().is_err());
        assert_attached_heading(&app, camera, player, 256.0);
        assert_eq!(
            app.world().get::<Transform>(player).unwrap().translation,
            Vec3::new(0.0, 3.0, 0.0)
        );

        // Releasing after applying the turn leaves that completed orientation
        // available for the next sample without another mouse event.
        crate::camera::testing::button(&mut app, MouseButton::Right, false);
        app.world_mut().resource_mut::<Controls>().last_accepted = Instant::now()
            .checked_sub(Duration::from_millis(100))
            .unwrap();
        app.update();
        let ClientCommand::Move(mut first) = receiver.try_recv().unwrap() else {
            panic!("expected a stationary mouse turn");
        };
        assert_eq!(first.mode, MovementMode::Forward);
        assert_eq!(first.heading, HeadingUpdate::Snapshot);
        assert_eq!(
            (first.position.x, first.position.y, first.position.z),
            (origin.x, origin.y, origin.z)
        );
        assert!(arc(256.0, first.position.heading).abs() < 0.001);
        assert_attached_heading(&app, camera, player, 256.0);
        first.created = start + Duration::from_millis(100);
        session
            .send_move_sample(&first, first.created, |sample| {
                assert_eq!(sample.animation, 0);
                assert_eq!(sample.delta_heading, 0);
                Ok(())
            })
            .unwrap();
        accept(&mut app, &first);

        app.update();
        assert!(receiver.try_recv().is_err());
        assert_attached_heading(&app, camera, player, 256.0);
    }

    #[test]
    fn a_rapid_mouse_reversal_during_forward_ignores_older_playback_yaw() {
        let (mut app, receiver, camera, player) = mouse_look_app();
        crate::camera::testing::button(&mut app, MouseButton::Right, true);
        crate::camera::testing::button(&mut app, MouseButton::Left, true);
        app.update();
        let ClientCommand::Move(old) = receiver.try_recv().unwrap() else {
            panic!("expected a forward step");
        };
        let pixels = std::f32::consts::PI
            / app
                .world()
                .resource::<crate::options::OptionsState>()
                .options
                .turn_per_pixel(crate::camera::screen_width(None));
        mouse_drag(&mut app, -pixels, 0.0);
        app.update();
        assert_attached_heading(&app, camera, player, 256.0);
        assert!(receiver.try_recv().is_err(), "another sample is in flight");
        assert_eq!(
            app.world().get::<Transform>(player).unwrap().translation,
            Vec3::new(0.0, 3.0, 0.0)
        );
        accept(&mut app, &old);
        app.update();
        assert_attached_heading(&app, camera, player, 256.0);
        let old_point = Vec3::from_array(eq_client_core::render_position(old.position));
        assert!(
            app.world()
                .get::<Transform>(player)
                .unwrap()
                .translation
                .abs_diff_eq(old_point, 0.001)
        );
        let ClientCommand::Move(reversed) = receiver.try_recv().unwrap() else {
            panic!("expected a forward step after reversing");
        };
        assert_eq!(reversed.heading, HeadingUpdate::Snapshot);
        let displacement =
            Vec3::from_array(eq_client_core::render_position(reversed.position)) - old_point;
        assert!(
            displacement
                .normalize()
                .abs_diff_eq(turned_to(256.0) * Vec3::Z, 0.001)
        );
        let span = app.world().resource::<Controls>().sent_span;
        assert!((displacement.length() - 6.0 * span).abs() < 0.001);
    }

    #[test]
    fn blur_and_authoritative_placement_drop_unsent_mouse_heading() {
        for corrected in [false, true] {
            let (mut app, receiver, camera, player) = mouse_look_app();
            crate::camera::testing::button(&mut app, MouseButton::Right, true);
            app.world_mut().resource_mut::<Controls>().last_accepted = Instant::now();
            mouse_drag(&mut app, 80.0, 0.0);
            app.update();
            assert!(receiver.try_recv().is_err());
            let shown = app.world().get::<Transform>(player).unwrap().rotation;
            assert!(shown.angle_between(turned_to(0.0)) > 0.1);
            let expected = if corrected {
                let world = app.world_mut();
                crate::online::testing::place(
                    &mut world.resource_mut::<online::OnlineState>(),
                    world_position([0.0, 3.0, 0.0], 64.0),
                );
                world.resource_mut::<Controls>().reset(None);
                64.0
            } else {
                let world = app.world_mut();
                world
                    .query::<&mut Window>()
                    .single_mut(world)
                    .unwrap()
                    .focused = false;
                0.0
            };
            app.update();
            assert!(receiver.try_recv().is_err());
            assert_attached_heading(&app, camera, player, expected);
        }
    }

    #[test]
    fn physically_held_mouse_forward_moves_each_sample_and_stops_on_either_release_or_blur() {
        for release in [Some(MouseButton::Left), Some(MouseButton::Right), None] {
            let (mut app, receiver, camera, player) = mouse_look_app();
            crate::camera::testing::button(&mut app, MouseButton::Right, true);
            app.update();
            assert!(receiver.try_recv().is_err());
            crate::camera::testing::button(&mut app, MouseButton::Left, true);
            let mut position = world_position([0.0, 3.0, 0.0], 0.0);
            for x in [0.0, 60.0, 0.0, 0.0] {
                mouse_drag(&mut app, x, 0.0);
                app.update();
                let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
                    panic!("expected a held forward sample");
                };
                assert_attached_heading(&app, camera, player, request.position.heading);
                assert_eq!(request.mode, MovementMode::Forward);
                let displacement =
                    Vec3::from_array(eq_client_core::render_position(request.position))
                        - Vec3::from_array(eq_client_core::render_position(position));
                assert!(displacement.length() > 0.55);
                assert!(
                    displacement
                        .normalize()
                        .abs_diff_eq(turned_to(request.position.heading) * Vec3::Z, 0.001)
                );
                if x != 0.0 {
                    assert!(arc(position.heading, request.position.heading) < -1.0);
                }
                position = request.position;
                accept(&mut app, &request);
            }
            let window = app
                .world_mut()
                .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
                .single(app.world())
                .unwrap();
            if let Some(button) = release {
                crate::camera::testing::button(&mut app, button, false);
            } else {
                app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
            }
            app.update();
            assert_attached_heading(&app, camera, player, position.heading);
            let ClientCommand::Move(stop) = receiver.try_recv().unwrap() else {
                panic!("expected a stop");
            };
            assert_eq!(stop.position, position);
            accept(&mut app, &stop);
            app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
            app.update();
            assert!(receiver.try_recv().is_err());
        }
    }

    fn keyboard_mouse_look_app() -> (App, mpsc::Receiver<ClientCommand>, Entity, Entity) {
        use bevy::input::keyboard::{Key, KeyboardFocusLost, KeyboardInput, keyboard_input_system};
        let (mut app, receiver, camera, player) = mouse_look_app();
        app.init_resource::<ButtonInput<Key>>()
            .add_message::<KeyboardInput>()
            .add_message::<KeyboardFocusLost>()
            .add_systems(
                PreUpdate,
                (keyboard_input_system, crate::keys::presses::capture).chain(),
            );
        for (act, key) in [
            (Act::Forward, KeyCode::KeyW),
            (Act::TurnLeft, KeyCode::KeyA),
            (Act::TurnRight, KeyCode::KeyD),
        ] {
            crate::keys::testing::bind(&mut app, act, crate::keys::Chord::key(key));
        }
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.diagonal_rates.run = Some(3.0);
        controls.strafe_speed = Some(4.0);
        (app, receiver, camera, player)
    }

    fn native_movement_key(app: &mut App, key_code: KeyCode, pressed: bool) {
        use bevy::input::{
            ButtonState,
            keyboard::{Key, KeyboardInput},
        };
        let world = app.world_mut();
        let window = world
            .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
            .single(world)
            .unwrap();
        let letter = match key_code {
            KeyCode::KeyW => "w",
            KeyCode::KeyA => "a",
            KeyCode::KeyD => "d",
            _ => unreachable!("this fixture only assigns W/A/D"),
        };
        world.write_message(KeyboardInput {
            key_code,
            logical_key: Key::Character(letter.into()),
            state: if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
            text: None,
            repeat: false,
            window,
        });
    }

    fn assert_diagonal_step(
        app: &App,
        from: eq_client_core::WorldPosition,
        request: &MovementRequest,
        side: StrafeSide,
    ) {
        assert_eq!(
            request.mode,
            MovementMode::GroundedDiagonal(GroundedDiagonal {
                along: DiagonalAlong::Run,
                side
            })
        );
        let facing = turned_to(request.position.heading) * Vec3::Z;
        let delta = Vec3::from_array(eq_client_core::render_position(request.position))
            - Vec3::from_array(eq_client_core::render_position(from));
        let span = app.world().resource::<Controls>().sent_span;
        let sign = if side == StrafeSide::Left { -1.0 } else { 1.0 };
        assert!((delta.dot(facing) - 6.0 * span).abs() < 0.001);
        assert!((delta.dot(facing.cross(Vec3::Y)) - sign * 3.0 * span).abs() < 0.001);
    }

    #[test]
    fn native_diagonal_keys_start_together_or_join_forward_on_the_first_sample() {
        for (key, side) in [
            (KeyCode::KeyA, StrafeSide::Left),
            (KeyCode::KeyD, StrafeSide::Right),
        ] {
            for already_forward in [false, true] {
                let (mut app, receiver, _, player) = keyboard_mouse_look_app();
                crate::camera::testing::button(&mut app, MouseButton::Right, true);
                native_movement_key(&mut app, KeyCode::KeyW, true);
                let from = if already_forward {
                    app.update();
                    let ClientCommand::Move(forward) = receiver.try_recv().unwrap() else {
                        panic!("expected forward movement");
                    };
                    assert_eq!(forward.mode, MovementMode::Forward);
                    native_movement_key(&mut app, key, true);
                    app.update();
                    assert!(receiver.try_recv().is_err());
                    accept(&mut app, &forward);
                    forward.position
                } else {
                    native_movement_key(&mut app, key, true);
                    world_position([0.0, 3.0, 0.0], 0.0)
                };
                app.update();
                let ClientCommand::Move(chord) = receiver.try_recv().unwrap() else {
                    panic!("expected a diagonal on the first eligible sample");
                };
                assert_eq!(chord.heading, HeadingUpdate::Rate);
                assert_diagonal_step(&app, from, &chord, side);

                accept(&mut app, &chord);
                native_movement_key(&mut app, key, false);
                app.update();
                let ClientCommand::Move(released) = receiver.try_recv().unwrap() else {
                    panic!("expected forward after strafe release");
                };
                assert_eq!(released.mode, MovementMode::Forward);
                assert!((released.position.x - chord.position.x).abs() < 0.001);
                assert!(released.position.y > chord.position.y);
                assert!(
                    app.world()
                        .get::<Transform>(player)
                        .unwrap()
                        .translation
                        .abs_diff_eq(
                            Vec3::from_array(eq_client_core::render_position(chord.position)),
                            0.001,
                        )
                );
            }
        }
    }

    #[test]
    fn settled_mouse_yaw_and_physical_diagonal_share_the_first_movement_snapshot() {
        use eq_network_game::movement::{MotionSession, UpdateGate};
        for (key, side) in [
            (KeyCode::KeyA, StrafeSide::Left),
            (KeyCode::KeyD, StrafeSide::Right),
        ] {
            let (mut app, receiver, camera, player) = keyboard_mouse_look_app();
            crate::camera::testing::button(&mut app, MouseButton::Right, true);
            native_movement_key(&mut app, KeyCode::KeyW, true);
            native_movement_key(&mut app, key, true);
            for pixels in [2.0, 3.0, 4.0] {
                app.world_mut().resource_mut::<Controls>().last_accepted = Instant::now();
                mouse_drag(&mut app, pixels, 0.0);
                app.update();
                assert!(receiver.try_recv().is_err());
            }
            let pending = app.world().resource::<Controls>().mouse_heading.unwrap();
            assert!(arc(0.0, pending) < -1.0);
            assert_attached_heading(&app, camera, player, pending);

            // RMB is still held, but this frame has zero yaw. The chord and
            // the pending facing use one sample and one displacement budget.
            app.world_mut().resource_mut::<Controls>().last_accepted = Instant::now()
                .checked_sub(Duration::from_millis(100))
                .unwrap();
            app.update();
            assert_eq!(app.world().resource::<MouseAim>().horizontal, Some(0.0));
            let ClientCommand::Move(mut chord) = receiver.try_recv().unwrap() else {
                panic!("expected a settled snapshot and diagonal together");
            };
            assert_eq!(chord.heading, HeadingUpdate::Snapshot);
            assert!(arc(pending, chord.position.heading).abs() < 0.001);
            let origin = world_position([0.0, 3.0, 0.0], 0.0);
            assert_diagonal_step(&app, origin, &chord, side);

            let start = Instant::now();
            let spacing = Duration::from_millis(280);
            let mut session = MotionSession::new(11, 7, origin, start)
                .unwrap()
                .with_heading_snapshots(true)
                .with_gate(Some(UpdateGate::Changed { spacing }));
            session
                .calibrate(
                    eq_client_core::MotionCalibration {
                        swim_backward_units_per_second: None,
                        units_per_second: 6.0,
                        diagonal: DiagonalRates {
                            run: Some(3.0),
                            walk: None,
                        },
                        strafe: None,
                        walk: None,
                        backward: None,
                        velocity_scale: 1.0,
                        animation: 1,
                    },
                    start,
                )
                .unwrap();
            chord.created = start + Duration::from_millis(100);
            session
                .send_move_sample(&chord, chord.created, |_| panic!("paced chord sent early"))
                .unwrap();
            assert_eq!(session.position(), chord.position);
            accept(&mut app, &chord);
            native_movement_key(&mut app, key, false);
            native_movement_key(&mut app, KeyCode::KeyW, false);
            app.update();
            assert_attached_heading(&app, camera, player, pending);
            let ClientCommand::Move(stop) = receiver.try_recv().unwrap() else {
                panic!("expected a stop on release");
            };
            assert_eq!(stop.position, chord.position);
            assert!(
                session
                    .tick_sample(start + spacing, |sample| {
                        assert_eq!(sample.position, chord.position);
                        assert_eq!((sample.animation, sample.delta_heading), (1, 0));
                        Ok(())
                    })
                    .unwrap()
            );
        }
    }

    #[test]
    fn fresh_mouse_yaw_suppresses_new_strafe_only_for_its_frame() {
        let (mut app, receiver, _, _) = mouse_look_app();
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.diagonal_rates.run = Some(3.0);
        controls.strafe_speed = Some(3.0);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        crate::camera::testing::button(&mut app, MouseButton::Right, true);
        crate::camera::testing::button(&mut app, MouseButton::Left, true);
        mouse_drag(&mut app, 50.0, 0.0);
        app.update();
        let ClientCommand::Move(turn) = receiver.try_recv().unwrap() else {
            panic!("expected a cardinal forward turn");
        };
        assert_eq!(turn.mode, MovementMode::Forward);
        assert!(arc(0.0, turn.position.heading) < -1.0);
        // The next mouse delta arrives during the previous proposal's wait.
        mouse_drag(&mut app, 2.0, 0.0);
        app.update();
        assert!(receiver.try_recv().is_err());
        accept(&mut app, &turn);

        app.update();
        let ClientCommand::Move(combined) = receiver.try_recv().unwrap() else {
            panic!("expected the held diagonal after the turn");
        };
        assert_eq!(
            combined.mode,
            MovementMode::GroundedDiagonal(GroundedDiagonal {
                along: DiagonalAlong::Run,
                side: StrafeSide::Right,
            })
        );
        assert_eq!(combined.heading, HeadingUpdate::Snapshot);
        assert!(arc(turn.position.heading, combined.position.heading) < -0.5);
    }

    #[test]
    fn released_mid_turn_the_heading_sent_reaches_the_facing_then_stops() {
        // D faces 256, half a turn from 0. Released after one sample, the
        // player stays put and keeps facing 256 while the heading sent turns
        // the rest of the way, a sample each tenth of a second, then stops.
        let (mut app, receiver) = app();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::KeyD);
        app.update();
        let ClientCommand::Move(pressed) = receiver.try_recv().unwrap() else {
            panic!("expected movement")
        };
        accept(&mut app, &pressed);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        let mut heading = pressed.position.heading;
        let mut samples = 0;
        loop {
            app.update();
            let Ok(ClientCommand::Move(request)) = receiver.try_recv() else {
                break;
            };
            samples += 1;
            assert!(samples < 20, "the heading sent never reached the facing");
            let turned = arc(heading, request.position.heading);
            assert!(turned.abs() <= TURN_RATE * MAX_CYCLE, "turned {turned}");
            assert!((request.position.x - pressed.position.x).abs() < 0.0001);
            assert!((request.position.y - pressed.position.y).abs() < 0.0001);
            heading = request.position.heading;
            accept(&mut app, &request);
            let visual = app.world().resource::<Controls>().visual.as_ref().unwrap();
            assert!(visual.to.rotation.angle_between(turned_to(256.0)) < 0.001);
        }
        assert!(arc(heading, 256.0).abs() < 0.01, "stopped at {heading}");
        assert!(app.world().resource::<Controls>().facing.is_none());
        // The 232 units left take at most ten samples, then one says it stopped.
        assert!(samples <= 11, "{samples} samples");
    }

    #[test]
    fn a_new_way_mid_move_shows_at_once_while_the_heading_sent_keeps_its_rate() {
        // W faces 384; S, half a turn from it, shows in the next sample while
        // the heading sent keeps turning at the turn keys' rate.
        let (mut app, receiver) = app();
        app.update();
        let ClientCommand::Move(first) = receiver.try_recv().unwrap() else {
            panic!("expected movement")
        };
        accept(&mut app, &first);
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::KeyS);
        app.update();
        let ClientCommand::Move(second) = receiver.try_recv().unwrap() else {
            panic!("expected movement")
        };
        let turned = arc(first.position.heading, second.position.heading);
        assert!(
            turned > 0.0 && turned <= TURN_RATE * MAX_CYCLE,
            "turned {turned}"
        );
        // S walks back the way W came.
        assert!(second.position.x > first.position.x);
        accept(&mut app, &second);
        let controls = app.world().resource::<Controls>();
        assert!(
            controls
                .facing
                .is_some_and(|facing| arc(facing, 128.0).abs() < 0.001)
        );
        let visual = controls.visual.as_ref().unwrap();
        assert!(visual.from.rotation.angle_between(turned_to(128.0)) < 0.001);
    }

    #[test]
    fn eq_networks_movement_session_admits_every_sample() {
        use eq_network_game::movement::MotionSession;
        // The session refuses a sample whose heading turns faster than the
        // wire's turn field holds (`heading_velocity`), and its position with
        // it. Its clock moves on between two samples at least as far as the
        // input's own span did, as it does live.
        let (mut app, receiver) = app();
        let start = Instant::now();
        let mut session =
            MotionSession::new(11, 7, world_position([0.0, 3.0, 0.0], 0.0), start).unwrap();
        session
            .calibrate(
                eq_client_core::MotionCalibration {
                    swim_backward_units_per_second: None,
                    diagonal: eq_client_core::DiagonalRates::default(),
                    strafe: None,
                    walk: None,
                    backward: None,
                    units_per_second: 6.0,
                    velocity_scale: 1.0,
                    animation: 1,
                },
                start,
            )
            .unwrap();
        let mut clock = start;
        let mut waited = app.world().resource::<Controls>().last_accepted;
        // W; half a turn to S; E and the forward arrow while the heading sent
        // still turns; D and A; then released until it reaches the facing.
        let keys = [KeyCode::KeyW; 3]
            .into_iter()
            .chain([KeyCode::KeyS; 2])
            .chain([KeyCode::KeyE; 2])
            .chain([KeyCode::ArrowUp; 2])
            .chain([KeyCode::KeyD; 2])
            .chain([KeyCode::KeyA])
            .map(Some)
            .chain([None; 16]);
        for key in keys {
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            input.reset_all();
            if let Some(key) = key {
                input.press(key);
            }
            app.update();
            let Ok(ClientCommand::Move(mut request)) = receiver.try_recv() else {
                continue;
            };
            clock += waited.elapsed();
            request.created = clock;
            if let Err(error) = session.send_move(&request, clock, |_| Ok(())) {
                panic!("{key:?}: {error}");
            }
            waited = accept(&mut app, &request);
        }
        let heading = session.position().heading;
        assert!(arc(heading, 0.0).abs() < 0.01, "stopped at {heading}");
        assert!(app.world().resource::<Controls>().facing.is_none());
    }

    #[test]
    fn stalled_frames_are_capped_and_only_one_proposal_can_be_in_flight() {
        let (mut app, receiver) = app();
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected movement")
        };
        assert_eq!(request.session_id, 11);
        assert!((request.position.x + 0.6).abs() < 0.0001);
        assert!((request.position.z - 3.0).abs() < 0.0001);
        // Rendering waits for locally accepted motion instead of predicting rejected input.
        let world = app.world_mut();
        let mut players = world.query_filtered::<&Transform, With<Player>>();
        assert_eq!(
            players.single(world).unwrap().translation,
            Vec3::new(0.0, 3.0, 0.0)
        );
        app.update();
        assert!(receiver.try_recv().is_err());
        let aged = age_pending(&mut app);
        app.update();
        assert!(app.world().resource::<Controls>().speed.is_some());
        assert!(app.world().resource::<Controls>().pending.is_some());
        assert!(receiver.try_recv().is_err());
        // A late answer releases only the proposal; the grant was never lost.
        let mut controls = app.world_mut().resource_mut::<Controls>();
        assert!(controls.moved(Some(request.session_id), aged));
        controls.last_accepted = Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
        app.update();
        let ClientCommand::Move(resumed) = receiver.try_recv().unwrap() else {
            panic!("expected resumed movement")
        };
        // Delay does not accumulate distance to catch up.
        assert!((resumed.position.x - request.position.x).abs() < 0.0001);
    }

    #[test]
    fn a_held_key_covers_the_grant_with_each_round_trip_included() {
        // Each answer comes 40 ms after its sample, and the next sample goes
        // when the 100 ms gate after it opens: 140 ms a cycle, which each
        // sample covers whole, so a held key moves at the grant.
        let (mut app, receiver) = app();
        let cycle = Duration::from_millis(140);
        let (mut held, mut moved, mut last_x) = (0.0_f32, 0.0_f32, 0.0_f32);
        // The time the test itself takes between cycles, which the samples
        // cover too.
        let mut extra = 0.0_f32;
        let mut set_at = Instant::now();
        while held < 1.0 {
            app.update();
            if moved > 0.0 {
                extra += set_at.elapsed().as_secs_f32();
            }
            let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
                panic!("expected movement")
            };
            // A fresh start covers a tenth of a second; then whole cycles.
            held += if moved == 0.0 {
                0.1
            } else {
                cycle.as_secs_f32()
            };
            moved += (request.position.x - last_x).abs();
            last_x = request.position.x;
            accept(&mut app, &request);
            set_at = Instant::now();
            app.world_mut().resource_mut::<Controls>().last_sent = set_at.checked_sub(cycle);
        }
        let granted = 6.0 * held;
        assert!(
            (moved - granted).abs() <= granted * 0.01,
            "moved {moved} in {held} s"
        );
        // Never more than the grant over the time that passed.
        assert!(
            moved <= 6.0 * (held + extra) + 0.0001,
            "moved {moved} in {held} s and {extra} s more"
        );
    }

    #[test]
    fn a_player_left_in_the_air_falls_with_no_key_held() {
        // The server put the player 50 units over the floor, as a #goto can.
        let (mut app, receiver) = app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut().resource_mut::<Controls>().airborne = Some(AirborneController::default());
        let mut state = app.world_mut().resource_mut::<online::OnlineState>();
        let mut player = state.world().player().unwrap().clone();
        player.position = world_position([0.0, 53.0, 0.0], 0.0);
        crate::online::testing::news(
            &mut state,
            [eq_client_core::WorldEvent::Entered {
                capabilities: eq_client_core::Capability::ALL.to_vec(),
                choices: Vec::new(),
                session_id: 11,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        crate::online::testing::connect(&mut state, true);
        app.update();
        let Ok(ClientCommand::Move(request)) = receiver.try_recv() else {
            panic!("the player should start to fall")
        };
        assert_eq!(request.mode, MovementMode::Fall);
        assert!(request.position.z < 53.0, "{request:?}");
    }

    #[test]
    fn withdrawn_player_input_clears_taps_and_autorun_but_keeps_falling() {
        let (mut app, receiver) = app();
        let mut state = app.world_mut().resource_mut::<online::OnlineState>();
        let mut player = state.world().player().unwrap().clone();
        player.position = world_position([0.0, 53.0, 0.0], 0.0);
        crate::online::testing::admit(&mut state, 11, player);
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.autorun = true;
        controls.jump = Some(Instant::now());
        controls.grant(eq_client_core::world::MotionGrant {
            diagonal: eq_client_core::DiagonalRates::default(),
            units_per_second: None,
            backward_units_per_second: None,
            walk_units_per_second: None,
            strafe_units_per_second: None,
            strafe_walk_units_per_second: None,
            swim_backward_units_per_second: None,
            falls: true,
            jump_factor: 1.0,
        });
        assert!(!controls.autorun && controls.jump.is_none());
        controls.last_accepted = Instant::now()
            .checked_sub(Duration::from_millis(150))
            .unwrap();
        app.update();
        // A withdrawn grant resets the climb advertisement as well as input.
        assert!(matches!(
            receiver.try_recv(),
            Ok(ClientCommand::ClimbReach { .. })
        ));
        let Ok(ClientCommand::Move(request)) = receiver.try_recv() else {
            panic!("passive fall missing");
        };
        assert_eq!(request.mode, MovementMode::Fall);
        assert!(request.position.x.abs() < f32::EPSILON && request.position.y.abs() < f32::EPSILON);
        assert!(request.position.z < 53.0);
    }

    #[test]
    fn positive_recalibration_keeps_autorun_but_a_real_withdrawal_clears_it() {
        let mut controls = Controls {
            speed: Some(10.0),
            autorun: true,
            ..Default::default()
        };
        let grant = |speed| eq_client_core::world::MotionGrant {
            diagonal: eq_client_core::DiagonalRates::default(),
            units_per_second: speed,
            backward_units_per_second: None,
            walk_units_per_second: None,
            strafe_units_per_second: None,
            strafe_walk_units_per_second: None,
            swim_backward_units_per_second: None,
            falls: true,
            jump_factor: 1.0,
        };
        controls.grant(grant(Some(9.0)));
        assert!(controls.autorun);
        assert_eq!(controls.speed, Some(9.0));
        controls.grant(grant(None));
        assert!(!controls.autorun);
        controls.grant(grant(Some(10.0)));
        assert!(
            !controls.autorun,
            "a regrant cannot revive withdrawn intent"
        );
        controls.autorun = true;
        controls.grant(grant(Some(f32::NAN)));
        assert!(!controls.autorun);
    }

    #[test]
    fn the_session_hears_the_bodys_climb_at_each_admission_and_change() {
        let (mut app, receiver) = app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.airborne = Some(AirborneController::default());
        controls.climb_sent = None;
        app.update();
        let climb = |feet: f32, size: f32| ClientCommand::ClimbReach {
            session_id: 11,
            per_step: eq_client_assets::falls::TITANIUM.climb_per_step(feet, size),
        };
        assert_eq!(receiver.try_recv().ok(), Some(climb(3.0, 6.0)));
        // Once is enough while the body stays the same.
        app.update();
        assert!(receiver.try_recv().is_err());
        // A body that changes size climbs its own reach.
        let mut bodies = app.world_mut().query::<&mut PlayerBody>();
        let mut body = bodies.single_mut(app.world_mut()).unwrap();
        (body.feet_offset, body.size) = (1.875, 3.0);
        app.update();
        assert_eq!(receiver.try_recv().ok(), Some(climb(1.875, 3.0)));
    }

    #[test]
    fn a_player_on_the_floor_with_no_key_held_sends_nothing() {
        let (mut app, receiver) = app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut().resource_mut::<Controls>().airborne = Some(AirborneController::default());
        app.update();
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn a_full_queue_preserves_the_grant_and_retries_after_drain() {
        let (mut app, _) = app();
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(ClientCommand::SelectTarget {
                request_id: 1,
                session_id: 11,
                spawn_id: None,
                expected_pet: None,
            })
            .unwrap();
        app.insert_resource(crate::outbox::Outbox::new(Some(sender)));
        app.update();
        assert!(app.world().resource::<Controls>().speed.is_some());
        assert!(app.world().resource::<Controls>().pending.is_none());
        receiver.try_recv().unwrap();
        app.world_mut().resource_mut::<Controls>().last_accepted =
            Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected retry")
        };
        assert!((request.position.x + 0.6).abs() < 0.0001);
        assert!(app.world().resource::<Controls>().pending.is_some());
    }

    #[test]
    fn a_late_result_does_not_restore_withdrawn_movement() {
        let mut controls = Controls {
            speed: Some(6.0),
            ..default()
        };
        let created = Instant::now();
        controls.wait_for(Some(11), created, PendingKind::Move);
        controls.reset(None);
        assert!(controls.pending.is_some());
        assert!(controls.moved(Some(11), created));
        assert!(controls.speed.is_none());
        controls.send_failed(crate::outbox::Refusal::Ended);
        assert!(controls.speed.is_none());
    }

    #[test]
    fn visual_lag_does_not_change_outgoing_movement() {
        let (mut app, receiver) = app();
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Transform, With<Player>>();
        players.single_mut(world).unwrap().translation = Vec3::new(10.0, 8.0, 10.0);
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected movement")
        };
        assert!((request.position.x + 0.6).abs() < 0.0001);
        assert!(request.position.y.abs() < 0.0001);
        assert!((request.position.z - 3.0).abs() < 0.0001);
    }

    #[test]
    fn disabled_disconnected_and_dead_states_cannot_produce_movement() {
        let (mut app, receiver) = app();
        app.world_mut().resource_mut::<Controls>().reset(None);
        app.update();
        assert!(receiver.try_recv().is_err());
        app.world_mut().resource_mut::<Controls>().reset(Some(6.0));
        crate::online::testing::connect(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            false,
        );
        app.update();
        assert!(app.world().resource::<Controls>().speed.is_none());
        assert!(receiver.try_recv().is_err());
        let mut state = app.world_mut().resource_mut::<online::OnlineState>();
        crate::online::testing::connect(&mut state, true);
        crate::online::testing::news(
            &mut state,
            [eq_client_core::WorldEvent::Death(eq_client_core::Death {
                spawn_id: 7,
                killer_id: 0,
                corpse_id: 0,
                bind_zone_id: 0,
                corpse_name: None,
            })],
        );
        app.world_mut().resource_mut::<Controls>().reset(Some(6.0));
        app.update();
        assert!(app.world().resource::<Controls>().speed.is_none());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn loss_of_focus_sends_stop_without_displacing_the_character() {
        let (mut app, receiver) = app();
        app.world_mut().resource_mut::<Controls>().moving = true;
        let world = app.world_mut();
        let mut windows = world.query::<&mut Window>();
        windows.single_mut(world).unwrap().focused = false;
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected stop")
        };
        assert!((request.position.x).abs() < 0.0001 && request.position.y.abs() < 0.0001);
    }

    #[test]
    fn free_camera_stops_autorun_and_sends_no_character_input() {
        let (mut app, receiver) = app();
        let world = app.world_mut();
        let mut cameras = world.query::<(Entity, &OrbitCamera)>();
        let (entity, orbit) = cameras.single(world).unwrap();
        let pose = crate::camera::orbit_transform(orbit);
        world.entity_mut(entity).insert(pose);
        app.add_systems(Update, crate::camera::toggle_free.before(input));
        app.insert_resource(MouseLook {
            held: true,
            left: false,
        });
        {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.strafe_speed = Some(3.0);
            controls.autorun = true;
            controls.moving = true;
            controls.jump = Some(Instant::now());
            controls.facing = Some(128.0);
        }
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            for key in [
                KeyCode::F9,
                KeyCode::KeyW,
                KeyCode::ArrowUp,
                KeyCode::KeyE,
                KeyCode::Space,
                KeyCode::NumLock,
            ] {
                keys.press(key);
            }
        }
        app.update();
        assert!(
            app.world()
                .get::<crate::camera::FreeCamera>(entity)
                .is_some()
        );
        let ClientCommand::Move(stop) = receiver.try_recv().unwrap() else {
            panic!("expected a stop")
        };
        assert!(
            Vec3::from_array(eq_client_core::render_position(stop.position))
                .abs_diff_eq(Vec3::new(0.0, 3.0, 0.0), 1e-5)
        );
        assert!(stop.position.heading.abs() < 1e-5);
        assert!(receiver.try_recv().is_err());
        assert!(!app.world().resource::<Controls>().autorun);
        assert!(app.world().resource::<Controls>().jump.is_none());
        accept(&mut app, &stop);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        assert!(receiver.try_recv().is_err());
        // With every key released, returning does not resurrect autorun or a tap.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::F9);
        }
        app.update();
        assert!(
            app.world()
                .get::<crate::camera::FreeCamera>(entity)
                .is_none()
        );
        assert!(receiver.try_recv().is_err());
    }

    /// The app of [`app`], standing still with the player a step above its
    /// floor and falling from 100 units up; admitted with this much Safe
    /// Fall, offering these capabilities.
    fn landing(
        safe_fall: Option<u32>,
        capabilities: Vec<eq_client_core::Capability>,
    ) -> (App, mpsc::Receiver<ClientCommand>) {
        use eq_client_core::movement::{CollisionWorld, MotionStep};
        let floor = CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ])
        .unwrap();
        let mut airborne = AirborneController::default();
        let mut feet = Vec3::Y * 100.0;
        loop {
            let mut next = airborne.clone();
            let below = next.step(
                &floor,
                feet,
                climbing(
                    physics(InstalledClient::Titanium, None),
                    InstalledClient::Titanium,
                    &HUMAN,
                ),
                MotionStep {
                    horizontal: Vec3::ZERO,
                    jump: false,
                    seconds: 0.05,
                    height: 6.0,
                },
            );
            if next.take_landing().is_some() {
                break;
            }
            (feet, airborne) = (below, next);
        }
        let (mut app, receiver) = app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut().resource_mut::<Controls>().airborne = Some(airborne);
        let mut state = app.world_mut().resource_mut::<online::OnlineState>();
        let mut player = state.world().player().unwrap().clone();
        player.position = world_position([0.0, feet.y + 3.0, 0.0], 0.0);
        player.skills = safe_fall.map(|skill| {
            let mut skills = vec![0; 75];
            skills[usize::try_from(eq_client_core::skills::SAFE_FALL).unwrap()] = skill;
            skills
        });
        crate::online::testing::news(
            &mut state,
            [eq_client_core::WorldEvent::Entered {
                capabilities,
                choices: Vec::new(),
                session_id: 11,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        crate::online::testing::connect(&mut state, true);
        (app, receiver)
    }

    /// The damage the landing sample reported, after the sample itself.
    fn reported(app: &mut App, receiver: &mpsc::Receiver<ClientCommand>) -> Option<ClientCommand> {
        app.update();
        take_landing_report(receiver)
    }

    fn take_landing_report(receiver: &mpsc::Receiver<ClientCommand>) -> Option<ClientCommand> {
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected the landing sample")
        };
        // On the floor, three units under the player's position.
        assert!((request.position.z - 3.0).abs() < 0.001, "{request:?}");
        receiver.try_recv().ok()
    }

    #[test]
    fn a_landing_reports_its_damage_after_the_sample_that_lands() {
        let all = eq_client_core::Capability::ALL.to_vec();
        // Safe Fall is announced where the player has it.
        for (safe_fall, amount, used) in [
            // 7.855 units a step at touchdown, the read step's from 100
            // units up: trunc(10 × 3.855²).
            (None, 148, false),
            (Some(0), 148, false),
            // The skill takes off all of it, and is still announced.
            (Some(200), 0, true),
            (Some(254), 148, false),
        ] {
            let (mut app, receiver) = landing(safe_fall, all.clone());
            assert_eq!(
                reported(&mut app, &receiver),
                Some(ClientCommand::EnvironmentalDamage {
                    session_id: 11,
                    hazard: Hazard::Falling,
                    amount,
                    safe_fall: used,
                }),
                "{safe_fall:?}"
            );
            // The landing is reported once; the next sample only stops.
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.finish();
            controls.last_accepted = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
            app.update();
            assert!(matches!(receiver.try_recv(), Ok(ClientCommand::Move(_))));
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn landing_during_a_drop_reports_damage_and_keeps_custody_held() {
        use eq_client_core::inventory::{InventorySlot, InventoryUpdate};

        for (updates, uncertain_purse, amount) in [
            (vec![], false, Some(276)),
            (vec![], true, None),
            (
                vec![
                    InventoryUpdate::Set(vec![drop_item(1, 19)]),
                    InventoryUpdate::Remove(InventorySlot::CURSOR),
                ],
                false,
                Some(276),
            ),
            (vec![InventoryUpdate::Invalidated], false, None),
        ] {
            let (mut app, _) = landing(Some(10), eq_client_core::Capability::ALL.to_vec());
            let mut online = app.world_mut().resource_mut::<online::OnlineState>();
            hold_drop(
                &mut online,
                50,
                std::iter::once(InventoryUpdate::Cursor(vec![drop_item(1_500, 18)])).chain(updates),
            );
            if uncertain_purse {
                online::testing::news(
                    &mut online,
                    [eq_client_core::WorldEvent::PurseUncertain { session_id: 11 }],
                );
                assert!(online.world().fall_weight().is_none());
            }
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.grant(eq_client_core::world::MotionGrant {
                units_per_second: None,
                backward_units_per_second: None,
                walk_units_per_second: None,
                strafe_units_per_second: None,
                strafe_walk_units_per_second: None,
                swim_backward_units_per_second: None,
                diagonal: eq_client_core::DiagonalRates::default(),
                falls: true,
                jump_factor: 1.0,
            });
            // Age the new sample window past its pacing gate and let the
            // real update reannounce the climb before the landing sample.
            controls.last_accepted = Instant::now()
                .checked_sub(Duration::from_millis(100))
                .unwrap();
            let (sender, receiver) = mpsc::sync_channel(3);
            app.insert_resource(crate::outbox::Outbox::new(Some(sender)));
            app.update();
            assert!(matches!(
                receiver.try_recv(),
                Ok(ClientCommand::ClimbReach { session_id: 11, .. })
            ));
            assert_eq!(
                take_landing_report(&receiver),
                amount.map(|amount| ClientCommand::EnvironmentalDamage {
                    session_id: 11,
                    hazard: Hazard::Falling,
                    amount,
                    safe_fall: true,
                })
            );
            assert!(receiver.try_recv().is_err());
            let chat = app.world().resource::<crate::chat::ChatState>();
            assert_eq!(
                chat.newest().contains("carried weight is unknown"),
                amount.is_none()
            );
        }
    }

    fn drop_item(weight_tenths: u32, serial: u32) -> eq_client_core::inventory::InventoryItem {
        let mut item = crate::preview::items().remove(0);
        item.slot = eq_client_core::inventory::InventorySlot::CURSOR;
        item.bag_slots = 0;
        item.details.weight_tenths = weight_tenths;
        item.rules.ground_drop_serial = std::num::NonZeroU32::new(serial);
        item
    }

    fn hold_drop(
        online: &mut online::OnlineState,
        weight_tenths: u32,
        updates: impl IntoIterator<Item = eq_client_core::inventory::InventoryUpdate>,
    ) {
        use eq_client_core::{WorldEvent, inventory::InventoryUpdate};
        online::testing::generation(online, eq_client_core::GameDialect::Titanium);
        online::testing::empty_carried_coins(online);
        let item = drop_item(weight_tenths, 17);
        let serial = item.rules.ground_drop_serial.unwrap();
        online::testing::inventory(online, InventoryUpdate::Snapshot(vec![item]));
        let revision = online.world().worker_inventory().revision();
        online::testing::news(
            online,
            [WorldEvent::InventoryAction {
                session_id: 11,
                revision,
                sequence: None,
                update: Some(InventoryUpdate::GroundDropPending(serial)),
                error: None,
            }],
        );
        for update in updates {
            online::testing::inventory(online, update);
        }
        assert!(online.world().worker_inventory().ground_drop_pending());
    }

    #[test]
    fn a_cursor_resync_keeps_a_52_unit_impact_harmless_at_50_weight() {
        use eq_client_core::inventory::{InventorySlot, InventoryUpdate};
        let mut online = online::OnlineState::new(true);
        online::testing::admit(&mut online, 11, online::testing::player(7));
        hold_drop(
            &mut online,
            500,
            [
                InventoryUpdate::Set(vec![drop_item(1, 18)]),
                InventoryUpdate::Remove(InventorySlot::CURSOR),
            ],
        );
        assert!(online.world().movement_weight().is_err());
        let floor = CollisionWorld::new(flat_surface(0.0)).unwrap();
        let mut impact = Landing {
            fall_distance: 10.0,
            impact_speed: 52.0,
        };
        let cost = landing_cost(
            InstalledClient::Titanium,
            &floor,
            Vec3::ZERO,
            impact,
            online.world(),
        )
        .expect("cursor resync retains a load bound");
        assert_eq!(cost, FallCost::default());
        let (sender, receiver) = mpsc::sync_channel(1);
        let outbox = crate::outbox::Outbox::new(Some(sender));
        assert!(report_fall(&outbox, online.world(), &Settings::default(), cost).is_none());
        assert!(receiver.try_recv().is_err());

        online::testing::inventory(&mut online, InventoryUpdate::Invalidated);
        assert!(online.world().fall_weight().is_none());
        let cost = |impact| {
            landing_cost(
                InstalledClient::Titanium,
                &floor,
                Vec3::ZERO,
                impact,
                online.world(),
            )
        };
        assert_eq!(cost(impact), None);
        impact.impact_speed = 50.0;
        assert_eq!(cost(impact), Some(FallCost::default()));
        impact.impact_speed = 260.0;
        assert_eq!(
            cost(impact),
            Some(FallCost {
                damage: 20_000,
                safe_fall: false
            })
        );
    }

    #[test]
    fn speed_recalibration_preserves_the_jump_at_rise_and_apex() {
        use eq_client_core::movement::{MotionStep, PROGRAM_STEP_SECONDS};

        let floor = CollisionWorld::new(flat_surface(0.0)).unwrap();
        let vertical = VerticalPhysics {
            gravity: 100.0,
            jump_speed: 25.0,
            air_factor: 1.0,
            ground_tolerance: 0.0,
            ..physics(InstalledClient::Titanium, None)
        };
        let step = |jump| MotionStep {
            horizontal: Vec3::X,
            jump,
            seconds: PROGRAM_STEP_SECONDS,
            height: 6.0,
        };
        let mut airborne = AirborneController::default();
        let mut feet = airborne.step(&floor, Vec3::ZERO, vertical, step(true));
        for expected_velocity in [20.0_f32, 15.0, 10.0, 5.0, 0.0] {
            let velocity = airborne.velocity();
            assert!((velocity - expected_velocity).abs() < f32::EPSILON);
            let peak = airborne.peak();
            assert!(peak.is_some());
            let mut controls = Controls {
                airborne: Some(airborne.clone()),
                ..Controls::default()
            };
            controls.grant(eq_client_core::world::MotionGrant {
                units_per_second: Some(4.0),
                backward_units_per_second: None,
                walk_units_per_second: None,
                strafe_units_per_second: None,
                strafe_walk_units_per_second: None,
                swim_backward_units_per_second: None,
                diagonal: DiagonalRates::default(),
                falls: true,
                jump_factor: 1.0,
            });
            let preserved = controls.airborne.unwrap();
            assert!((preserved.velocity() - velocity).abs() < f32::EPSILON);
            assert_eq!(preserved.peak(), peak);
            assert_eq!(preserved.drift(), Vec3::ZERO);
            feet = airborne.step(&floor, feet, vertical, step(false));
        }
    }

    #[test]
    fn received_placement_and_death_clear_the_preserved_descent_and_cost() {
        use eq_client_core::{WorldEvent, WorldUpdate, movement::MotionStep};

        for update in [
            WorldEvent::Position {
                spawn_id: 7,
                position: world_position([4.0, 3.0, 0.0], 0.0),
                velocity: [0.0; 3],
                forward: 0.0,
            },
            WorldEvent::Death(eq_client_core::Death {
                spawn_id: 7,
                killer_id: 7,
                corpse_id: 7,
                bind_zone_id: 9,
                corpse_name: None,
            }),
        ] {
            let mut app = crate::testing::app();
            let mut online = online::OnlineState::new(true);
            online::testing::admit(&mut online, 11, online::testing::player(7));
            hold_drop(&mut online, 500, []);
            let (sender, receiver) = mpsc::sync_channel(1);
            app.insert_resource(online)
                .insert_resource(online::Updates(std::sync::Mutex::new(Some(receiver))))
                .init_resource::<Assets<Mesh>>()
                .init_resource::<Assets<StandardMaterial>>()
                .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
                .add_systems(Update, online::receive);
            let floor = CollisionWorld::new(flat_surface(0.0)).unwrap();
            let mut airborne = AirborneController::default();
            let _ = airborne.step(
                &floor,
                Vec3::Y * 100.0,
                physics(InstalledClient::Titanium, None),
                MotionStep {
                    horizontal: Vec3::ZERO,
                    jump: false,
                    seconds: 0.1,
                    height: 6.0,
                },
            );
            assert!(airborne.velocity() < 0.0 && airborne.peak().is_some());
            let peak = airborne.peak();
            let cost = FallCost {
                damage: 42,
                safe_fall: true,
            };
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.airborne = Some(airborne);
            controls.unreported = Some(cost);
            sender
                .send(WorldUpdate::Game(WorldEvent::MotionState {
                    session_id: 11,
                    units_per_second: None,
                    backward_units_per_second: None,
                    walk_units_per_second: None,
                    strafe_units_per_second: None,
                    strafe_walk_units_per_second: None,
                    swim_backward_units_per_second: None,
                    diagonal: DiagonalRates::default(),
                    falls: true,
                    jump_factor: 1.0,
                }))
                .unwrap();
            app.update();
            let controls = app.world().resource::<Controls>();
            let fall = controls.airborne.as_ref().unwrap();
            assert!(fall.velocity() < 0.0 && fall.peak() == peak);
            assert_eq!(controls.unreported, Some(cost));

            sender.send(WorldUpdate::Game(update)).unwrap();
            app.update();
            let controls = app.world().resource::<Controls>();
            assert!(controls.airborne.is_none());
            assert!(controls.unreported.is_none());
        }
    }

    #[test]
    fn a_landings_damage_the_full_queue_refused_goes_before_the_next_sample() {
        let ready = |app: &mut App| {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.finish();
            controls.last_accepted = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        };
        let (mut app, _) = landing(None, eq_client_core::Capability::ALL.to_vec());
        let (sender, full) = mpsc::sync_channel(1);
        app.insert_resource(crate::outbox::Outbox::new(Some(sender)));
        // The landing sample takes the queue's only place, so its damage
        // waits, for as long as the queue stays full.
        app.update();
        ready(&mut app);
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().unreported,
            Some(FallCost {
                damage: 148,
                safe_fall: false
            })
        );
        assert!(matches!(full.try_recv(), Ok(ClientCommand::Move(_))));
        assert!(full.try_recv().is_err());
        // With room again, it goes before the next sample, once.
        let (sender, receiver) = mpsc::sync_channel(3);
        app.insert_resource(crate::outbox::Outbox::new(Some(sender)));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        ready(&mut app);
        app.update();
        assert_eq!(
            receiver.try_recv().ok(),
            Some(ClientCommand::EnvironmentalDamage {
                session_id: 11,
                hazard: Hazard::Falling,
                amount: 148,
                safe_fall: false,
            })
        );
        assert!(matches!(receiver.try_recv(), Ok(ClientCommand::Move(_))));
        assert!(receiver.try_recv().is_err());
        assert_eq!(app.world().resource::<Controls>().unreported, None);
    }

    #[test]
    fn a_session_that_takes_no_damage_from_the_client_is_sent_none() {
        let without = eq_client_core::Capability::ALL
            .into_iter()
            .filter(|capability| *capability != eq_client_core::Capability::EnvironmentalDamage)
            .collect();
        let (mut app, receiver) = landing(None, without);
        assert_eq!(reported(&mut app, &receiver), None);
    }

    #[test]
    fn a_player_who_turns_the_worlds_damage_off_reports_none() {
        let (mut app, receiver) = landing(None, eq_client_core::Capability::ALL.to_vec());
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(Fix::TakeEnvironmentalDamage, false);
        assert_eq!(reported(&mut app, &receiver), None);
    }
}
