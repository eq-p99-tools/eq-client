//! Actual received-popup chrome with Bevy focus and fabricated layout bounds.
//! These are offline routing tests, not GPU/layout or gameplay evidence.
use super::*;
use bevy::{
    app::{HierarchyPropagatePlugin, PropagateSet},
    camera::RenderTarget,
    input::touch::Touches,
    ui::{UiStack, ui_focus_system, update::propagate_ui_target_cameras},
};
use eq_client_core::{
    WorldEvent,
    popups::{Popup, PopupOffer},
};

fn receive(app: &mut App, serial: u64) {
    crate::online::testing::news(
        &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
        [WorldEvent::PopupOpened(PopupOffer {
            session_id: 5,
            serial,
            expires_at: None,
            popup: Popup {
                title: format!("Entity Info {serial}"),
                text: "<c \"#FFFF66\">Stats</c><br><table><tr><td>Name</td><td>A test NPC</td></tr></table>".into(),
                buttons: 1,
                duration: None,
                popup_id: 10,
                negative_id: 0,
            },
        })],
    );
    app.update();
}

fn panel(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &WindowId)>()
        .iter(world)
        .find_map(|(entity, id)| (*id == WindowId::ServerDialog).then_some(entity))
        .unwrap()
}

fn setup() -> App {
    let mut app = crate::testing::app();
    app.init_resource::<Layouts>()
        .init_resource::<Touches>()
        .init_resource::<UiStack>()
        .init_resource::<UiScale>()
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
            PostUpdate,
        ))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
            PostUpdate,
        ))
        .add_systems(PreUpdate, ui_focus_system.before(crate::clicks::track))
        .add_systems(
            Update,
            (crate::dialogs::present, input, layout::remember).chain(),
        )
        .add_systems(
            PostUpdate,
            (
                block_clicks,
                propagate_ui_target_cameras
                    .before(PropagateSet::<ComputedUiTargetCamera>::default())
                    .before(PropagateSet::<ComputedUiRenderTargetInfo>::default()),
            ),
        );
    app.world_mut().spawn((
        Camera2d,
        Camera::default(),
        RenderTarget::default(),
        IsDefaultUiCamera,
    ));
    let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
    crate::online::testing::admit(&mut online, 5, crate::online::testing::player(7));
    receive(&mut app, 9);
    let frame = panel(&mut app);
    let mut node = app.world_mut().get_mut::<Node>(frame).unwrap();
    node.left = px(100);
    node.top = px(80);
    node.margin = UiRect::ZERO;
    node.right = Val::Auto;
    node.bottom = Val::Auto;
    app.update();
    measure(&mut app);
    app.update();
    app
}

fn walk(world: &World, entity: Entity, stack: &mut Vec<Entity>) {
    if world.get::<Node>(entity).is_some() {
        stack.push(entity);
    }
    if let Some(children) = world.get::<Children>(entity) {
        for child in children {
            walk(world, *child, stack);
        }
    }
}

/// Supply only the measured rectangles normally produced by layout. The
/// actual factory hierarchy, border order and focus policies remain intact.
fn measure(app: &mut App) {
    let frame = panel(app);
    let world = app.world_mut();
    let node = world.get::<Node>(frame).unwrap();
    let (Val::Px(x), Val::Px(y), Val::Px(width), Val::Px(height)) =
        (node.left, node.top, node.width, node.height)
    else {
        panic!("pixel rectangle")
    };
    let origin = Vec2::new(x, y);
    let size = Vec2::new(width, height);
    let mut stack = Vec::new();
    walk(world, frame, &mut stack);
    let mut response_index = 0;
    for &entity in &stack {
        let node = world.get::<Node>(entity).unwrap();
        let (local, extent) = if entity == frame {
            (size * 0.5, size)
        } else if world.get::<TitleBar>(entity).is_some() {
            (Vec2::new(width * 0.5, 24.0), Vec2::new(width - 24.0, 22.0))
        } else if world.get::<crate::dialogs::ScrollBody>(entity).is_some() {
            (
                Vec2::new(width * 0.5, height * 0.5),
                Vec2::new(width - 24.0, height - 120.0),
            )
        } else if world.get::<crate::dialogs::DialogButton>(entity).is_some() {
            let offset = 180.0 - f32::from(response_index) * 70.0;
            response_index += 1_u8;
            (
                Vec2::new(width - offset, height - 30.0),
                Vec2::new(60.0, 24.0),
            )
        } else if world.get::<DragHandle>(entity).is_some() {
            if node.width == px(6) {
                (
                    Vec2::new(
                        if node.left == px(0) { 3.0 } else { width - 3.0 },
                        height * 0.5,
                    ),
                    Vec2::new(6.0, height),
                )
            } else {
                (
                    Vec2::new(
                        width * 0.5,
                        if node.top == px(0) { 3.0 } else { height - 3.0 },
                    ),
                    Vec2::new(width - 12.0, 6.0),
                )
            }
        } else {
            (Vec2::ZERO, Vec2::ZERO)
        };
        world.entity_mut(entity).insert((
            ComputedNode {
                size: extent,
                inverse_scale_factor: 1.0,
                ..default()
            },
            UiGlobalTransform::from_translation(origin + local),
            InheritedVisibility::VISIBLE,
        ));
    }
    let length = stack.len();
    *world.resource_mut::<UiStack>() = UiStack {
        partition: std::iter::once(0..length).collect(),
        uinodes: stack,
    };
}

fn pointer(app: &mut App, at: Vec2, down: Option<bool>) {
    let world = app.world_mut();
    world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .single_mut(world)
        .unwrap()
        .set_physical_cursor_position(Some(at.as_dvec2()));
    if let Some(down) = down {
        let mut mouse = world.resource_mut::<ButtonInput<MouseButton>>();
        if down {
            mouse.press(MouseButton::Left);
        } else {
            mouse.release(MouseButton::Left);
        }
    }
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}

#[test]
fn server_dialog_chrome_moves_resizes_and_preserves_offer_content() {
    let mut app = setup();
    let frame = panel(&mut app);
    let world = app.world_mut();
    assert_eq!(world.query::<&Minimize>().iter(world).count(), 0);
    assert_eq!(
        world.query::<&crate::skinned::Closes>().iter(world).count(),
        0
    );
    assert_eq!(world.query::<&DragHandle>().iter(world).count(), 6);
    assert!(
        world
            .query::<&Text>()
            .iter(world)
            .any(|text| text.0 == "Entity Info 9")
    );
    pointer(&mut app, Vec2::new(300.0, 104.0), Some(true));
    pointer(&mut app, Vec2::new(330.0, 124.0), None);
    assert_eq!(app.world().get::<Node>(frame).unwrap().left, px(130));
    assert_eq!(app.world().get::<Node>(frame).unwrap().top, px(100));
    pointer(&mut app, Vec2::new(330.0, 124.0), Some(false));
    measure(&mut app);
    pointer(&mut app, Vec2::new(588.0, 418.0), Some(true));
    pointer(&mut app, Vec2::new(688.0, 478.0), None);
    let node = app.world().get::<Node>(frame).unwrap();
    assert_eq!((node.width, node.height), (px(560), px(380)));
    pointer(&mut app, Vec2::new(688.0, 478.0), Some(false));
    measure(&mut app);
    pointer(&mut app, Vec2::new(132.0, 102.0), Some(true));
    pointer(&mut app, Vec2::new(670.0, 450.0), None);
    let node = app.world().get::<Node>(frame).unwrap();
    assert_eq!((node.width, node.height), (px(240), px(180)));
    assert_eq!((node.left, node.top), (px(450), px(300)));
    pointer(&mut app, Vec2::new(670.0, 450.0), Some(false));
    assert_eq!(
        app.world().get::<Frame>(frame).unwrap().preferred_size(),
        Some(Vec2::new(240.0, 180.0))
    );
    receive(&mut app, 10);
    let replacement = panel(&mut app);
    assert_ne!(replacement, frame);
    let node = app.world().get::<Node>(replacement).unwrap();
    assert_eq!(
        (node.left, node.top, node.width, node.height),
        (px(450), px(300), px(240), px(180))
    );
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&crate::dialogs::DialogButton>()
            .iter(world)
            .count(),
        3
    );
    assert!(
        world
            .query::<&TextSpan>()
            .iter(world)
            .any(|text| text.0 == "A test NPC")
    );
    app.update();
    assert_eq!(panel(&mut app), replacement);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.insert_resource(crate::outbox::Outbox::new(Some(sender)))
        .add_systems(Update, crate::dialogs::buttons.after(input));
    measure(&mut app);
    pointer(&mut app, Vec2::new(650.0, 450.0), Some(true));
    pointer(&mut app, Vec2::new(650.0, 450.0), Some(false));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        eq_client_core::ClientCommand::RespondToPopup {
            session_id: 5,
            serial: 10,
            action: eq_client_core::popups::PopupAction::Dismiss,
            ..
        }
    ));
    crate::online::testing::news(
        &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
        [WorldEvent::PopupClosed {
            session_id: 5,
            serial: 10,
        }],
    );
    app.update();
    receive(&mut app, 11);
    let reopened = panel(&mut app);
    let node = app.world().get::<Node>(reopened).unwrap();
    assert_eq!(
        (node.left, node.top, node.width, node.height),
        (px(450), px(300), px(240), px(180))
    );
    let mut fresh = setup();
    let initial = panel(&mut fresh);
    let node = fresh.world().get::<Node>(initial).unwrap();
    assert_eq!((node.width, node.height), (px(460), px(320)));
}

#[test]
fn server_dialog_interior_and_response_controls_keep_input_and_skin_changes_end_drags() {
    let mut app = setup();
    let frame = panel(&mut app);
    for at in [Vec2::new(300.0, 230.0), Vec2::new(520.0, 370.0)] {
        pointer(&mut app, at, Some(true));
        assert!(app.world().resource::<DragState>().active.is_none());
        pointer(&mut app, at + Vec2::splat(20.0), None);
        let node = app.world().get::<Node>(frame).unwrap();
        assert_eq!(
            (node.left, node.top, node.width, node.height),
            (px(100), px(80), px(460), px(320))
        );
        pointer(&mut app, at, Some(false));
    }
    pointer(&mut app, Vec2::new(558.0, 240.0), Some(true));
    assert_eq!(
        app.world()
            .resource::<DragState>()
            .active
            .as_ref()
            .unwrap()
            .edges,
        Some(resize::Edges { x: 1, y: 0 })
    );
    app.world_mut().resource_mut::<crate::skin::UiSkin>().0 = "velious".into();
    pointer(&mut app, Vec2::new(658.0, 240.0), None);
    assert!(app.world().resource::<DragState>().active.is_none());
    assert_eq!(app.world().get::<Node>(frame).unwrap().width, px(460));
    pointer(&mut app, Vec2::new(658.0, 240.0), Some(false));
    crate::online::testing::news(
        &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
        [WorldEvent::PopupClosed {
            session_id: 5,
            serial: 9,
        }],
    );
    app.update();
    assert!(app.world().get_entity(frame).is_err());
    receive(&mut app, 11);
    let reopened = panel(&mut app);
    assert_eq!(app.world().get::<Node>(reopened).unwrap().width, px(460));
    assert!(!WindowId::ServerDialog.describe().persists);
    assert!(!WindowId::ServerDialog.can_minimize());
}
