//! Shared world-input blocking using the renderer's visible UI geometry.
use bevy::{ecs::system::SystemParam, prelude::*};

type Surfaces<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static UiGlobalTransform,
        &'static ComputedNode,
        Option<&'static InheritedVisibility>,
    ),
    super::PointerSurface,
>;

#[derive(SystemParam)]
pub(crate) struct PointerUi<'w, 's> {
    surfaces: Surfaces<'w, 's>,
    clipping: Query<
        'w,
        's,
        (
            &'static ComputedNode,
            &'static UiGlobalTransform,
            &'static Node,
        ),
    >,
    parents: Query<'w, 's, &'static ChildOf, Without<bevy::ui::OverrideClip>>,
}

impl PointerUi<'_, '_> {
    /// Matches UI hit testing, including rounded corners and ancestor scroll clipping.
    pub fn contains(&self, cursor: Vec2) -> bool {
        self.surfaces
            .iter()
            .any(|(entity, transform, node, visibility)| {
                visibility.is_none_or(|visibility| visibility.get())
                    && node.size().cmpgt(Vec2::ZERO).all()
                    && node.contains_point(*transform, cursor)
                    && bevy::ui::clip_check_recursive(cursor, entity, &self.clipping, &self.parents)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct Hit(bool);

    #[test]
    fn clipped_and_hidden_controls_do_not_block_world_input() {
        let mut app = App::new();
        app.init_resource::<Hit>()
            .add_systems(Update, |ui: PointerUi, mut hit: ResMut<Hit>| {
                hit.0 = ui.contains(Vec2::splat(50.0));
            });
        let parent = app
            .world_mut()
            .spawn((
                Node {
                    overflow: Overflow::clip(),
                    ..default()
                },
                ComputedNode {
                    size: Vec2::splat(40.0),
                    ..default()
                },
                UiGlobalTransform::default(),
            ))
            .id();
        let button = app
            .world_mut()
            .spawn((
                Button,
                InheritedVisibility::VISIBLE,
                ComputedNode {
                    size: Vec2::splat(200.0),
                    ..default()
                },
                UiGlobalTransform::default(),
                ChildOf(parent),
            ))
            .id();
        app.update();
        assert!(!app.world().resource::<Hit>().0);
        app.world_mut().get_mut::<Node>(parent).unwrap().overflow = Overflow::visible();
        app.update();
        assert!(app.world().resource::<Hit>().0);
        app.world_mut()
            .entity_mut(button)
            .insert(InheritedVisibility::HIDDEN);
        app.update();
        assert!(!app.world().resource::<Hit>().0);
        app.world_mut()
            .entity_mut(button)
            .insert(InheritedVisibility::VISIBLE);
        app.world_mut()
            .get_mut::<ComputedNode>(button)
            .unwrap()
            .size = Vec2::ZERO;
        app.update();
        assert!(!app.world().resource::<Hit>().0);
    }
}
