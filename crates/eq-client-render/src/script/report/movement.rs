use crate::{
    PlayerBody,
    online::{OnlineState, PlacementSource, ServerPlacement},
};
use bevy::prelude::*;
use eq_client_core::{
    PlayerState, WorldPosition,
    movement::{CollisionWorld, Liquid, LiquidContact, SwimDepth},
    world::ClientWorld,
};
use serde_json::{Value, json};
use std::time::Instant;

use super::Observed;

/// A bounded observation. The session position is the basis for local physics
/// and can include locally accepted samples held for pacing. The renderer
/// interpolates separately.
pub(in crate::script) fn report(
    label: &str,
    online: &OnlineState,
    transform: Option<&Transform>,
    body: Option<&PlayerBody>,
    collision: Option<&CollisionWorld>,
    observed: &Observed,
    camera_detached: Option<bool>,
) {
    let world = online.world();
    let admission = world
        .session_id()
        .filter(|_| world.connected() && !world.is_dead());
    let player = admission.and(world.player());
    let session_position = player.map(|player| player.position);
    let rendered_position = admission.and(transform).map(super::placement);
    let liquid = session_position.zip(body.zip(collision)).map_or(
        LiquidObservation::UNKNOWN,
        |(position, (body, collision))| {
            let origin = Vec3::from_array(eq_client_core::render_position(position));
            let feet = origin - Vec3::Y * body.feet_offset;
            LiquidObservation {
                contact: collision.liquid_contact(feet, body.height),
                origin: collision.liquid_at(origin),
                surface: collision.liquid_surface_at_feet(feet),
            }
        },
    );
    let scene_loaded = !online.scene_loading();
    let local_geometry = player.map(|player| {
        LocalGeometry::read(
            world,
            player,
            body.filter(|_| scene_loaded),
            collision.filter(|_| scene_loaded),
        )
    });
    let mut movement_state = snapshot(
        admission,
        session_position,
        rendered_position,
        online.server_placement(),
        &liquid,
        world.can(eq_client_core::Capability::Swimming),
        local_geometry,
    );
    movement_state["last_local_pad"] = json!(
        world
            .last_local_pad()
            .filter(|pad| Some(pad.session_id) == admission)
    );
    movement_state["settings"] = json!(observed.14.as_ref().zip(observed.9.as_ref()).map(
        |(sync, state)| {
            let wanted = eq_client_core::movement::offered_options(&state.options.qol, world);
            sync.report(admission, wanted, Instant::now())
        },
    ));
    movement_state["camera_detached"] = json!(camera_detached);
    info!(label, %movement_state, "Script movement report");
}

struct LiquidObservation {
    contact: LiquidContact,
    origin: Option<Liquid>,
    surface: Option<Liquid>,
}

impl LiquidObservation {
    const UNKNOWN: Self = Self {
        contact: LiquidContact::Unknown,
        origin: None,
        surface: None,
    };
}

fn liquid_name(liquid: Liquid) -> &'static str {
    match liquid {
        Liquid::Water => "water",
        Liquid::Lava => "lava",
    }
}

struct LocalGeometry {
    admitted_model_size: Option<f32>,
    body: Option<PlayerBody>,
    collision_ready: bool,
    support_z_at_session_feet: Option<f32>,
    /// The door whose floor is under the centre of the feet, if a door's,
    /// so a report tells a lift's floor from the zone's own.
    support_door_at_session_feet: Option<u8>,
    selected_target_los: Option<LocalTargetLos>,
}

impl LocalGeometry {
    /// Projects the renderer's body and geometry for the admitted player.
    fn read(
        world: &ClientWorld,
        player: &PlayerState,
        body: Option<&PlayerBody>,
        collision: Option<&CollisionWorld>,
    ) -> Self {
        let origin = Vec3::from_array(eq_client_core::render_position(player.position));
        Self {
            admitted_model_size: (player.size.is_finite() && player.size > 0.0)
                .then_some(player.size),
            body: body.copied(),
            collision_ready: collision.is_some(),
            support_z_at_session_feet: collision.zip(body).and_then(|(collision, body)| {
                collision.support(origin - Vec3::Y * body.feet_offset)
            }),
            support_door_at_session_feet: collision.zip(body).and_then(|(collision, body)| {
                collision.ground_door(origin - Vec3::Y * body.feet_offset, 0.01, 0.05)
            }),
            selected_target_los: collision
                .and_then(|collision| LocalTargetLos::read(world, collision, origin)),
        }
    }

    fn json(self) -> Value {
        json!({
            "admitted_model_size": self.admitted_model_size,
            "body": self.body.map(|body| json!({
                "size": body.size,
                "feet_offset": body.feet_offset,
            })),
            "collision_ready": self.collision_ready,
            "support_z_at_session_feet": self.support_z_at_session_feet,
            "support_door_at_session_feet": self.support_door_at_session_feet,
            "selected_target_los": self.selected_target_los.map(|line| json!({
                "spawn_id": line.spawn_id,
                "spawn_revision": line.spawn_revision,
                "clear": line.clear,
            })),
        })
    }
}

struct LocalTargetLos {
    spawn_id: u16,
    spawn_revision: u64,
    clear: bool,
}

impl LocalTargetLos {
    /// Tests the straight segment from the session origin to the selected
    /// spawn's reported origin against the client's solid collision only.
    fn read(world: &ClientWorld, collision: &CollisionWorld, origin: Vec3) -> Option<Self> {
        let spawn_id = world.target().selected.filter(|_| !world.target_stale())?;
        let spawn = world.spawn(spawn_id)?;
        let target = Vec3::from_array(eq_client_core::render_position(spawn.state.position));
        let offset = target - origin;
        let direction = offset.try_normalize()?;
        Some(Self {
            spawn_id,
            spawn_revision: spawn.revision,
            clear: collision
                .ray_distance(origin, direction, offset.length())
                .is_none(),
        })
    }
}

fn snapshot(
    admission: Option<u64>,
    session_position: Option<WorldPosition>,
    rendered_position: Option<(f32, f32, f32, f32)>,
    server: Option<ServerPlacement>,
    liquid: &LiquidObservation,
    swimming_offered: bool,
    local_geometry: Option<LocalGeometry>,
) -> Value {
    let received = server
        .filter(|entry| Some(entry.admission_id) == admission)
        .map(|entry| {
            json!({
                "source": match entry.source {
                    PlacementSource::Admission => "admission",
                    PlacementSource::PositionUpdate => "position_update",
                    PlacementSource::SameZoneRelocation => "same_zone_relocation",
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
        "liquid_contact_at_session_position": match liquid.contact {
            LiquidContact::Unknown => "unknown",
            LiquidContact::Unmarked => "unmarked",
            LiquidContact::Wading => "wading",
            LiquidContact::Swimming(SwimDepth::Surface) => "surface",
            LiquidContact::Swimming(SwimDepth::Submerged) => "submerged",
            LiquidContact::Lava => "lava",
        },
        "mapped_liquid_at_session_origin": liquid.origin.map(liquid_name),
        "mapped_liquid_surface_at_session_feet": liquid.surface.map(liquid_name),
        "swimming_offered": swimming_offered,
        "local_geometry": local_geometry.map(LocalGeometry::json),
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
                created: std::time::Instant::now(),
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
            &LiquidObservation {
                contact: LiquidContact::Swimming(SwimDepth::Submerged),
                origin: Some(Liquid::Water),
                surface: None,
            },
            true,
            None,
        );
        assert_eq!(value["session_position"]["z"], 3.0);
        assert_eq!(value["rendered_position"]["z"], 4.0);
        assert_eq!(value["last_server_placement"]["position"]["z"], 8.0);
        assert_eq!(value["last_server_placement"]["source"], "position_update");
        assert_eq!(value["liquid_contact_at_session_position"], "submerged");
        assert_eq!(value["mapped_liquid_at_session_origin"], "water");
        assert!(value["mapped_liquid_surface_at_session_feet"].is_null());

        let newer = snapshot(
            Some(3),
            None,
            None,
            Some(received),
            &LiquidObservation::UNKNOWN,
            true,
            None,
        );
        assert!(newer["last_server_placement"].is_null());
        assert!(newer["local_geometry"].is_null());
        assert_eq!(newer["liquid_contact_at_session_position"], "unknown");
        let over_lava = snapshot(
            Some(2),
            Some(session),
            None,
            None,
            &LiquidObservation {
                contact: LiquidContact::Unmarked,
                origin: None,
                surface: Some(Liquid::Lava),
            },
            true,
            None,
        );
        assert_eq!(over_lava["liquid_contact_at_session_position"], "unmarked");
        assert!(over_lava["mapped_liquid_at_session_origin"].is_null());
        assert_eq!(over_lava["mapped_liquid_surface_at_session_feet"], "lava");
    }

    #[test]
    fn local_geometry_reports_floor_support_and_the_current_target_ray() {
        use crate::online::testing;
        use eq_client_core::{WorldEvent, targets::TargetState};

        let mut online = OnlineState::new(true);
        let mut player = testing::player(4);
        player.size = 7.0;
        player.position.z = 4.375;
        let mut target = testing::pet(8, 4);
        target.position = WorldPosition {
            y: 8.0,
            ..player.position
        };
        testing::admit(&mut online, 7, player);
        testing::spawns(&mut online, vec![target.clone()]);
        testing::news(
            &mut online,
            [WorldEvent::TargetChanged {
                session_id: 7,
                request_id: 0,
                state: TargetState {
                    revision: 1,
                    spawn_id: Some(8),
                    available: true,
                },
            }],
        );

        let body = PlayerBody {
            size: 7.0,
            feet_offset: 4.375,
            height: 6.0,
        };
        let floor = [
            [[-2.0, 0.0, -2.0], [10.0, 0.0, -2.0], [10.0, 0.0, 2.0]],
            [[-2.0, 0.0, -2.0], [10.0, 0.0, 2.0], [-2.0, 0.0, 2.0]],
        ];
        let wall = [
            [[4.0, 0.0, -2.0], [4.0, 8.0, -2.0], [4.0, 8.0, 2.0]],
            [[4.0, 0.0, -2.0], [4.0, 8.0, 2.0], [4.0, 0.0, 2.0]],
        ];
        let open = CollisionWorld::new(floor).unwrap();
        let blocked = CollisionWorld::new(floor.into_iter().chain(wall)).unwrap();
        let read = |online: &OnlineState, collision| {
            let world = online.world();
            LocalGeometry::read(world, world.player().unwrap(), Some(&body), collision).json()
        };

        let open_report = read(&online, Some(&open));
        assert_eq!(open_report["admitted_model_size"], 7.0);
        assert_eq!(open_report["body"]["feet_offset"], 4.375);
        assert_eq!(open_report["collision_ready"], true);
        assert!(
            open_report["support_z_at_session_feet"]
                .as_f64()
                .unwrap()
                .abs()
                < 0.001
        );
        assert_eq!(open_report["selected_target_los"]["spawn_id"], 8);
        assert_eq!(open_report["selected_target_los"]["clear"], true);
        assert_eq!(
            read(&online, Some(&blocked))["selected_target_los"]["clear"],
            false
        );

        let missing = read(&online, None);
        assert_eq!(missing["collision_ready"], false);
        assert_eq!(missing["body"]["size"], 7.0);
        assert!(missing["support_z_at_session_feet"].is_null());
        assert!(missing["support_door_at_session_feet"].is_null());
        assert!(missing["selected_target_los"].is_null());

        let world = online.world();
        let mut no_size = world.player().unwrap().clone();
        no_size.size = 0.0;
        let fallback = LocalGeometry::read(world, &no_size, Some(&body), None).json();
        assert!(fallback["admitted_model_size"].is_null());
        assert_eq!(fallback["body"]["size"], 7.0);

        testing::news(
            &mut online,
            [WorldEvent::Despawn(8), WorldEvent::Spawns(vec![target])],
        );
        assert!(read(&online, Some(&open))["selected_target_los"].is_null());
    }
}
