//! Pointer navigation stays inside the map and never moves the character/camera.
use super::{MapCanvas, MapView, Projection, ZOOM_STEPS, entities::Marker};
use crate::windows::{Shown, WindowId, pointer::Wheel};
use bevy::{prelude::*, window::PrimaryWindow};

#[derive(Default)]
pub(crate) struct Gesture {
    drag: Option<Drag>,
    wheel: f32,
}

struct Drag {
    surface: Entity,
    canvas: Entity,
    start: Vec2,
    pan: Vec2,
    scale: f32,
}

type Surfaces<'w, 's> =
    Query<'w, 's, (Entity, &'static Interaction), Or<(With<MapCanvas>, With<Marker>)>>;

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
    windows: Query<&Window, With<PrimaryWindow>>,
    shown: Res<Shown>,
    mouse: Res<ButtonInput<MouseButton>>,
    wheel: Res<Wheel>,
    canvases: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<MapCanvas>>,
    surfaces: Surfaces,
    mut clicks: ResMut<crate::clicks::LeftButton>,
    mut view: ResMut<MapView>,
    mut gesture: Local<Gesture>,
) {
    let context = windows
        .single()
        .ok()
        .filter(|window| window.focused)
        .and_then(Window::physical_cursor_position)
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
        gesture.drag = surfaces
            .iter()
            .find(|(_, interaction)| **interaction == Interaction::Pressed)
            .zip(view.projection)
            .map(|((surface, _), projection)| Drag {
                surface,
                canvas,
                start: at,
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
            clicks.claim(drag.surface);
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
