//! Diagnose a fresh placement that would send gravity into empty space.
use crate::{PlayerBody, chat, online::ServerPlacement};
use bevy::prelude::*;
use eq_client_core::{
    WorldPosition,
    movement::{CollisionWorld, LiquidContact},
};

/// Titanium's explicit relocation floor probe starts one unit above the
/// received origin. Ordinary fall and stair rules use their own probes.
const TITANIUM_PLACEMENT_PROBE: f32 = 1.0;

/// Caches the geometry check for the current received placement.
#[derive(Clone, Copy, Default)]
pub(super) struct Check {
    result: Option<(ServerPlacement, bool)>,
}

impl Check {
    /// Holds new motion only while its starting point is still the suspect
    /// received placement. A later server correction gets its own check.
    pub(super) fn holds(
        &mut self,
        server: Option<ServerPlacement>,
        position: WorldPosition,
        collision: &CollisionWorld,
        body: &PlayerBody,
        chat: &mut chat::ChatState,
    ) -> bool {
        let Some(server) = server.filter(|entry| entry.position == position) else {
            self.result = None;
            return false;
        };
        if let Some((_, held)) = self.result.filter(|(previous, _)| *previous == server) {
            return held;
        }
        let origin = Vec3::from_array(eq_client_core::render_position(position));
        let feet = origin - Vec3::Y * body.feet_offset;
        let floor = match collision.liquid_contact(feet, body.height) {
            LiquidContact::Unknown | LiquidContact::Unmarked => {
                collision.floor_above_placement(origin, TITANIUM_PLACEMENT_PROBE, body.size)
            }
            _ => None,
        };
        if let Some(floor) = floor {
            warn!(
                admission = server.admission_id,
                source = ?server.source,
                received = ?position,
                installed_floor_z = floor,
                "Movement held below the installed floor"
            );
            chat.history.push(chat::system_line(
                "The server placed you below the installed floor. Waiting for a new server placement.",
            ));
        }
        self.result = Some((server, floor.is_some()));
        floor.is_some()
    }
}
