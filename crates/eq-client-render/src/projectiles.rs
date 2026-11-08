//! Brief, admission-scoped visuals for received item flights.
use crate::{SceneEntity, asset_loads::Asset, item_models::ItemLibrary, online::OnlineState};
use bevy::prelude::*;
use eq_client_core::{
    SpawnKind,
    effects::Projectile,
    render_position, render_vector,
    world::{ClientWorld, Reset},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

const MAX_FLIGHTS: usize = 48;
const MAX_MODELS: usize = 64;
const MAX_RANGE: f32 = 240.0;
const DISPLAY_UNITS_PER_SECOND: f32 = 100.0;

#[derive(Resource, Default)]
pub(super) struct Flights {
    active: BTreeMap<u64, Flight>,
    models: BTreeSet<String>,
    next: u64,
    received: u64,
    spawned: u64,
    scene_instances: usize,
}

/// A spawn ID is usable only for the incarnation known at receipt.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Actor {
    id: u16,
    revision: Option<u64>,
}

impl Actor {
    fn current(world: &ClientWorld, id: u16) -> Option<Self> {
        let revision = match world.spawn(id) {
            Some(spawn) if matches!(spawn.state.kind, SpawnKind::Player | SpawnKind::Npc) => {
                Some(spawn.revision)
            }
            None if world.player().is_some_and(|player| player.spawn_id == id) => None,
            _ => return None,
        };
        Some(Self { id, revision })
    }

    /// Aim above the actor's reported placement using its reported size.
    fn position(self, world: &ClientWorld, now: Instant) -> Option<Vec3> {
        if Self::current(world, self.id)? != self {
            return None;
        }
        let (at, size) = if let Some(player) = world.player().filter(|p| p.spawn_id == self.id) {
            (player.position, player.size)
        } else {
            let spawn = world.spawn(self.id)?;
            (spawn.carried(world.generation(), now), spawn.state.size)
        };
        let point = Vec3::from_array(render_position(at)) + Vec3::Y * size.clamp(0.0, 30.0) * 0.3;
        point.is_finite().then_some(point)
    }
}

struct Flight {
    admission: u64,
    source: Actor,
    target: Actor,
    model: String,
    origin: Vec3,
    launched: Instant,
    duration: Duration,
}

impl Flight {
    fn new(world: &ClientWorld, event: &Projectile, now: Instant) -> Option<Self> {
        let admission = world.session_id().filter(|_| world.in_world())?;
        let source = Actor::current(world, event.source_id)?;
        let target = Actor::current(world, event.target_id?)?;
        let origin = Vec3::from_array(render_vector(event.start));
        let distance = origin.distance(target.position(world, now)?);
        let observer = Vec3::from_array(render_position(world.player()?.position));
        if source == target
            || !(0.01..=MAX_RANGE).contains(&distance)
            || !observer.is_finite()
            || origin.distance_squared(observer) > MAX_RANGE * MAX_RANGE
        {
            return None;
        }
        if !event.velocity.is_finite() || event.velocity <= 0.0 {
            return None;
        }
        // Draw a bounded straight path from the packet's origin. Native
        // integration of launch angle, tilt, arc and velocity is unverified.
        let duration =
            Duration::from_secs_f32((distance / DISPLAY_UNITS_PER_SECOND).clamp(0.12, 1.4));
        Some(Self {
            admission,
            source,
            target,
            model: item_model(event)?,
            origin,
            launched: now,
            duration,
        })
    }

    /// Interpolate toward the same actor, ending at expiry or on a stale endpoint.
    fn transform(&self, world: &ClientWorld, now: Instant) -> Option<Transform> {
        let age = now.saturating_duration_since(self.launched);
        if age >= self.duration
            || world.session_id() != Some(self.admission)
            || !world.in_world()
            || Actor::current(world, self.source.id) != Some(self.source)
        {
            return None;
        }
        let target = self.target.position(world, now)?;
        let delta = target - self.origin;
        if !(0.01..=MAX_RANGE).contains(&delta.length()) {
            return None;
        }
        let progress = age.as_secs_f32() / self.duration.as_secs_f32();
        Some(
            Transform::from_translation(self.origin.lerp(target, progress))
                .with_rotation(Quat::from_rotation_arc(Vec3::X, delta.normalize())),
        )
    }
}

/// Accept only the ordinary item model family for arrows and throwing items.
fn item_model(event: &Projectile) -> Option<String> {
    // EQEmu's large throwing, small throwing and arrow item types.
    if !matches!(event.item_type, 7 | 19 | 27)
        || !matches!(event.behavior, None | Some(1))
        || event.item_id.is_none_or(|id| id == 0)
    {
        return None;
    }
    let key = eq_client_assets::model_key(&event.model);
    let digits = key.strip_prefix("IT")?;
    (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(key)
}

impl Flights {
    /// Bind the received event before another update can replace a spawn ID.
    pub(super) fn receive(&mut self, world: &ClientWorld, event: &Projectile, now: Instant) {
        self.received = self.received.saturating_add(1);
        let Some(flight) = Flight::new(world, event, now) else {
            return;
        };
        if self.active.len() >= MAX_FLIGHTS
            || (self.models.len() >= MAX_MODELS && !self.models.contains(&flight.model))
        {
            return;
        }
        self.models.insert(flight.model.clone());
        self.next = self.next.wrapping_add(1);
        self.active.insert(self.next, flight);
    }

    /// End every flight, even when a refused transfer restores the admission
    /// before the next rendered frame.
    pub(super) fn reset(&mut self, reason: Reset) {
        self.active.clear();
        self.received = 0;
        self.spawned = 0;
        self.scene_instances = 0;
        if matches!(
            reason,
            Reset::Entered | Reset::Camped | Reset::Lost { ended: true, .. }
        ) {
            self.models.clear();
        }
    }

    /// Report counts by presentation stage, without treating a flight as a hit.
    pub(super) fn report(&self, label: &str, admission: Option<u64>) {
        info!(
            label,
            ?admission,
            received = self.received,
            tracked = self.active.len(),
            spawned = self.spawned,
            scene_instances = self.scene_instances,
            "Script projectile report"
        );
    }
}

#[derive(Component)]
pub(super) struct Drawn {
    id: u64,
    axis_to_forward: Quat,
}

/// Face the mesh's long axis along the flight using its installed bounds.
fn axis_to_forward(bounds: Option<(Vec3, Vec3)>) -> Quat {
    let Some((min, max)) = bounds else {
        return Quat::IDENTITY;
    };
    let extent = max - min;
    let axis = if extent.y > extent.x && extent.y > extent.z {
        Vec3::Y
    } else if extent.z > extent.x {
        Vec3::Z
    } else {
        Vec3::X
    };
    Quat::from_rotation_arc(axis, Vec3::X)
}

/// Update the zone's visual-only flight entities with the shared item library.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(super) fn update(
    mut commands: Commands,
    settings: Res<crate::ViewerSettings>,
    online: Res<OnlineState>,
    scene: Option<Res<crate::doors::Models>>,
    mut flights: ResMut<Flights>,
    mut library: ResMut<ItemLibrary>,
    mut drawn: Query<(Entity, &Drawn, &mut Transform)>,
) {
    let now = Instant::now();
    let mut wanted = BTreeMap::new();
    flights.active.retain(|&id, flight| {
        let Some(transform) = flight.transform(online.world(), now) else {
            return false;
        };
        if scene.is_some() {
            wanted.insert(id, transform);
        }
        true
    });
    let mut instances = 0;
    for (entity, marker, mut transform) in &mut drawn {
        if let Some(mut next) = wanted.remove(&marker.id) {
            next.rotation *= marker.axis_to_forward;
            *transform = next;
            instances += 1;
        } else {
            commands.entity(entity).despawn();
        }
    }

    for (id, mut transform) in wanted {
        let flight = &flights.active[&id];
        let shape = match library.shape(&flight.model, settings.0.eq_directory.as_deref()) {
            Asset::Ready(shape) if !shape.primitives.is_empty() => shape,
            Asset::Ready(_) | Asset::Missing => {
                flights.active.remove(&id);
                continue;
            }
            Asset::Pending => continue,
        };
        let axis_to_forward = axis_to_forward(shape.bounds);
        transform.rotation *= axis_to_forward;
        commands
            .spawn((
                SceneEntity,
                Drawn {
                    id,
                    axis_to_forward,
                },
                transform,
                Visibility::Inherited,
            ))
            .with_children(|parent| {
                for (mesh, material) in &shape.primitives {
                    parent.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone())));
                }
            });
        flights.spawned = flights.spawned.saturating_add(1);
        instances += 1;
    }
    flights.scene_instances = instances;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{WorldEvent, WorldPosition};

    fn arrow() -> Projectile {
        Projectile {
            source_id: 1,
            target_id: Some(2),
            start: [0.0, 0.0, 2.0],
            velocity: 4.0,
            launch_angle: 0.0,
            tilt: 0.0,
            arc: 0.0,
            item_id: Some(12),
            item_type: 27,
            skill: 7,
            model: "IT68".into(),
            light: None,
            behavior: None,
        }
    }

    fn target() -> eq_client_core::SpawnState {
        let mut spawn = testing::pet(2, 1);
        spawn.name = "a_synthetic_target".into();
        spawn.position = WorldPosition {
            x: 20.0,
            ..default()
        };
        spawn
    }

    fn admitted() -> OnlineState {
        let mut state = OnlineState::new(true);
        testing::admit(&mut state, 10, testing::player(1));
        testing::spawns(&mut state, vec![target()]);
        state
    }

    #[test]
    fn a_projectile_uses_only_a_received_item_model() {
        let mut event = arrow();
        event.model = "it68_actordef".into();
        assert_eq!(item_model(&event).as_deref(), Some("IT68"));
        for item_type in [7, 19, 27] {
            event.item_type = item_type;
            assert!(item_model(&event).is_some());
        }
        event.behavior = Some(0);
        assert_eq!(item_model(&event), None);
        event.behavior = Some(1);
        event.model = "../IT68".into();
        assert_eq!(item_model(&event), None);
        event.model = "IT68".into();
        event.item_id = None;
        assert_eq!(item_model(&event), None);
        event.item_id = Some(12);
        event.item_type = 28;
        assert_eq!(item_model(&event), None);
    }

    #[test]
    fn interpolates_from_wire_origin_and_expires_at_the_end_of_the_visual() {
        let state = admitted();
        let now = Instant::now();
        let event = arrow();
        let flight = Flight::new(state.world(), &event, now).unwrap();
        let start = Vec3::from_array(render_vector(event.start));
        let end = flight.target.position(state.world(), now).unwrap();
        assert_eq!(
            flight.transform(state.world(), now).unwrap().translation,
            start
        );
        let halfway = now + flight.duration / 2;
        let transform = flight.transform(state.world(), halfway).unwrap();
        assert!(transform.translation.distance(start.lerp(end, 0.5)) < 0.001);
        assert!(
            flight
                .transform(state.world(), now + flight.duration)
                .is_none()
        );
    }

    #[test]
    fn an_endpoint_despawn_and_same_id_replacement_do_not_revive_a_flight() {
        let mut state = admitted();
        let now = Instant::now();
        let flight = Flight::new(state.world(), &arrow(), now).unwrap();
        testing::news(&mut state, [WorldEvent::Despawn(2)]);
        assert!(flight.transform(state.world(), now).is_none());
        testing::spawns(&mut state, vec![target()]);
        assert!(flight.transform(state.world(), now).is_none());
        let flight = Flight::new(state.world(), &arrow(), now).unwrap();
        testing::admit(&mut state, 11, testing::player(1));
        testing::spawns(&mut state, vec![target()]);
        assert!(flight.transform(state.world(), now).is_none());
    }

    #[test]
    fn unknown_endpoints_and_caps_are_bounded_and_reset_preserves_serials() {
        let state = admitted();
        let now = Instant::now();
        let mut flights = Flights::default();
        let mut unknown = arrow();
        unknown.target_id = Some(90);
        flights.receive(state.world(), &unknown, now);
        assert_eq!(flights.received, 1);
        assert!(flights.active.is_empty());
        for _ in 0..MAX_FLIGHTS + 5 {
            flights.receive(state.world(), &arrow(), now);
        }
        assert_eq!(flights.active.len(), MAX_FLIGHTS);
        let old = flights.next;
        flights.reset(Reset::Zoning { to_bind: false });
        assert!(flights.active.is_empty());
        assert_eq!(flights.received, 0);
        flights.receive(state.world(), &arrow(), now);
        assert!(flights.next > old);
    }
}
