//! A cap on the frame rate, the Options window's Max FPS. Without one the
//! client draws as fast as vsync allows, which on a fast monitor is its full
//! refresh rate, in the background too, and keeps the machine hot for nothing
//! anyone can see.
use bevy::prelude::*;
use std::time::{Duration, Instant};

/// When the next frame may begin.
#[derive(Resource)]
struct FrameLimit {
    /// The earliest the next frame may start.
    next: Instant,
}

/// Holds the client to the frame rate the options set.
pub(crate) fn install(app: &mut App) {
    app.insert_resource(FrameLimit {
        next: Instant::now(),
    })
    .add_systems(Last, wait_for_next_frame);
}

/// Sleeps out the rest of the frame's interval; with no cap, the next frame
/// may start at once.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn wait_for_next_frame(mut limit: ResMut<FrameLimit>, options: Res<crate::options::OptionsState>) {
    let Some(rate) = options.options.frame_cap() else {
        limit.next = Instant::now();
        return;
    };
    let (wait, next) = pace(limit.next, Instant::now(), Duration::from_secs(1) / rate);
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
    limit.next = next;
}

/// How long to wait before the next frame, and when the one after it may
/// start. Frames keep a steady cadence; one that ran a whole interval late
/// starts a new cadence, so the frames after it do not hurry to catch up.
fn pace(next: Instant, now: Instant, interval: Duration) -> (Duration, Instant) {
    match next.checked_duration_since(now) {
        Some(wait) => (wait, next + interval),
        None if now.duration_since(next) >= interval => (Duration::ZERO, now + interval),
        None => (Duration::ZERO, next + interval),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_keep_a_steady_cadence_and_late_ones_start_a_new_one() {
        let interval = Duration::from_millis(16);
        let start = Instant::now();
        let early = start + Duration::from_millis(10);
        assert_eq!(
            pace(early, start, interval),
            (Duration::from_millis(10), early + interval)
        );
        // A little late keeps the cadence, so the next frame is a little sooner.
        let late = start + Duration::from_millis(2);
        assert_eq!(
            pace(start, late, interval),
            (Duration::ZERO, start + interval)
        );
        // A whole interval late starts again from now.
        let stalled = start + Duration::from_millis(40);
        assert_eq!(
            pace(start, stalled, interval),
            (Duration::ZERO, stalled + interval)
        );
    }
}
