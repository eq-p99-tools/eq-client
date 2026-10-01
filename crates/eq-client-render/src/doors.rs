//! Local-model presentation of server-defined doors. Door actions remain separate from placement.
use bevy::prelude::*;
use eq_client_core::movement::CollisionMesh;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(super) type Primitives = Vec<(Handle<Mesh>, Handle<StandardMaterial>)>;

/// Shared GPU geometry for this zone only; replaced whenever zone assets are loaded.
#[derive(Resource, Default)]
pub(super) struct Models(pub BTreeMap<String, Model>);

pub(super) struct Model {
    pub primitives: Primitives,
    pub collision: Vec<[[f32; 3]; 3]>,
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
    id: u8,
    model: String,
    /// How far the door has swung from its placed heading, in EQ heading units.
    swing: f32,
    definition: eq_client_core::doors::Door,
    collider: Option<Arc<CollisionMesh>>,
}

/// A hinged door's swing: a quarter turn, in EQ heading units.
const QUARTER_TURN: f32 = eq_client_core::FULL_TURN / 4.0;

/// Hinged types turn a quarter of the way around, to opposite sides, from their
/// placed heading. The turn is in EQ's own heading units, so it shows the right
/// way round in the renderer like every other heading. Other mechanisms need
/// their own motion.
fn hinge_target(door: &eq_client_core::doors::Door) -> Option<f32> {
    if door.incline != 0 {
        return None;
    }
    let direction = match door.open_type {
        0..=4 | 8 => 1.0,
        5..=7 => -1.0,
        _ => return None,
    };
    Some(if door.active_endpoint()? {
        direction * QUARTER_TURN
    } else {
        0.0
    })
}

/// Cosmetic interpolation only, half a turn a second; this rate does not drive
/// packets or claim original-client timing.
fn advance_hinge(current: f32, target: f32, seconds: f32) -> f32 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return current;
    }
    let step = 2.0 * QUARTER_TURN * seconds.min(0.1);
    current + (target - current).clamp(-step, step)
}

/// The door's transform once it has swung this far from its placed heading.
fn posed(door: &eq_client_core::doors::Door, swing: f32) -> Transform {
    let mut transform = placement(door);
    transform.rotation =
        Quat::from_rotation_y(eq_client_core::static_yaw(door.position.heading + swing));
    transform
}

/// Nearest usable server definition in this admission with its distance, which
/// is measured in all three axes.
pub(super) fn nearest(
    state: &super::online::OnlineState,
) -> Option<(f32, &eq_client_core::doors::Door)> {
    if !state.world().connected() || state.world().death().is_some() {
        return None;
    }
    let player = state.world().player()?;
    state
        .world()
        .doors()
        .entries()
        .values()
        .filter_map(|door| {
            let distance = (door.position.x - player.position.x)
                .hypot(door.position.y - player.position.y)
                .hypot(door.position.z - player.position.z);
            (distance.is_finite() && distance <= eq_client_core::doors::Doors::USE_DISTANCE)
                .then_some((distance, door))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// Asks to use a door; no state is predicted. A click sent says nothing; a
/// refusal from the session or the outbox says why.
pub(super) fn open(
    door_id: u8,
    state: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
    door: &mut crate::notices::Line,
) {
    let sent = outbox.post(state.world(), |stamp| {
        eq_client_core::ClientCommand::ClickDoor {
            session_id: stamp.session_id,
            door_id,
            created: stamp.created,
        }
    });
    if sent.is_ok() {
        door.clear();
    }
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
#[allow(clippy::needless_pass_by_value)]
pub(super) fn reconcile(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<super::online::OnlineState>,
    models: Option<Res<Models>>,
    mut collision: Option<ResMut<super::Collision>>,
    mut rendered: Query<(Entity, &mut DoorEntity, &mut Transform)>,
) {
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
            // A changed definition may reuse an ID for a different mechanism.
            let mut previous = door.definition.clone();
            previous.action = definition.action;
            if previous != *definition {
                door.swing = hinge_target(definition).unwrap_or(0.0);
            } else if let Some(target) = hinge_target(definition) {
                door.swing = advance_hinge(door.swing, target, time.delta_secs());
            }
            door.definition = definition.clone();
            let next = posed(definition, door.swing);
            if *transform != next {
                door.collider = models
                    .as_ref()
                    .and_then(|models| models.0.get(&door.model))
                    .and_then(|model| collider(model, &next));
                *transform = next;
            }
            obstacles.extend(door.collider.iter().cloned());
        }
    }
    let Some(models) = models else {
        if let Some(world) = collision
            .as_mut()
            .and_then(|collision| collision.0.as_mut())
        {
            world.set_obstacles(Vec::new());
        }
        return;
    };
    for id in remaining {
        let door = &state.world().doors().entries()[&id];
        let Some(model) = models.0.get(&model_key(&door.model)) else {
            continue;
        };
        let transform = posed(door, hinge_target(door).unwrap_or(0.0));
        let collider = collider(model, &transform);
        obstacles.extend(collider.iter().cloned());
        commands
            .spawn((
                super::SceneEntity,
                DoorEntity {
                    id,
                    model: model_key(&door.model),
                    swing: hinge_target(door).unwrap_or(0.0),
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
        world.set_obstacles(obstacles);
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
        door.action = Some(2);
        let swing = hinge_target(door).unwrap();
        assert_eq!(swing, QUARTER_TURN);
        door.open_type = 5;
        assert_eq!(hinge_target(door), Some(-swing));
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
                base_attributes: None,
                spawn_id: 1,
                race: 1,
                class: None,
                deity: None,
                skills: None,
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
            },
        );
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.init_resource::<crate::notices::Lines>()
            .init_resource::<Time>();
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
            .add_systems(Update, reconcile);
        app.update();
        let mut roots = app.world_mut().query::<(&DoorEntity, &Children)>();
        assert_eq!(roots.iter(app.world()).count(), 1);
        assert_eq!(roots.single(app.world()).unwrap().1.len(), 1);
        let ground = |app: &App| {
            app.world()
                .resource::<super::super::Collision>()
                .0
                .as_ref()
                .unwrap()
                .ground(Vec3::ZERO, 0.8, 1.5)
                .unwrap()
        };
        assert!((ground(&app) - 0.5).abs() < 0.001);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyF);
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
        assert!(ground(&app).abs() < 0.001);
    }

    #[test]
    fn an_opened_door_swings_shut_on_the_clients_own_timer() {
        use eq_client_core::doors::{CLOSE_DELAY, DoorUpdate};
        let opened = std::time::Instant::now()
            .checked_sub(CLOSE_DELAY)
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
        let open = posed(&doors[0], QUARTER_TURN);
        assert!((open.rotation * Vec3::X - Vec3::NEG_X).length() < 0.0001);
    }
}
