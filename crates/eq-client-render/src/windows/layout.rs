//! In-memory layout continuity across zone-driven HUD reconstruction.
use super::{DragHandle, Frame, LayoutKey, MinimizeLabel, TitleBar, collapse};
use bevy::prelude::*;
use std::collections::BTreeMap;

/// A window's placement, and the frame it was last seen on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Saved {
    /// A different frame with the same key takes this placement.
    pub entity: Entity,
    pub edges: [Val; 4],
    pub margin: UiRect,
    pub position_type: PositionType,
    pub minimized: bool,
    /// Moved or minimized by the player; only these are kept between runs.
    pub placed: bool,
}

/// Window placements by window, kept through scene despawns.
#[derive(Resource, Default)]
pub(crate) struct Layouts(pub(super) BTreeMap<super::WindowId, Saved>);

/// Keeps explicitly placed panels reachable after resizing or UI scale changes.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn constrain(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut frames: Query<(&mut Node, &ComputedNode), With<Frame>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let viewport = window.physical_size().as_vec2();
    if viewport.min_element() <= 0.0 {
        return;
    }
    for (mut node, computed) in &mut frames {
        // Leave responsive default anchors and hidden/unlaid-out panels alone.
        let (Val::Px(left), Val::Px(top)) = (node.left, node.top) else {
            continue;
        };
        if node.position_type != PositionType::Absolute
            || node.right != Val::Auto
            || node.bottom != Val::Auto
            || node.margin != UiRect::ZERO
            || computed.size().min_element() <= 0.0
        {
            continue;
        }
        let maximum =
            ((viewport - computed.size()) * computed.inverse_scale_factor()).max(Vec2::ZERO);
        let position = Vec2::new(left, top).clamp(Vec2::ZERO, maximum);
        if position != Vec2::new(left, top) {
            node.left = px(position.x);
            node.top = px(position.y);
        }
    }
}

/// Restores replacement frames before layout, saving subsequent user changes.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn remember(
    keys: Query<(&LayoutKey, &DragHandle)>,
    mut frames: Query<(&mut Node, &mut Frame)>,
    children: Query<&Children>,
    titles: Query<(), With<TitleBar>>,
    mut body: Query<&mut Node, Without<Frame>>,
    mut labels: Query<(&MinimizeLabel, &mut Text)>,
    mut layouts: ResMut<Layouts>,
) {
    for (key, handle) in &keys {
        let Ok((mut node, mut frame)) = frames.get_mut(handle.0) else {
            continue;
        };
        if let Some(saved) = layouts
            .0
            .get(&key.0)
            .filter(|saved| saved.entity != handle.0)
        {
            [node.left, node.top, node.right, node.bottom] = saved.edges;
            node.margin = saved.margin;
            node.position_type = saved.position_type;
            frame.placed |= saved.placed;
            if saved.minimized {
                frame.minimized = true;
                collapse(
                    &mut node,
                    &mut frame,
                    children.get(handle.0).ok(),
                    &titles,
                    &mut body,
                );
                for (label, mut text) in &mut labels {
                    if label.0 == handle.0 {
                        text.0 = "+".into();
                    }
                }
            }
        }
        layouts.0.insert(
            key.0,
            Saved {
                entity: handle.0,
                edges: [node.left, node.top, node.right, node.bottom],
                margin: node.margin,
                position_type: node.position_type,
                minimized: frame.minimized,
                placed: frame.placed,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_keeps_its_place_while_a_full_window_screen_lays_out_in_pixels() {
        // A 1920 by 1080 screen at 150%: 1280 by 720 units at the desktop's
        // scale, 1920 by 1080 while an official screen takes one unit to a
        // pixel (device_pixels).
        let mut app = App::new();
        app.add_systems(Update, constrain);
        app.world_mut().spawn((
            Window {
                resolution: bevy::window::WindowResolution::new(1920, 1080)
                    .with_scale_factor_override(1.5),
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        let panel = app
            .world_mut()
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(1000),
                    top: px(600),
                    ..default()
                },
                ComputedNode {
                    size: Vec2::new(300.0, 150.0),
                    inverse_scale_factor: 1.0 / 1.5,
                    ..default()
                },
                Frame::default(),
            ))
            .id();
        let laid_out = |app: &mut App, scale: f32| {
            let mut computed = app.world_mut().get_mut::<ComputedNode>(panel).unwrap();
            computed.size = Vec2::new(200.0, 100.0) * scale;
            computed.inverse_scale_factor = scale.recip();
            app.update();
            let node = app.world().get::<Node>(panel).unwrap();
            (node.left, node.top)
        };
        let place = (px(1000), px(600));
        assert_eq!(laid_out(&mut app, 1.5), place);
        assert_eq!(laid_out(&mut app, 1.0), place);
        assert_eq!(laid_out(&mut app, 1.5), place);

        // A scroll position is in logical pixels, which the toggle leaves
        // alone: 1000 units of chat in a 300 unit view scroll 700 at either
        // scale, so a part-scrolled chat keeps its place and a chat at its
        // newest line stays there.
        let scrolled = |state: crate::chat::ChatState, scales: &[f32]| {
            let mut app = App::new();
            app.insert_resource(state)
                .add_systems(Update, crate::chat::scroll);
            let list = app
                .world_mut()
                .spawn((
                    ComputedNode::default(),
                    ScrollPosition::default(),
                    crate::chat::Viewport,
                ))
                .id();
            scales
                .iter()
                .map(|&scale| {
                    let mut computed = app.world_mut().get_mut::<ComputedNode>(list).unwrap();
                    computed.size = Vec2::new(200.0, 300.0) * scale;
                    computed.content_size = Vec2::new(200.0, 1000.0) * scale;
                    computed.inverse_scale_factor = scale.recip();
                    app.update();
                    app.world().get::<ScrollPosition>(list).unwrap().y
                })
                .collect::<Vec<_>>()
        };
        let toggled = [1.5, 1.0, 1.5];
        for y in scrolled(crate::chat::ChatState::scrolled_for_tests(250.0), &toggled) {
            assert!((y - 250.0).abs() < 0.001, "{y}");
        }
        for y in scrolled(crate::chat::ChatState::default(), &toggled) {
            assert!((y - 700.0).abs() < 0.001, "{y}");
        }
    }

    #[test]
    fn resizing_keeps_moved_panels_reachable_without_changing_default_anchors() {
        let mut app = App::new();
        app.add_systems(Update, constrain);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(800, 600),
                    ..default()
                },
                bevy::window::PrimaryWindow,
            ))
            .id();
        let panel = app
            .world_mut()
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(290),
                    top: px(240),
                    margin: UiRect::ZERO,
                    ..default()
                },
                ComputedNode {
                    size: Vec2::new(200.0, 100.0),
                    inverse_scale_factor: 0.5,
                    ..default()
                },
                Frame::default(),
            ))
            .id();
        let anchored = app
            .world_mut()
            .spawn((
                Node {
                    left: percent(50),
                    top: px(16),
                    ..default()
                },
                ComputedNode::default(),
                Frame::default(),
            ))
            .id();
        app.update();
        assert_eq!(app.world().get::<Node>(panel).unwrap().left, px(290));
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_physical_resolution(600, 400);
        app.update();
        let node = app.world().get::<Node>(panel).unwrap();
        assert_eq!((node.left, node.top), (px(200), px(150)));
        // A panel larger than the viewport keeps its title at the top left.
        app.world_mut().get_mut::<ComputedNode>(panel).unwrap().size = Vec2::splat(900.0);
        app.update();
        let node = app.world().get::<Node>(panel).unwrap();
        assert_eq!((node.left, node.top), (px(0), px(0)));
        assert_eq!(app.world().get::<Node>(anchored).unwrap().left, percent(50));
    }

    fn window(app: &mut App, height: f32) -> (Entity, Entity) {
        let frame = app
            .world_mut()
            .spawn((
                Node {
                    height: px(height),
                    ..default()
                },
                Frame::default(),
            ))
            .id();
        let title = app
            .world_mut()
            .spawn((
                Node::default(),
                TitleBar,
                LayoutKey(super::super::WindowId::Chat),
                DragHandle(frame),
            ))
            .id();
        let body = app
            .world_mut()
            .spawn(Node {
                display: Display::Grid,
                ..default()
            })
            .id();
        app.world_mut()
            .entity_mut(frame)
            .add_children(&[title, body]);
        (frame, body)
    }

    #[test]
    fn replacement_uses_saved_placement_but_its_own_body_and_dimensions() {
        let mut app = App::new();
        app.init_resource::<Layouts>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<super::super::DragState>()
            .add_systems(Update, (super::super::input, remember).chain());
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        let (old, _) = window(&mut app, 200.0);
        {
            let mut node = app.world_mut().get_mut::<Node>(old).unwrap();
            node.position_type = PositionType::Absolute;
            node.left = px(71);
            node.top = px(93);
            node.right = Val::Auto;
        }
        app.world_mut().get_mut::<Frame>(old).unwrap().minimized = true;
        app.update();
        app.world_mut().despawn(old);
        let (new, body) = window(&mut app, 240.0);
        app.update();
        let node = app.world().get::<Node>(new).unwrap();
        assert_eq!(
            (node.left, node.top, node.height),
            (px(71), px(93), Val::Auto)
        );
        assert_eq!(
            app.world().get::<Node>(body).unwrap().display,
            Display::None
        );
        let frame = app.world().get::<Frame>(new).unwrap();
        assert_eq!(frame.restored_height, Some((px(240), Val::Auto)));
        assert_eq!(frame.restored_display, vec![(body, Display::Grid)]);
        app.update();
        assert_eq!(
            app.world().get::<Frame>(new).unwrap().restored_height,
            Some((px(240), Val::Auto))
        );
        assert_eq!(app.world().resource::<Layouts>().0.len(), 1);
        app.world_mut()
            .spawn((Interaction::Pressed, super::super::Minimize(new)));
        app.update();
        assert_eq!(app.world().get::<Node>(new).unwrap().height, px(240));
        assert_eq!(
            app.world().get::<Node>(body).unwrap().display,
            Display::Grid
        );
        assert!(!app.world().get::<Frame>(new).unwrap().minimized);
    }
}
