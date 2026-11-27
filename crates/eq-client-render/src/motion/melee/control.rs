//! One local intent/facing owner around the existing bounded approach.
use super::{
    Assist, Body, ClientWorld, Instant, MAX_DISTANCE, OrbitInput, Vec3, approach_reason, follow,
    orbit, target, transition_reason,
};

pub(in crate::motion) struct Steering {
    pub follow: follow::Step,
    pub orbit: Option<orbit::Plan>,
}

impl Assist {
    pub fn observe(
        &mut self,
        world: &ClientWorld,
        enabled: bool,
        attack: Option<u64>,
        now: Instant,
    ) {
        if !self.can_observe(world, enabled, attack, now) {
            return;
        }
        let arm = world
            .session_id()
            .zip(attack)
            .map(|(admission, attack)| orbit::Arm {
                admission,
                attack,
                choice: world.next_target_request(),
            });
        if self.lock.sync(arm) {
            self.scope = None;
            self.approaching = true;
            self.paused_until = None;
            self.target_sample = None;
            self.safe_stop = None;
            self.orbit_hold = orbit::Held::default();
        }
        self.observe_target(world, now);
    }

    fn can_observe(
        &mut self,
        world: &ClientWorld,
        enabled: bool,
        attack: Option<u64>,
        now: Instant,
    ) -> bool {
        if !enabled {
            self.clear();
            return false;
        }
        if attack.is_none() {
            if self.lock.active() {
                if let Some(player) = world.player() {
                    if let Err(reason) = target(world, player.position, now) {
                        self.invalidate(reason);
                        return false;
                    }
                } else {
                    self.invalidate("admission");
                    return false;
                }
            }
            // A received teardown can turn attack off before Input. Retain
            // that cause rather than misreporting it as a deliberate release.
            if self.lock.phase == orbit::Phase::Invalidated {
                return false;
            }
        }
        true
    }

    fn observe_target(&mut self, world: &ClientWorld, now: Instant) {
        if !self.lock.active() {
            self.driving = false;
            return;
        }
        let Some(player) = world.player() else {
            self.invalidate("no_player");
            return;
        };
        let (scope, there, radius, _) = match target(world, player.position, now) {
            Ok(target) => target,
            Err(reason) => {
                if reason == "target_authority" && self.scope.is_none() {
                    // A fresh choice owns its pending confirmation; lost
                    // authority for an existing lock still invalidates.
                    self.suspend("target_pending");
                } else {
                    self.invalidate(reason);
                }
                return;
            }
        };
        if self.scope.is_some_and(|old| {
            old.session != scope.session
                || old.target_id != scope.target_id
                || old.spawn_revision != scope.spawn_revision
        }) {
            self.invalidate("target_identity");
            return;
        }
        self.scope = Some(scope);
        let from = Vec3::from_array(eq_client_core::render_position(player.position));
        let to = Vec3::from_array(eq_client_core::render_position(there));
        let distance = (to - from).with_y(0.).length();
        let dz = to.y - from.y;
        if !distance.is_finite() || distance > MAX_DISTANCE || dz.abs() >= radius {
            self.invalidate("distance_or_height");
            return;
        }
        let reach = (radius * radius - dz * dz).sqrt();
        let stop = (reach - 1_f32.min(reach * 0.1)).max(0.);
        // Acquisition waits for the actual sample's speed-derived stand-off.
        // An existing lock can only shrink when the body's envelope shrinks.
        self.lock.radius = self.lock.radius.map(|radius| radius.min(stop));
    }

    pub fn active(&self) -> bool {
        self.lock.active()
    }

    pub fn owns_facing(&self) -> bool {
        self.lock.active() && self.lock.phase != orbit::Phase::Suspended
    }

    pub fn resume(&mut self, now: Instant) {
        if self.lock.phase == orbit::Phase::Suspended && self.scope.is_some() && self.ready(now) {
            self.lock.set(
                if self.lock.radius.is_some() {
                    orbit::Phase::Locked
                } else {
                    orbit::Phase::Approaching
                },
                "ready",
            );
        }
    }

    pub fn radius(&self) -> Option<f32> {
        self.lock.radius
    }

    pub fn camera_locked(&self, world: &ClientWorld, now: Instant) -> bool {
        self.lock.camera_locked()
            && world.player().is_some_and(|player| {
                target(world, player.position, now)
                    .is_ok_and(|(scope, _, _, _)| self.scope == Some(scope))
            })
    }

    /// The ordinary facing owner chooses the turn; this only names its goal.
    pub fn facing_goal(
        &self,
        world: &ClientWorld,
        body: &Body,
        input: OrbitInput,
        now: Instant,
    ) -> Option<f32> {
        if !self.owns_facing() || !self.ready(now) {
            return None;
        }
        let (scope, there, _, _) = target(world, body.position, now).ok()?;
        if self.scope != Some(scope) {
            return None;
        }
        let from = Vec3::from_array(eq_client_core::render_position(body.position));
        let to = Vec3::from_array(eq_client_core::render_position(there));
        if self
            .lock
            .radius
            .is_some_and(|radius| (to - from).with_y(0.).length() <= radius + 0.25)
            && input.side != 0.
            && let Some(plan) = orbit::plan(from, to, body.position.heading, body.seconds, input)
        {
            return Some(plan.heading);
        }
        let toward = (to - from).with_y(0.);
        (toward.length_squared() > f32::EPSILON)
            .then(|| eq_client_core::world_heading(toward.x.atan2(toward.z)))
    }

    pub fn status(&self) -> (&'static str, &'static str) {
        (self.lock.phase.word(), self.lock.reason)
    }

    pub fn target_id(&self) -> Option<u16> {
        self.scope.map(|scope| scope.target_id)
    }

    pub fn admission_id(&self) -> Option<u64> {
        self.scope.map(|scope| scope.session)
    }

    pub fn owns_facing_name(&self) -> &'static str {
        if self.owns_facing() {
            "melee"
        } else {
            "ordinary"
        }
    }

    pub fn begin_frame(&mut self) {
        self.proposed = false;
    }

    pub fn submitted(&mut self) {
        self.proposed = true;
        self.orbit_hold.consume();
    }

    pub fn proposed(&self) -> bool {
        self.proposed
    }

    pub fn observe_orbit(&mut self, axis: f32, now: Instant, eligible: bool) {
        self.orbit_hold.observe(axis, now, eligible);
    }

    pub fn orbit_seconds(&self, now: Instant, span: f32) -> f32 {
        self.orbit_hold.seconds(now, span)
    }

    pub fn release(&mut self, reason: &'static str) {
        if self.lock.active() {
            self.lock.release(reason);
        }
        self.driving = false;
        self.orbit_hold = orbit::Held::default();
    }

    pub fn invalidate(&mut self, reason: &'static str) {
        self.lock.invalidate(reason);
        self.scope = None;
        self.driving = false;
        self.target_sample = None;
        self.safe_stop = None;
        self.orbit_hold = orbit::Held::default();
    }

    pub fn suspend(&mut self, reason: &'static str) {
        self.lock.suspend(reason);
        self.driving = false;
        self.orbit_hold = orbit::Held::default();
    }

    /// No assist translation or facing change is needed on this native idle frame.
    pub fn native_idle(&self, world: &ClientWorld, side: f32, now: Instant) -> bool {
        if !self.lock.camera_locked() || side != 0. {
            return false;
        }
        let Some(player) = world.player() else {
            return false;
        };
        let Ok((_, there, _, in_range)) = target(world, player.position, now) else {
            return false;
        };
        let flat = Vec3::from_array(eq_client_core::render_position(there))
            - Vec3::from_array(eq_client_core::render_position(player.position));
        let desired = eq_client_core::world_heading(flat.x.atan2(flat.z));
        in_range
            && self
                .lock
                .radius
                .is_some_and(|radius| flat.with_y(0.).length() <= radius + 0.25)
            && super::super::arc(player.position.heading, desired).abs() <= 0.01
    }

    pub fn steer(
        &mut self,
        world: &ClientWorld,
        collision: &eq_client_core::movement::CollisionWorld,
        regions: &eq_client_assets::regions::ZoneRegions,
        body: &Body,
        input: OrbitInput,
        now: Instant,
    ) -> Option<Steering> {
        if !self.lock.active() {
            return None;
        }
        if !self.ready(now) {
            self.suspend("manual_release_pause");
            return None;
        }
        if self.lock.phase == orbit::Phase::Suspended {
            return None;
        }
        if !(body.seconds.is_finite() && body.seconds > 0. && body.seconds <= 0.25) {
            self.suspend("sample_interval");
            return None;
        }
        let (_, there, radius, in_range) = match target(world, body.position, now) {
            Ok(target) => target,
            Err(reason) => {
                self.invalidate(reason);
                return None;
            }
        };
        let from = Vec3::from_array(eq_client_core::render_position(body.position));
        let to = Vec3::from_array(eq_client_core::render_position(there));
        let distance = (to - from).with_y(0.).length();
        if !distance.is_finite() || distance > MAX_DISTANCE || (to.y - from.y).abs() >= radius {
            self.invalidate("distance_or_height");
            return None;
        }
        let feet = to - Vec3::Y * body.offset;
        if collision
            .ground_spawn(feet, 0.01, super::super::boats::sweep_depth(feet))
            .is_some()
        {
            self.suspend("target_carrier");
            return None;
        }
        let Some(follow) = self.step(world, collision, regions, body, now) else {
            self.invalidate("approach_unavailable");
            return None;
        };
        let stop = self.safe_stop?;
        self.lock.radius = self.lock.radius.map(|radius| radius.min(stop));
        if self.lock.radius.is_none() && in_range && distance <= stop {
            self.lock.capture(distance, stop);
        }
        if self.lock.radius.is_some() {
            self.locked_steering(collision, regions, body, to, input, follow)
        } else {
            if self.lock.phase == orbit::Phase::Suspended {
                return None;
            }
            if !follow.forward && self.decision_reason != "turning" {
                self.suspend(self.decision_reason);
                return None;
            }
            self.lock.set(orbit::Phase::Approaching, "approach");
            self.driving = follow.forward;
            Some(Steering {
                follow,
                orbit: None,
            })
        }
    }

    fn locked_steering(
        &mut self,
        collision: &eq_client_core::movement::CollisionWorld,
        regions: &eq_client_assets::regions::ZoneRegions,
        body: &Body,
        to: Vec3,
        input: OrbitInput,
        mut follow: follow::Step,
    ) -> Option<Steering> {
        let from = Vec3::from_array(eq_client_core::render_position(body.position));
        let distance = (to - from).with_y(0.).length();
        if let Some(radius) = self.lock.radius {
            if radius < 2. * eq_client_core::movement::capsule_radius()
                || distance < 2. * eq_client_core::movement::capsule_radius()
            {
                self.suspend("orbit_radius");
                return None;
            }
            if distance > radius + 0.25 {
                let beyond = distance - radius;
                let speed = if body.walking {
                    body.speeds.1
                } else {
                    body.speeds.0
                };
                let reason = approach_reason(
                    collision,
                    regions,
                    body,
                    from,
                    to,
                    beyond.min(speed.max(0.) * body.seconds),
                );
                if reason != "forward" {
                    self.suspend(reason);
                    return None;
                }
                let bearing = to - from;
                let desired = eq_client_core::world_heading(bearing.x.atan2(bearing.z));
                follow.forward = super::super::arc(follow.heading, desired).abs() <= 0.01;
                follow.run = !body.walking;
                follow.beyond = if follow.forward { beyond } else { 0. };
                self.lock.set(orbit::Phase::Approaching, "radius_recovery");
            } else {
                follow.forward = false;
                follow.beyond = 0.;
                self.lock.set(orbit::Phase::Locked, "in_range");
                let seconds = input
                    .observed_seconds
                    .unwrap_or(body.seconds)
                    .min(body.seconds);
                if let Some(mut plan) = orbit::plan(from, to, body.position.heading, seconds, input)
                {
                    plan.motion.speed *= seconds / body.seconds;
                    let delta = plan.motion.direction * plan.motion.speed * body.seconds;
                    if orbit_clear(collision, regions, body, from, to, delta) {
                        self.lock.set(orbit::Phase::Locked, "orbit");
                        self.driving = true;
                        follow.heading = plan.heading;
                        return Some(Steering {
                            follow,
                            orbit: Some(plan),
                        });
                    }
                    self.suspend("orbit_obstructed");
                    return None;
                }
            }
        }
        self.driving = follow.forward;
        Some(Steering {
            follow,
            orbit: None,
        })
    }
}

fn orbit_clear(
    collision: &eq_client_core::movement::CollisionWorld,
    regions: &eq_client_assets::regions::ZoneRegions,
    body: &Body,
    from: Vec3,
    target: Vec3,
    delta: Vec3,
) -> bool {
    let sight = target - from;
    if collision
        .ray_distance(from, sight.normalize_or_zero(), sight.length())
        .is_some_and(|hit| hit < sight.length() - 0.5)
    {
        return false;
    }
    let feet = from - Vec3::Y * body.offset;
    let achieved = collision.step(feet, delta, body.height);
    (achieved - feet)
        .with_y(0.)
        .abs_diff_eq(delta.with_y(0.), 0.02)
        && collision.support(achieved).is_some()
        && !collision.in_liquid(achieved)
        && transition_reason(
            collision,
            regions,
            from,
            achieved + Vec3::Y * body.offset,
            body.height,
            body.offset,
        )
        .is_none()
}
