//! Ground-item markers use object IDs, never the spawn IDs used for targeting.
use super::{MapCanvas, MapView, entities, map_point};
use crate::{online::OnlineState, tooltip::Tooltip};
use bevy::prelude::*;
use eq_client_core::{
    WorldPosition,
    ground::{GroundObject, ObjectKind},
};
use std::collections::BTreeSet;

const SIZE: f32 = 6.0;
const INK: Color = Color::srgb_u8(240, 205, 100);

#[derive(Component)]
pub(crate) struct ItemMarker(u32);

fn visible(object: &GroundObject, origin: Option<WorldPosition>) -> bool {
    let Some(origin) = origin else { return false };
    let p = object.position;
    let delta = Vec3::new(p.x - origin.x, p.y - origin.y, p.z - origin.z);
    object.kind() == ObjectKind::Item
        && delta.length_squared() <= crate::ground::DRAW_RADIUS.powi(2)
}

/// Reconcile server-reported items; fixtures are not loose items to pick up.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn update(
    mut commands: Commands,
    online: Res<OnlineState>,
    view: Res<MapView>,
    canvases: Query<Entity, With<MapCanvas>>,
    mut markers: Query<(Entity, &ItemMarker, &mut Node, &mut Tooltip, &Interaction)>,
) {
    let Ok(canvas) = canvases.single() else {
        return;
    };
    let Some(projection) = view.projection else {
        return;
    };
    let world = online.world();
    let origin = world
        .player()
        .filter(|_| world.connected())
        .map(|player| player.position);
    let objects = world.objects().entries();
    let mut drawn = BTreeSet::new();
    for (entity, marker, mut node, mut tooltip, interaction) in &mut markers {
        let Some(object) = objects
            .get(&marker.0)
            .filter(|object| visible(object, origin))
        else {
            commands.entity(entity).despawn();
            continue;
        };
        drawn.insert(marker.0);
        entities::place(
            &mut node,
            projection.place(map_point(object.position)),
            projection.size,
            SIZE,
        );
        if *interaction != Interaction::None {
            let p = object.position;
            let text = format!(
                "Ground item · {}\nY {:.1} · X {:.1} · Z {:.1}\nObject {}",
                object.model, p.y, p.x, p.z, object.drop_id
            );
            if tooltip.0 != text {
                tooltip.0 = text;
            }
        } else if !tooltip.0.is_empty() {
            tooltip.0.clear();
        }
    }
    for object in objects
        .values()
        .filter(|object| visible(object, origin) && !drawn.contains(&object.drop_id))
    {
        let mut node = Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(px(1)),
            ..default()
        };
        entities::place(
            &mut node,
            projection.place(map_point(object.position)),
            projection.size,
            SIZE,
        );
        commands.entity(canvas).with_child((
            Button,
            ItemMarker(object.drop_id),
            node,
            BackgroundColor(INK),
            BorderColor::all(Color::BLACK),
            Tooltip::default(),
            ZIndex(1),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing as session;
    use eq_client_core::{WorldEvent, WorldPosition, ground::ObjectUpdate};

    #[test]
    fn squares_follow_items_and_disappear_on_pickup_or_zone_replacement() {
        let mut app = App::new();
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(1));
        let item = |id, model: &str, x| GroundObject {
            drop_id: id,
            model: model.into(),
            object_type: 0,
            position: WorldPosition { x, ..default() },
        };
        session::news(
            &mut online,
            [WorldEvent::Objects(ObjectUpdate::Snapshot(vec![
                item(1, "IT63_ACTORDEF", 0.0), // Collides with a player ID, intentionally.
                item(70_000, "IT10", 20.0),    // Object IDs are wider than spawn IDs.
                item(3, "FORGE", 0.0),
                item(4, "IT63", f32::NAN),
                item(5, "IT63", 241.0),
            ]))],
        );
        app.insert_resource(online)
            .insert_resource(MapView {
                projection: super::super::Projection::new(
                    (Vec2::splat(-100.0), Vec2::splat(100.0)),
                    Vec2::splat(500.0),
                    0,
                    Vec2::ZERO,
                ),
                ..default()
            })
            .add_systems(Update, update);
        app.world_mut().spawn((MapCanvas, Node::default()));
        app.update();
        let markers = |app: &mut App| {
            let world = app.world_mut();
            world
                .query::<(Entity, &ItemMarker)>()
                .iter(world)
                .map(|(entity, marker)| (marker.0, entity))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let first = markers(&mut app);
        assert_eq!(first.len(), 2);
        let entity = first[&70_000];
        let before = app.world().get::<Node>(entity).unwrap().left;
        *app.world_mut().get_mut::<Interaction>(entity).unwrap() = Interaction::Hovered;
        session::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [
                WorldEvent::Objects(ObjectUpdate::Spawn(item(70_000, "IT10", 40.0))),
                WorldEvent::Objects(ObjectUpdate::Remove {
                    drop_id: 1,
                    taken_by: None,
                }),
            ],
        );
        app.update();
        assert_eq!(markers(&mut app).len(), 1);
        let node = app.world().get::<Node>(entity).unwrap();
        assert_ne!(node.left, before);
        assert_eq!(node.width, px(SIZE));
        assert_eq!(node.border_radius, BorderRadius::ZERO);
        assert!(
            app.world()
                .get::<Tooltip>(entity)
                .unwrap()
                .0
                .contains("Object 70000")
        );
        assert!(app.world().get::<entities::Action>(entity).is_none());
        session::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Position {
                spawn_id: 1,
                position: WorldPosition {
                    x: 400.0,
                    ..default()
                },
                velocity: [0.0; 3],
                forward: 0.0,
            }],
        );
        app.update();
        let nearby = markers(&mut app);
        assert_eq!(nearby.len(), 1);
        assert!(nearby.contains_key(&5));
        assert!(app.world().get_entity(entity).is_err());
        session::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            2,
            session::player(1),
        );
        app.update();
        assert!(markers(&mut app).is_empty());
    }
}
