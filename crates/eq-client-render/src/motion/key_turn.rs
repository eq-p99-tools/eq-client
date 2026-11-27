//! A held keyboard turn owns facing each frame, separately from position playback.
use super::{TURN_STEPS_PER_SECOND, arc, boats, ramped_turn};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(super) enum Source {
    Script(std::sync::Arc<()>),
    Melee {
        admission: u64,
        attack: u64,
        choice: u64,
    },
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Script(a), Self::Script(b)) => std::sync::Arc::ptr_eq(a, b),
            (
                Self::Melee {
                    admission: a,
                    attack: b,
                    choice: c,
                },
                Self::Melee {
                    admission: x,
                    attack: y,
                    choice: z,
                },
            ) => (a, b, c) == (x, y, z),
            _ => false,
        }
    }
}

pub(super) struct Facing {
    pub heading: f32,
    pub follow: bool,
    pub source: Option<Source>,
    observed: Instant,
    carrier: Option<boats::DisplayFrame>,
    guidance: Option<(f32, Instant)>,
    consumed: bool,
}

impl Facing {
    pub fn new(heading: f32, now: Instant, carrier: Option<boats::DisplayFrame>) -> Self {
        Self {
            heading,
            follow: false,
            source: None,
            observed: now,
            carrier,
            guidance: None,
            consumed: false,
        }
    }

    /// Natural carrier yaw remains distinct from the player's stopped turn.
    pub fn carry(&mut self, frame: Option<boats::DisplayFrame>) {
        if let (Some(old), Some(new)) = (self.carrier, frame)
            && old.sample.spawn_id == new.sample.spawn_id
            && old.sample.incarnation == new.sample.incarnation
        {
            self.heading =
                (self.heading + arc(old.pose.heading, new.pose.heading)).rem_euclid(512.);
        }
        self.carrier = frame;
    }

    pub fn observe(
        &mut self,
        now: Instant,
        first: Duration,
        turn: Option<(f32, Instant)>,
        cap: f32,
    ) {
        self.guidance = None;
        self.source = None;
        self.advance(now, first, turn, cap);
    }

    /// Guidance holds the same turn a player would hold, stopping at its goal.
    /// Goal reversals restart the ordinary ramp; waypoint changes in the same
    /// direction keep the hold. No second facing clock or rate is introduced.
    pub fn guide(&mut self, now: Instant, first: Duration, goal: f32, cap: f32) {
        let remaining = arc(self.heading, goal);
        let turn = if goal.is_finite() && remaining.abs() > 0.0001 {
            let sign = remaining.signum();
            Some(
                self.guidance
                    .filter(|(old, _)| *old * sign > 0.)
                    .unwrap_or((sign, now)),
            )
        } else {
            None
        };
        self.guidance = turn;
        let before = self.heading;
        self.advance(now, first, turn, cap);
        if turn.is_some() && arc(before, self.heading).abs() >= remaining.abs() {
            self.heading = goal.rem_euclid(512.);
            self.guidance = None;
            self.follow = arc(before, self.heading).abs() > 0.0001;
        }
    }

    fn advance(&mut self, now: Instant, first: Duration, turn: Option<(f32, Instant)>, cap: f32) {
        self.follow = turn.is_some();
        if self.consumed && self.observed == now {
            return;
        }
        let seconds = if self.observed == now {
            first
        } else {
            now.saturating_duration_since(self.observed)
        }
        .min(Duration::from_millis(150))
        .as_secs_f32();
        self.observed = now;
        self.consumed = true;
        if let Some((sign, since)) = turn {
            let held =
                now.saturating_duration_since(since).as_secs_f32() + 1. / TURN_STEPS_PER_SECOND;
            self.heading =
                (self.heading + sign * ramped_turn(held - seconds, held, cap)).rem_euclid(512.);
        }
        // No step is consumed on the frame that observes release. Queued
        // samples may still report already-held time, but cannot add facing.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guidance_corners_use_the_actual_keyboard_ramp_and_race_cap() {
        let step = Duration::from_nanos(16_666_667);
        for goal in [128., 256.] {
            for cap in [12., 8., 3.] {
                let start = Instant::now();
                let mut guided = Facing::new(0., start, None);
                let mut physical = Facing::new(0., start, None);
                let sign = arc(0., goal).signum();
                let mut arrived = false;
                for frame in 0..360 {
                    let now = start + step * frame;
                    let before = guided.heading;
                    let ordinary = physical.heading;
                    physical.observe(now, step, Some((sign, start)), cap);
                    guided.guide(now, step, goal, cap);
                    let actual = arc(before, guided.heading).abs();
                    let allowed = arc(ordinary, physical.heading).abs();
                    assert!(
                        actual <= allowed + 0.0001,
                        "goal={goal} cap={cap} frame={frame}"
                    );
                    if frame == 0 {
                        assert!((actual - 1. / 3.).abs() < 0.0001);
                    }
                    if arc(guided.heading, goal).abs() < 0.0001 {
                        arrived = true;
                        break;
                    }
                }
                assert!(arrived, "goal={goal} cap={cap}");
                let reached = guided.heading;
                guided.guide(start + step * 361, step, goal, cap);
                assert_eq!(guided.heading.to_bits(), reached.to_bits());
                assert!(!guided.follow);
            }
        }
    }

    #[test]
    fn guidance_reversal_release_and_duplicate_observation_do_not_replay_turns() {
        let start = Instant::now();
        let step = Duration::from_nanos(16_666_667);
        let mut facing = Facing::new(511.5, start, None);
        facing.guide(start, step, 128., 12.);
        let first = facing.heading;
        facing.guide(start, step, 128., 12.);
        assert_eq!(facing.heading.to_bits(), first.to_bits());
        for frame in 1..32 {
            facing.guide(start + step * frame, step, 128., 12.);
        }
        let before = facing.heading;
        facing.guide(start + step * 32, step, 0., 12.);
        assert!((arc(before, facing.heading) + 1. / 3.).abs() < 0.0001);
        let stopped = facing.heading;
        facing.observe(start + step * 33, step, None, 12.);
        assert_eq!(facing.heading.to_bits(), stopped.to_bits());
        facing.guide(start + step * 34, step, 128., 12.);
        assert!((arc(stopped, facing.heading) - 1. / 3.).abs() < 0.0001);
    }

    #[test]
    fn release_has_no_extra_frame_and_repress_restarts_the_ramp() {
        for sign in [-1., 1.] {
            let start = Instant::now();
            let step = Duration::from_nanos(16_666_667);
            let mut facing = Facing::new(511.5, start, None);
            for frame in 0..32 {
                facing.observe(start + step * frame, step, Some((sign, start)), 12.);
            }
            let held = facing.heading;
            assert!((arc(511.5, held) - sign * 106.).abs() < 0.000_2);
            for frame in 32..48 {
                facing.observe(start + step * frame, step, None, 12.);
                assert_eq!(facing.heading.to_bits(), held.to_bits());
                assert!(!facing.follow);
            }
            let pressed = start + step * 48;
            facing.observe(pressed, step, Some((sign, pressed)), 12.);
            assert!((arc(held, facing.heading) - sign / 3.).abs() < 0.000_1);
        }
    }
}
