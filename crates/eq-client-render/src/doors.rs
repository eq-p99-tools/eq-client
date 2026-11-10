//! Local-model presentation of server-defined doors. Door actions remain separate from placement.
use bevy::prelude::*;
use eq_client_core::movement::CollisionMesh;
use eq_client_core::{GameDialect, doors::Door};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(super) type Primitives = Vec<(Handle<Mesh>, Handle<StandardMaterial>)>;

/// Shared GPU geometry for this zone only; replaced whenever zone assets are loaded.
#[derive(Resource, Default)]
pub(super) struct Models(pub BTreeMap<String, Model>);

pub(super) struct Model {
    pub primitives: Primitives,
    pub collision: Vec<[[f32; 3]; 3]>,
    pub pick: Vec<PickMesh>,
}

/// Keeps just the drawn positions and indices available after GPU extraction.
pub(super) struct PickMesh {
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
    double_sided: bool,
}

impl PickMesh {
    pub(super) fn from_model(model: &eq_client_assets::ZoneModel) -> Vec<Self> {
        model
            .primitives
            .iter()
            .enumerate()
            .filter(|(_, primitive)| super::gpu_assets::drawn(primitive))
            .map(|(index, primitive)| Self {
                positions: primitive.positions.clone(),
                indices: primitive.indices.clone(),
                double_sided: model
                    .materials
                    .get(index)
                    .is_none_or(|flags| flags.double_sided),
            })
            .collect()
    }
}

/// Nearest drawn triangle on this door's current model and pose.
pub(super) fn hit(
    ray: Ray3d,
    door: &DoorEntity,
    transform: &Transform,
    visibility: Visibility,
    models: &Models,
) -> Option<f32> {
    if visibility == Visibility::Hidden {
        return None;
    }
    let model = models.0.get(&door.model)?;
    let inverse = transform.to_matrix().inverse();
    let origin = inverse.transform_point3(ray.origin);
    // Keep the transformed direction unnormalized so distances stay in world units.
    let direction = inverse.transform_vector3(*ray.direction);
    model
        .pick
        .iter()
        .flat_map(|mesh| {
            mesh.indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(move |indices| {
                    let [Some(a), Some(b), Some(c)] = indices.map(|index| {
                        mesh.positions
                            .get(index as usize)
                            .copied()
                            .map(Vec3::from_array)
                    }) else {
                        return None;
                    };
                    super::target::picking::triangle_distance(
                        origin,
                        direction,
                        [a, b, c],
                        mesh.double_sided,
                    )
                })
        })
        .min_by(f32::total_cmp)
}

/// Rebuilds only this small obstacle when its rendered pose changes.
fn collider(model: &Model, transform: &Transform) -> Option<Arc<CollisionMesh>> {
    if model.collision.is_empty() {
        return None;
    }
    match CollisionMesh::new(model.collision.iter().map(|triangle| {
        triangle.map(|point| {
            transform
                .transform_point(Vec3::from_array(point))
                .to_array()
        })
    })) {
        Ok(mesh) => Some(Arc::new(mesh)),
        Err(error) => {
            warn!("Door collision unavailable: {error}");
            None
        }
    }
}

#[derive(Component)]
pub(super) struct DoorEntity {
    pub(super) id: u8,
    model: String,
    /// How far the door has swung from its placed heading, in EQ heading units.
    swing: f32,
    /// How far the door has slid from its placed position, in world units.
    slide: f32,
    definition: Door,
    collider: Option<Arc<CollisionMesh>>,
}

impl DoorEntity {
    fn update_pose(
        &mut self,
        definition: &Door,
        generation: GameDialect,
        (hinge, lift): (Option<f32>, Option<f32>),
        seconds: f32,
    ) -> Transform {
        // A changed definition may reuse an ID for a different mechanism.
        let mut previous = self.definition.clone();
        previous.action = definition.action;
        let replaced = previous != *definition;
        if let Some(fraction) = hinge {
            self.swing = open_swing(definition).unwrap_or(0.0) * fraction;
        } else if replaced {
            self.swing = hinge_target(definition).unwrap_or(0.0);
        } else if let Some(target) = hinge_target(definition) {
            self.swing = advance_hinge(self.swing, target, seconds);
        }
        let (distance, slide) = slide_pose(
            definition,
            generation,
            (!replaced).then_some(self.slide),
            seconds,
        );
        self.slide = distance;
        self.definition = definition.clone();
        posed(definition, self.swing, slide + rise(lift))
    }
}

/// A vertical door's offset from its placed height, in renderer axes.
fn rise(lift: Option<f32>) -> Vec3 {
    Vec3::from_array(eq_client_core::render_vector([
        0.0,
        0.0,
        lift.unwrap_or(0.0),
    ]))
}

/// Where each Titanium lift the player may ride stands and how near the
/// player must be for its motion to sweep them onto its floor: the door
/// model's radius, scaled, plus the player's 10 and 200 (0x4368b3; the
/// model's radius is inferred from its geometry, recording checklist 6.426).
#[derive(Resource, Default)]
pub(crate) struct LiftReach(pub BTreeMap<u8, (Vec3, f32)>);

/// The radius of a door model at its size: the farthest point of its solid
/// geometry from its origin; the program's 100 where it has none.
fn reach_of(model: Option<&Model>, size: u16) -> f32 {
    let radius = model
        .map(|model| {
            model
                .collision
                .iter()
                .flatten()
                .map(|point| Vec3::from_array(*point).length())
                .fold(0.0, f32::max)
        })
        .filter(|radius| *radius > 0.0)
        .map_or(100.0, |radius| radius * f32::from(size) / 100.0);
    10.0 + radius + 200.0
}

/// How far each drawn hinged door has swung toward its open pose, from 0
/// shut to 1 open, as drawn this frame; the same pose its collider takes.
#[derive(Resource, Default)]
pub(super) struct Hinges(pub BTreeMap<u8, f32>);

/// A hinged door's swing: a quarter turn, in EQ heading units.
pub(super) const QUARTER_TURN: f32 = eq_client_core::FULL_TURN / 4.0;

/// Hinged types turn a quarter of the way around, to opposite sides, from their
/// placed heading. The turn is in EQ's own heading units, so it shows the right
/// way round in the renderer like every other heading. Other mechanisms need
/// their own motion.
fn hinge_target(door: &Door) -> Option<f32> {
    let open = open_swing(door)?;
    Some(if door.active_endpoint()? { open } else { 0.0 })
}

/// How far a hinged door turns from its placed heading to stand open.
fn open_swing(door: &Door) -> Option<f32> {
    if door.incline != 0 {
        return None;
    }
    match door.open_type {
        0..=4 | 8 => Some(QUARTER_TURN),
        5..=7 => Some(-QUARTER_TURN),
        _ => None,
    }
}

/// A door a route may pass by clicking it while it is shut, as the player
/// would: one this client swings on its hinge, so its open pose is drawn and
/// walked against. Its state now does not matter, since an open door shuts
/// again on its own before the walker may reach it. Other mechanisms stay
/// walls to a route.
pub(super) fn swings(door: &Door) -> bool {
    hinge_target(door).is_some()
}

/// Titanium's slides (type 15, and 78-81, which also rise or sink) move along
/// one horizontal axis chosen by the heading, at 0.35 a 50 ms step
/// (0x4366e8; [`eq_client_core::doors::Slide`]). Like the hinge, the renderer
/// caps each frame's step at 100 ms.
fn slide_pose(
    door: &Door,
    generation: GameDialect,
    previous: Option<f32>,
    seconds: f32,
) -> (f32, Vec3) {
    let Some(slide) = (generation == GameDialect::Titanium)
        .then(|| eq_client_core::doors::Slide::of(door))
        .flatten()
    else {
        return (0.0, Vec3::ZERO);
    };
    let target = door
        .active_endpoint()
        .map(|active| if active { slide.distance } else { 0.0 });
    let distance = match (previous, target) {
        (Some(current), Some(target)) => advance_toward(current, target, seconds, slide.speed),
        (Some(current), None) => current,
        (None, target) => target.unwrap_or(0.0),
    };
    (
        distance,
        Vec3::from_array(eq_client_core::render_vector(slide.direction)) * distance,
    )
}

/// Cosmetic interpolation only, half a turn a second; this rate does not drive
/// packets or claim original-client timing. A frame swings the hinge by at
/// most a tenth of a second's turn, and the last one lands it exactly on its
/// target, so a walk waiting for the swing to end sees it end.
pub(super) fn advance_hinge(current: f32, target: f32, seconds: f32) -> f32 {
    advance_toward(current, target, seconds, 2.0 * QUARTER_TURN)
}

fn advance_toward(current: f32, target: f32, seconds: f32, speed: f32) -> f32 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return current;
    }
    let step = speed * seconds.min(0.1);
    if (target - current).abs() <= step {
        target
    } else {
        current + step.copysign(target - current)
    }
}

/// How far a hinge swung this far stands toward its open swing, 0 to 1.
fn hinge_fraction(swing: f32, open: f32) -> f32 {
    (swing / open).clamp(0.0, 1.0)
}

/// The same transform positions the model, pick mesh and collision mesh.
fn posed(door: &Door, swing: f32, slide: Vec3) -> Transform {
    let mut transform = placement(door);
    transform.rotation =
        Quat::from_rotation_y(eq_client_core::static_yaw(door.position.heading + swing));
    transform.translation += slide;
    transform
}

/// Nearest usable server definition in this admission with its distance, which
/// uses the generation's dimensions. Unoffered, unsupported or moving doors
/// produce no hint or nearest-use action.
pub(super) fn nearest(
    state: &super::online::OnlineState,
) -> Option<(f32, &eq_client_core::doors::Door)> {
    let player = player_for_use(state)?;
    let now = std::time::Instant::now();
    state
        .world()
        .doors()
        .entries()
        .values()
        .filter_map(|door| {
            state
                .world()
                .doors()
                .use_distance(door.id, player.position, now)
                .map(|distance| (distance, door))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

fn player_for_use(state: &super::online::OnlineState) -> Option<&eq_client_core::PlayerState> {
    if !state.world().connected()
        || state.world().is_dead()
        || !crate::outbox::offered(state.world(), eq_client_core::Capability::Doors)
    {
        return None;
    }
    if state.world().generation().requires_clear_cursor_for_doors() {
        let inventory = state.world().inventory();
        if !inventory.received()
            || inventory.stale()
            || inventory
                .items()
                .contains_key(&eq_client_core::inventory::InventorySlot::CURSOR)
        {
            return None;
        }
    }
    state.world().player()
}

/// Uses the door under the pointer with the same reach and eligibility as Use.
pub(super) fn click(
    door_id: u8,
    state: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
) {
    if let Some(player) = player_for_use(state)
        && state
            .world()
            .doors()
            .use_distance(door_id, player.position, std::time::Instant::now())
            .is_some()
    {
        open(door_id, state, outbox);
    }
}

/// Asks to use a door; no state is predicted. A click sent says nothing; a
/// refusal from the session or the outbox is said in the chat.
pub(super) fn open(
    door_id: u8,
    state: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
) {
    let _ = outbox.post(state.world(), |stamp| {
        eq_client_core::ClientCommand::ClickDoor {
            session_id: stamp.session_id,
            door_id,
            created: stamp.created,
        }
    });
}

/// Normalizes an asset identifier without interpreting server strings as paths.
pub(super) fn model_key(name: &str) -> String {
    eq_client_assets::model_key(name)
}

/// Door meshes are already in renderer axes; unlike characters they need no model-facing offset.
fn placement(door: &eq_client_core::doors::Door) -> Transform {
    Transform {
        translation: Vec3::from_array(eq_client_core::render_position(door.position)),
        rotation: Quat::from_rotation_y(eq_client_core::static_yaw(door.position.heading)),
        scale: Vec3::splat(f32::from(door.size) / 100.0),
    }
}

/// Reuses zone meshes and limits presentation to nearby server definitions.
#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
pub(super) fn reconcile(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<super::online::OnlineState>,
    models: Option<Res<Models>>,
    mut collision: Option<ResMut<super::Collision>>,
    mut rendered: Query<(Entity, &mut DoorEntity, &mut Transform)>,
    (mut hinges, mut reach): (ResMut<Hinges>, ResMut<LiftReach>),
) {
    hinges.0.clear();
    reach.0.clear();
    let now = std::time::Instant::now();
    let generation = state.world().generation();
    let doors = state.world().doors();
    let rideable = |id: u8| doors.solidity(id).floor && doors.lifts().motion(id).is_some();
    let mut obstacles = Vec::new();
    let mut remaining = BTreeSet::new();
    if state.world().connected()
        && let Some(player) = state.world().player()
    {
        for door in state.world().doors().entries().values() {
            let delta = Vec3::from_array(eq_client_core::render_position(door.position))
                - Vec3::from_array(eq_client_core::render_position(player.position));
            if delta.length_squared() <= 240.0 * 240.0 {
                remaining.insert(door.id);
            }
        }
    }
    for (entity, mut door, mut transform) in &mut rendered {
        let definition = state.world().doors().entries().get(&door.id);
        if !remaining.contains(&door.id)
            || definition.is_none_or(|definition| model_key(&definition.model) != door.model)
        {
            commands.entity(entity).despawn();
        } else if let Some(definition) = definition {
            remaining.remove(&door.id);
            let hinge = state.world().doors().hinge_fraction(door.id);
            let lift = doors.lift_offset(door.id, now);
            let next = door.update_pose(definition, generation, (hinge, lift), time.delta_secs());
            if let Some(open) = open_swing(definition) {
                hinges.0.insert(door.id, hinge_fraction(door.swing, open));
            }
            if *transform != next {
                door.collider = models
                    .as_ref()
                    .and_then(|models| models.0.get(&door.model))
                    .and_then(|model| collider(model, &next));
                *transform = next;
            }
            obstacles.extend(
                door.collider
                    .iter()
                    .map(|mesh| (door.id, Arc::clone(mesh), doors.solidity(door.id))),
            );
            if rideable(door.id) {
                let model = models.as_ref().and_then(|models| models.0.get(&door.model));
                reach.0.insert(
                    door.id,
                    (next.translation, reach_of(model, definition.size)),
                );
            }
        }
    }
    let Some(models) = models else {
        if let Some(world) = collision
            .as_mut()
            .and_then(|collision| collision.0.as_mut())
        {
            world.set_door_obstacles(Vec::new());
        }
        return;
    };
    for id in remaining {
        let door = &state.world().doors().entries()[&id];
        let Some(model) = models.0.get(&model_key(&door.model)) else {
            continue;
        };
        let swing = state
            .world()
            .doors()
            .hinge_fraction(id)
            .and_then(|fraction| open_swing(door).map(|open| open * fraction))
            .unwrap_or_else(|| hinge_target(door).unwrap_or(0.0));
        if let Some(open) = open_swing(door) {
            hinges.0.insert(id, hinge_fraction(swing, open));
        }
        let (slide, offset) = slide_pose(door, generation, None, 0.0);
        let transform = posed(door, swing, offset + rise(doors.lift_offset(id, now)));
        let collider = collider(model, &transform);
        obstacles.extend(
            collider
                .iter()
                .map(|mesh| (id, Arc::clone(mesh), doors.solidity(id))),
        );
        if rideable(id) {
            reach.0.insert(
                id,
                (transform.translation, reach_of(Some(model), door.size)),
            );
        }
        commands
            .spawn((
                super::SceneEntity,
                DoorEntity {
                    id,
                    model: model_key(&door.model),
                    swing,
                    slide,
                    definition: door.clone(),
                    collider,
                },
                transform,
                Visibility::Inherited,
            ))
            .with_children(|parent| {
                for (mesh, material) in &model.primitives {
                    parent.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone())));
                }
            });
    }
    if let Some(world) = collision
        .as_mut()
        .and_then(|collision| collision.0.as_mut())
    {
        world.set_door_obstacles_with(obstacles);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn door_collision_follows_the_visible_hinge_pose() {
        use eq_client_core::movement::CollisionWorld;
        let model = Model {
            primitives: Vec::new(),
            collision: vec![
                [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 10.0, 0.0]],
                [[0.0, 0.0, 0.0], [4.0, 10.0, 0.0], [0.0, 10.0, 0.0]],
            ],
            pick: Vec::new(),
        };
        let mut world = CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ])
        .unwrap();
        // Include translation and scale: collision must match the mesh, not just its raw vertices.
        let closed =
            Transform::from_translation(Vec3::new(3.0, 0.0, 5.0)).with_scale(Vec3::splat(1.5));
        let start = Vec3::new(6.0, 0.0, 3.0);
        let movement = Vec3::Z * 4.0;
        world.set_obstacles(vec![collider(&model, &closed).unwrap()]);
        assert!(world.step(start, movement, 6.0).z < 4.6);
        let open = closed.with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        world.set_obstacles(vec![collider(&model, &open).unwrap()]);
        assert!(world.step(start, movement, 6.0).distance(start + movement) < 0.001);
        world.set_obstacles(vec![collider(&model, &closed).unwrap()]);
        assert!(world.step(start, movement, 6.0).z < 4.6);
    }

    fn sliding_door() -> Door {
        Door {
            id: 23,
            model: "TEST".into(),
            position: default(),
            incline: 0,
            size: 100,
            open_type: 15,
            state_at_spawn: 0,
            invert_state: 0,
            parameter: 0,
            action: None,
        }
    }

    #[test]
    fn type_15_slide_keeps_its_titanium_axis_and_waits_on_unknown_actions() {
        let mut door = sliding_door();
        door.state_at_spawn = 1;
        for (heading, expected) in [
            (0.0, Vec3::Z),
            (128.0, Vec3::X),
            (256.0, Vec3::NEG_Z),
            (384.0, Vec3::NEG_X),
            (64.0, Vec3::Z),
        ] {
            door.position.heading = heading;
            let (_, offset) = slide_pose(&door, GameDialect::Titanium, None, 0.0);
            assert_eq!(offset, expected * 10.0);
        }
        assert_eq!(
            slide_pose(&door, GameDialect::EqMac, Some(4.0), 0.1),
            (0.0, Vec3::ZERO)
        );
        door.state_at_spawn = 255;
        assert_eq!(
            slide_pose(&door, GameDialect::Titanium, None, 0.0),
            (0.0, Vec3::ZERO)
        );
        door.action = Some(255);
        assert_eq!(
            slide_pose(&door, GameDialect::Titanium, Some(4.0), 0.1),
            (4.0, Vec3::Z * 4.0)
        );
        door.action = Some(3);
        let (closing, _) = slide_pose(&door, GameDialect::Titanium, Some(4.0), 0.05);
        assert!((closing - 3.65).abs() < 0.0001);
    }

    #[test]
    fn received_type_15_actions_slide_the_drawn_picked_and_solid_door_together() {
        use crate::online::{OnlineState, testing};
        use eq_client_core::{doors::DoorUpdate, movement::CollisionWorld};
        use std::time::{Duration, Instant};

        let mut state = OnlineState::new(true);
        testing::generation(&mut state, GameDialect::Titanium);
        testing::admit(&mut state, 11, testing::player(1));
        let now = Instant::now();
        testing::doors(&mut state, &DoorUpdate::Spawn(vec![sliding_door()]), now);
        let triangle = [[0.0, 0.0, -2.0], [0.0, 4.0, 0.0], [0.0, 0.0, 2.0]];
        let models = Models(BTreeMap::from([(
            "TEST".into(),
            Model {
                primitives: vec![(Handle::default(), Handle::default())],
                collision: vec![triangle],
                pick: vec![PickMesh {
                    positions: triangle.to_vec(),
                    indices: vec![0, 1, 2],
                    double_sided: true,
                }],
            },
        )]));
        let terrain =
            CollisionWorld::new([[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [0.0, 0.0, 5.0]]]).unwrap();
        let mut app = App::new();
        app.insert_resource(state)
            .insert_resource(models)
            .insert_resource(super::super::Collision(Some(terrain)))
            .init_resource::<Time>()
            .init_resource::<Hinges>()
            .init_resource::<LiftReach>()
            .add_systems(Update, reconcile);
        app.update();
        let mut query = app.world_mut().query::<(&DoorEntity, &Transform)>();
        let mut observe = |app: &App, z| {
            let world = app.world();
            let (door, transform) = query.single(world).unwrap();
            let ray = Ray3d::new(Vec3::new(-5.0, 1.0, z), Dir3::X);
            let model_hit = hit(
                ray,
                door,
                transform,
                Visibility::Inherited,
                world.resource(),
            );
            let collision_hit = world
                .resource::<super::super::Collision>()
                .0
                .as_ref()
                .unwrap()
                .ray_distance(ray.origin, *ray.direction, 20.0);
            (
                transform.translation,
                model_hit.is_some(),
                collision_hit.is_some(),
            )
        };
        assert_eq!(observe(&app, 0.0), (Vec3::ZERO, true, true));
        assert_eq!(observe(&app, 10.0), (Vec3::ZERO, false, false));
        let advance = |app: &mut App| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(100));
            app.update();
        };
        let move_door = |app: &mut App, action| {
            testing::doors(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                &DoorUpdate::Move { id: 23, action },
                now,
            );
            advance(app);
        };
        move_door(&mut app, 2);
        assert!((observe(&app, 0.0).0 - Vec3::Z * 0.7).length() < 0.0001);
        for _ in 0..14 {
            advance(&mut app);
        }
        let opened = Vec3::Z * 10.0;
        assert_eq!(observe(&app, 0.0), (opened, false, false));
        assert_eq!(observe(&app, 10.0), (opened, true, true));
        assert!(app.world().resource::<Hinges>().0.is_empty());
        move_door(&mut app, 3);
        for _ in 0..14 {
            advance(&mut app);
        }
        assert_eq!(observe(&app, 0.0), (Vec3::ZERO, true, true));
        assert_eq!(observe(&app, 10.0), (Vec3::ZERO, false, false));
    }

    #[test]
    fn a_route_may_open_only_a_hinged_door_open_or_shut() {
        let mut door = eq_client_core::doors::Door {
            id: 1,
            model: "DOOR".into(),
            position: default(),
            incline: 0,
            size: 100,
            open_type: 5,
            state_at_spawn: 0,
            invert_state: 0,
            parameter: 0,
            action: None,
        };
        assert!(swings(&door));
        door.action = Some(2);
        assert!(swings(&door), "open now, shut again later");
        door.action = Some(3);
        assert!(swings(&door));
        for open_type in [9, 15, 58, 255] {
            assert!(!swings(&eq_client_core::doors::Door {
                open_type,
                ..door.clone()
            }));
        }
        assert!(!swings(&eq_client_core::doors::Door {
            incline: 90,
            ..door
        }));
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies unchanged or explicitly assigned state"
    )]
    fn hinge_endpoints_reverse_without_overshooting_and_unknown_types_stay_unknown() {
        let bytes = [0u8; 80];
        let eq_client_core::doors::DoorUpdate::Spawn(mut doors) =
            eq_client_core::doors::decode(0x4c24, &bytes)
                .unwrap()
                .unwrap()
        else {
            panic!("spawn")
        };
        let door = &mut doors[0];
        assert_eq!(hinge_target(door), Some(0.0));
        let shut_action = door.action;
        door.action = Some(2);
        let swing = hinge_target(door).unwrap();
        assert_eq!(swing, QUARTER_TURN);
        door.open_type = 5;
        assert_eq!(hinge_target(door), Some(-swing));
        // A door first drawn while the server holds it open is drawn at its
        // open pose, so a walk reads its hinge as fully swung, whichever way
        // it turns; one drawn shut reads 0.
        for open_type in [0, 5] {
            door.open_type = open_type;
            let open = open_swing(door).unwrap();
            assert_eq!(hinge_fraction(hinge_target(door).unwrap(), open), 1.0);
            let shut = eq_client_core::doors::Door {
                action: shut_action,
                ..door.clone()
            };
            assert_eq!(hinge_fraction(hinge_target(&shut).unwrap(), open), 0.0);
        }
        door.open_type = 5;
        let partway = advance_hinge(0.0, swing, 0.1);
        assert!(partway > 0.0 && partway < swing);
        assert_eq!(advance_hinge(partway, 0.0, 0.1), 0.0);
        assert_eq!(advance_hinge(0.0, swing, 5.0), partway);
        let mut turned = 0.0;
        for _ in 0..10 {
            turned = advance_hinge(turned, swing, 0.1);
        }
        assert_eq!(turned, swing);
        door.action = Some(255);
        assert_eq!(hinge_target(door), None);
        door.action = Some(2);
        door.open_type = 59;
        assert_eq!(hinge_target(door), None);
        door.open_type = 5;
        door.incline = 1;
        assert_eq!(hinge_target(door), None);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered integration scenario and its assertions together"
    )]
    fn nearby_doors_reuse_meshes_and_disappear_on_disconnect_or_model_change() {
        let mut state = super::super::online::OnlineState::new(true);
        crate::online::testing::admit(
            &mut state,
            11,
            eq_client_core::PlayerState {
                name: "Example".into(),
                titanium_environment: None,
                base_attributes: None,
                spawn_id: 1,
                race: 1,
                class: None,
                deity: None,
                skills: None,
                practice_points: None,
                languages: None,
                disciplines: None,
                gender: 0,
                level: 1,
                position: default(),
                mana: 0,
                endurance: None,
                spell_refresh_ms: None,
                memorized_spells: [None; 8],
                size: 6.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                hp_percent: None,
                appearance: eq_client_core::outfit::Appearance::default(),
                listing: eq_client_core::listing::Listing::default(),
                name_parts: eq_client_core::names::NameParts::default(),
            },
        );
        let mut bytes = [0u8; 80];
        bytes[..4].copy_from_slice(b"TEST");
        bytes[52] = 100;
        let update = eq_client_core::doors::decode(0x4c24, &bytes)
            .unwrap()
            .unwrap();
        crate::online::testing::doors(&mut state, &update, std::time::Instant::now());
        let mut models = Models::default();
        models.0.insert(
            "TEST".into(),
            Model {
                primitives: vec![(Handle::default(), Handle::default())],
                collision: vec![
                    [[-10.0, 0.5, -10.0], [10.0, 0.5, -10.0], [10.0, 0.5, 10.0]],
                    [[-10.0, 0.5, -10.0], [10.0, 0.5, 10.0], [-10.0, 0.5, 10.0]],
                ],
                pick: Vec::new(),
            },
        );
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.init_resource::<Time>();
        app.insert_resource(super::super::Collision(Some(
            eq_client_core::movement::CollisionWorld::new([
                [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
                [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
            ])
            .unwrap(),
        )));
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<super::super::chat::ChatState>()
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, super::super::interact::input);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: false,
                    ..default()
                },
                bevy::window::PrimaryWindow,
            ))
            .id();
        app.insert_resource(state)
            .insert_resource(models)
            .init_resource::<Hinges>()
            .init_resource::<LiftReach>()
            .add_systems(Update, reconcile);
        app.update();
        let mut roots = app.world_mut().query::<(&DoorEntity, &Children)>();
        assert_eq!(roots.iter(app.world()).count(), 1);
        assert_eq!(roots.single(app.world()).unwrap().1.len(), 1);
        // Drawn from its first frame, so a walk knows how far it has swung.
        let (drawn, _) = roots.single(app.world()).unwrap();
        assert_eq!(
            app.world().resource::<Hinges>().0.get(&drawn.id).copied(),
            open_swing(&drawn.definition).map(|open| hinge_fraction(drawn.swing, open))
        );
        assert!(open_swing(&drawn.definition).is_some(), "a hinged door");
        let ground = |app: &App| {
            app.world()
                .resource::<super::super::Collision>()
                .0
                .as_ref()
                .unwrap()
                .ground(Vec3::ZERO, 0.8, 1.5)
                .unwrap()
        };
        let obstacles = |app: &App| {
            app.world()
                .resource::<super::super::Collision>()
                .0
                .as_ref()
                .unwrap()
                .obstacle_count()
        };
        // A hinged door (type 0) is a wall, never a floor (eqgame.exe
        // 0x490370), so its collider is in but nobody stands on it.
        assert_eq!(obstacles(&app), 1);
        assert!(ground(&app).abs() < 0.001);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyU);
        app.update();
        assert!(receiver.try_recv().is_err());
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.update();
        assert!(receiver.try_recv().is_err());
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = false;
        app.update();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            eq_client_core::ClientCommand::ClickDoor {
                session_id: 11,
                door_id: 0,
                ..
            }
        ));
        assert_eq!(
            app.world()
                .resource::<super::super::online::OnlineState>()
                .world()
                .doors()
                .entries()[&0]
                .action,
            None
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert_eq!(roots.iter(app.world()).count(), 1);
        // A changed model must not keep the previous door mesh.
        bytes[..4].copy_from_slice(b"NONE");
        crate::online::testing::doors(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            &eq_client_core::doors::decode(0x4c24, &bytes)
                .unwrap()
                .unwrap(),
            std::time::Instant::now(),
        );
        app.update();
        assert_eq!(roots.iter(app.world()).count(), 0);
        assert_eq!(obstacles(&app), 0);
        assert!(ground(&app).abs() < 0.001);
        crate::online::testing::doors(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            &update,
            std::time::Instant::now(),
        );
        app.update();
        assert_eq!(roots.iter(app.world()).count(), 1);
        crate::online::testing::place_axis(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            |position| position.x = 241.0,
        );
        app.update();
        assert_eq!(roots.iter(app.world()).count(), 0);
        crate::online::testing::place_axis(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            |position| position.x = 0.0,
        );
        app.update();
        assert_eq!(roots.iter(app.world()).count(), 1);
        crate::online::testing::connect(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            false,
        );
        app.update();
        assert_eq!(roots.iter(app.world()).count(), 0);
        crate::online::testing::connect(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            true,
        );
        app.update();
        assert_eq!(roots.iter(app.world()).count(), 1);
        crate::online::testing::doors(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            &eq_client_core::doors::DoorUpdate::RemoveAll,
            std::time::Instant::now(),
        );
        app.update();
        assert_eq!(roots.iter(app.world()).count(), 0);
        assert!(nearest(app.world().resource::<super::super::online::OnlineState>()).is_none());
        assert_eq!(obstacles(&app), 0);
        assert!(ground(&app).abs() < 0.001);
    }

    #[test]
    fn an_opened_door_swings_shut_on_the_clients_own_timer() {
        use eq_client_core::doors::DoorUpdate;
        // Type 0 returns 10 s after the action (eqgame.exe 0x48faa5).
        let opened = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(10))
            .expect("the clock has run longer than the delay");
        let mut state = super::super::online::OnlineState::new(true);
        let spawn = eq_client_core::doors::decode(0x4c24, &[0u8; 80])
            .unwrap()
            .unwrap();
        crate::online::testing::doors(&mut state, &spawn, opened);
        crate::online::testing::doors(&mut state, &DoorUpdate::Move { id: 0, action: 2 }, opened);
        let mut app = App::new();
        app.insert_resource(state)
            .add_systems(Update, crate::online::tick);
        app.update();
        let door = &app
            .world()
            .resource::<super::super::online::OnlineState>()
            .world()
            .doors()
            .entries()[&0];
        assert_eq!(door.active_endpoint(), Some(false));
    }

    #[test]
    fn model_identifiers_and_door_axes_do_not_use_character_facing_offset() {
        assert_eq!(model_key(" door01_ACTORDEF "), "DOOR01");
        assert_eq!(model_key("door01"), "DOOR01");
        let mut bytes = [0u8; 80];
        for (offset, value) in [(32, 12f32), (36, -7.0), (40, 3.0), (44, 128.0)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[52] = 150;
        let eq_client_core::doors::DoorUpdate::Spawn(doors) =
            eq_client_core::doors::decode(0x4c24, &bytes)
                .unwrap()
                .unwrap()
        else {
            panic!("door table")
        };
        let transform = placement(&doors[0]);
        // EQ's X runs along the renderer's -Z.
        assert_eq!(transform.translation, Vec3::new(12.0, 3.0, 7.0));
        assert_eq!(transform.scale, Vec3::splat(1.5));
        // A model's +X faces its heading: 128 is west, the world's +X.
        assert!((transform.rotation * Vec3::X - Vec3::NEG_Z).length() < 0.0001);
        // An open door turns a quarter further round.
        let open = posed(&doors[0], QUARTER_TURN, Vec3::ZERO);
        assert!((open.rotation * Vec3::X - Vec3::NEG_X).length() < 0.0001);
    }
}
