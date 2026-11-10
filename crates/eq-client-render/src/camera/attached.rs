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
    FirstPerson {
        pitch: f32,
    },
}

impl OrbitCamera {
    /// The attached view retained by this camera, independent of a detached view.
    pub(crate) const fn view_name(&self) -> &'static str {
        match self.view {
            View::Chase => "chase",
            View::FirstPerson { .. } => "first_person",
        }
    }

    /// Attached pitch; first-person look has its own installed-client limits.
    pub(crate) const fn view_pitch(&self) -> f32 {
        match self.view {
            View::FirstPerson { pitch } => pitch,
            View::Chase => self.pitch,
        }
    }

    pub(crate) fn is_first_person(&self) -> bool {
        matches!(self.view, View::FirstPerson { .. })
    }

    /// Returns to the chosen chase frame on a placement/lifetime change or
    /// an explicit script/preview frame, clearing collision easing as well.
    pub(crate) fn reset_view(&mut self) {
        self.view = View::Chase;
        self.pulled = None;
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
        let (tilt, zoom) = if let View::FirstPerson { pitch } = self.view {
            if scroll >= 0.0 {
                let limit = limits.look_pitch_degrees.to_radians();
                let pitch = (pitch - tilt).clamp(-limit, limit);
                self.view = View::FirstPerson { pitch };
                return self.first_person_pose(pitch, eye);
            }

            // Place the chase camera this frame so the avatar can be drawn
            // immediately. Large outward turns still zoom past the gap.
            self.view = View::Chase;
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
            self.view = View::FirstPerson { pitch: 0.0 };
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
