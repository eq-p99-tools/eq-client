//! Dragging starts on a real tab press and uses the same hit testing as the UI.
use super::{ChatState, TabButton, TabStrip, groups::Insert};
use crate::{
    clicks::LeftButton,
    theme::{self, Size},
    windows::{Layouts, WindowId, pointer::PointerUi},
};
use bevy::{prelude::*, window::PrimaryWindow};
use eq_client_core::chat::ChatTab;

const THRESHOLD: f32 = 6.0;

#[derive(Resource, Default)]
pub(crate) struct TabDrag {
    press: Option<Press>,
    preview: Option<Preview>,
    pub(super) escaped: bool,
}

impl TabDrag {
    pub(super) fn dragging(&self) -> bool {
        self.press.as_ref().is_some_and(|press| press.dragged)
    }
}

struct Press {
    entity: Entity,
    tab: ChatTab,
    source: WindowId,
    start: Vec2,
    inverse_scale: f32,
    dragged: bool,
}

struct Preview {
    cursor: Vec2,
    screen: Vec2,
    target: Drop,
}

#[derive(Clone, Copy)]
enum Drop {
    Dock(WindowId, Insert, Rect),
    Detach,
    Cancel,
}

pub(crate) type TabHit<'w, 's> = Query<
    'w,
    's,
    (
        &'static TabButton,
        &'static ComputedNode,
        &'static UiGlobalTransform,
        &'static Interaction,
    ),
>;

pub(crate) type StripHit<'w, 's> = Query<
    'w,
    's,
    (
        &'static TabStrip,
        &'static ComputedNode,
        &'static UiGlobalTransform,
    ),
>;

impl Drop {
    /// Use the frontmost surface even while Bevy holds the pressed tab's
    /// Interaction, so covered or clipped strips never accept a drop.
    fn at(
        cursor: Vec2,
        hit: Option<Entity>,
        tabs: &TabHit,
        strips: &StripHit,
        can_detach: bool,
    ) -> Self {
        let Some(hit) = hit else {
            return if can_detach {
                Self::Detach
            } else {
                Self::Cancel
            };
        };
        if let Ok((TabButton(tab, group), node, transform, _)) = tabs.get(hit)
            && let Some(inverse) = transform.try_inverse()
        {
            let after = inverse.transform_point2(cursor).x > 0.0;
            let size = node.size() * node.inverse_scale_factor();
            let origin = transform.translation * node.inverse_scale_factor() - size * 0.5;
            let edge = origin.x + if after { size.x } else { 0.0 };
            return Self::Dock(
                *group,
                if after {
                    Insert::After(*tab)
                } else {
                    Insert::Before(*tab)
                },
                Rect::new(edge - 1.0, origin.y, edge + 1.0, origin.y + size.y),
            );
        }
        if let Ok((strip, node, transform)) = strips.get(hit) {
            let size = node.size() * node.inverse_scale_factor();
            let start = transform.translation * node.inverse_scale_factor() - size * 0.5;
            let end = start + size;
            return Self::Dock(
                strip.id,
                Insert::End,
                Rect::new(start.x, end.y - 2.0, end.x, end.y),
            );
        }
        Self::Cancel
    }
}

/// Claims a native tab press only once it crosses the drag threshold.
/// Scripted clicks still use the ordinary release-click tracker.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn drag_tabs(
    mut drag: ResMut<TabDrag>,
    mut chat: ResMut<ChatState>,
    mut layouts: ResMut<Layouts>,
    mut button: ResMut<LeftButton>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    pointer: PointerUi,
    tabs: TabHit,
    strips: StripHit,
    script: Option<Res<crate::script::Script>>,
    profile: Option<Res<crate::profile_files::Profile>>,
    skin: Option<Res<crate::skin::UiSkin>>,
) {
    drag.escaped = drag.press.is_some() && keys.just_pressed(KeyCode::Escape);
    drag.preview = None;
    let Some((window, cursor)) = windows
        .single()
        .ok()
        .filter(|window| window.focused)
        .and_then(|window| Some((window, window.physical_cursor_position()?)))
        .filter(|_| !script.as_ref().is_some_and(|script| script.pointer_owned()))
    else {
        if let Some(press) = drag.press.take() {
            button.claim(press.entity);
        }
        return;
    };
    let hit = pointer.top_at(cursor);
    if mouse.just_pressed(MouseButton::Left)
        && let Some(entity) = hit
        && let Ok((TabButton(tab, source), node, _, Interaction::Pressed)) = tabs.get(entity)
    {
        drag.press = Some(Press {
            entity,
            tab: *tab,
            source: *source,
            start: cursor,
            inverse_scale: node.inverse_scale_factor(),
            dragged: false,
        });
    }
    let Some(mut press) = drag.press.take() else {
        return;
    };
    drag.escaped |= keys.just_pressed(KeyCode::Escape);
    if drag.escaped
        || profile.as_ref().is_some_and(DetectChanges::is_changed)
        || skin.as_ref().is_some_and(DetectChanges::is_changed)
        || !tabs.get(press.entity).is_ok_and(|(_, node, _, _)| {
            !node.is_empty()
                && node.inverse_scale_factor().to_bits() == press.inverse_scale.to_bits()
        })
    {
        button.claim(press.entity);
        return;
    }
    press.dragged |= cursor.distance(press.start) * press.inverse_scale >= THRESHOLD;
    if press.dragged {
        button.claim(press.entity);
        let cursor_logical = cursor * press.inverse_scale;
        let target = Drop::at(
            cursor,
            hit,
            &tabs,
            &strips,
            chat.groups.can_detach(press.source),
        );
        if mouse.just_released(MouseButton::Left) {
            match target {
                Drop::Dock(group, at, _) => {
                    chat.groups.move_tab(press.source, press.tab, group, at);
                }
                Drop::Detach => {
                    if let Some(id) = chat.groups.detach(press.source, press.tab) {
                        layouts.place_chat_group(id, cursor_logical - Vec2::new(16.0, 10.0));
                    }
                }
                Drop::Cancel => (),
            }
        } else if mouse.pressed(MouseButton::Left) {
            drag.preview = Some(Preview {
                cursor: cursor_logical,
                screen: window.physical_size().as_vec2() * press.inverse_scale,
                target,
            });
        }
    }
    if mouse.pressed(MouseButton::Left) {
        drag.press = Some(press);
    }
}

#[derive(Component)]
pub(crate) struct DragHint;
#[derive(Component)]
pub(crate) struct InsertMarker;

/// Paint a pointer-following action label and the exact insertion edge.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(crate) fn paint(
    mut commands: Commands,
    drag: Res<TabDrag>,
    mut hints: Query<(Entity, &mut Node, &mut Text), (With<DragHint>, Without<InsertMarker>)>,
    mut markers: Query<(Entity, &mut Node), (With<InsertMarker>, Without<DragHint>)>,
) {
    let Some(preview) = &drag.preview else {
        for (entity, ..) in &hints {
            commands.entity(entity).despawn();
        }
        for (entity, _) in &markers {
            commands.entity(entity).despawn();
        }
        return;
    };
    let label = match preview.target {
        Drop::Dock(..) => "Dock tab",
        Drop::Detach => "New chat window",
        Drop::Cancel => "Cancel tab drag",
    };
    let position = (preview.cursor + Vec2::new(12.0, 14.0))
        .min((preview.screen - Vec2::new(142.0, 26.0)).max(Vec2::ZERO));
    if let Ok((_, mut node, mut text)) = hints.single_mut() {
        node.left = px(position.x);
        node.top = px(position.y);
        if text.0 != label {
            label.clone_into(&mut text.0);
        }
    } else {
        commands.spawn((
            DragHint,
            Node {
                position_type: PositionType::Absolute,
                left: px(position.x),
                top: px(position.y),
                padding: UiRect::axes(px(6), px(3)),
                ..default()
            },
            theme::text(label, Size::Small, theme::INK_BRIGHT),
            BackgroundColor(theme::SCRIM),
            GlobalZIndex(1000),
        ));
    }
    if let Drop::Dock(_, _, rect) = preview.target {
        let node = Node {
            position_type: PositionType::Absolute,
            left: px(rect.min.x),
            top: px(rect.min.y),
            width: px(rect.width()),
            height: px(rect.height()),
            ..default()
        };
        if let Ok((_, mut current)) = markers.single_mut() {
            current.set_if_neq(node);
        } else {
            commands.spawn((
                InsertMarker,
                node,
                BackgroundColor(theme::FOCUS),
                GlobalZIndex(999),
            ));
        }
    } else if let Ok((entity, _)) = markers.single() {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clicks::testing::{press, release};
    use bevy::ui::ComputedStackIndex;

    fn fixture() -> App {
        let mut app = crate::testing::app();
        app.init_resource::<TabDrag>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_systems(Update, (drag_tabs, super::super::input, paint).chain());
        app.update(); // The window's first profile and skin are established.
        app
    }

    fn point(app: &mut App, x: f64, y: f64) {
        let world = app.world_mut();
        world
            .query::<&mut Window>()
            .single_mut(world)
            .unwrap()
            .set_physical_cursor_position(Some(bevy::math::DVec2::new(x, y)));
    }

    fn tab(app: &mut App, id: WindowId, tab: ChatTab, x: f32, stack: u32) -> Entity {
        app.world_mut()
            .spawn((
                TabButton(tab, id),
                Button,
                Node::default(),
                ComputedNode {
                    size: Vec2::new(40.0, 20.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(x, 100.0)),
                ComputedStackIndex(stack),
                InheritedVisibility::VISIBLE,
            ))
            .id()
    }

    fn groups(app: &App) -> &crate::chat::Groups {
        &app.world().resource::<ChatState>().groups
    }

    #[test]
    fn native_click_reorder_detach_and_dock_share_the_release_rule() {
        let mut app = fixture();
        let main = WindowId::Chat;
        let say = tab(&mut app, main, ChatTab::Say, 120.0, 2);
        let guild = tab(&mut app, main, ChatTab::Guild, 180.0, 3);

        point(&mut app, 180.0, 100.0);
        press(&mut app, Some(guild));
        point(&mut app, 183.0, 100.0);
        release(&mut app, Some(guild));
        assert_eq!(groups(&app).active(main), Some(ChatTab::Guild));
        assert_eq!(groups(&app).get(main).unwrap().tabs, ChatTab::ALL);

        // The marker appears at the right edge of Guild. Reordering uses
        // that side after removing Say from its old position.
        point(&mut app, 120.0, 100.0);
        press(&mut app, Some(say));
        point(&mut app, 192.0, 100.0);
        app.update();
        let world = app.world_mut();
        let marker = world
            .query_filtered::<&Node, With<InsertMarker>>()
            .single(world)
            .unwrap();
        assert_eq!(marker.left, px(199));
        release(&mut app, Some(guild));
        let ordered = &groups(&app).get(main).unwrap().tabs;
        let index = ordered
            .iter()
            .position(|tab| *tab == ChatTab::Guild)
            .unwrap();
        assert_eq!(ordered[index + 1], ChatTab::Say);

        point(&mut app, 120.0, 100.0);
        press(&mut app, Some(say));
        point(&mut app, 310.0, 190.0);
        release(&mut app, None);
        let float = groups(&app).owner();
        assert_eq!(float, WindowId::ChatGroup(1));
        assert_eq!(groups(&app).active(float), Some(ChatTab::Say));

        app.world_mut().despawn(say);
        let say = tab(&mut app, float, ChatTab::Say, 310.0, 4);
        point(&mut app, 310.0, 100.0);
        press(&mut app, Some(say));
        point(&mut app, 167.0, 100.0);
        release(&mut app, Some(guild));
        assert!(!groups(&app).contains(float));
        let ordered = &groups(&app).get(main).unwrap().tabs;
        assert!(
            ordered
                .windows(2)
                .any(|pair| pair == [ChatTab::Say, ChatTab::Guild])
        );
    }

    #[test]
    fn escape_focus_loss_and_covered_drops_cancel_without_clicking() {
        let mut app = fixture();
        let main = WindowId::Chat;
        let guild = tab(&mut app, main, ChatTab::Guild, 120.0, 2);
        let say = tab(&mut app, main, ChatTab::Say, 180.0, 3);
        let cover = app
            .world_mut()
            .spawn((
                Button,
                Node::default(),
                ComputedNode {
                    size: Vec2::splat(48.0),
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(180.0, 100.0)),
                ComputedStackIndex(4),
                InheritedVisibility::VISIBLE,
            ))
            .id();

        point(&mut app, 120.0, 100.0);
        press(&mut app, Some(guild));
        point(&mut app, 190.0, 100.0);
        release(&mut app, Some(cover));
        assert_eq!(groups(&app), &crate::chat::Groups::default());
        app.world_mut().despawn(cover);

        point(&mut app, 120.0, 100.0);
        press(&mut app, Some(guild));
        point(&mut app, 180.0, 100.0);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(
            app.world()
                .resource::<crate::keys::Typing>()
                .escape_consumed
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        point(&mut app, 120.0, 100.0);
        release(&mut app, Some(guild));
        assert_eq!(groups(&app), &crate::chat::Groups::default());

        press(&mut app, Some(guild));
        point(&mut app, 180.0, 100.0);
        app.update();
        assert!(app.world().resource::<TabDrag>().dragging());
        let world = app.world_mut();
        world
            .query::<&mut Window>()
            .single_mut(world)
            .unwrap()
            .focused = false;
        release(&mut app, Some(say));
        assert_eq!(groups(&app), &crate::chat::Groups::default());
        assert!(!app.world().resource::<TabDrag>().dragging());
    }
}
