//! Items on the ground and world containers, drawn with the installed models.
//! Which object a click or key press means is decided in
//! `eq_client_core::ground`; whether a pickup may be sent, in the network session.
use super::item_models::{ItemLibrary, bounds};
use bevy::prelude::*;
use eq_client_core::ground::{GroundObject, ObjectKind};
use std::collections::BTreeMap;

/// Objects are drawn this close to the player, as doors are.
const DRAW_RADIUS: f32 = 240.0;
/// What an item without an installed model shows: the small bag servers use
/// by default.
const DEFAULT_ITEM_MODEL: &str = "IT63";

/// An object on the ground as drawn.
#[derive(Component)]
pub(super) struct GroundEntity {
    object: GroundObject,
    /// Where a click picks it up, in model space; None for fixtures.
    pick: Option<(Vec3, Vec3)>,
}

fn placement(object: &GroundObject) -> Transform {
    Transform::from_translation(Vec3::from_array(eq_client_core::render_position(
        object.position,
    )))
    .with_rotation(Quat::from_rotation_y(eq_client_core::static_yaw(
        object.position.heading,
    )))
}

/// Draws the objects near the player: a model the zone ships with first, then
/// an installed item model; items without one show the default bag, and
/// fixtures without one are not drawn.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn reconcile(
    mut commands: Commands,
    settings: Res<super::ViewerSettings>,
    state: Res<super::online::OnlineState>,
    zone_models: Option<Res<super::doors::Models>>,
    mut library: ResMut<ItemLibrary>,
    (mut images, mut meshes, mut materials): super::item_models::GpuAssets,
    rendered: Query<(Entity, &GroundEntity)>,
) {
    let mut wanted = BTreeMap::new();
    if state.world().connected()
        && zone_models.is_some()
        && let Some(player) = state.world().player()
    {
        let origin = Vec3::from_array(eq_client_core::render_position(player.position));
        for object in state.world().objects().entries().values() {
            let position = Vec3::from_array(eq_client_core::render_position(object.position));
            if position.distance_squared(origin) <= DRAW_RADIUS * DRAW_RADIUS {
                wanted.insert(object.drop_id, object);
            }
        }
    }
    for (entity, drawn) in &rendered {
        // A changed placement or model is drawn again from scratch.
        if wanted.get(&drawn.object.drop_id) == Some(&&drawn.object) {
            wanted.remove(&drawn.object.drop_id);
        } else {
            commands.entity(entity).despawn();
        }
    }
    let directory = settings.0.eq_directory.as_deref();
    for object in wanted.into_values() {
        let key = eq_client_assets::model_key(&object.model);
        let item = object.kind() == ObjectKind::Item;
        let zone = zone_models.as_ref().and_then(|models| models.0.get(&key));
        let mut shape =
            |key: &str| library.shape(key, directory, (&mut images, &mut meshes, &mut materials));
        let (primitives, bounds) = if let Some(model) = zone {
            let points = model.collision.iter().flatten();
            (model.primitives.clone(), bounds(points))
        } else if let Some(shape) =
            shape(&key).or_else(|| item.then(|| shape(DEFAULT_ITEM_MODEL))?)
        {
            (shape.primitives.clone(), shape.bounds)
        } else {
            continue;
        };
        let pick = if item {
            let (min, max) = bounds.unwrap_or((Vec3::ZERO, Vec3::ZERO));
            let (min, max) = eq_client_core::ground::pick_box(min, max);
            Some((min, max))
        } else {
            None
        };
        commands
            .spawn((
                super::SceneEntity,
                GroundEntity {
                    object: object.clone(),
                    pick,
                },
                placement(object),
                Visibility::Inherited,
            ))
            .with_children(|parent| {
                for (mesh, material) in primitives {
                    parent.spawn((Mesh3d(mesh), MeshMaterial3d(material)));
                }
            });
    }
}

/// The nearest drawn item a ray passes through, with its distance.
pub(super) fn hit<'a>(
    ray: Ray3d,
    drawn: impl Iterator<Item = (&'a GroundEntity, &'a Transform)>,
) -> Option<(u32, f32)> {
    drawn
        .filter_map(|(entity, transform)| {
            let (min, max) = entity.pick?;
            let inverse = transform.to_matrix().inverse();
            let origin = inverse.transform_point3(ray.origin);
            let direction = inverse.transform_vector3(*ray.direction);
            let distance = eq_client_core::ground::ray_box(origin, direction, min, max)?;
            Some((entity.object.drop_id, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Asks to pick an item up; the session checks the cursor and reach and
/// answers with the result. The chat line the player reads when they cannot
/// pick anything up now; the outbox shows its own refusals.
pub(super) fn pick_up(
    drop_id: u32,
    state: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
) -> Option<String> {
    if !state.in_world() {
        return Some("You can't pick anything up right now.".into());
    }
    // A refusal shows in the feedback line.
    let _ = outbox.post(state.world(), |stamp| {
        eq_client_core::ClientCommand::PickUp {
            session_id: stamp.session_id,
            drop_id,
            created: stamp.created,
        }
    });
    None
}

/// A refused pickup as a chat line, such as "Too far away to pick that up."
pub(super) fn refusal(reason: &str) -> String {
    let mut characters = reason.trim_end_matches('.').chars();
    characters.next().map_or_else(String::new, |first| {
        format!("{}{}.", first.to_uppercase(), characters.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(drop_id: u32, model: &str, x: f32) -> GroundObject {
        GroundObject {
            drop_id,
            model: model.into(),
            position: eq_client_core::WorldPosition { x, ..default() },
            object_type: 0,
        }
    }

    fn online() -> super::super::online::OnlineState {
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
            },
        );
        state
    }

    fn app(state: super::super::online::OnlineState) -> App {
        let mut library = ItemLibrary::default();
        library.insert(
            "IT63",
            super::super::item_models::Shape {
                primitives: vec![(Handle::default(), Handle::default())],
                bounds: Some((Vec3::new(-0.3, 0.0, -0.3), Vec3::new(0.3, 0.8, 0.3))),
                flat: false,
            },
        );
        let mut app = App::new();
        app.insert_resource(super::super::ViewerSettings(default()))
            .insert_resource(state)
            .insert_resource(super::super::doors::Models::default())
            .insert_resource(library)
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, reconcile);
        app
    }

    fn drawn(app: &mut App) -> Vec<u32> {
        let mut query = app.world_mut().query::<&GroundEntity>();
        let mut ids: Vec<_> = query
            .iter(app.world())
            .map(|entity| entity.object.drop_id)
            .collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn nearby_objects_are_drawn_and_leave_with_the_table() {
        use eq_client_core::ground::ObjectUpdate;
        let mut state = online();
        for update in [
            object(71, "IT63_ACTORDEF", 10.0),
            // No installed model: an item shows the default bag.
            object(72, "IT999999_ACTORDEF", 20.0),
            // A fixture without a model is not drawn.
            object(73, "A FIXTURE 1", 30.0),
            object(74, "IT63_ACTORDEF", DRAW_RADIUS + 1.0),
        ] {
            crate::online::testing::objects(&mut state, &ObjectUpdate::Spawn(update));
        }
        let mut app = app(state);
        app.update();
        assert_eq!(drawn(&mut app), [71, 72]);
        let mut state = app
            .world_mut()
            .resource_mut::<super::super::online::OnlineState>();
        crate::online::testing::objects(
            &mut state,
            &ObjectUpdate::Remove {
                drop_id: 71,
                taken_by: Some(1),
            },
        );
        app.update();
        assert_eq!(drawn(&mut app), [72]);
        crate::online::testing::connect(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            false,
        );
        app.update();
        assert!(drawn(&mut app).is_empty());
    }

    #[test]
    fn clicks_pick_the_nearest_item_the_ray_passes_through() {
        let bag = |drop_id, x| {
            (
                GroundEntity {
                    object: object(drop_id, "IT63_ACTORDEF", 0.0),
                    pick: Some(eq_client_core::ground::pick_box(
                        Vec3::new(-0.3, 0.0, -0.3),
                        Vec3::new(0.3, 0.8, 0.3),
                    )),
                },
                Transform::from_xyz(x, 0.0, 0.0),
            )
        };
        let near = bag(1, 5.0);
        let far = bag(2, 10.0);
        let fixture = (
            GroundEntity {
                object: object(3, "A FIXTURE 1", 0.0),
                pick: None,
            },
            Transform::from_xyz(2.0, 0.0, 0.0),
        );
        let entities = [&near, &far, &fixture];
        let along_x = Ray3d::new(Vec3::new(0.0, 0.4, 0.0), Dir3::X);
        let (id, distance) =
            hit(along_x, entities.iter().map(|(entity, at)| (entity, at))).unwrap();
        assert_eq!(id, 1);
        // The small bag's box is widened to the minimum pick size.
        let half = eq_client_core::ground::MIN_PICK_SIZE / 2.0;
        assert!((distance - (5.0 - half)).abs() < 1e-4);
        let above = Ray3d::new(Vec3::new(0.0, 5.0, 0.0), Dir3::X);
        assert!(hit(above, entities.iter().map(|(entity, at)| (entity, at))).is_none());
    }

    #[test]
    fn refusals_read_as_sentences() {
        assert_eq!(
            refusal("too far away to pick that up"),
            "Too far away to pick that up."
        );
        assert_eq!(
            refusal("Wait for scribing to finish"),
            "Wait for scribing to finish."
        );
        assert_eq!(refusal(""), "");
    }
}
