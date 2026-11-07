//! A local safety hold for a received placement with no supported path down.
use crate::{PlayerBody, chat, online::ServerPlacement};
use bevy::prelude::*;
use eq_client_core::{
    WorldPosition,
    movement::{CollisionWorld, LiquidContact},
};
use std::time::{Duration, Instant};

/// Titanium's explicit relocation floor probe starts one unit above the
/// received origin. Ordinary fall and stair rules use their own probes.
const TITANIUM_PLACEMENT_PROBE: f32 = 1.0;

/// Doors can change collision within an admission. A held placement gets a
/// new probe without making every frame pay for the footprint raycasts.
const RECHECK_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy)]
enum Assessment {
    Clear,
    Held { recheck_at: Instant },
}

/// Caches the geometry check for the current received placement.
#[derive(Clone, Copy, Default)]
pub(super) struct Check {
    result: Option<(ServerPlacement, Assessment)>,
}

fn same_xyz(first: WorldPosition, second: WorldPosition) -> bool {
    [first.x, first.y, first.z]
        .into_iter()
        .eq([second.x, second.y, second.z])
}

impl Check {
    /// Holds travel while its starting point is still the suspect received
    /// placement and the installed geometry offers no way down. The retry
    /// deadline schedules another collision probe.
    pub(super) fn holds(
        &mut self,
        server: Option<ServerPlacement>,
        position: WorldPosition,
        collision: &CollisionWorld,
        body: &PlayerBody,
        chat: &mut chat::ChatState,
        now: Instant,
    ) -> bool {
        let Some(server) = server.filter(|entry| same_xyz(entry.position, position)) else {
            self.result = None;
            return false;
        };
        let previous = self
            .result
            .filter(|(placement, _)| {
                placement.admission_id == server.admission_id
                    && same_xyz(placement.position, server.position)
            })
            .map(|(_, assessment)| assessment);
        match previous {
            Some(Assessment::Clear) => return false,
            Some(Assessment::Held { recheck_at }) if now < recheck_at => return true,
            _ => {}
        }
        let origin = Vec3::from_array(eq_client_core::render_position(position));
        let feet = origin - Vec3::Y * body.feet_offset;
        let floor = match collision.liquid_contact(feet, body.height) {
            LiquidContact::Unknown | LiquidContact::Unmarked => {
                collision.floor_above_placement(origin, TITANIUM_PLACEMENT_PROBE, body.size)
            }
            _ => None,
        }
        .filter(|_| !collision.liquid_below(feet));
        if let Some(floor) = floor.filter(|_| previous.is_none()) {
            warn!(
                admission = server.admission_id,
                source = ?server.source,
                received = ?server.position,
                installed_floor_z = floor,
                "Movement held below the installed floor"
            );
            chat.history.push(chat::system_line(
                "The server placed you below the installed floor. Travel is paused.",
            ));
        }
        let assessment = if floor.is_some() {
            Assessment::Held {
                recheck_at: now + RECHECK_INTERVAL,
            }
        } else {
            Assessment::Clear
        };
        self.result = Some((server, assessment));
        floor.is_some()
    }
}
