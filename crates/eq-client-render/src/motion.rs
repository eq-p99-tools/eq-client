//! Calibrated P99 input; no movement is generated without a session grant.
use super::{
    Collision, OrbitCamera, Player, PlayerBody, camera_relative_direction,
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
    Capability, ClientCommand, MovementMode, MovementRequest, Posture,
    hazards::Hazard,
    movement::{AirborneController, CollisionWorld, Landing, PROVISIONAL_PHYSICS, VerticalPhysics},
    qol::{Fix, Settings},
    world::ClientWorld,
    world_position,
};
use std::time::{Duration, Instant};

#[derive(Resource)]
#[allow(clippy::struct_excessive_bools)] // Independent movement states.
pub(super) struct Controls {
    pub speed: Option<f32>,
    pub backward_speed: Option<f32>,
    pub walk_speed: Option<f32>,
    pub strafe_speed: Option<f32>,
    pub walking: bool,
    /// Running forward by itself: on and off by the autorun key; a forward
    /// or back key or a posture change turns it off, while turning,
    /// strafing, jumping and attacking leave it on (Titanium program read
    /// N6 of 11-01; the Mac-era client's is unread, and other ways the
    /// program sets it, such as following, are not in this client:
    /// recording checklist 6.359).
    autorun: bool,
    waiting: bool,
    pub last_accepted: Instant,
    queued_at: Instant,
    moving: bool,
    /// Seconds of play the latest sample sent covers: the model shows it
    /// over that long once the session accepts it.
    sent_span: f32,
    boundary: BoundaryTracker,
    taps: TapBuffer,
    visual: Option<VisualMotion>,
    /// Vertical momentum, present only when the session accepts falls; without it,
    /// stepping off a ledge stops at the edge.
    pub airborne: Option<eq_client_core::movement::AirborneController>,
    /// Why the movement guard refused the latest sample, until one is sent.
    pub refused: Option<String>,
    /// When a jump was pressed since the last sample; only where falls are simulated.
    pub jump: Option<Instant>,
    /// When the latest jump started; it outlasts a reset, so a new
    /// admission cannot hurry the next.
    last_jump: Option<Instant>,
    /// A jump from standing still crouching first, on a client that does.
    crouch: Option<Crouch>,
    /// The cost of a landing the session's queue had no room to tell; it
    /// goes before the next sample.
    unreported: Option<FallCost>,
    /// Where the player faces while the heading sent still turns toward it;
    /// None once the two match.
    facing: Option<f32>,
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
}

/// How far a jump from standing still has come on a client that crouches
/// before it.
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
            walking: false,
            autorun: false,
            waiting: false,
            last_accepted: Instant::now(),
            queued_at: Instant::now(),
            moving: false,
            sent_span: 0.1,
            boundary: BoundaryTracker::default(),
            taps: TapBuffer::default(),
            visual: None,
            airborne: None,
            refused: None,
            jump: None,
            last_jump: None,
            crouch: None,
            unreported: None,
            facing: None,
            recent: RecentMotion::default(),
            last_sent: None,
            last_answer: None,
            held: None,
            climb_sent: None,
            stun_revision: None,
            stunned: false,
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
        let snap = asked.angle_between(rotation) > limit * 1.01;
        let segment = Segment { to, span, snap };
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
        looking_ahead: bool,
    ) -> MotionInput {
        let focused = focused && !self.stunned;
        let looking_ahead = looking_ahead && !self.stunned;
        // A complete tap can arrive between render frames. Preserve its press edge
        // for this one sample without mutating Bevy's shared held-key state.
        let keys = sample_keys(keys, map);
        let held = map.holding(&keys, scripted);
        let mut intent = movement_input(
            held,
            (focused, looking_ahead, self.autorun && !self.stunned),
            yaw,
            heading,
            self.speed.unwrap_or(0.0),
            self.backward_speed,
        );
        if self.walking && intent.mode == MovementMode::Forward {
            intent.mode = MovementMode::Walk;
            intent.speed = self.walk_speed.unwrap_or(0.0);
        }
        // Pure sidesteps have priority over camera-relative keys, but do not
        // combine with forward/back arrows until diagonal wire behavior is measured.
        let sideways = held.axis(Act::StrafeLeft, Act::StrafeRight);
        if focused && sideways != 0.0 && !held.held(Act::Forward) && !held.held(Act::Back) {
            let radians = eq_client_core::render_heading(heading);
            intent.preserve_facing = true;
            if let Some(speed) = self.strafe_speed {
                intent.mode = MovementMode::Strafe;
                intent.speed = speed;
                let facing = Vec3::new(radians.sin(), 0.0, radians.cos());
                intent.direction = facing.cross(Vec3::Y) * sideways;
            } else {
                intent.mode = MovementMode::Forward;
                intent.direction = Vec3::ZERO;
            }
        }
        intent
    }
    /// Starts motion afresh, as an admission, a zone or a server's
    /// correction does. The walk toggle is the player's and stays as they
    /// left it, as walking is never faster than running (whether the
    /// program keeps it is unread, recording checklist 6.363).
    pub fn reset(&mut self, speed: Option<f32>) {
        *self = Self {
            speed,
            last_sent: self.last_sent,
            last_answer: self.last_answer,
            last_jump: self.last_jump,
            walking: self.walking,
            stun_revision: self.stun_revision,
            ..Self::default()
        };
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
        self.stunned = stunned;
        if stunned || (changed && revision.is_some()) {
            self.taps = TapBuffer::default();
            self.jump = None;
            self.crouch = None;
            self.facing = None;
        }
        if changed && revision.is_some() {
            self.moving = false;
        }
        stunned
    }

    /// Stops prediction without revoking the session's calibration. A pending
    /// proposal still owns the movement slot until its result or a reset arrives.
    fn pause_prediction(&mut self) {
        self.moving = false;
        self.visual = None;
        self.taps = TapBuffer::default();
        self.jump = None;
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
        self.reset(grant.units_per_second);
        self.autorun = autorun;
        self.backward_speed = grant.backward_units_per_second;
        self.walk_speed = grant.walk_units_per_second;
        self.strafe_speed = grant.strafe_units_per_second;
        self.airborne = grant
            .falls
            .then(eq_client_core::movement::AirborneController::default);
    }

    pub fn accepted(&mut self) {
        let now = Instant::now();
        self.waiting = false;
        self.last_accepted = now;
        self.last_answer = Some(now);
    }

    /// Whether a sample is out and its answer has not come back.
    pub const fn waiting(&self) -> bool {
        self.waiting
    }
}

/// Longest span one proposal may cover; the session admits up to 0.25 s per sample.
const MAX_CYCLE: f32 = 0.25;

/// How fast the turn keys turn the player, and the heading sent follows the
/// way the player faces, in heading units a second.
const TURN_RATE: f32 = 240.0;

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
    mut controls: ResMut<Controls>,
    players: Query<&PlayerBody, With<Player>>,
    cameras: Query<(&OrbitCamera, Has<crate::camera::FreeCamera>)>,
    (settings, messages, mut chat): (
        Res<crate::ViewerSettings>,
        Res<crate::hud::messages::Messages>,
        ResMut<crate::chat::ChatState>,
    ),
    look: Option<Res<crate::camera::MouseLook>>,
) {
    let client = settings.0.installed_client;
    if !online.enabled || !online.world().connected() || online.world().is_dead() {
        controls.reset(None);
        controls.held = Some(Held::Withdrawn);
        return;
    }
    let detached = cameras.single().is_ok_and(|(_, free)| free);
    let stunned = controls.observe_stun(online.world());
    if detached {
        controls.autorun = false;
        controls.taps = TapBuffer::default();
        controls.jump = None;
        controls.facing = None;
    }
    if controls.speed.is_none() && controls.airborne.is_none() {
        controls.held = Some(Held::Withdrawn);
        return;
    }
    let focused = keys.focused() && !detached && !stunned && controls.speed.is_some();
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
    // Held until the next sample; jumps rise and fall like falls, so only
    // where the session takes both, and no sooner after the last than the
    // installed client allows.
    if focused
        && keys.pressed(Act::Jump)
        && controls.airborne.is_some()
        && controls.crouch.is_none()
        && online.world().can(Capability::Jumping)
    {
        if jump_too_soon(client, controls.last_jump, now) {
            debug!("Jump refused: too soon after the last");
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
    controls
        .taps
        .observe(&keyboard, &map.movement_keys(), focused, now);
    if controls.waiting {
        if now.duration_since(controls.queued_at) > Duration::from_millis(250) {
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
    let (Ok(body), Ok((camera, _)), Some(world), Some(accepted)) = (
        players.single(),
        cameras.single(),
        collision.0.as_ref(),
        online.world().player(),
    ) else {
        controls.held = Some(Held::NoBody);
        return;
    };
    if let Some(player) = online.world().player() {
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
                controls.waiting = true;
                controls.queued_at = now;
                controls.held = Some(Held::Gate);
            }
            return;
        }
    }
    let current_heading = accepted.position.heading;
    // A script's `face` step turns the player as moving with the
    // camera-relative keys does: the facing it names at once, the heading
    // sent toward it at the turn keys' rate.
    if focused && let Some(heading) = keys.scripted_facing() {
        controls.facing = Some(heading.rem_euclid(512.0));
    }
    let shown = controls.facing.unwrap_or(current_heading);
    let sampled = controls.taps.take(&keyboard);
    let looking_ahead = focused && look.is_some_and(|look| look.held && look.left);
    let intent = controls.intent(
        &sampled,
        map,
        keys.scripted(),
        focused,
        (camera.yaw, shown),
        looking_ahead,
    );
    let MotionInput {
        mode,
        speed,
        direction,
        turn,
        preserve_facing,
    } = intent;
    let physics = climbing(physics(client, online.world().zone_gravity()), client, body);
    // The session's guard counts a grounded climb by the body's own reach a
    // step, where the program's is read; it hears it at each admission and
    // whenever it changes, before any sample that climbs by it.
    if controls.airborne.is_some()
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
    let origin = Vec3::from_array(eq_client_core::render_position(accepted.position));
    let feet = origin - Vec3::Y * body.feet_offset;
    // A jump or fall keeps going after the keys are released, and a player
    // who may fall starts to where no ground is under them, as the server
    // can leave them after a move it makes (a `#goto` into the air).
    let airborne = controls
        .airborne
        .as_ref()
        .is_some_and(|airborne| airborne.velocity() != 0.0 || unsupported(world, feet));
    // So does a heading sent that still turns toward the facing.
    if direction == Vec3::ZERO
        && turn == 0.0
        && !controls.moving
        && !airborne
        && controls.jump.is_none()
        && controls.facing.is_none()
    {
        controls.held = Some(Held::NoSpeed);
        return;
    }
    // The clock runs from the previous sample's request stamp (which is
    // never after its answer came), so each sample covers the time since
    // the last one, its round trip included, and never more than the time
    // between the two stamps; a fresh start covers at most a tenth of a
    // second.
    let since_sent = controls.last_sent.map_or(elapsed, |sent| {
        now.saturating_duration_since(sent).max(elapsed)
    });
    let span = controls.span(since_sent);
    let delta = direction * speed * span;
    let jump = controls.jump.take().is_some();
    let crouched = controls.crouch == Some(Crouch::Over);
    if crouched {
        controls.crouch = None;
    }
    // Where the installed client crouches before a jump from standing still,
    // that jump ducks now and goes once the crouch is over; backing up
    // cancels it. The jump a finished crouch hands on goes as it is.
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
    let (landing, mode, jumped) = match controls.airborne.as_mut() {
        Some(airborne) => {
            let rising = airborne.velocity() > 0.0;
            let landing = fall_step(
                airborne,
                world,
                physics,
                client.falls().map(|_| physics.climb),
                feet,
                delta,
                span,
                body.height,
                jump,
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
    };
    // The server charges the jump's endurance; the arc travels in position updates.
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
    }
    let position = landing + Vec3::Y * body.feet_offset;
    let position = stop_at_zone_line(&online.regions, origin, position);
    // The turn keys turn the facing at their rate, and moving with the
    // camera-relative keys faces the way the player moves at once. The
    // heading sent follows the facing at the turn keys' rate: eq-network's
    // movement session refuses a sample that turns faster than the wire's
    // turn field holds (`heading_velocity`: at most 127 heading units in its
    // longest sample), and the position goes with it.
    let facing = if direction == Vec3::ZERO || preserve_facing {
        (shown + turn * TURN_RATE * span).rem_euclid(512.0)
    } else {
        eq_client_core::world_heading(direction.x.atan2(direction.z))
    };
    let heading = turn_toward(current_heading, facing, TURN_RATE * span);
    trace_proposal(mode, delta, position - origin, heading, facing);
    let position_sent = world_position(position.to_array(), heading);
    let mut created = now;
    let sent = outbox.post(online.world(), |stamp| {
        created = stamp.created;
        ClientCommand::Move(MovementRequest {
            mode,
            session_id: stamp.session_id,
            position: position_sent,
            created: stamp.created,
        })
    });
    if let Err(reason) = sent {
        controls.airborne = previous_airborne;
        controls.send_failed(reason);
        controls.held = Some(Held::Refused);
        return;
    }
    controls.recent.record(now, delta, position - origin);
    // The host's movement session budgets each sample from the previous
    // request's stamp to this one's, so the next span runs from the stamp.
    controls.last_sent = Some(created);
    controls.sent_span = span;
    controls.held = None;
    // A landing's damage follows the sample that lands.
    if let Some(fall) = controls
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
        debug!(
            fall_distance = fall.fall_distance,
            impact_speed = fall.impact_speed,
            carried_weight = online.world().fall_weight(),
            damage = cost.damage,
            safe_fall = cost.safe_fall,
            "Landing"
        );
        say_fall(client, cost, online.world(), &messages, &mut chat);
        controls.unreported = report_fall(&outbox, online.world(), &options.options.qol, cost);
    }
    controls.moving = position.distance_squared(origin) > 0.000_001
        || (heading - current_heading).abs() > f32::EPSILON;
    // Until the heading sent reaches the facing, the model shows the facing.
    controls.facing = (arc(heading, facing).abs() > 0.01).then_some(facing);
    controls.waiting = true;
    controls.queued_at = now;
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
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // At most five slices.
    let slices = (span / 0.05).ceil().clamp(1.0, 5.0) as u8;
    let slice = f32::from(slices);
    let mut position = feet;
    let start = airborne.velocity().max(0.0);
    let mut rising = start;
    for index in 0..slices {
        let mut trial = airborne.clone();
        let next = trial.step(
            world,
            position,
            physics,
            eq_client_core::movement::MotionStep {
                horizontal: delta / slice,
                jump: jump && index == 0,
                seconds: span / slice,
                height,
            },
        );
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
) -> FallCost {
    if collision.in_liquid(feet) {
        return FallCost::default();
    }
    let skill = safe_fall(world);
    match client.falls() {
        Some(falls) => falls.landing_cost(FallLanding {
            impact_speed: landing.impact_speed,
            carried_weight: world.fall_weight(),
            safe_fall: skill,
            safe_fall_ranks: 0,
            zone: eq_client_core::zones::number(world.zone()).unwrap_or(0),
        }),
        None => FallCost {
            damage: collision.landing_damage(feet, landing, skill),
            safe_fall: skill > 0 && collision.landing_damage(feet, landing, 0) > 0,
        },
    }
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
}

/// The accepted samples the model plays back, one after another, each over
/// its own span; never past the newest accepted position.
struct VisualMotion {
    from: Transform,
    to: Transform,
    elapsed: f32,
    duration: f32,
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
    mut controls: ResMut<Controls>,
    mut players: Query<&mut Transform, With<Player>>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    let Some(visual) = controls.visual.as_mut() else {
        return;
    };
    let Ok(mut transform) = players.single_mut() else {
        return;
    };
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
    for mut camera in &mut cameras {
        camera.focus = transform.translation;
    }
    if visual.finished() {
        controls.visual = None;
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
}

/// Resolves character-relative arrows and camera-relative WASD without mixing
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
    let turn = if focused {
        held.axis(Act::TurnRight, Act::TurnLeft)
    } else {
        0.0
    };
    MotionInput {
        mode,
        speed,
        direction,
        turn,
        preserve_facing: backing || forward,
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
mod tests {

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
        // The same but for the climb: the floor sweep starts at the origin
        // on Titanium, 0.5 over it on the Mac-era client, radius size × 0.1
        // against size / 4 (read 6.344).
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
        assert!((ti.climb - 3.15).abs() < 0.001, "{}", ti.climb);
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
    fn a_titanium_jump_from_standing_still_goes_at_once() {
        let (mut app, receiver) = standing_jump(InstalledClient::Titanium);
        app.update();
        assert_eq!(stances_and_jump(&receiver), (Vec::new(), true));
        let controls = app.world().resource::<Controls>();
        assert!(controls.airborne.as_ref().unwrap().velocity() > 0.0);
        assert_eq!(controls.crouch, None);
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
        let cost = |client| landing_cost(client, &floor, Vec3::ZERO, landing, &world);
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
            let intent = controls.intent(&keys, &map, &[], focused, (0.0, 0.0), false);
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
        let intent = controls.intent(&keys, &map, &[], true, (0.0, 0.0), false);
        assert!(intent.direction != Vec3::ZERO && intent.turn != 0.0);
        // Back wins while held.
        keys.press(KeyCode::ArrowDown);
        let intent = controls.intent(&keys, &map, &[], true, (0.0, 0.0), false);
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
                .intent(&keys, &KeyMap::default(), &[], true, (0.0, 0.0), false)
                .direction,
            Vec3::ZERO
        );
        assert_eq!(
            controls
                .intent(&keys, &KeyMap::default(), &[], false, (0.0, 0.0), false)
                .direction,
            Vec3::ZERO
        );
        keys.clear();
        assert_eq!(
            controls
                .intent(&keys, &KeyMap::default(), &[], true, (0.0, 0.0), false)
                .direction,
            Vec3::ZERO
        );
        keys.press(KeyCode::ArrowDown);
        keys.release(KeyCode::ArrowDown);
        assert_eq!(
            controls
                .intent(&keys, &KeyMap::default(), &[], true, (0.0, 0.0), false)
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

    fn app() -> (App, mpsc::Receiver<ClientCommand>) {
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
        let floor = eq_client_core::movement::CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ])
        .unwrap();
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
                // The session has heard this body's Titanium climb, 3 − 0.6
                // a step, so the samples are the only commands these tests
                // see ([`the_session_hears_the_bodys_climb_at_each_admission_and_change`]).
                climb_sent: Some((
                    11,
                    eq_client_assets::falls::TITANIUM.climb_per_step(3.0, 6.0),
                )),
                ..Controls::default()
            })
            .add_systems(Update, input);
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
                    .intent(&keys, &map, &[], true, (0.0, heading), false)
                    .direction,
                Vec3::ZERO
            );
            controls.strafe_speed = Some(3.0);
            let right = controls.intent(&keys, &map, &[], true, (0.0, heading), false);
            assert_eq!(right.mode, MovementMode::Strafe);
            assert!(right.preserve_facing);
            assert_eq!(right.speed, 3.0);
            assert!(right.direction.dot(facing).abs() < 0.0001);
            assert!(right.direction.dot(facing.cross(Vec3::Y)) > 0.999);
            keys.reset_all();
            keys.press(KeyCode::ArrowLeft);
            let left = controls.intent(&keys, &map, &[], true, (0.0, heading), false);
            assert!((left.direction + right.direction).length() < 0.0001);
            assert_eq!(
                controls
                    .intent(&keys, &map, &[], false, (0.0, heading), false)
                    .direction,
                Vec3::ZERO
            );
            keys.press(KeyCode::ArrowRight);
            assert_eq!(
                controls
                    .intent(&keys, &map, &[], true, (0.0, heading), false)
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
                .intent(&keys, &titanium, scripted, focused, (0.0, heading), false)
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
        // A server's correction, as after a gm goto, starts motion afresh
        // but leaves the toggle as the player left it (6.363), so the next
        // grant walks again.
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.reset(None);
        assert!(controls.walking);
        assert!(controls.walk_speed.is_none());
        controls.grant(eq_client_core::world::MotionGrant {
            units_per_second: Some(6.0),
            backward_units_per_second: None,
            walk_units_per_second: Some(2.0),
            strafe_units_per_second: None,
            falls: false,
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
    fn turning_in_place_changes_heading_without_displacing_the_player() {
        let (mut app, receiver) = app();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::KeyE);
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected turn")
        };
        assert!((request.position.heading - 488.0).abs() < 0.001);
        assert!(request.position.x.abs() < 0.001 && request.position.y.abs() < 0.001);
        assert!((turn_toward(508.0, 4.0, 3.0) - 511.0).abs() < 0.001);
        assert!((turn_toward(4.0, 508.0, 3.0) - 1.0).abs() < 0.001);
    }

    /// The model's rotation when it faces this heading.
    fn turned_to(heading: f32) -> Quat {
        Quat::from_rotation_y(eq_client_core::render_heading(heading))
    }

    /// Takes the sample as the session would and draws it, then lets the next
    /// sample go after a tenth of a second; returns when that wait began.
    fn accept(app: &mut App, request: &MovementRequest) -> Instant {
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            [eq_client_core::WorldEvent::MotionSent {
                session_id: request.session_id,
                position: request.position,
                refused: None,
            }],
        );
        let world = app.world_mut();
        let mut players = world.query_filtered::<&Transform, With<Player>>();
        let drawn = *players.single(world).unwrap();
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.accepted();
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
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.queued_at = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        app.update();
        assert!(app.world().resource::<Controls>().speed.is_some());
        assert!(app.world().resource::<Controls>().waiting);
        assert!(receiver.try_recv().is_err());
        // A late answer releases only the proposal; the grant was never lost.
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.accepted();
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
            units_per_second: None,
            backward_units_per_second: None,
            walk_units_per_second: None,
            strafe_units_per_second: None,
            falls: true,
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
            units_per_second: speed,
            backward_units_per_second: None,
            walk_units_per_second: None,
            strafe_units_per_second: None,
            falls: true,
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
            })
            .unwrap();
        app.insert_resource(crate::outbox::Outbox::new(Some(sender)));
        app.update();
        assert!(app.world().resource::<Controls>().speed.is_some());
        assert!(!app.world().resource::<Controls>().waiting);
        receiver.try_recv().unwrap();
        app.world_mut().resource_mut::<Controls>().last_accepted =
            Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected retry")
        };
        assert!((request.position.x + 0.6).abs() < 0.0001);
        assert!(app.world().resource::<Controls>().waiting);
    }

    #[test]
    fn admission_reset_cannot_be_undone_by_a_late_result() {
        let mut controls = Controls {
            speed: Some(6.0),
            waiting: true,
            ..default()
        };
        controls.reset(None);
        controls.accepted();
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
        {
            let mut controls = app.world_mut().resource_mut::<Controls>();
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
            }],
        );
        crate::online::testing::connect(&mut state, true);
        (app, receiver)
    }

    /// The damage the landing sample reported, after the sample itself.
    fn reported(app: &mut App, receiver: &mpsc::Receiver<ClientCommand>) -> Option<ClientCommand> {
        app.update();
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
            controls.accepted();
            controls.last_accepted = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
            app.update();
            assert!(matches!(receiver.try_recv(), Ok(ClientCommand::Move(_))));
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn a_landings_damage_the_full_queue_refused_goes_before_the_next_sample() {
        let ready = |app: &mut App| {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.accepted();
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
