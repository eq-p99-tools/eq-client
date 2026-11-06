use crate::{
    PlayerBody,
    online::{OnlineState, PlacementSource, ServerPlacement},
};
use bevy::prelude::*;
use eq_client_core::{
    WorldPosition,
    movement::{CollisionWorld, LiquidContact, SwimDepth},
};
use serde_json::{Value, json};

/// A bounded observation. The session position is the basis for local physics
/// and can include submitted proposals. The renderer interpolates separately.
pub(in crate::script) fn report(
    label: &str,
    online: &OnlineState,
    transform: Option<&Transform>,
    geometry: Option<(&PlayerBody, &CollisionWorld)>,
) {
    let world = online.world();
    let admission = world
        .session_id()
        .filter(|_| world.connected() && !world.is_dead());
    let session_position = admission.and_then(|_| world.player().map(|p| p.position));
    let rendered_position = admission.and_then(|_| transform).map(super::placement);
    let contact = session_position.zip(geometry).map_or(
        LiquidContact::Unknown,
        |(position, (body, collision))| {
            let origin = Vec3::from_array(eq_client_core::render_position(position));
            collision.liquid_contact(origin - Vec3::Y * body.feet_offset, body.height)
        },
    );
    let movement_state = snapshot(
        admission,
        session_position,
        rendered_position,
        online.server_placement(),
        contact,
        world.can(eq_client_core::Capability::Swimming),
    );
    info!(label, %movement_state, "Script movement report");
}

fn snapshot(
    admission: Option<u64>,
    session_position: Option<WorldPosition>,
    rendered_position: Option<(f32, f32, f32, f32)>,
    server: Option<ServerPlacement>,
    contact: LiquidContact,
    swimming_offered: bool,
) -> Value {
    let received = server
        .filter(|entry| Some(entry.admission_id) == admission)
        .map(|entry| {
            json!({
                "source": match entry.source {
                    PlacementSource::Admission => "admission",
                    PlacementSource::PositionUpdate => "position_update",
                },
                "position": entry.position,
            })
        });
    let rendered = rendered_position.map(|(x, y, z, heading)| WorldPosition { x, y, z, heading });
    json!({
        "admission_id": admission,
        "session_position": session_position,
        "rendered_position": rendered,
        "last_server_placement": received,
        "liquid_contact_at_session_position": match contact {
            LiquidContact::Unknown => "unknown",
            LiquidContact::Unmarked => "unmarked",
            LiquidContact::Wading => "wading",
            LiquidContact::Swimming(SwimDepth::Surface) => "surface",
            LiquidContact::Swimming(SwimDepth::Submerged) => "submerged",
            LiquidContact::Lava => "lava",
        },
        "swimming_offered": swimming_offered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_proposal_does_not_replace_the_received_placement() {
        use crate::online::testing;
        use eq_client_core::WorldEvent;

        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(4));
        let admission = online.server_placement().unwrap();
        assert!(matches!(admission.source, PlacementSource::Admission));
        let proposed = WorldPosition {
            z: -3.0,
            ..admission.position
        };
        testing::news(
            &mut online,
            [WorldEvent::MotionSent {
                session_id: 7,
                position: proposed,
                refused: None,
            }],
        );
        assert_eq!(online.world().player().unwrap().position, proposed);
        assert_eq!(
            online.server_placement().unwrap().position,
            admission.position
        );

        let placed = WorldPosition {
            z: 4.0,
            ..admission.position
        };
        testing::place(&mut online, placed);
        let received = online.server_placement().unwrap();
        assert_eq!(received.position, placed);
        assert!(matches!(received.source, PlacementSource::PositionUpdate));
    }

    #[test]
    fn script_reports_geometry_at_the_session_position_and_scopes_server_placement() {
        let received = ServerPlacement {
            admission_id: 2,
            position: WorldPosition {
                z: 8.0,
                ..WorldPosition::default()
            },
            source: PlacementSource::PositionUpdate,
        };
        let session = WorldPosition {
            z: 3.0,
            ..WorldPosition::default()
        };
        let value = snapshot(
            Some(2),
            Some(session),
            Some((0.0, 0.0, 4.0, 0.0)),
            Some(received),
            LiquidContact::Swimming(SwimDepth::Submerged),
            true,
        );
        assert_eq!(value["session_position"]["z"], 3.0);
        assert_eq!(value["rendered_position"]["z"], 4.0);
        assert_eq!(value["last_server_placement"]["position"]["z"], 8.0);
        assert_eq!(value["last_server_placement"]["source"], "position_update");
        assert_eq!(value["liquid_contact_at_session_position"], "submerged");

        let newer = snapshot(
            Some(3),
            None,
            None,
            Some(received),
            LiquidContact::Unknown,
            true,
        );
        assert!(newer["last_server_placement"].is_null());
        assert_eq!(newer["liquid_contact_at_session_position"], "unknown");
    }
}
