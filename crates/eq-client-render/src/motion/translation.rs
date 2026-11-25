//! Display-only translation cursor. Rotation retains the ordinary sample clock.

use super::{Segment, VisualMotion};
use bevy::prelude::*;
use std::collections::VecDeque;

/// [I] Display-only finish window: three frames at 60 Hz. Catch-up changes
/// neither a submitted endpoint nor a command's clock.
pub(super) const RELEASE_FINISH_SECONDS: f32 = 0.05;

#[derive(Clone, Copy)]
struct Leg {
    to: Vec3,
    span: f32,
}

pub(super) struct Playhead {
    from: Vec3,
    to: Vec3,
    elapsed: f32,
    duration: f32,
    queue: VecDeque<Leg>,
    released: bool,
    finish_left: Option<f32>,
}

impl Playhead {
    pub(super) fn at(position: Vec3) -> Self {
        Self {
            from: position,
            to: position,
            elapsed: 0.0,
            duration: 0.0,
            queue: VecDeque::new(),
            released: false,
            finish_left: None,
        }
    }

    pub(super) fn from_visual(visual: &VisualMotion) -> Self {
        Self {
            from: visual.from.translation,
            to: visual.to.translation,
            elapsed: visual.elapsed,
            duration: visual.duration,
            queue: visual
                .queue
                .iter()
                .map(|segment| Leg {
                    to: segment.to.translation,
                    span: segment.span,
                })
                .collect(),
            released: false,
            finish_left: None,
        }
    }

    pub(super) fn release(&mut self, released: bool) {
        if released != self.released {
            self.released = released;
            self.finish_left = released.then(|| self.remaining().min(RELEASE_FINISH_SECONDS));
        }
    }

    pub(super) fn push(&mut self, segment: &Segment) {
        // A delayed zero-input stop must not replay distance or bank idle time.
        if self.finished() {
            self.elapsed = self.duration;
            if segment.to.translation == self.to {
                return;
            }
            if self.released {
                self.finish_left = Some(segment.span.min(RELEASE_FINISH_SECONDS));
            }
        }
        self.queue.push_back(Leg {
            to: segment.to.translation,
            span: segment.span,
        });
    }

    pub(super) fn finished(&self) -> bool {
        self.queue.is_empty() && self.elapsed >= self.duration
    }

    /// Only an observed release makes reaching the endpoint a standing pose.
    /// A held path may briefly run out while the next sample is in flight;
    /// restore also starts at an empty, unreleased cursor.
    pub(super) fn rested(&self) -> bool {
        self.released && self.finished()
    }

    pub(super) fn map(&mut self, mut map: impl FnMut(Vec3) -> Vec3) {
        self.from = map(self.from);
        self.to = map(self.to);
        for leg in &mut self.queue {
            leg.to = map(leg.to);
        }
    }

    pub(super) fn advance(&mut self, seconds: f32) -> Vec3 {
        let seconds = seconds.max(0.0);
        let remaining = self.remaining();
        if self
            .finish_left
            .is_some_and(|left| left <= seconds + f32::EPSILON * RELEASE_FINISH_SECONDS)
        {
            // Spending the whole remaining path must reach its exact endpoint.
            // Subtracting several f32 leg durations can otherwise leave a tiny
            // residual and introduce an extra rendered frame at the deadline.
            if let Some(last) = self.queue.back() {
                self.to = last.to;
                self.duration = last.span;
            }
            self.queue.clear();
            self.elapsed = self.duration;
            self.finish_left = Some(0.0);
            return self.to;
        }
        let play = self
            .finish_left
            .map_or(seconds, |left| remaining * seconds / left);
        if let Some(left) = self.finish_left.as_mut() {
            *left = (*left - seconds).max(0.0);
        }
        self.elapsed += play;
        while self.elapsed >= self.duration
            && let Some(next) = self.queue.pop_front()
        {
            self.elapsed -= self.duration;
            self.from = self.to;
            self.to = next.to;
            self.duration = next.span;
        }
        // Once the last endpoint is reached there is no future path to spend
        // idle time on. An eventual new submission begins from that endpoint.
        if self.queue.is_empty() {
            self.elapsed = self.elapsed.min(self.duration);
        }
        if self.finished() {
            return self.to;
        }
        let fraction = (self.elapsed / self.duration.max(f32::EPSILON)).clamp(0.0, 1.0);
        self.from.lerp(self.to, fraction)
    }

    fn remaining(&self) -> f32 {
        (self.duration - self.elapsed).max(0.0) + self.queue.iter().map(|leg| leg.span).sum::<f32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visual() -> VisualMotion {
        let rotation = Quat::from_rotation_y(0.731);
        VisualMotion::start(
            Transform::default().with_rotation(rotation),
            Segment {
                to: Transform::from_xyz(1.0, 0.0, 0.0).with_rotation(rotation),
                span: 0.2,
                snap: false,
                follow_heading: false,
                pose: None,
            },
        )
    }

    #[test]
    fn release_rest_requires_release_and_clears_on_repress_or_restore() {
        let mut playhead = Playhead::from_visual(&visual());
        assert!(!playhead.rested());
        playhead.release(true);
        assert!(!playhead.rested());
        assert_eq!(playhead.advance(RELEASE_FINISH_SECONDS), Vec3::X);
        assert!(playhead.rested());
        playhead.release(false);
        assert!(playhead.finished());
        assert!(!playhead.rested());
        let restored = Playhead::at(Vec3::X);
        assert!(restored.finished());
        assert!(!restored.rested());
    }

    #[test]
    fn release_finishes_translation_in_three_frames_without_changing_rotation_time() {
        let (mut before, mut after) = (visual(), visual());
        let (mut ordinary, mut shown) = (Transform::default(), Transform::default());
        before.advance(0.02, &mut ordinary);
        after.advance(0.02, &mut shown);
        after.release_translation(true);
        for frame in 1..=3 {
            before.advance(RELEASE_FINISH_SECONDS / 3.0, &mut ordinary);
            after.release_translation(true);
            after.advance(RELEASE_FINISH_SECONDS / 3.0, &mut shown);
            assert_eq!(ordinary.rotation, shown.rotation);
            assert_eq!(before.elapsed, after.elapsed);
            assert!(shown.translation.x >= ordinary.translation.x);
            assert!(shown.translation.x <= 1.0);
            if frame < 3 {
                assert!(shown.translation.x < 1.0);
            }
        }
        assert_eq!(shown.translation, Vec3::X);
        assert!(
            !after.finished(),
            "the ordinary rotation clock was shortened"
        );
    }

    #[test]
    fn short_remaining_motion_is_never_slowed_by_the_finish_window() {
        let mut visual = visual();
        let mut shown = Transform::default();
        visual.advance(0.19, &mut shown);
        visual.release_translation(true);
        visual.advance(0.011, &mut shown);
        assert_eq!(shown.translation, Vec3::X);
        let playhead = visual.translation.as_mut().unwrap();
        playhead.push(&Segment {
            to: Transform::from_xyz(2.0, 0.0, 0.0),
            span: 0.005,
            snap: false,
            follow_heading: false,
            pose: None,
        });
        assert_eq!(playhead.advance(0.006), Vec3::X * 2.0);
    }

    #[test]
    fn restarting_keeps_the_advanced_cursor_and_does_not_replay_distance() {
        let mut visual = visual();
        let mut shown = Transform::default();
        visual.advance(0.02, &mut shown);
        visual.release_translation(true);
        visual.advance(0.02, &mut shown);
        let released = shown.translation.x;
        visual.release_translation(false);
        visual.advance(0.01, &mut shown);
        assert!(shown.translation.x >= released);
        assert!((shown.translation.x - released - 0.05).abs() < 0.000_1);
        visual.release_translation(true);
        visual.advance(RELEASE_FINISH_SECONDS, &mut shown);
        assert_eq!(shown.translation, Vec3::X);
        let playhead = visual.translation.as_mut().unwrap();
        for _ in 0..100 {
            assert_eq!(playhead.advance(0.016), Vec3::X);
        }
        playhead.push(&Segment {
            to: Transform::from_xyz(2.0, 0.0, 0.0),
            span: 0.1,
            snap: false,
            follow_heading: false,
            pose: None,
        });
        let next = playhead.advance(0.01);
        assert!(next.x > 1.0 && next.x < 2.0, "idle time banked movement");
    }

    #[test]
    fn queued_corners_stay_on_the_ordered_submitted_path() {
        let mut visual = visual();
        visual.queue.extend([
            Segment {
                to: Transform::from_xyz(1.0, 0.0, 1.0),
                span: 0.2,
                snap: false,
                follow_heading: false,
                pose: None,
            },
            Segment {
                to: Transform::from_xyz(2.0, 0.0, 1.0),
                span: 0.2,
                snap: false,
                follow_heading: false,
                pose: None,
            },
        ]);
        let mut playhead = Playhead::from_visual(&visual);
        playhead.release(true);
        let mut previous = Vec3::ZERO;
        for _ in 0..5 {
            let point = playhead.advance(0.01);
            assert!(point.x >= previous.x && point.z >= previous.z);
            assert!(point.y == 0.0 && point.x <= 2.0 && point.z <= 1.0);
            assert!(
                point.z == 0.0
                    || (point.x - 1.0).abs() < f32::EPSILON
                    || (point.z - 1.0).abs() < f32::EPSILON
            );
            previous = point;
        }
        assert_eq!(previous, Vec3::new(2.0, 0.0, 1.0));
    }

    #[test]
    fn zero_stop_and_carrier_mapping_never_repeat_or_double_transform_an_endpoint() {
        let mut playhead = Playhead::from_visual(&visual());
        playhead.release(true);
        assert_eq!(playhead.advance(0.05), Vec3::X);
        playhead.push(&Segment {
            to: Transform::from_xyz(1.0, 0.0, 0.0),
            span: 0.1,
            snap: false,
            follow_heading: false,
            pose: None,
        });
        let offset = Vec3::new(20.0, 4.0, -5.0);
        playhead.map(|point| point + offset);
        assert_eq!(playhead.advance(0.1), Vec3::X + offset);
    }

    #[test]
    fn changing_height_or_facing_keeps_the_existing_playback() {
        for rotate in [false, true] {
            let mut visual = visual();
            if rotate {
                visual.to.rotation = Quat::from_rotation_y(1.1);
            } else {
                visual.to.translation.y = 0.5;
            }
            visual.release_translation(true);
            assert!(visual.translation.is_none());
        }
    }
}
