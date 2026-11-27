//! Bounded semantic world gestures. No OS pointer or camera pose is changed.
use std::time::Duration;

use bevy::prelude::*;

use super::super::{ExpectedWorld, Script};
use crate::{camera::MouseLook, online::OnlineState, windows::pointer::Wheel};

/// This frame's relative world input, read by the ordinary camera consumers.
#[derive(Default, Resource)]
pub(crate) struct WorldInput {
    pub(crate) active: bool,
    pub(crate) held: bool,
    pub(crate) delta: Vec2,
}

/// Owned by the executor, so all ordinary release/cancel paths withdraw it.
pub(in crate::script) struct WorldGesture {
    scope: Option<ExpectedWorld>,
    pub(in crate::script) held: bool,
    pub(in crate::script) delta: Vec2,
    pub(in crate::script) scroll: f32,
    sampled: Duration,
}

impl WorldGesture {
    pub(in crate::script) fn new(online: &OnlineState, held: bool, scroll: f32) -> Self {
        Self {
            scope: ExpectedWorld::current(online),
            held,
            delta: Vec2::ZERO,
            scroll,
            sampled: Duration::ZERO,
        }
    }

    pub(in crate::script) fn refusal(&self, online: &OnlineState) -> Option<String> {
        match &self.scope {
            Some(scope) => scope.refusal(online),
            None if online.enabled => Some("World gesture lost its offline input scope".into()),
            None => None,
        }
    }

    /// Keeps the final displacement held for one input frame before release.
    /// The sum is independent of frame cadence, including a delayed frame.
    pub(in crate::script) fn sample(
        &mut self,
        total: Vec2,
        duration: Duration,
        elapsed: Duration,
    ) -> bool {
        if self.sampled >= duration {
            return true;
        }
        let until = elapsed.min(duration);
        self.delta =
            total * (until.saturating_sub(self.sampled).as_secs_f32() / duration.as_secs_f32());
        self.sampled = until;
        false
    }

    pub(in crate::script) fn clear_frame(&mut self) {
        self.delta = Vec2::ZERO;
        self.scroll = 0.0;
    }
}

/// Shares the existing right-press arbitration and wheel surface, after fresh
/// admission guards and before character/camera input. Script deltas remain
/// separate from native device messages, avoiding replay and self-takeover.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn apply(
    script: Option<Res<Script>>,
    online: Res<OnlineState>,
    mut input: ResMut<WorldInput>,
    mut look: ResMut<MouseLook>,
    mut aim: ResMut<crate::camera::MouseAim>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: ResMut<Wheel>,
) {
    let was_held = input.held;
    *input = WorldInput::default();
    let gesture = script
        .filter(|script| script.background_owns_input())
        .and_then(|script| {
            script
                .world_gesture
                .as_ref()
                .filter(|gesture| {
                    gesture.refusal(&online).is_none()
                        && online.selection.is_none()
                        && !online.world().is_dead()
                        && !online.world().is_unconscious()
                        && (!online.enabled || online.in_world())
                })
                .map(|gesture| (gesture.held, gesture.delta, gesture.scroll))
        });
    if let Some((held, delta, scroll)) = gesture {
        input.active = true;
        input.held = held;
        input.delta = delta;
        if held {
            // The executor already hit-tested the drawn script pointer over
            // visible world space. Once captured, ordinary look stays held.
            look.right(true, true, true);
            if !was_held {
                aim.acquired = true;
            }
            aim.interrupted = false;
        }
        *wheel = Wheel {
            pixels: scroll * crate::windows::pointer::LINE_PIXELS,
            lines: scroll,
            surface: None,
        };
    } else if was_held && !mouse.pressed(MouseButton::Right) {
        // A real right press belongs to native look now and survives takeover.
        look.right(false, false, false);
        aim.interrupted = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displacement_is_bounded_and_final_motion_precedes_release() {
        let mut gesture = WorldGesture::new(&OnlineState::new(false), true, 0.0);
        let total = Vec2::new(120.0, -40.0);
        let duration = Duration::from_millis(100);
        let mut sum = Vec2::ZERO;
        for ms in [10, 30, 300] {
            gesture.clear_frame();
            assert!(!gesture.sample(total, duration, Duration::from_millis(ms)));
            sum += gesture.delta;
        }
        assert!((sum - total).length() < 0.0001);
        gesture.clear_frame();
        assert!(gesture.sample(total, duration, Duration::from_millis(301)));
        assert_eq!(gesture.delta, Vec2::ZERO);
    }
}
