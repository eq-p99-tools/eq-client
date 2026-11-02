//! A normal movable, minimizable map when an installed skin has no usable one.
use super::{MapButton, MapCanvas};
use crate::{
    theme,
    windows::{self, Shown, WindowId},
};
use bevy::prelude::*;

/// Keeps the skin loader from replacing the locally supplied map window.
#[derive(Component)]
pub(crate) struct FallbackMap(bool);

/// Skins from older clients can omit the map entirely. Preserve a usable
/// skin canvas, otherwise supply the same map and controls in a native frame.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn ensure_window(
    mut commands: Commands,
    shown: Res<Shown>,
    presentation: Res<super::Presentation>,
    frames: Query<(Entity, &WindowId, &Node, Option<&FallbackMap>)>,
    canvases: Query<&Node, With<MapCanvas>>,
) {
    let frame = frames.iter().find(|(_, id, _, _)| **id == WindowId::Map);
    if !shown.displayed(WindowId::Map) {
        if let Some((entity, _, _, Some(_))) = frame {
            commands.entity(entity).despawn();
        }
        return;
    }
    if let Some((entity, _, _, Some(fallback))) = frame {
        if fallback.0 == presentation.plain() {
            return;
        }
        if !presentation.plain() {
            // Let the normal skin loader recreate its frame on the next update.
            commands.entity(entity).despawn();
            return;
        }
    }
    if !presentation.plain()
        && (frame.is_some_and(|(_, _, node, _)| usable(node.width) && usable(node.height))
            && canvases
                .iter()
                .any(|node| usable(node.width) && usable(node.height)))
    {
        return;
    }
    if let Some((entity, _, _, _)) = frame {
        commands.entity(entity).despawn();
    }
    let root = windows::frame(
        &mut commands,
        WindowId::Map,
        Node {
            width: px(600),
            height: px(500),
            max_width: percent(95),
            max_height: percent(90),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    commands
        .entity(root)
        .insert((
            FallbackMap(presentation.plain()),
            super::Backdrop::Fill(Color::NONE),
        ))
        .with_children(|window| {
            window
                .spawn(Node {
                    padding: UiRect::all(px(6)),
                    column_gap: px(4),
                    row_gap: px(4),
                    flex_wrap: FlexWrap::Wrap,
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|toolbar| {
                    for (label, action) in [
                        ("+", MapButton::ZoomIn),
                        ("-", MapButton::ZoomOut),
                        ("<", MapButton::Pan(-1, 0)),
                        (">", MapButton::Pan(1, 0)),
                        ("^", MapButton::Pan(0, -1)),
                        ("v", MapButton::Pan(0, 1)),
                        ("Labels", MapButton::Labels),
                        ("1", MapButton::Layer(1)),
                        ("2", MapButton::Layer(2)),
                        ("3", MapButton::Layer(3)),
                    ] {
                        toolbar.spawn((action, button(label)));
                    }
                    toolbar.spawn((windows::SelectorButton(WindowId::Map), button("Close")));
                });
            window.spawn((
                MapCanvas,
                windows::KeepsPress,
                ImageNode::default(),
                BackgroundColor(Color::srgb(0.86, 0.83, 0.73)),
                super::Backdrop::Fill(if presentation.plain() {
                    Color::srgb(0.80, 0.82, 0.84)
                } else {
                    Color::srgb(0.86, 0.83, 0.73)
                }),
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    width: percent(100),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
        });
}

fn usable(size: Val) -> bool {
    !matches!(size, Val::Px(value) | Val::Percent(value) if value <= 0.0 || !value.is_finite())
}

fn button(label: &str) -> impl Bundle {
    (
        Button,
        theme::text(label, theme::Size::Small, theme::INK_BRIGHT),
        Node {
            padding: UiRect::axes(px(8), px(5)),
            ..default()
        },
        BackgroundColor(theme::BUTTON),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_hidden_map_skins_get_one_fallback_that_closes_normally() {
        for hidden in [false, true] {
            let mut app = App::new();
            app.init_resource::<Shown>()
                .init_resource::<super::super::Presentation>()
                .add_systems(Update, ensure_window);
            app.world_mut().resource_mut::<Shown>().open(WindowId::Map);
            if hidden {
                let root = app
                    .world_mut()
                    .spawn((
                        WindowId::Map,
                        Node {
                            width: px(0),
                            height: px(0),
                            ..default()
                        },
                    ))
                    .id();
                app.world_mut().spawn((
                    MapCanvas,
                    Node {
                        width: px(300),
                        height: px(300),
                        ..default()
                    },
                    ChildOf(root),
                ));
            }
            app.update();
            let world = app.world_mut();
            let root = world
                .query_filtered::<Entity, With<FallbackMap>>()
                .single(world)
                .unwrap();
            assert_eq!(
                world
                    .query_filtered::<Entity, With<MapCanvas>>()
                    .iter(world)
                    .count(),
                1
            );
            assert_eq!(world.query::<&windows::Minimize>().iter(world).count(), 1);
            app.update();
            assert!(app.world().get::<FallbackMap>(root).is_some());
            app.world_mut().resource_mut::<Shown>().close(WindowId::Map);
            app.update();
            assert!(app.world().get_entity(root).is_err());
        }
    }

    #[test]
    fn usable_skin_canvas_is_preserved() {
        let mut app = App::new();
        app.init_resource::<Shown>()
            .init_resource::<super::super::Presentation>()
            .add_systems(Update, ensure_window);
        app.world_mut().resource_mut::<Shown>().open(WindowId::Map);
        let root = app
            .world_mut()
            .spawn((
                WindowId::Map,
                Node {
                    width: px(600),
                    height: px(500),
                    ..default()
                },
            ))
            .id();
        let canvas = app
            .world_mut()
            .spawn((
                MapCanvas,
                Node {
                    width: px(400),
                    height: px(300),
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        app.update();
        assert!(app.world().get_entity(canvas).is_ok());
        assert!(app.world().get::<FallbackMap>(root).is_none());
    }
}
