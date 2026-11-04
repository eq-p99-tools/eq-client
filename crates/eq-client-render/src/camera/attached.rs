//! Attached camera modes. Zoom changes the view, never the player's heading.
use super::{HEIGHT_PER_RADIAN, OrbitCamera, chase, eased_distance, unobstructed_orbit};
use bevy::prelude::*;
use eq_client_core::movement::CollisionWorld;

#[derive(Clone, Copy, Default)]
pub(crate) enum View {
    #[default]
    Chase,
    FirstPerson {
        pitch: f32,
    },
    Returning {
        offset: Vec3,
        rotation: Quat,
    },
}

impl OrbitCamera {
    /// The attached view retained by this camera, independent of a detached view.
    pub(crate) const fn view_name(&self) -> &'static str {
        match self.view {
            View::Chase => "chase",
            View::FirstPerson { .. } => "first_person",
            View::Returning { .. } => "returning",
        }
    }

    /// Attached pitch; first-person look has its own installed-client limits.
    pub(crate) const fn view_pitch(&self) -> f32 {
        match self.view {
            View::FirstPerson { pitch } => pitch,
            _ => self.pitch,
        }
    }

    pub(super) fn inside_body(&self) -> bool {
        !matches!(self.view, View::Chase)
    }

    /// Returns to the chosen chase frame on a placement/lifetime change or
    /// an explicit script/preview frame, clearing collision easing as well.
    pub(crate) fn reset_view(&mut self) {
        self.view = View::Chase;
        self.pulled = None;
    }

    /// Handles wheel mode changes and first-person look through the ordinary
    /// camera input owner. Chase pitch/zoom survive a visit to first person.
    pub(super) fn attached_view(
        &mut self,
        limits: eq_client_assets::camera::CameraLimits,
        tilt: f32,
        scroll: f32,
        world: Option<&CollisionWorld>,
        head: f32,
        seconds: f32,
    ) -> Transform {
        let eye = eye_offset(self.focus, head, world);
        if let View::FirstPerson { pitch } = &mut self.view {
            let limit = limits.look_pitch_degrees.to_radians();
            *pitch = (*pitch - tilt).clamp(-limit, limit);
            let rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, *pitch, 0.0);
            if scroll >= 0.0 {
                return Transform::from_translation(self.focus + eye).with_rotation(rotation);
            }
            self.view = View::Returning {
                offset: eye,
                rotation,
            };
        }

        let zoom = (-scroll * 0.12).exp();
        let distance = self.radius * self.pitch.cos() * zoom;
        chase(self, limits, tilt * HEIGHT_PER_RADIAN, zoom);
        // Only an inward wheel input changes mode. Collision pulling the
        // camera close, or a script choosing a close frame, is not a zoom.
        if scroll > 0.0 && distance <= limits.chase_distance.0 {
            self.pulled = None;
            self.view = View::FirstPerson { pitch: 0.0 };
            return Transform::from_translation(self.focus + eye)
                .with_rotation(Quat::from_rotation_y(self.yaw));
        }

        let target = unobstructed_orbit(self, world, head, seconds);
        let View::Returning { offset, rotation } = self.view else {
            return target;
        };
        let share = eased_distance(0.0, 1.0, seconds);
        let next = offset.lerp(target.translation - self.focus, share);
        let next = clear_offset(self.focus + eye, next - eye, world) + eye;
        let rotation = rotation.slerp(target.rotation, share);
        if next.distance(target.translation - self.focus) < 0.05
            && rotation.angle_between(target.rotation) < 0.01
        {
            self.view = View::Chase;
            target
        } else {
            self.view = View::Returning {
                offset: next,
                rotation,
            };
            Transform::from_translation(self.focus + next).with_rotation(rotation)
        }
    }
}

// Reuse the body's existing head line until authored model eye positions
// are available. A low ceiling must not put the eye through its surface.
fn eye_offset(focus: Vec3, head: f32, world: Option<&CollisionWorld>) -> Vec3 {
    clear_offset(focus, Vec3::Y * head.max(0.0), world)
}

fn clear_offset(origin: Vec3, offset: Vec3, world: Option<&CollisionWorld>) -> Vec3 {
    let length = offset.length();
    let Ok(direction) = Dir3::new(offset) else {
        return Vec3::ZERO;
    };
    world
        .and_then(|world| world.ray_distance(origin, *direction, length))
        .map_or(offset, |hit| *direction * (hit - 0.1).max(0.0))
}

#[cfg(test)]
mod tests;
