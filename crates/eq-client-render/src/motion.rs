//! Calibrated P99 input; no movement is generated without a session grant.
use super::{
    Collision, OrbitCamera, Player, PlayerBody, axis, camera_relative_direction, online, target,
};
use bevy::{prelude::*, window::PrimaryWindow};
use eq_client_assets::regions::ZoneLine;
use eq_client_core::{ClientCommand, MovementMode, MovementRequest, world_position};
use std::time::{Duration, Instant};

#[derive(Resource)]
pub(super) struct Controls {
    pub speed: Option<f32>,
    pub backward_speed: Option<f32>,
    pub walk_speed: Option<f32>,
    pub strafe_speed: Option<f32>,
    pub walking: bool,
    pub waiting: bool,
    pub last_accepted: Instant,
    queued_at: Instant,
    moving: bool,
    /// Seconds between the last two accepted samples while moving continuously.
    cycle: f32,
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
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            speed: None,
            backward_speed: None,
            walk_speed: None,
            strafe_speed: None,
            walking: false,
            waiting: false,
            last_accepted: Instant::now(),
            queued_at: Instant::now(),
            moving: false,
            cycle: 0.1,
            boundary: BoundaryTracker::default(),
            taps: TapBuffer::default(),
            visual: None,
            airborne: None,
            refused: None,
            jump: None,
        }
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
                position,
                heading,
            } => eq_client_core::ZoneLineDestination::Absolute {
                zone_id,
                position: eq_client_core::WorldPosition {
                    x: position[1],
                    y: position[0],
                    z: position[2],
                    heading,
                },
            },
        })
    }
}
impl Controls {
    /// Smooths a locally accepted sample without changing the simulation position.
    pub fn display_sample(&mut self, from: Transform, position: eq_client_core::WorldPosition) {
        self.visual = Some(VisualMotion {
            from,
            to: Transform::from_translation(Vec3::from_array(eq_client_core::render_position(
                position,
            )))
            .with_rotation(Quat::from_rotation_y(eq_client_core::render_heading(
                position.heading,
            ))),
            elapsed: 0.0,
            duration: self.cycle,
        });
    }
    /// Selects a granted locomotion mode without modifying its timing budget.
    fn intent(
        &self,
        keys: &ButtonInput<KeyCode>,
        focused: bool,
        yaw: f32,
        heading: f32,
    ) -> MotionInput {
        // A complete tap can arrive between render frames. Preserve its press edge
        // for this one sample without mutating Bevy's shared held-key state.
        let keys = sample_keys(keys);
        let keys = &keys;
        let mut intent = movement_input(
            keys,
            focused,
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
        let sideways = axis(keys, KeyCode::ArrowLeft, KeyCode::ArrowRight);
        if focused && sideways != 0.0 && !keys.any_pressed([KeyCode::ArrowUp, KeyCode::ArrowDown]) {
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
    pub fn reset(&mut self, speed: Option<f32>) {
        *self = Self {
            speed,
            ..Self::default()
        };
    }
    /// Continuous motion covers the whole cycle, including the proposal round trip;
    /// a fresh start covers at most 0.1 s so an idle gap never becomes a jump.
    fn span(&self, elapsed: Duration) -> f32 {
        elapsed
            .as_secs_f32()
            .min(if self.moving { MAX_CYCLE } else { 0.1 })
    }
    pub fn accepted(&mut self) {
        let now = Instant::now();
        // Spread each sample over the whole proposal cycle so continuous motion
        // does not pause while the next proposal is in flight.
        self.cycle = if self.moving {
            now.saturating_duration_since(self.last_accepted)
                .as_secs_f32()
                .clamp(0.05, MAX_CYCLE)
        } else {
            0.1
        };
        self.waiting = false;
        self.last_accepted = now;
    }
}

/// Longest span one proposal may cover; the session admits up to 0.25 s per sample.
const MAX_CYCLE: f32 = 0.25;

/// Movement bindings whose press edges may arrive between network updates.
const MOTION_KEYS: [KeyCode; 10] = [
    KeyCode::KeyW,
    KeyCode::KeyA,
    KeyCode::KeyS,
    KeyCode::KeyD,
    KeyCode::KeyQ,
    KeyCode::KeyE,
    KeyCode::ArrowUp,
    KeyCode::ArrowDown,
    KeyCode::ArrowLeft,
    KeyCode::ArrowRight,
];

/// Holds short press edges across the send throttle, never across focus loss or expiry.
#[derive(Default)]
struct TapBuffer(Vec<(KeyCode, Instant)>);

impl TapBuffer {
    fn observe(&mut self, keys: &ButtonInput<KeyCode>, focused: bool, now: Instant) {
        if !focused {
            self.0.clear();
            return;
        }
        self.0
            .retain(|(_, time)| now.saturating_duration_since(*time) < Duration::from_millis(250));
        for key in MOTION_KEYS {
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
fn sample_keys(keys: &ButtonInput<KeyCode>) -> ButtonInput<KeyCode> {
    let mut sampled = keys.clone();
    for key in MOTION_KEYS {
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
    keyboard: Res<ButtonInput<KeyCode>>,
    navigation: Res<super::navigation::NavigationKeys>,
    online: Res<online::OnlineState>,
    chat: Res<super::chat::ChatState>,
    collision: Res<Collision>,
    sender: Res<target::CommandsToServer>,
    mut controls: ResMut<Controls>,
    players: Query<&PlayerBody, With<Player>>,
    cameras: Query<&OrbitCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if !online.enabled || !online.connected || online.death.is_some() {
        controls.reset(None);
        return;
    }
    let Some(_) = controls.speed else {
        return;
    };
    let Some(session_id) = online.session_id else {
        return;
    };
    let focused = windows.single().is_ok_and(|window| window.focused) && !chat.composing;
    if focused && keyboard.just_pressed(KeyCode::Insert) && controls.walk_speed.is_some() {
        controls.walking = !controls.walking;
    }
    // Held until the next sample; jumps rise and fall like falls, so only then.
    if focused && keyboard.just_pressed(KeyCode::Space) && controls.airborne.is_some() {
        controls.jump = Some(Instant::now());
    }
    let now = Instant::now();
    let keyboard = navigation.sample(&keyboard);
    controls.taps.observe(&keyboard, focused, now);
    if controls.waiting {
        if now.duration_since(controls.queued_at) > Duration::from_millis(250) {
            controls.reset(None);
        }
        return;
    }
    let elapsed = now.duration_since(controls.last_accepted);
    if elapsed < Duration::from_millis(100) {
        return;
    }
    let (Ok(body), Ok(camera), Some(world), Some(sender), Some(accepted)) = (
        players.single(),
        cameras.single(),
        collision.0.as_ref(),
        sender.0.as_ref(),
        online.player.as_ref(),
    ) else {
        return;
    };
    if let Some(player) = &online.player {
        let position = player.position;
        let boundary = online
            .regions
            .zone_line_at([position.y, position.x, position.z]);
        if let Some(destination) = controls.boundary.observe(boundary) {
            if sender
                .try_send(ClientCommand::CrossZoneLine {
                    session_id,
                    destination,
                    position,
                    created: now,
                })
                .is_err()
            {
                controls.reset(None);
            } else {
                controls.waiting = true;
                controls.queued_at = now;
            }
            return;
        }
    }
    let current_heading = accepted.position.heading;
    let sampled = controls.taps.take(&keyboard);
    let intent = controls.intent(&sampled, focused, camera.yaw, current_heading);
    let MotionInput {
        mode,
        speed,
        direction,
        turn,
        preserve_facing,
    } = intent;
    // A jump or fall keeps going after the keys are released.
    let airborne = controls
        .airborne
        .as_ref()
        .is_some_and(|airborne| airborne.velocity() != 0.0);
    if direction == Vec3::ZERO
        && turn == 0.0
        && !controls.moving
        && !airborne
        && controls.jump.is_none()
    {
        return;
    }
    let origin = Vec3::from_array(eq_client_core::render_position(accepted.position));
    let feet = origin - Vec3::Y * body.feet_offset;
    // Clock starts at receipt of the last locally accepted sample, not render delta.
    let span = controls.span(elapsed);
    let delta = direction * speed * span;
    let jump = controls.jump.take().is_some();
    let (landing, mode, jumped) = match controls.airborne.as_mut() {
        Some(airborne) => {
            let rising = airborne.velocity() > 0.0;
            let landing = fall_step(airborne, world, feet, delta, span, body.height, jump);
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
        && sender
            .try_send(ClientCommand::Jump {
                session_id,
                created: now,
            })
            .is_err()
    {
        controls.reset(None);
        return;
    }
    let position = landing + Vec3::Y * body.feet_offset;
    let position = stop_at_zone_line(&online.regions, origin, position);
    trace_proposal(mode, delta, position - origin);
    let turn_limit = 240.0 * span;
    let heading = if direction == Vec3::ZERO || preserve_facing {
        (current_heading + turn * turn_limit).rem_euclid(512.0)
    } else {
        let desired = eq_client_core::world_heading(direction.x.atan2(direction.z));
        turn_toward(current_heading, desired, turn_limit)
    };
    let request = MovementRequest {
        mode,
        session_id,
        position: world_position(position.to_array(), heading),
        created: now,
    };
    if sender.try_send(ClientCommand::Move(request)).is_err() {
        controls.reset(None);
        return;
    }
    controls.moving = position.distance_squared(origin) > 0.000_001
        || (heading - current_heading).abs() > f32::EPSILON;
    controls.waiting = true;
    controls.queued_at = now;
}

/// Walks like a grounded step but keeps going past ledges and falls under gravity,
/// in the 50 ms slices the airborne controller integrates; `jump` starts a jump
/// in the first slice when the character stands on the ground. Each slice may
/// climb, so the sample stops before its total rise passes the one riser the
/// server's movement guard accepts; a larger climb takes several accepted samples
/// instead of one that would be refused and retried forever.
pub(super) fn fall_step(
    airborne: &mut eq_client_core::movement::AirborneController,
    world: &eq_client_core::movement::CollisionWorld,
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
    for index in 0..slices {
        let mut trial = airborne.clone();
        let next = trial.step(
            world,
            position,
            eq_client_core::movement::PROVISIONAL_PHYSICS,
            eq_client_core::movement::MotionStep {
                horizontal: delta / slice,
                jump: jump && index == 0,
                seconds: span / slice,
                height,
            },
        );
        if next.y - feet.y > eq_client_core::movement::MAX_GROUNDED_STEP {
            break;
        }
        *airborne = trial;
        position = next;
    }
    position
}

/// A bounded visual transition; never extrapolates past the accepted destination.
struct VisualMotion {
    from: Transform,
    to: Transform,
    elapsed: f32,
    duration: f32,
}

impl VisualMotion {
    fn advance(&mut self, seconds: f32, transform: &mut Transform) {
        self.elapsed += seconds;
        let fraction = (self.elapsed / self.duration.max(0.01)).clamp(0.0, 1.0);
        transform.translation = self.from.translation.lerp(self.to.translation, fraction);
        transform.rotation = self.from.rotation.slerp(self.to.rotation, fraction);
    }
}

/// Advances the player and follow camera every rendered frame between network samples.
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
    visual.advance(time.delta_secs(), &mut transform);
    for mut camera in &mut cameras {
        camera.focus = transform.translation;
    }
    if visual.elapsed >= visual.duration {
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
        .zone_line_entry([start.x, start.z, start.y], [end.x, end.z, end.y])
        .map_or(end, |point| Vec3::new(point[0], point[2], point[1]))
}

/// Logs displacement magnitudes without recording the character's world coordinates.
fn trace_proposal(mode: MovementMode, requested: Vec3, resolved: Vec3) {
    debug!(
        ?mode,
        requested_distance = requested.length(),
        resolved_distance = resolved.length(),
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

/// Resolves character-relative arrows and camera-relative WASD without mixing modes.
fn movement_input(
    keyboard: &ButtonInput<KeyCode>,
    focused: bool,
    camera_yaw: f32,
    current_heading: f32,
    speed: f32,
    backward_speed: Option<f32>,
) -> MotionInput {
    let backing =
        focused && keyboard.pressed(KeyCode::ArrowDown) && !keyboard.pressed(KeyCode::ArrowUp);
    let forward =
        focused && keyboard.pressed(KeyCode::ArrowUp) && !keyboard.pressed(KeyCode::ArrowDown);
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
                axis(keyboard, KeyCode::KeyA, KeyCode::KeyD),
                axis(keyboard, KeyCode::KeyS, KeyCode::KeyW),
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
        axis(keyboard, KeyCode::KeyE, KeyCode::KeyQ)
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
    let difference = (desired - current + 256.0).rem_euclid(512.0) - 256.0;
    (current + difference.clamp(-limit, limit)).rem_euclid(512.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn visual_motion_interpolates_without_extrapolation_and_preserves_scale() {
        let mut visual = super::VisualMotion {
            from: bevy::prelude::Transform::IDENTITY,
            to: bevy::prelude::Transform::from_xyz(10.0, 0.0, 0.0),
            elapsed: 0.0,
            duration: 0.1,
        };
        let mut transform = bevy::prelude::Transform::from_scale(bevy::prelude::Vec3::splat(2.0));
        visual.advance(0.05, &mut transform);
        assert!((transform.translation.x - 5.0).abs() < 0.001);
        visual.advance(1.0, &mut transform);
        assert_eq!(transform.translation, visual.to.translation);
        assert_eq!(transform.scale, bevy::prelude::Vec3::splat(2.0));
    }

    #[test]
    fn continuous_samples_are_displayed_over_their_whole_cycle() {
        let mut controls = super::Controls::default();
        controls.accepted();
        assert!((controls.cycle - 0.1).abs() < f32::EPSILON);
        controls.moving = true;
        controls.last_accepted = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(150))
            .unwrap();
        controls.accepted();
        assert!((controls.cycle - 0.15).abs() < 0.02, "{}", controls.cycle);
        controls.moving = true;
        controls.last_accepted = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(3))
            .unwrap();
        controls.accepted();
        assert!((controls.cycle - super::MAX_CYCLE).abs() < f32::EPSILON);
        controls.display_sample(
            bevy::prelude::Transform::IDENTITY,
            eq_client_core::world_position([10.0, 0.0, 0.0], 0.0),
        );
        let visual = controls.visual.as_ref().unwrap();
        assert!((visual.duration - super::MAX_CYCLE).abs() < f32::EPSILON);
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
            start,
            Vec3::X * 3.0,
            0.25,
            6.0,
            false,
        );
        assert!(end.x > start.x, "{end:?}");
        assert!(end.y - start.y <= MAX_GROUNDED_STEP, "{end:?}");
        let next = super::fall_step(&mut airborne, &world, end, Vec3::X * 3.0, 0.25, 6.0, false);
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
            Vec3::ZERO,
            Vec3::ZERO,
            0.15,
            6.0,
            false,
        );
        assert!(still.y.abs() < 0.001 && airborne.velocity() == 0.0);
        let rising = super::fall_step(&mut airborne, &world, still, Vec3::X, 0.15, 6.0, true);
        assert!(rising.y > 0.5 && rising.x > 0.9, "{rising:?}");
        assert!(airborne.velocity() > 0.0);
        // Released keys: the arc continues and lands back on the ground.
        let mut feet = rising;
        for _ in 0..10 {
            feet = super::fall_step(&mut airborne, &world, feet, Vec3::ZERO, 0.15, 6.0, false);
        }
        assert!(
            feet.y.abs() < 0.001 && airborne.velocity() == 0.0,
            "{feet:?}"
        );
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
    fn throttled_taps_survive_until_one_sample_but_not_focus_loss_or_expiry() {
        let start = Instant::now();
        let mut keys = ButtonInput::default();
        let mut taps = TapBuffer::default();
        keys.press(KeyCode::ArrowDown);
        keys.release(KeyCode::ArrowDown);
        taps.observe(&keys, true, start);
        keys.clear();
        taps.observe(&keys, true, start + Duration::from_millis(100));
        assert!(taps.take(&keys).pressed(KeyCode::ArrowDown));
        assert!(!taps.take(&keys).pressed(KeyCode::ArrowDown));

        keys.press(KeyCode::ArrowDown);
        keys.release(KeyCode::ArrowDown);
        taps.observe(&keys, true, start);
        keys.clear();
        taps.observe(&keys, false, start + Duration::from_millis(1));
        assert!(!taps.take(&keys).pressed(KeyCode::ArrowDown));

        keys.press(KeyCode::ArrowDown);
        keys.release(KeyCode::ArrowDown);
        taps.observe(&keys, true, start);
        keys.clear();
        taps.observe(&keys, true, start + Duration::from_millis(250));
        assert!(!taps.take(&keys).pressed(KeyCode::ArrowDown));
    }

    use super::*;
    use std::sync::mpsc;

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
        assert_ne!(controls.intent(&keys, true, 0.0, 0.0).direction, Vec3::ZERO);
        assert_eq!(
            controls.intent(&keys, false, 0.0, 0.0).direction,
            Vec3::ZERO
        );
        keys.clear();
        assert_eq!(controls.intent(&keys, true, 0.0, 0.0).direction, Vec3::ZERO);
        keys.press(KeyCode::ArrowDown);
        keys.release(KeyCode::ArrowDown);
        assert_eq!(
            controls.intent(&keys, true, 0.0, 0.0).mode,
            MovementMode::Backward
        );
        assert!(!keys.pressed(KeyCode::ArrowDown));
    }

    #[test]
    fn boundary_crossing_is_edge_triggered_and_absolute_axes_are_converted() {
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
                    x: 34.0,
                    y: 12.0,
                    z: 56.0,
                    heading: 64.0
                }
            })
        );
    }

    fn app() -> (App, mpsc::Receiver<ClientCommand>) {
        let (sender, receiver) = mpsc::sync_channel(2);
        let mut app = App::new();
        let mut state = online::OnlineState::new(true);
        state.connected = true;
        state.session_id = Some(11);
        state.player = Some(eq_client_core::PlayerState {
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
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
        });
        let floor = eq_client_core::movement::CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ])
        .unwrap();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        app.insert_resource(state)
            .init_resource::<crate::chat::ChatState>()
            .init_resource::<crate::navigation::NavigationKeys>()
            .insert_resource(Collision(Some(floor)))
            .insert_resource(target::CommandsToServer(Some(sender)))
            .insert_resource(keys)
            .insert_resource(Controls {
                speed: Some(6.0),
                last_accepted: Instant::now().checked_sub(Duration::from_secs(2)).unwrap(),
                ..Controls::default()
            })
            .add_systems(Update, input);
        app.world_mut().spawn((
            Player,
            Transform::from_xyz(0.0, 3.0, 0.0),
            PlayerBody {
                feet_offset: 3.0,
                height: 6.0,
            },
        ));
        app.world_mut().spawn(OrbitCamera {
            focus: Vec3::ZERO,
            radius: 30.0,
            yaw: 0.0,
            pitch: 1.0,
        });
        app.world_mut().spawn((
            Window {
                focused: true,
                ..Window::default()
            },
            PrimaryWindow,
        ));
        (app, receiver)
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies unchanged or explicitly assigned state"
    )]
    fn strafing_is_perpendicular_to_facing_and_never_borrows_forward_speed() {
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
                controls.intent(&keys, true, 0.0, heading).direction,
                Vec3::ZERO
            );
            controls.strafe_speed = Some(3.0);
            let right = controls.intent(&keys, true, 0.0, heading);
            assert_eq!(right.mode, MovementMode::Strafe);
            assert!(right.preserve_facing);
            assert_eq!(right.speed, 3.0);
            assert!(right.direction.dot(facing).abs() < 0.0001);
            assert!(right.direction.dot(facing.cross(Vec3::Y)) > 0.999);
            keys.reset_all();
            keys.press(KeyCode::ArrowLeft);
            let left = controls.intent(&keys, true, 0.0, heading);
            assert!((left.direction + right.direction).length() < 0.0001);
            assert_eq!(
                controls.intent(&keys, false, 0.0, heading).direction,
                Vec3::ZERO
            );
            keys.press(KeyCode::ArrowRight);
            assert_eq!(
                controls.intent(&keys, true, 0.0, heading).direction,
                Vec3::ZERO
            );
        }
    }

    #[test]
    fn walk_toggle_uses_its_grant_and_is_ignored_while_typing() {
        let (mut app, receiver) = app();
        app.world_mut().resource_mut::<Controls>().walk_speed = Some(2.0);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Insert);
        app.update();
        let ClientCommand::Move(request) = receiver.try_recv().unwrap() else {
            panic!("expected walking movement")
        };
        assert_eq!(request.mode, MovementMode::Walk);
        assert!((request.position.x.hypot(request.position.y) - 0.2).abs() < 0.001);
        app.world_mut().resource_mut::<Controls>().reset(None);
        assert!(!app.world().resource::<Controls>().walking);
        assert!(app.world().resource::<Controls>().walk_speed.is_none());
        let (mut app, _) = self::app();
        app.world_mut().resource_mut::<Controls>().walk_speed = Some(2.0);
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .composing = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Insert);
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
        assert!(app.world().resource::<Controls>().speed.is_none());
        assert!(receiver.try_recv().is_err());
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
        app.world_mut()
            .resource_mut::<online::OnlineState>()
            .connected = false;
        app.update();
        assert!(app.world().resource::<Controls>().speed.is_none());
        assert!(receiver.try_recv().is_err());
        let mut state = app.world_mut().resource_mut::<online::OnlineState>();
        state.connected = true;
        state.death = Some(eq_client_core::Death {
            spawn_id: 7,
            killer_id: 0,
            corpse_id: 0,
            bind_zone_id: 0,
        });
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
}
