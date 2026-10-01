//! Shared world-input blocking using the renderer's visible UI geometry, and the
//! one reader of the mouse wheel.
use bevy::{
    ecs::system::SystemParam,
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    ui::ComputedStackIndex,
    window::PrimaryWindow,
};

/// Pixels one wheel line scrolls.
const LINE_PIXELS: f32 = 24.0;

/// A surface the mouse wheel scrolls when it is the topmost under the pointer:
/// a scrolling list, a window that scrolls as a whole, or the chat.
#[derive(Component)]
pub(crate) struct TakesWheel;

/// This frame's wheel turn and the one surface it belongs to.
#[derive(Resource, Default)]
pub(crate) struct Wheel {
    /// Pixels to scroll, upward positive.
    pub(crate) pixels: f32,
    /// The same turn in wheel lines, for zooming the camera.
    pub(crate) lines: f32,
    /// The topmost visible surface under the pointer that takes the wheel;
    /// with none, the wheel zooms the camera.
    pub(crate) surface: Option<Entity>,
}

/// The surfaces that take the wheel, with where they are drawn and how high.
type WheelSurfaces<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static UiGlobalTransform,
        &'static ComputedNode,
        &'static ComputedStackIndex,
        Option<&'static InheritedVisibility>,
    ),
    With<TakesWheel>,
>;

/// Reads the wheel once and gives it to the topmost surface under the pointer
/// that takes it, so one turn scrolls one thing.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn wheel(
    mut events: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    surfaces: WheelSurfaces,
    mut wheel: ResMut<Wheel>,
) {
    let (mut pixels, mut lines) = (0.0, 0.0);
    for event in events.read() {
        match event.unit {
            MouseScrollUnit::Line => {
                lines += event.y;
                pixels += event.y * LINE_PIXELS;
            }
            MouseScrollUnit::Pixel => {
                lines += event.y / LINE_PIXELS;
                pixels += event.y;
            }
        }
    }
    let cursor = windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position);
    *wheel = Wheel {
        pixels,
        lines,
        surface: cursor.and_then(|cursor| {
            surfaces
                .iter()
                .filter(|(_, transform, node, _, visibility)| {
                    visibility.is_none_or(|visibility| visibility.get())
                        && super::contains(cursor, transform, node)
                })
                .max_by_key(|(_, _, _, stack, _)| stack.0)
                .map(|(entity, ..)| entity)
        }),
    };
}

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
