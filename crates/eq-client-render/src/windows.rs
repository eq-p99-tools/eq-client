//! What every window shares: the registry that describes each one, its frame,
//! title bar, dragging and minimizing, the stack that orders the floating
//! windows, and the placements kept between runs.

use crate::theme::{self, Size};
use bevy::{prelude::*, ui::FocusPolicy, window::PrimaryWindow};
mod layout;
pub(super) mod pointer;
mod registry;
mod stack;
mod store;
pub(super) use layout::Layouts;
pub(crate) use registry::{Layer, WindowId};
#[cfg(test)]
pub(crate) use stack::toggle;
pub(crate) use stack::{Shown, Stack, spawn_selector};

/// Restores saved positions before layout and constrains measured frames afterward;
/// placements persist between runs per character. Orders the floating windows
/// and opens and closes the toggled ones.
pub(super) fn register_layout(app: &mut App) {
    app.add_systems(
        Update,
        (
            stack::raise,
            stack::toggle,
            stack::light_selector,
            stack::restack,
        )
            .chain()
            .after(super::escape::route)
            .in_set(super::Stage::Route),
    );
    app.add_systems(Update, store::persist.in_set(super::Stage::Present));
    app.add_systems(PostUpdate, block_clicks);
    app.add_systems(
        PostUpdate,
        layout::remember.before(bevy::ui::UiSystems::Layout),
    );
    app.add_systems(
        PostUpdate,
        layout::constrain.after(bevy::ui::UiSystems::Layout),
    );
}

/// The window a drag surface belongs to, which keys its saved placement.
#[derive(Component)]
pub(super) struct LayoutKey(WindowId);

/// Gives a passive panel its window's identity, which survives HUD
/// reconstruction and keys its saved placement.
pub(super) fn identify(commands: &mut Commands, frame: Entity, id: WindowId) {
    commands.entity(frame).insert((LayoutKey(id), id));
}

/// A node where the registry says this window opens.
pub(crate) fn placed(id: WindowId, mut node: Node) -> Node {
    id.describe().placement.apply(&mut node);
    node
}

/// Spawns a window's frame where the registry says it opens, in its layer,
/// with its id and background and, for a titled window, its title bar; the
/// caller adds its own markers and body after the title bar.
pub(crate) fn frame(commands: &mut Commands, id: WindowId, mut node: Node) -> Entity {
    let description = id.describe();
    description.placement.apply(&mut node);
    node.border = UiRect::all(px(1));
    let frame = commands
        .spawn((
            node,
            theme::surface(),
            GlobalZIndex(description.layer.base()),
            Frame::default(),
            id,
        ))
        .id();
    if !description.title.is_empty() {
        commands
            .entity(frame)
            .with_children(|parent| title_bar(parent, frame, id));
    }
    frame
}

/// Visible windows and standalone controls consume pointer input before the world.
pub(super) type PointerSurface = Or<(With<Frame>, With<Button>)>;

/// Wheel travel since the last read, in logical pixels; positive scrolls up.
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
pub(super) fn titled(commands: &mut Commands, entity: Entity, id: WindowId) {
    commands.entity(entity).remove::<(Button, DragHandle)>();
    commands.entity(entity).insert(id);
    commands
        .entity(entity)
        .with_children(|parent| title_bar(parent, entity, id));
}

/// Adds a compact drag bar, with the window's title, and a minimize button.
pub(super) fn title_bar(parent: &mut ChildSpawnerCommands, frame: Entity, id: WindowId) {
    let title = id.describe().title;
    parent
        .spawn((
            Button,
            DragHandle(frame),
            LayoutKey(id),
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
            BackgroundColor(theme::TITLE_BAR),
        ))
        .with_children(|bar| {
            bar.spawn(theme::text(title, Size::Small, theme::INK));
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
                BackgroundColor(theme::BUTTON),
            ))
            .with_children(|button| {
                button.spawn((
                    MinimizeLabel(frame),
                    theme::text("_", Size::Body, theme::INK_BRIGHT),
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

type NewSurface = Or<(Added<Frame>, Added<Button>)>;

/// Window frames and controls keep the clicks that land on them, so nothing drawn
/// beneath (such as another window's buttons) is pressed through them. A node
/// passes clicks on by default, and a button added to an existing node keeps that.
fn block_clicks(mut policies: Query<&mut FocusPolicy, NewSurface>) {
    for mut policy in &mut policies {
        if *policy != FocusPolicy::Block {
            *policy = FocusPolicy::Block;
        }
    }
}

/// Scrolls a node's content by `delta` logical pixels (positive scrolls up),
/// within its content: Bevy clamps only the drawn offset, so an unclamped position
/// would leave dead travel to scroll back through.
pub(super) fn scroll_by(position: &mut ScrollPosition, node: &ComputedNode, delta: f32) {
    let maximum = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
    let y = (position.y - delta).clamp(0.0, maximum);
    if y.to_bits() != position.y.to_bits() {
        position.y = y;
    }
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

    #[test]
    fn scrolling_stops_at_the_end_of_the_content() {
        let node = ComputedNode {
            size: Vec2::new(100.0, 200.0),
            content_size: Vec2::new(100.0, 500.0),
            inverse_scale_factor: 1.0,
            ..default()
        };
        let mut position = ScrollPosition::default();
        scroll_by(&mut position, &node, -1000.0);
        assert!((position.y - 300.0).abs() < 0.001);
        scroll_by(&mut position, &node, 30.0);
        assert!((position.y - 270.0).abs() < 0.001);
        scroll_by(&mut position, &node, 1000.0);
        assert!(position.y.abs() < 0.001);
    }

    #[test]
    fn frames_and_late_buttons_block_clicks_to_what_lies_beneath() {
        let mut app = App::new();
        app.add_systems(Update, block_clicks);
        let frame = app
            .world_mut()
            .spawn((Node::default(), Frame::default()))
            .id();
        let button = app.world_mut().spawn(Node::default()).id();
        app.world_mut().entity_mut(button).insert(Button);
        let text = app.world_mut().spawn(Node::default()).id();
        app.update();
        let policy = |entity| *app.world().get::<FocusPolicy>(entity).unwrap();
        assert_eq!(policy(frame), FocusPolicy::Block);
        assert_eq!(policy(button), FocusPolicy::Block);
        assert_eq!(policy(text), FocusPolicy::Pass);
    }
}
