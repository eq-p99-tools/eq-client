//! Attached camera modes. Zoom changes the view, never the player's heading.
use super::{CameraBody, OrbitCamera, chase, place_orbit};
use bevy::prelude::*;
use eq_client_core::movement::CollisionWorld;

/// Snap in before reaching the chase floor. The outward step clears the
/// threshold by more than one normal wheel notch.
const SNAP_DISTANCE_SCALE: f32 = 2.0;
const RETURN_DISTANCE_SCALE: f32 = 1.25;

#[derive(Clone, Copy, Default)]
pub(crate) enum View {
    #[default]
    Chase,
    /// First person, looking at the character's own pitch
    /// ([`OrbitCamera::look`]).
    FirstPerson,
}

impl OrbitCamera {
    /// The attached view retained by this camera, independent of a detached view.
    pub(crate) const fn view_name(&self) -> &'static str {
        match self.view {
            View::Chase => "chase",
            View::FirstPerson => "first_person",
        }
    }

    /// Attached pitch; first-person look has its own installed-client limits.
    pub(crate) const fn view_pitch(&self) -> f32 {
        match self.view {
            View::FirstPerson => self.look,
            View::Chase => self.pitch,
        }
    }

    /// The character's look pitch in radians, upward positive.
    pub(crate) const fn look_pitch(&self) -> f32 {
        self.look
    }

    pub(crate) fn is_first_person(&self) -> bool {
        matches!(self.view, View::FirstPerson)
    }

    /// Puts a test's camera in first person, level, as the wheel would.
    #[cfg(test)]
    pub(crate) const fn look_first_person_for_test(&mut self) {
        self.view = View::FirstPerson;
        self.look = 0.0;
    }

    /// Returns to the chosen chase frame on a placement/lifetime change or
    /// an explicit script/preview frame, clearing collision easing as well.
    /// The character's look pitch stays; see [`Self::level_look`].
    pub(crate) fn reset_view(&mut self) {
        self.view = View::Chase;
        self.pulled = None;
    }

    /// Levels the character's look pitch, as each zone entry makes the
    /// player's spawn anew (inferred, recording checklist 6.480). Besides
    /// this, Center View in first person and a wheel change of view level
    /// it, so a pitch set in the chase view, where it does not show, stays
    /// until one of them.
    pub(crate) fn level_look(&mut self) {
        self.look = 0.0;
    }

    /// Switches on wheel input before collision shortens the chase view.
    /// First-person look keeps its own pitch and preserves the chase angle.
    pub(super) fn attached_view(
        &mut self,
        limits: eq_client_assets::camera::CameraLimits,
        tilt: f32,
        scroll: f32,
        world: Option<&CollisionWorld>,
        body: CameraBody,
        seconds: f32,
    ) -> Transform {
        let center = self.focus;
        let eye = body.eye(center, world);
        let zoom = (-scroll * 0.12).exp();
        // The chase camera's distance behind the player, which the wheel
        // moves and the snap into first person measures.
        let distance = self.behind_distance();
        let snap_distance = limits.chase_distance.0 * SNAP_DISTANCE_SCALE;
        let (tilt, zoom) = if let View::FirstPerson = self.view {
            if scroll >= 0.0 {
                let limit = limits.look_pitch_degrees.to_radians();
                self.look = (self.look - tilt).clamp(-limit, limit);
                return self.first_person_pose(self.look, eye);
            }

            // Place the chase camera this frame so the avatar can be drawn
            // immediately. Large outward turns still zoom past the gap. The
            // program's zoom hand-off levels the character's pitch on the
            // way out (0x4946ac, read R39 D).
            self.view = View::Chase;
            self.look = 0.0;
            let outward = snap_distance * zoom.max(RETURN_DISTANCE_SCALE);
            (0.0, outward / distance)
        } else {
            (tilt, zoom)
        };
        chase(self, limits, tilt, zoom);

        // Only an inward wheel input changes mode. Collision pulling the
        // camera close, or a script choosing a close frame, is not a zoom.
        if scroll > 0.0 && distance * zoom <= snap_distance {
            self.pulled = None;
            // On the way in the program copies its camera's pitch, whose
            // match to this chase pitch is unread, so the view opens level
            // as before (inferred, recording checklist 6.480).
            self.view = View::FirstPerson;
            self.look = 0.0;
            return self.first_person_pose(0.0, eye);
        }

        let clearance = body.feet.map(|feet| feet + (eye.y - center.y));
        place_orbit(self, world, eye, center, clearance, seconds)
    }

    /// Yaw and pitch define a rotation even when looking straight up or down.
    fn first_person_pose(&self, pitch: f32, eye: Vec3) -> Transform {
        Transform::from_translation(eye).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            self.yaw,
            pitch,
            0.0,
        ))
    }
}

#[cfg(test)]
mod tests;
