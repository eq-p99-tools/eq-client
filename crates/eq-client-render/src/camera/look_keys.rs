//! The program's look keys (0x4619cd, commands 17 to 19, read R13). Look Up
//! and Look Down tilt the first-person view while held, ramping as the turn
//! keys ramp; Center View levels it; and with Look Spring on, the view comes
//! back level by itself.
//!
//! The program's keys change the character's pitch in every camera mode,
//! and its chase cameras have pitch commands of their own. How a chase
//! camera uses the character's pitch is unread, so here the keys tilt only
//! the first-person view and the chase view ignores them (recording
//! checklist 6.422).
use super::{CHASE_PITCH, FreeCamera, MouseLook, OrbitCamera, attached::View, start_behind};
use crate::{Player, keys::Act, options};
use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

/// The program's pitch units in a quarter turn: it holds the pitch within
/// +/-128, straight up and straight down.
const UNITS_PER_QUARTER_TURN: f32 = 128.0;

/// The most steps the pitch ramp takes: the program adds one a frame up to
/// 12, as the turn ramp does. Each step is taken as the turn ramp's 20 units
/// a second, stepping 60 times a second (its frame scale is unread,
/// inferred, 6.422).
const PITCH_STEPS: f32 = 12.0;

/// How fast Look Spring levels the view, in pitch units a second: the
/// program takes 10 a frame, here 60 frames a second (inferred, 6.422).
const SPRING_RATE: f32 = 600.0;

/// Which look key holds the view, and for how long.
#[derive(Default)]
pub(crate) struct LookHold {
    /// Up (else down), and the seconds since its press; None while neither
    /// holds it.
    held: Option<(bool, f32)>,
    /// Whether each key was down last frame, so that only a fresh press
    /// takes the view.
    down: (bool, bool),
}

impl LookHold {
    /// Follows this frame's keys: a fresh press takes the view from the
    /// other key and starts its ramp over, and letting the key holding it
    /// go ends the hold. A key still down after Center View holds nothing
    /// until pressed again, as in the program.
    fn follow(&mut self, up: bool, down: bool) {
        let (was_up, was_down) = std::mem::replace(&mut self.down, (up, down));
        if up && !was_up {
            self.held = Some((true, 0.0));
        } else if down && !was_down {
            self.held = Some((false, 0.0));
        }
        if self
            .held
            .is_some_and(|(key_up, _)| !(if key_up { up } else { down }))
        {
            self.held = None;
        }
    }

    /// The view's tilt this frame in radians, upward positive, and the hold
    /// carried on by `seconds`.
    fn tilt(&mut self, seconds: f32) -> Option<f32> {
        let (up, since) = self.held?;
        let until = since + seconds;
        self.held = Some((up, until));
        let units = crate::motion::ramped_turn(since, until, PITCH_STEPS);
        let tilt = units * FRAC_PI_2 / UNITS_PER_QUARTER_TURN;
        Some(if up { tilt } else { -tilt })
    }
}

/// Runs the look keys before the camera places its view.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn look_keys(
    keys: crate::keys::Keys,
    navigation: Res<crate::navigation::NavigationKeys>,
    time: Res<Time>,
    look: Res<MouseLook>,
    options: Res<options::OptionsState>,
    settings: Option<Res<crate::ViewerSettings>>,
    mut hold: Local<LookHold>,
    mut cameras: Query<&mut OrbitCamera, Without<FreeCamera>>,
    players: Query<&Transform, (With<Player>, Without<OrbitCamera>)>,
) {
    let focused = keys.focused();
    let input = navigation.sample(&keys.input);
    let look_key = |act| focused && keys.map.held_exactly(&input, act);
    hold.follow(look_key(Act::LookUp), look_key(Act::LookDown));
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    if keys.pressed(Act::CenterView) {
        hold.held = None;
        if camera.is_first_person() {
            camera.view = View::FirstPerson { pitch: 0.0 };
        } else {
            // The program calls the chase camera's own reset, unread; here it
            // returns to the view's opening pitch behind the player (inferred,
            // 6.422).
            camera.pitch = CHASE_PITCH;
            if let Ok(player) = players.single() {
                start_behind(&mut camera, player);
            }
        }
        return;
    }
    let View::FirstPerson { pitch } = camera.view else {
        // The ramp still counts in the chase view, as the program's does.
        hold.tilt(time.delta_secs());
        return;
    };
    let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
    let limit = client.camera_limits().look_pitch_degrees.to_radians();
    let pitch = if let Some(tilt) = hold.tilt(time.delta_secs()) {
        (pitch + tilt).clamp(-limit, limit)
    } else if options.options.look_spring && !look.held {
        let step = SPRING_RATE * time.delta_secs() * FRAC_PI_2 / UNITS_PER_QUARTER_TURN;
        pitch.signum() * (pitch.abs() - step).max(0.0)
    } else {
        return;
    };
    camera.view = View::FirstPerson { pitch };
}

#[cfg(test)]
mod tests;
