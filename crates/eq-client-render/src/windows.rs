//! Shared dragging and minimization behavior for HUD windows.

use bevy::{prelude::*, window::PrimaryWindow};
mod layout;
pub(super) mod pointer;
mod store;
pub(super) use layout::Layouts;

/// Restores saved positions before layout and constrains measured frames afterward;
/// placements persist between runs per character.
pub(super) fn register_layout(app: &mut App) {
    app.add_systems(Update, store::persist.after(super::online::receive));
    app.add_systems(
        PostUpdate,
        layout::remember.before(bevy::ui::UiSystems::Layout),
    );
    app.add_systems(
        PostUpdate,
        layout::constrain.after(bevy::ui::UiSystems::Layout),
    );
}

#[derive(Component)]
pub(super) struct LayoutKey(String);

/// Gives a passive panel a stable identity across HUD reconstruction.
pub(super) fn identify(commands: &mut Commands, frame: Entity, key: &str) {
    commands.entity(frame).insert(LayoutKey(key.into()));
}

/// Visible windows and standalone controls consume pointer input before the world.
pub(super) type PointerSurface = Or<(With<Frame>, With<Button>)>;

/// Wheel travel since the last read, in logical pixels; positive scrolls up.
pub(super) fn wheel_pixels(wheel: &mut MessageReader<bevy::input::mouse::MouseWheel>) -> f32 {
    wheel
        .read()
        .map(|event| match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => event.y * 24.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => event.y,
        })
        .sum()
}

/// Whether a physical cursor position lies inside a laid-out UI node.
pub(super) fn contains(cursor: Vec2, transform: &UiGlobalTransform, node: &ComputedNode) -> bool {
    transform.try_inverse().is_some_and(|inverse| {
        inverse
            .transform_point2(cursor)
            .abs()
            .cmple(node.size() * 0.5)
            .all()
    })
}

#[derive(Component, Default)]
pub(super) struct Frame {
    minimized: bool,
    /// Moved or minimized by the player, so its placement is worth keeping.
    placed: bool,
    restored_display: Vec<(Entity, Display)>,
    restored_height: Option<(Val, Val)>,
}

#[derive(Component)]
pub(super) struct DragHandle(Entity);

#[derive(Component)]
pub(super) struct TitleBar;

#[derive(Component)]
pub(super) struct Minimize(Entity);

#[derive(Component)]
pub(super) struct MinimizeLabel(Entity);

#[derive(Resource, Default)]
pub(super) struct DragState {
    active: Option<ActiveDrag>,
}

struct ActiveDrag {
    frame: Entity,
    cursor: Vec2,
    origin: Vec2,
    inverse_scale: f32,
}

/// Marks a passive panel so its entire surface can be used to move it.
pub(super) fn passive(commands: &mut Commands, entity: Entity) {
    commands
        .entity(entity)
        .insert((Frame::default(), Button, DragHandle(entity)));
}

/// Converts a passive frame to a title-bar-driven interactive window.
pub(super) fn titled(commands: &mut Commands, entity: Entity, title: &str) {
    commands.entity(entity).remove::<(Button, DragHandle)>();
    commands
        .entity(entity)
        .with_children(|parent| title_bar(parent, entity, title));
}

/// Adds a compact drag bar and minimize button to an interactive window.
pub(super) fn title_bar(parent: &mut ChildSpawnerCommands, frame: Entity, title: &str) {
    parent
        .spawn((
            Button,
            DragHandle(frame),
            LayoutKey(title.into()),
            TitleBar,
            Node {
                width: percent(100),
                min_height: px(22),
                padding: UiRect::horizontal(px(5)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgb(0.055, 0.067, 0.078)),
        ))
        .with_children(|bar| {
            bar.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(Color::srgb(0.68, 0.71, 0.74)),
            ));
            bar.spawn((
                Button,
                Minimize(frame),
                Node {
                    width: px(20),
                    height: px(18),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.12, 0.14)),
            ))
            .with_children(|button| {
                button.spawn((
                    MinimizeLabel(frame),
                    Text::new("_"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.82, 0.82, 0.78)),
                ));
            });
        });
}

/// Moves windows from their drag surfaces and collapses interactive window bodies.
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
pub(super) fn input(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    handles: Query<(&Interaction, &DragHandle), Changed<Interaction>>,
    minimizes: Query<(&Interaction, &Minimize), Changed<Interaction>>,
    children: Query<&Children>,
    title_bars: Query<(), With<TitleBar>>,
    mut frames: Query<(&mut Node, &ComputedNode, &UiGlobalTransform, &mut Frame)>,
    mut child_nodes: Query<&mut Node, Without<Frame>>,
    mut labels: Query<(&MinimizeLabel, &mut Text)>,
    mut drag: ResMut<DragState>,
) {
    let Ok(window) = windows.single() else {
        drag.active = None;
        return;
    };
    if !window.focused {
        drag.active = None;
        return;
    }
    for (interaction, Minimize(frame)) in &minimizes {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Ok((mut node, _, _, mut state)) = frames.get_mut(*frame) else {
            continue;
        };
        state.minimized = !state.minimized;
        state.placed = true;
        collapse(
            &mut node,
            &mut state,
            children.get(*frame).ok(),
            &title_bars,
            &mut child_nodes,
        );
        for (label, mut text) in &mut labels {
            if label.0 == *frame {
                text.0 = if state.minimized { "+" } else { "_" }.into();
            }
        }
    }

    let cursor = window.physical_cursor_position();
    if mouse.just_released(MouseButton::Left) || !mouse.pressed(MouseButton::Left) {
        drag.active = None;
    }
    if mouse.just_pressed(MouseButton::Left)
        && let Some(cursor) = cursor
        && let Some(frame) = handles
            .iter()
            .find(|(interaction, _)| **interaction == Interaction::Pressed)
            .map(|(_, handle)| handle.0)
        && let Ok((_, computed, transform, _)) = frames.get(frame)
    {
        drag.active = Some(ActiveDrag {
            frame,
            cursor: cursor * computed.inverse_scale_factor(),
            origin: (transform.translation - computed.size() * 0.5)
                * computed.inverse_scale_factor(),
            inverse_scale: computed.inverse_scale_factor(),
        });
    }
    let Some(active) = drag.active.as_ref() else {
        return;
    };
    let Some(cursor) = cursor else {
        return;
    };
    if let Ok((mut node, computed, _, mut frame)) = frames.get_mut(active.frame) {
        if computed.inverse_scale_factor().to_bits() != active.inverse_scale.to_bits() {
            drag.active = None;
            return;
        }
        let position = active.origin + cursor * active.inverse_scale - active.cursor;
        let maximum = ((window.physical_size().as_vec2() - computed.size()) * active.inverse_scale)
            .max(Vec2::ZERO);
        let position = position.clamp(Vec2::ZERO, maximum);
        node.position_type = PositionType::Absolute;
        node.left = px(position.x);
        node.top = px(position.y);
        node.right = Val::Auto;
        node.bottom = Val::Auto;
        node.margin = UiRect::ZERO;
        frame.placed = true;
    }
}

/// Inserts the shared state on an interactive frame.
pub(super) fn interactive(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).insert(Frame::default());
}

/// Applies minimized state using the current entity's body and original dimensions.
fn collapse(
    node: &mut Node,
    state: &mut Frame,
    children: Option<&Children>,
    title_bars: &Query<(), With<TitleBar>>,
    child_nodes: &mut Query<&mut Node, Without<Frame>>,
) {
    if state.minimized {
        state.restored_height = Some((node.height, node.min_height));
        node.height = Val::Auto;
        node.min_height = Val::Auto;
        if let Some(children) = children {
            for child in children {
                if !title_bars.contains(*child)
                    && let Ok(mut node) = child_nodes.get_mut(*child)
                {
                    state.restored_display.push((*child, node.display));
                    node.display = Display::None;
                }
            }
        }
    } else {
        if let Some((height, min_height)) = state.restored_height.take() {
            node.height = height;
            node.min_height = min_height;
        }
        for (child, display) in state.restored_display.drain(..) {
            if let Ok(mut node) = child_nodes.get_mut(child) {
                node.display = display;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_drag_preserves_offset_clamps_to_viewport_and_cancels_on_focus_loss() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<DragState>()
            .add_systems(Update, input);
        let mut window = Window {
            focused: true,
            resolution: bevy::window::WindowResolution::new(800, 600)
                .with_scale_factor_override(2.0),
            ..default()
        };
        window.set_physical_cursor_position(Some(bevy::math::DVec2::new(200.0, 150.0)));
        let window = app.world_mut().spawn((window, PrimaryWindow)).id();
        let frame = app
            .world_mut()
            .spawn((
                Node::default(),
                ComputedNode {
                    size: Vec2::new(200.0, 100.0),
                    inverse_scale_factor: 0.5,
                    ..default()
                },
                UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::new(
                    200.0, 150.0,
                ))),
                Frame::default(),
            ))
            .id();
        app.world_mut()
            .spawn((Interaction::Pressed, DragHandle(frame)));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let node = app.world().get::<Node>(frame).unwrap();
        assert_eq!((node.left, node.top), (px(50.0), px(50.0)));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_physical_cursor_position(Some(bevy::math::DVec2::new(240.0, 170.0)));
        app.update();
        let node = app.world().get::<Node>(frame).unwrap();
        assert_eq!((node.left, node.top), (px(70.0), px(60.0)));
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_physical_cursor_position(Some(bevy::math::DVec2::new(790.0, 590.0)));
        app.update();
        let node = app.world().get::<Node>(frame).unwrap();
        assert_eq!((node.left, node.top), (px(300.0), px(250.0)));
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert!(app.world().resource::<DragState>().active.is_none());
        {
            let mut window = app.world_mut().get_mut::<Window>(window).unwrap();
            window.focused = true;
            window.set_physical_cursor_position(Some(bevy::math::DVec2::new(100.0, 100.0)));
        }
        app.update();
        let node = app.world().get::<Node>(frame).unwrap();
        assert_eq!((node.left, node.top), (px(300.0), px(250.0)));
    }

    #[test]
    fn interactive_window_minimizes_and_restores_only_its_body() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<DragState>()
            .add_systems(Update, input);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let frame = app
            .world_mut()
            .spawn((
                Node {
                    height: px(202),
                    min_height: px(120),
                    ..default()
                },
                ComputedNode::default(),
                UiGlobalTransform::default(),
                Frame::default(),
            ))
            .id();
        let title = app.world_mut().spawn((Node::default(), TitleBar)).id();
        let body = app
            .world_mut()
            .spawn(Node {
                display: Display::Grid,
                ..default()
            })
            .id();
        let hidden = app
            .world_mut()
            .spawn(Node {
                display: Display::None,
                ..default()
            })
            .id();
        let button = app
            .world_mut()
            .spawn((Interaction::Pressed, Minimize(frame)))
            .id();
        app.world_mut()
            .entity_mut(frame)
            .add_children(&[title, body, hidden]);

        app.update();
        assert_eq!(
            app.world().get::<Node>(body).unwrap().display,
            Display::None
        );
        assert!(app.world().get::<Frame>(frame).unwrap().minimized);
        let node = app.world().get::<Node>(frame).unwrap();
        assert_eq!((node.height, node.min_height), (Val::Auto, Val::Auto));

        app.world_mut().entity_mut(button).insert(Interaction::None);
        app.update();
        app.world_mut()
            .entity_mut(button)
            .insert(Interaction::Pressed);
        app.update();
        assert_eq!(
            app.world().get::<Node>(body).unwrap().display,
            Display::Grid
        );
        assert_eq!(
            app.world().get::<Node>(hidden).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Node>(title).unwrap().display,
            Display::Flex
        );
        assert!(
            app.world()
                .get::<Frame>(frame)
                .unwrap()
                .restored_display
                .is_empty()
        );
        assert!(!app.world().get::<Frame>(frame).unwrap().minimized);
        let node = app.world().get::<Node>(frame).unwrap();
        assert_eq!((node.height, node.min_height), (px(202), px(120)));
        assert!(
            app.world()
                .get::<Frame>(frame)
                .unwrap()
                .restored_height
                .is_none()
        );
    }
}
