//! Pointer navigation stays inside the map and never moves the character/camera.
use super::{MapCanvas, MapView, Projection, ZOOM_STEPS, entities::Marker};
use crate::windows::{Shown, WindowId, pointer::Wheel};
use bevy::{
    input::ButtonState,
    prelude::*,
    window::{PrimaryWindow, WindowEvent},
};

#[derive(Default)]
pub(crate) struct Gesture {
    drag: Option<Drag>,
    wheel: f32,
    cursor: Option<Vec2>,
}

impl Gesture {
    /// Retain the press's position before later motion updates the window cursor.
    fn press<'a>(
        &mut self,
        events: impl Iterator<Item = &'a WindowEvent>,
        window: Entity,
        scale: f32,
    ) -> Option<Vec2> {
        let mut pressed = None;
        for event in events {
            match event {
                WindowEvent::CursorMoved(event) if event.window == window => {
                    self.cursor = Some(event.position * scale);
                }
                WindowEvent::MouseButtonInput(event)
                    if event.window == window
                        && event.button == MouseButton::Left
                        && event.state == ButtonState::Pressed =>
                {
                    pressed = self.cursor;
                }
                WindowEvent::WindowFocused(event) if event.window == window && !event.focused => {
                    self.cursor = None;
                    self.drag = None;
                    pressed = None;
                }
                _ => (),
            }
        }
        pressed
    }
}

struct Drag {
    canvas: Entity,
    start: Vec2,
    pan: Vec2,
    scale: f32,
}

type Surfaces<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Interaction),
    Or<(
        With<MapCanvas>,
        With<Marker>,
        With<super::items::ItemMarker>,
    )>,
>;

/// Converts physical screen coordinates into this canvas's logical coordinates.
fn local(cursor: Vec2, transform: &UiGlobalTransform, node: &ComputedNode) -> Option<Vec2> {
    Some(
        (transform.try_inverse()?.transform_point2(cursor) + node.size() / 2.0)
            * node.inverse_scale_factor(),
    )
}

/// Zooms without moving the world point currently beneath the cursor.
fn zoom_at(view: &mut MapView, size: Vec2, cursor: Vec2, steps: i32) {
    let Some(bounds) = view.bounds else { return };
    let projection =
        |zoom, pan| Projection::for_canvas(bounds, size, zoom, pan, view.selected.is_some());
    let Some(before) = projection(view.zoom, view.pan) else {
        return;
    };
    let zoom = view
        .zoom
        .saturating_add(steps)
        .clamp(-ZOOM_STEPS, ZOOM_STEPS);
    let Some(after) = projection(zoom, view.pan) else {
        return;
    };
    view.pan += (cursor - before.offset) / before.scale - (cursor - after.offset) / after.scale;
    view.zoom = zoom;
}

/// Pan by dragging empty canvas or a marker; a drag cancels marker selection.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn input(
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    shown: Res<Shown>,
    mouse: Res<ButtonInput<MouseButton>>,
    wheel: Res<Wheel>,
    canvases: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<MapCanvas>>,
    surfaces: Surfaces,
    mut clicks: ResMut<crate::clicks::LeftButton>,
    mut view: ResMut<MapView>,
    mut gesture: Local<Gesture>,
    mut events: MessageReader<WindowEvent>,
    ui: crate::windows::pointer::PointerUi,
) {
    let Ok((window_id, window)) = windows.single() else {
        events.clear();
        return;
    };
    let pressed = gesture.press(events.read(), window_id, window.scale_factor());
    let context = window
        .focused
        .then(|| window.physical_cursor_position())
        .flatten()
        .zip(canvases.single().ok());
    let Some((cursor, (canvas, node, transform))) =
        context.filter(|_| shown.displayed(WindowId::Map))
    else {
        *gesture = Gesture::default();
        return;
    };
    let Some(at) = local(cursor, transform, node) else {
        return;
    };
    let size = node.size() * node.inverse_scale_factor();
    if mouse.just_pressed(MouseButton::Left) {
        let origin = pressed.unwrap_or(cursor);
        gesture.drag = ui
            .top_at(origin)
            .filter(|surface| surfaces.contains(*surface))
            .zip(local(origin, transform, node))
            .zip(view.projection)
            .map(|((_, start), projection)| Drag {
                canvas,
                start,
                pan: view.pan,
                scale: projection.scale,
            });
    }
    if let Some(drag) = &gesture.drag {
        if drag.canvas != canvas {
            gesture.drag = None;
        } else if at.distance(drag.start) > 3.0 {
            // Release tracking precedes this system; claim both the pending and
            // just-released click so a long one-frame drag cannot select a dot.
            clicks.cancel();
            view.pan = drag.pan - (at - drag.start) / drag.scale;
        }
    }
    if !mouse.pressed(MouseButton::Left) {
        gesture.drag = None;
    }
    let hovered = surfaces
        .iter()
        .any(|(_, interaction)| *interaction != Interaction::None);
    if wheel.surface == Some(canvas) && hovered && gesture.drag.is_none() {
        gesture.wheel += wheel.lines;
        if gesture.wheel.abs() >= 1.0 {
            #[allow(clippy::cast_possible_truncation)] // Saturating cast; bounded zoom steps below.
            let steps = gesture.wheel as i32;
            gesture.wheel = gesture.wheel.fract();
            zoom_at(&mut view, size, at, steps);
        }
    } else {
        gesture.wheel = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A complete native drag can arrive between two UI frames.
    fn short_drag(covered: bool) -> Vec2 {
        let mut app = App::new();
        let mut shown = Shown::default();
        shown.open(WindowId::Map);
        let projection = Projection::new(
            (Vec2::splat(-250.0), Vec2::splat(250.0)),
            Vec2::splat(500.0),
            0,
            Vec2::ZERO,
        )
        .unwrap();
        app.insert_resource(shown)
            .insert_resource(MapView {
                projection: Some(projection),
                ..default()
            })
            .init_resource::<Wheel>()
            .init_resource::<crate::clicks::LeftButton>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<WindowEvent>()
            .add_systems(Update, input);
        let mut window = Window::default();
        window.set_physical_cursor_position(Some(bevy::math::DVec2::new(160.0, 140.0)));
        let window = app.world_mut().spawn((PrimaryWindow, window)).id();
        app.world_mut().spawn((
            MapCanvas,
            Node::default(),
            InheritedVisibility::VISIBLE,
            ComputedNode {
                size: Vec2::splat(500.0),
                ..default()
            },
            UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::splat(250.0))),
            bevy::ui::ComputedStackIndex(1),
        ));
        if covered {
            // The press starts on a panel above the map, then ends on the map.
            app.world_mut().spawn((
                crate::windows::KeepsPress,
                Node::default(),
                InheritedVisibility::VISIBLE,
                ComputedNode {
                    size: Vec2::splat(40.0),
                    ..default()
                },
                UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::splat(100.0))),
                bevy::ui::ComputedStackIndex(2),
            ));
        }
        let moved = |position| {
            WindowEvent::CursorMoved(bevy::window::CursorMoved {
                window,
                position,
                delta: None,
            })
        };
        let button = |state| {
            WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                window,
                button: MouseButton::Left,
                state,
            })
        };
        app.world_mut()
            .resource_mut::<Messages<WindowEvent>>()
            .write_batch([
                moved(Vec2::splat(100.0)),
                button(ButtonState::Pressed),
                moved(Vec2::new(160.0, 140.0)),
                button(ButtonState::Released),
            ]);
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.press(MouseButton::Left);
        mouse.release(MouseButton::Left);
        app.update();
        app.world().resource::<MapView>().pan * projection.scale
    }

    #[test]
    fn a_drag_within_one_frame_uses_the_press_position_and_respects_covering_panels() {
        assert!((short_drag(false) - Vec2::new(-60.0, -40.0)).length() < 0.001);
        assert_eq!(short_drag(true), Vec2::ZERO);
    }

    #[test]
    fn zoom_keeps_the_cursor_anchor_and_clamps_without_drift() {
        let mut view = MapView {
            bounds: Some(super::super::nearby_bounds(Vec2::new(20.0, -70.0))),
            ..default()
        };
        let size = Vec2::new(600.0, 400.0);
        let cursor = Vec2::new(200.0, 270.0);
        let projection = |view: &MapView| {
            Projection::for_canvas(view.bounds.unwrap(), size, view.zoom, view.pan, false).unwrap()
        };
        let before = projection(&view);
        let point = before.centre + (cursor - before.offset) / before.scale;
        zoom_at(&mut view, size, cursor, 3);
        assert!(projection(&view).place(point).distance(cursor) < 0.001);
        zoom_at(&mut view, size, cursor, i32::MAX);
        let pan = view.pan;
        zoom_at(&mut view, size, cursor, 1);
        assert_eq!(view.zoom, ZOOM_STEPS);
        assert_eq!(view.pan, pan);
        assert!(projection(&view).place(point).distance(cursor) < 0.01);
    }
}
