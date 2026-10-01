//! A depth-tested ground ring follows the selected rendered entity, without gameplay actions.
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct Marker;

/// Keeps the marker at the interpolated entity position and removes it from stale admissions.
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
pub(crate) fn update(
    mut commands: Commands,
    target: Res<super::TargetState>,
    online: Res<crate::online::OnlineState>,
    nearby: Res<crate::entities::NearbyEntities>,
    poses: Query<&Transform, Without<Marker>>,
    player: Query<Entity, With<crate::Player>>,
    collision: Option<Res<crate::Collision>>,
    mut rings: Query<(&mut Transform, &mut Visibility), With<Marker>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let selected = target
        .selected
        .filter(|_| online.connected && online.death.is_none());
    let pose = selected.and_then(|id| {
        let own = online
            .player
            .as_ref()
            .filter(|player| player.spawn_id == id);
        let (entity, size) = if let Some(own) = own {
            (player.single().ok()?, own.size)
        } else {
            let spawn = online.spawns.get(&id).filter(|spawn| !spawn.invisible)?;
            (*nearby.rendered.get(&id)?, spawn.size)
        };
        let position = poses.get(entity).ok()?.translation;
        let size = if size.is_finite() && size > 0.0 {
            size
        } else {
            6.0
        };
        let ground = collision
            .as_ref()
            .and_then(|world| world.0.as_ref())
            .and_then(|world| world.ground(position, 0.05, size.max(6.0) * 2.0));
        Some(marker_pose(position, size, ground))
    });
    if let Ok((mut transform, mut visibility)) = rings.single_mut() {
        *visibility = if let Some(pose) = pose {
            *transform = pose;
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    } else if let Some(pose) = pose {
        commands.spawn((
            crate::SceneEntity,
            Marker,
            pose,
            Visibility::Inherited,
            Mesh3d(meshes.add(Annulus::new(0.90, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.95, 0.76, 0.22),
                unlit: true,
                cull_mode: None,
                ..default()
            })),
        ));
    }
}

fn marker_pose(position: Vec3, size: f32, ground: Option<f32>) -> Transform {
    Transform {
        translation: Vec3::new(
            position.x,
            ground.unwrap_or(position.y - size * 0.5) + 0.04,
            position.z,
        ),
        rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        scale: Vec3::splat((size * 0.25).clamp(0.75, 8.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies unchanged or explicitly assigned state"
    )]
    fn ring_follows_interpolated_entities_and_hides_when_selection_is_unavailable() {
        let mut app = App::new();
        let mut online = crate::online::OnlineState::new(false);
        online.connected = true;
        online.spawns.insert(
            2,
            eq_client_core::SpawnState {
                class: None,
                spawn_id: 2,
                name: "Synthetic target".into(),
                kind: eq_client_core::SpawnKind::Npc,
                race: 1,
                gender: 0,
                position: default(),
                velocity: [0.0; 3],
                size: 6.0,
                invisible: false,
                appearance: eq_client_core::outfit::Appearance::default(),
            },
        );
        let entity = app
            .world_mut()
            .spawn(Transform::from_xyz(4.0, 3.0, 8.0))
            .id();
        let mut nearby = crate::entities::NearbyEntities::default();
        nearby.rendered.insert(2, entity);
        app.insert_resource(online)
            .insert_resource(nearby)
            .insert_resource(super::super::TargetState {
                selected: Some(2),
                ..default()
            })
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, update);
        app.update();
        let mut ring = app
            .world_mut()
            .query_filtered::<(&Transform, &Visibility), With<Marker>>();
        assert_eq!(ring.single(app.world()).unwrap().0.translation.x, 4.0);
        assert_eq!(*ring.single(app.world()).unwrap().1, Visibility::Inherited);
        app.world_mut()
            .get_mut::<Transform>(entity)
            .unwrap()
            .translation
            .x = 11.0;
        app.update();
        assert_eq!(ring.single(app.world()).unwrap().0.translation.x, 11.0);
        app.world_mut()
            .resource_mut::<crate::online::OnlineState>()
            .spawns
            .get_mut(&2)
            .unwrap()
            .invisible = true;
        app.update();
        assert_eq!(*ring.single(app.world()).unwrap().1, Visibility::Hidden);
        app.world_mut()
            .resource_mut::<crate::online::OnlineState>()
            .spawns
            .get_mut(&2)
            .unwrap()
            .invisible = false;
        app.update();
        assert_eq!(*ring.single(app.world()).unwrap().1, Visibility::Inherited);
        app.world_mut()
            .resource_mut::<crate::entities::NearbyEntities>()
            .rendered
            .clear();
        app.update();
        assert_eq!(*ring.single(app.world()).unwrap().1, Visibility::Hidden);
        app.world_mut()
            .resource_mut::<crate::entities::NearbyEntities>()
            .rendered
            .insert(2, entity);
        app.world_mut()
            .resource_mut::<crate::online::OnlineState>()
            .connected = false;
        app.update();
        assert_eq!(*ring.single(app.world()).unwrap().1, Visibility::Hidden);
    }

    #[test]
    fn ground_marker_uses_rendered_position_and_bounded_radius() {
        let pose = marker_pose(Vec3::new(12.0, 10.0, -7.0), 6.0, Some(3.0));
        assert_eq!(pose.translation, Vec3::new(12.0, 3.04, -7.0));
        assert_eq!(pose.scale, Vec3::splat(1.5));
        assert!((pose.rotation * Vec3::Z - Vec3::Y).length() < 0.001);
        assert_eq!(marker_pose(Vec3::ZERO, 0.5, None).scale, Vec3::splat(0.75));
        assert_eq!(marker_pose(Vec3::ZERO, 100.0, None).scale, Vec3::splat(8.0));
    }
}
