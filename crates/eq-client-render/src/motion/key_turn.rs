//! A held keyboard turn owns facing each frame, separately from position playback.
use super::{TURN_STEPS_PER_SECOND, arc, boats, ramped_turn};
use std::time::{Duration, Instant};

pub(super) struct Facing {
    pub heading: f32,
    pub follow: bool,
    observed: Instant,
    carrier: Option<boats::DisplayFrame>,
}

impl Facing {
    pub fn new(heading: f32, now: Instant, carrier: Option<boats::DisplayFrame>) -> Self {
        Self {
            heading,
            follow: false,
            observed: now,
            carrier,
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
        let seconds = if self.observed == now {
            first
        } else {
            now.saturating_duration_since(self.observed)
        }
        .min(Duration::from_millis(150))
        .as_secs_f32();
        self.observed = now;
        self.follow = turn.is_some();
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
