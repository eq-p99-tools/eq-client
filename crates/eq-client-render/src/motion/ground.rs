//! Completed native dry frames; prediction and local admission remain separate.
use super::{
    Controls, Held, LiquidBasis, MOVE_RESULT_WAIT, MotionInput, OrbitCamera, Player, PlayerBody,
    START_CYCLE_DURATION, TURN_RATE, TURN_STEPS_PER_SECOND, TapBuffer, arc, boats, online, pose,
    ramped_turn, stop_at_zone_line, turn_cap, turn_toward,
};
use bevy::prelude::{Entity, Has, Quat, Query, Transform, Vec2, Vec3, With};
use eq_client_core::{
    ClientCommand, GroundState, HeadingUpdate, MovementMode, MovementRequest, WorldPosition,
    movement::{
        CollisionWorld, GroundMotion, MAX_GROUNDED_STEP, VerticalPhysics, climb_reach,
        program_steps,
    },
    world::ClientWorld,
    world_position,
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

// Normal queue refusal bounds this much earlier. Keeping identities is needed
// to retire delayed receipts without playing their distance for a second time.
const MAX_OBSERVATIONS: usize = 4096;

#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub at: Instant,
    pub motion: GroundMotion,
    pub position: WorldPosition,
    pub state: GroundState,
    pub drawn: Transform,
    pub pose: pose::Snapshot,
    pub follows: bool,
    pub heading: HeadingUpdate,
    pub facing: f32,
    pub carrier: Option<boats::Riding>,
    support: Option<eq_client_core::movement::CarrierSample>,
}

impl Frame {
    pub(super) fn observed(
        at: Instant,
        motion: GroundMotion,
        feet_offset: f32,
        (heading, facing): (f32, f32),
        forward: bool,
        update: HeadingUpdate,
        follows: bool,
    ) -> Option<Self> {
        let origin = motion.feet() + Vec3::Y * feet_offset;
        let velocity = world_position(motion.velocity().to_array(), 0.);
        let state = GroundState::new([velocity.x, velocity.y], forward).ok()?;
        Some(Self {
            at,
            motion,
            position: world_position(origin.to_array(), heading),
            state,
            drawn: Transform::from_translation(origin).with_rotation(Quat::from_rotation_y(
                eq_client_core::render_heading(facing),
            )),
            // Keep owned momentum for the pose as the existing airborne/dry
            // snapshots do. Collision travel never invents a new velocity.
            pose: pose::Snapshot {
                horizontal: Vec2::new(motion.velocity().x, motion.velocity().z),
                vertical: 0.,
            },
            follows,
            heading: update,
            facing,
            carrier: None,
            support: None,
        })
    }
    fn same_state(self, other: Self) -> bool {
        self.position == other.position && self.state == other.state
    }
    fn request(self, session: u64) -> MovementRequest {
        let mut request =
            MovementRequest::new(MovementMode::Forward, session, self.position, self.at);
        request.heading = self.heading;
        request.support = self.support.map(eq_client_core::Support::Spawn);
        request
    }
}

#[derive(Clone, Copy)]
struct Sent {
    session: u64,
    frame: Frame,
    ordinary: bool,
}

/// One current simulation and the genuine completed observations it published.
/// Receipts never advance this simulation or enqueue display playback.
#[derive(Default)]
pub(super) struct Control {
    pub current: Option<Frame>,
    session: Option<u64>,
    accepted: Option<Frame>,
    sent: VecDeque<Sent>,
    last_posted: Option<Instant>,
    pub restore: Option<WorldPosition>,
    pub blocked: bool,
    admitted: Option<Instant>,
    epoch: Option<Instant>,
    retired: Option<Frame>,
    pub cold_handback: bool,
    ordinary: bool,
    ordinary_answered: Option<Instant>,
}

impl Control {
    // This seeds only the already observed stationary state. The frame's
    // observation stamp is never treated as a new accepted worker time.
    pub fn start(&mut self, session: u64, rest: Frame) {
        if self.session != Some(session) {
            self.sent.clear();
        }
        self.session = Some(session);
        self.accepted = Some(rest);
        self.current = Some(rest);
        self.last_posted = None;
        self.admitted = None;
        self.epoch = Some(rest.at);
        self.retired = None;
        self.cold_handback = false;
        self.ordinary = false;
        self.ordinary_answered = None;
    }

    /// Retire presentation on a correction/control seam; pending identities
    /// remain known so their eventual local results cannot become playback.
    pub fn retire(&mut self) {
        self.current = None;
        self.accepted = None;
        self.last_posted = None;
        self.retired = None;
        self.cold_handback = false;
        self.ordinary = false;
        self.ordinary_answered = None;
    }

    pub fn active(&self) -> bool {
        self.current.is_some()
    }

    pub fn admitted(&self) -> Option<Instant> {
        self.admitted
    }

    fn within_walk_budget(&self, at: Instant, origin: Vec3, climb: f32) -> bool {
        // A delayed receipt does not tell which newer posted observation the
        // worker has already admitted. Check every possible current basis,
        // without treating any queued frame as an acceptance or reusing its
        // interval. This keeps per-sample rise/drop bounds before presentation.
        self.accepted
            .into_iter()
            .chain(self.sent.iter().filter_map(|sent| {
                (Some(sent.session) == self.session
                    && self.epoch.is_some_and(|epoch| sent.frame.at >= epoch))
                .then_some(sent.frame)
            }))
            .all(|basis| {
                let elapsed = at.saturating_duration_since(basis.at).as_secs_f32();
                let rise = climb_reach(climb, program_steps(elapsed));
                let delta = origin.y - basis.drawn.translation.y;
                delta <= rise && -delta <= MAX_GROUNDED_STEP
            })
    }

    pub fn handback_ready(&self, now: Instant) -> bool {
        // Every native request already queued has a genuine creation stamp
        // no later than last_posted. A normal cold span is therefore safe
        // after this interval even if a redundant receipt is still delayed.
        self.settled()
            && self
                .last_posted
                .is_none_or(|at| now.saturating_duration_since(at) >= START_CYCLE_DURATION)
    }

    fn handback(&mut self) {
        let checkpoint = self.current;
        self.retire();
        self.retired = checkpoint;
        self.cold_handback = true;
    }

    pub fn basis(&self, session: Option<u64>, now: Instant) -> Option<LiquidBasis> {
        if self.current.is_none() || self.session != session {
            return None;
        }
        Some(if self.settled() {
            LiquidBasis::Observed
        } else if self.timed_out(now) {
            LiquidBasis::Unconfirmed
        } else {
            LiquidBasis::Pending
        })
    }

    /// Report the previous completed state before collecting the next input.
    /// This never changes its creation stamp to manufacture an interval.
    pub fn report(
        &mut self,
        world: &ClientWorld,
        outbox: &crate::outbox::Outbox,
        now: Instant,
    ) -> Result<Option<Instant>, crate::outbox::Refusal> {
        let Some(frame) = self.current else {
            return Ok(None);
        };
        if self
            .accepted
            .is_some_and(|accepted| frame.same_state(accepted))
            || self.last_posted == Some(frame.at)
        {
            return Ok(None);
        }
        if self.sent.len() >= MAX_OBSERVATIONS {
            return Err(crate::outbox::Refusal::Busy);
        }
        let session = self.session.ok_or(crate::outbox::Refusal::NotAdmitted)?;
        if world.session_id() != Some(session) {
            return Err(crate::outbox::Refusal::NotAdmitted);
        }
        if self.ordinary
            && (self.sent.iter().any(|sent| sent.ordinary)
                || self.last_posted.is_some_and(|at| {
                    frame.at.saturating_duration_since(at) < START_CYCLE_DURATION
                })
                || self
                    .ordinary_answered
                    .is_some_and(|at| now.saturating_duration_since(at) < START_CYCLE_DURATION))
        {
            return Ok(None);
        }
        outbox.post(world, |_| {
            if self.ordinary {
                ClientCommand::Move(frame.request(session))
            } else {
                ClientCommand::GroundFrame {
                    request: frame.request(session),
                    state: frame.state,
                }
            }
        })?;
        if self.ordinary {
            bevy::prelude::debug!(
                session_id = session,
                created = ?frame.at,
                position = ?frame.position,
                support = ?frame.support,
                "Ordinary carrier handover current-state observation submitted"
            );
        }
        self.sent.push_back(Sent {
            session,
            frame,
            ordinary: self.ordinary,
        });
        self.last_posted = Some(frame.at);
        Ok(Some(frame.at))
    }

    /// Whether the entire displayed native prefix is genuinely admitted.
    pub fn settled(&self) -> bool {
        self.current
            .zip(self.accepted)
            .is_some_and(|(a, b)| a.same_state(b))
    }

    pub fn carrier(&self) -> Option<boats::Riding> {
        self.accepted.and_then(|frame| frame.carrier)
    }

    /// A stalled result bounds speculation by the unchanged maximum cycle.
    /// Waiting freezes simulation; it never catches up missed held time later.
    pub fn timed_out(&self, now: Instant) -> bool {
        let Some(epoch) = self.epoch.filter(|_| self.active()) else {
            return false;
        };
        self.sent
            .iter()
            .find(|sent| {
                sent.frame.at >= epoch
                    && self
                        .accepted
                        .is_none_or(|accepted| !sent.frame.same_state(accepted))
            })
            .is_some_and(|sent| now.saturating_duration_since(sent.frame.at) >= MOVE_RESULT_WAIT)
    }

    #[cfg(test)]
    pub fn old_timeout(&self, now: Instant) -> bool {
        self.sent
            .front()
            .is_some_and(|sent| now.saturating_duration_since(sent.frame.at) >= MOVE_RESULT_WAIT)
    }

    /// Recognize only a state actually queued by this owner. Acceptance retires
    /// its prefix once; a refusal/changed result restores the worker's position.
    pub fn reconcile(
        &mut self,
        session: Option<u64>,
        created: Instant,
        position: WorldPosition,
        refused: bool,
        received: Instant,
    ) -> bool {
        let Some(index) = self
            .sent
            .iter()
            .position(|sent| Some(sent.session) == session && sent.frame.at == created)
        else {
            return false;
        };
        let sent = self.sent[index];
        if sent.ordinary {
            self.ordinary_answered = Some(received);
        }
        self.sent.drain(..=index);
        if !refused
            && position == sent.frame.position
            && self.current.is_none()
            && self
                .retired
                .is_some_and(|retired| retired.same_state(sent.frame))
        {
            // These exact facts were admitted before handback. A redundant
            // late receipt cannot overwrite a new legacy preview or replay.
            self.admitted = Some(created);
            return true;
        }
        let current_owner =
            self.current.is_some() && self.epoch.is_some_and(|epoch| sent.frame.at >= epoch);
        if refused || position != sent.frame.position || !current_owner {
            self.retire();
            self.restore = Some(position);
            self.blocked = true;
        } else {
            self.accepted = Some(sent.frame);
            self.admitted = Some(created);
        }
        true
    }
}

#[derive(Clone, Copy)]
pub(super) struct Context<'a> {
    pub online: &'a online::OnlineState,
    pub world: &'a CollisionWorld,
    pub body: &'a PlayerBody,
    pub drawn: &'a Transform,
    pub outbox: &'a crate::outbox::Outbox,
    pub now: Instant,
    pub first_span: Duration,
    pub physics: VerticalPhysics,
    pub eligible: bool,
    pub mouse_heading: Option<f32>,
}

pub(super) fn present(
    controls: &mut Controls,
    players: &mut Query<(Entity, &mut Transform), With<Player>>,
    cameras: &mut Query<(&mut OrbitCamera, Has<crate::camera::FreeCamera>)>,
    displayed_mouse: &mut Option<Entity>,
) -> bool {
    let Some(frame) = controls.ground.current else {
        return false;
    };
    if let Ok((entity, mut transform)) = players.single_mut() {
        controls.apply_translation_restore(&mut transform);
        *transform = frame.drawn;
        let preview = controls.mouse_heading;
        if let Some(heading) = preview {
            transform.rotation = Quat::from_rotation_y(eq_client_core::render_heading(heading));
        }
        *displayed_mouse = preview.map(|_| entity);
        controls.drawn_pose = Some(frame.pose);
        for (mut camera, free) in cameras {
            camera.focus = transform.translation;
            if (frame.follows || preview.is_some()) && !free {
                crate::camera::start_behind(&mut camera, &transform);
            }
        }
    }
    true
}

/// Own only observed, collision-resolved ordinary ground frames. All other
/// modes retain their existing controller after the displayed prefix settles.
#[allow(
    clippy::too_many_lines,
    reason = "one completed-frame publication and disposable physics transaction"
)]
pub(super) fn input(controls: &mut Controls, context: Context<'_>, intent: MotionInput) -> bool {
    let Context {
        online,
        world,
        body,
        drawn,
        outbox,
        now,
        first_span,
        physics,
        eligible,
        mouse_heading,
    } = context;
    if controls.ground.blocked {
        controls.ground.blocked = false;
        return true;
    }
    match controls.ground.report(online.world(), outbox, now) {
        Ok(Some(created)) => {
            controls.last_sent = Some(created);
            controls.refused = None;
        }
        Ok(None) => (),
        Err(reason) => {
            controls.send_failed(reason);
            controls.held = Some(Held::Refused);
            return true;
        }
    }
    let eligible = eligible && intent.mode == MovementMode::Forward && intent.diagonal.is_none();
    if controls.ground.current.is_none() {
        if !eligible
            || controls.moving
            || controls.pending.is_some()
            || controls
                .visual
                .as_ref()
                .is_some_and(|visual| !visual.finished())
            || controls.preview.is_some()
        {
            return false;
        }
        let (Some(player), Some(session)) = (online.world().player(), online.world().session_id())
        else {
            return false;
        };
        let origin = Vec3::from_array(eq_client_core::render_position(player.position));
        let shown = controls.restored_translation.unwrap_or(drawn.translation);
        if shown.distance_squared(origin) > 0.000_001 {
            return false;
        }
        let Some(motion) = GroundMotion::new(origin - Vec3::Y * body.feet_offset) else {
            return false;
        };
        // Establish the same positive geometry evidence even on an idle frame.
        let Some(motion) = motion.frame_with_physics(
            world,
            body.height,
            Vec3::ZERO,
            intent.speed,
            false,
            Duration::ZERO,
            physics,
        ) else {
            return false;
        };
        let Some(rest) = Frame::observed(
            now,
            motion,
            body.feet_offset,
            (
                player.position.heading,
                controls.facing.unwrap_or(player.position.heading),
            ),
            false,
            HeadingUpdate::Rate,
            false,
        ) else {
            return false;
        };
        controls.ground.start(session, rest);
    }
    // Only an established native owner publishes its body reach. A restored
    // grant on unsupported geometry must not manufacture a setup command.
    if eligible
        && let Some(session) = online.world().session_id()
        && controls.climb_sent != Some((session, physics.reach()))
    {
        let per_step = physics.reach();
        if let Err(reason) = outbox.post(online.world(), |stamp| ClientCommand::ClimbReach {
            session_id: stamp.session_id,
            per_step,
        }) {
            controls.send_failed(reason);
            controls.held = Some(Held::Refused);
            return true;
        }
        controls.climb_sent = Some((session, per_step));
    }
    let current = controls.ground.current.expect("active ground frame");
    let eligible = eligible
        && !(controls.ground.ordinary
            && controls.ground.settled()
            && current.motion.velocity() == Vec3::ZERO
            && intent.direction == Vec3::ZERO
            && intent.turn == 0.);
    let seconds = if current.at == now {
        first_span
    } else {
        now.saturating_duration_since(current.at)
    };
    let seconds = seconds.min(Duration::from_millis(150));
    let forward = intent.mode == MovementMode::Forward && intent.direction != Vec3::ZERO;
    let mut facing = current.facing;
    if let Some(mouse) = mouse_heading {
        facing = mouse;
    } else if intent.preserve_facing || !forward {
        let cap = turn_cap(online.world().player().map_or(0, |player| player.race));
        let held = controls.turn_held.map_or(0., |(_, since)| {
            now.saturating_duration_since(since).as_secs_f32() + 1. / TURN_STEPS_PER_SECOND
        });
        facing = (facing + intent.turn * ramped_turn(held - seconds.as_secs_f32(), held, cap))
            .rem_euclid(512.);
    } else {
        facing = eq_client_core::world_heading(intent.direction.x.atan2(intent.direction.z));
    }
    let heading = mouse_heading.unwrap_or_else(|| {
        turn_toward(
            current.position.heading,
            facing,
            TURN_RATE * seconds.as_secs_f32(),
        )
    });
    let direction = if intent.preserve_facing {
        let yaw = eq_client_core::render_heading(facing);
        Vec3::new(yaw.sin(), 0., yaw.cos()) * f32::from(forward)
    } else {
        intent.direction
    };
    let trial = (eligible && !controls.ground.ordinary)
        .then(|| {
            current.motion.frame_with_physics(
                world,
                body.height,
                direction,
                intent.speed,
                forward,
                seconds,
                physics,
            )
        })
        .flatten();
    // The rejected static proof consumed no part of this actual frame. Use
    // the ordinary body sweep on a copy, retaining its owned velocity and
    // the whole unconsumed span once. A genuine carrier floor is required
    // to enter this bridge; it never widens GroundFrame's static guard.
    let ordinary_trial = if trial.is_none() && eligible {
        let carrier_unchanged = current.carrier.is_none_or(|riding| {
            riding
                .carried(online.world().carriers(), now)
                .is_some_and(|carried| carried.pose == riding.pose)
        });
        carrier_unchanged
            .then(|| {
                current.motion.ordinary_frame_with_physics(
                    world,
                    body.height,
                    direction,
                    intent.speed,
                    forward,
                    seconds,
                    physics,
                )
            })
            .flatten()
            .and_then(|motion| {
                let carrier = boats::under(
                    world,
                    online.world().carriers(),
                    motion.feet(),
                    current.carrier.map(|riding| riding.sample.spawn_id),
                    now,
                )
                .map(|(sample, pose)| boats::Riding {
                    sample,
                    pose,
                    position: world_position(
                        (motion.feet() + Vec3::Y * body.feet_offset).to_array(),
                        heading,
                    ),
                });
                // The static footprint can meet an elevated deck before
                // the narrow down-sweep links the feet to it. Positive
                // ordinary-reach geometry can select the ordinary owner;
                // only the actual down-sweep above supplies wire support.
                let approaching = world
                    .ground_spawn(motion.feet(), physics.reach(), MAX_GROUNDED_STEP)
                    .is_some_and(|id| online.world().carriers().latest(id).is_some());
                (controls.ground.ordinary || carrier.is_some() || approaching)
                    .then_some((motion, carrier))
            })
    } else {
        None
    };
    let ordinary = ordinary_trial.is_some();
    let carrier = ordinary_trial.and_then(|(_, carrier)| carrier);
    let trial = trial.or(ordinary_trial.map(|(motion, _)| motion));
    let trial = trial.and_then(|motion| {
        let origin = motion.feet() + Vec3::Y * body.feet_offset;
        let before = Vec3::from_array(eq_client_core::render_position(current.position));
        let within_height = controls
            .ground
            .within_walk_budget(now, origin, physics.reach());
        (within_height && stop_at_zone_line(&online.regions, before, origin) == origin)
            .then_some(motion)
    });
    if let Some(trial) = trial.filter(|_| !controls.ground.timed_out(now)) {
        let Some(mut frame) = Frame::observed(
            now,
            trial,
            body.feet_offset,
            (heading, facing),
            forward,
            if mouse_heading.is_some() {
                HeadingUpdate::Snapshot
            } else {
                HeadingUpdate::Rate
            },
            mouse_heading.is_some() || intent.turn != 0.,
        ) else {
            return true;
        };
        frame.carrier = carrier;
        frame.support = controls
            .ground
            .carrier()
            .or(carrier)
            .map(|riding| riding.sample);
        let legal = online
            .world()
            .session_id()
            .ok_or(crate::outbox::Refusal::NotAdmitted)
            .and_then(|session| {
                let command = if ordinary {
                    ClientCommand::Move(frame.request(session))
                } else {
                    ClientCommand::GroundFrame {
                        request: frame.request(session),
                        state: frame.state,
                    }
                };
                outbox.check_command(online.world(), &command)
            });
        if let Err(reason) = legal {
            controls.send_failed(reason);
            controls.held = Some(Held::Refused);
            return true;
        }
        if ordinary && !controls.ground.ordinary {
            bevy::prelude::debug!(
                seconds = seconds.as_secs_f32(),
                velocity = ?trial.velocity(),
                "Static-ground scope handed its unconsumed frame to ordinary carrier physics"
            );
        }
        controls.ground.current = Some(frame);
        controls.ground.ordinary = ordinary;
        controls.moving = forward || trial.velocity() != Vec3::ZERO;
        controls.visual = None;
        controls.preview = None;
        controls.sample_pose = None;
        controls.facing = (arc(heading, facing) != 0.).then_some(facing);
        controls.released_turn = 0.;
        controls.taps = TapBuffer::default();
        controls.surface.pause();
        controls.held = None;
        return true;
    }
    if controls.ground.handback_ready(now) {
        let admitted = controls.ground.admitted();
        let carrier = controls.ground.carrier();
        controls.ground.handback();
        if carrier.is_some() {
            controls.boat = carrier;
        }
        if let Some(admitted) = admitted {
            // This is an actual admitted creation time, never a fabricated
            // interval and never the time a delayed receipt arrived.
            controls.last_accepted = admitted;
            controls.last_sent = Some(admitted);
        }
        // No frozen interval becomes a catch-up sample. The existing cold
        // span is bounded by time since every queued native observation.
        controls.clear_poses();
        return false;
    }
    // A genuine observation of a paused simulation. No old request is
    // restamped, no refused step is committed and no lost time is replayed.
    controls.ground.current = Frame::observed(
        now,
        current.motion,
        body.feet_offset,
        (current.position.heading, current.facing),
        forward,
        current.heading,
        current.follows,
    )
    .map(|mut frame| {
        frame.carrier = current.carrier;
        frame.support = current.support;
        frame
    });
    controls.held = Some(Held::Waiting);
    true
}

#[cfg(test)]
mod walk_budget_tests {
    use super::*;
    fn frame(at: Instant, height: f32) -> Frame {
        Frame::observed(
            at,
            GroundMotion::new(Vec3::Y * height).unwrap(),
            0.,
            (0., 0.),
            true,
            HeadingUpdate::Rate,
            false,
        )
        .unwrap()
    }
    #[test]
    fn delayed_receipts_do_not_multiply_the_existing_per_sample_height_budget() {
        let start = Instant::now();
        let mut control = Control::default();
        control.start(7, frame(start, 0.));
        let posted = start + Duration::from_millis(50);
        control.sent.push_back(Sent {
            session: 7,
            frame: frame(posted, 0.),
            ordinary: false,
        });
        let now = start + Duration::from_millis(100);
        assert!(control.within_walk_budget(now, Vec3::Y * 3., 3.));
        assert!(
            !control.within_walk_budget(now, Vec3::Y * 6., 3.),
            "the older receipt alone permits6, but the50ms posted basis permits only3"
        );
        assert!(!control.within_walk_budget(now, Vec3::Y * -2.01, 3.));
        control.retire();
        control.start(7, frame(now, 100.));
        assert!(
            control.within_walk_budget(now + Duration::from_millis(50), Vec3::Y * 101., 3.),
            "a retained old receipt identity is not a new epoch's movement basis"
        );
    }
}
