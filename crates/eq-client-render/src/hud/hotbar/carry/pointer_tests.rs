//! Headless pointer regressions through the real button factories and Bevy
//! focus. Bounds are fabricated; these are not layout, GPU or live checks.
use super::*;
use crate::{
    hud::hotbar::Slot,
    online::{OnlineState, testing},
};
use bevy::{
    app::{HierarchyPropagatePlugin, PropagateSet},
    camera::RenderTarget,
    input::touch::Touches,
    math::{Affine2, DVec2},
    ui::{FocusPolicy, UiStack, ui_focus_system, update::propagate_ui_target_cameras},
    window::PrimaryWindow,
};

#[derive(Resource, Default)]
struct Requests(Vec<Action>);

#[allow(clippy::needless_pass_by_value)]
fn record(
    keys: crate::keys::Keys,
    bindings: Res<Bindings>,
    clicks: crate::clicks::Clicks<Slot>,
    mut requests: ResMut<Requests>,
) {
    if let Some(action) = super::super::requested(&keys, &bindings, &clicks) {
        requests.0.push(action);
    }
}

/// Install the actual focus/route/cursor systems, without a renderer or OS input.
pub(crate) fn app() -> App {
    let mut app = crate::testing::app();
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 1, testing::player(1));
    testing::news(
        &mut online,
        [eq_client_core::WorldEvent::WorldName {
            short_name: "Sample".into(),
        }],
    );
    testing::spell(
        &mut online,
        eq_client_core::SpellUpdate::Slot {
            slot: 0,
            spell_id: 73,
            mode: 1,
        },
    );
    app.insert_resource(online)
        .init_resource::<Touches>()
        .init_resource::<UiStack>()
        .init_resource::<UiScale>()
        .init_resource::<Requests>()
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
            PostUpdate,
        ))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
            PostUpdate,
        ))
        .add_systems(PreUpdate, ui_focus_system.before(crate::clicks::track))
        .add_systems(
            Update,
            (
                crate::profile_files::follow,
                super::super::page::input,
                crate::escape::route,
                route,
                record,
                crate::windows::toggle,
                crate::windows::input,
                crate::inventory::cursor::update,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            (
                crate::windows::block_clicks,
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
    app.world_mut().spawn((
        crate::inventory::cursor::Overlay,
        Node {
            display: Display::None,
            ..default()
        },
        BackgroundColor::default(),
        FocusPolicy::Pass,
    ));
    app
}

pub(crate) fn ordinary() -> Fixture {
    let mut app = app();
    app.add_systems(Startup, |mut commands: Commands| {
        let root = commands.spawn(Node::default()).id();
        super::super::spawn(&mut commands, root);
    });
    finish(app)
}

pub(crate) struct Fixture {
    pub(crate) app: App,
    slots: Vec<Entity>,
}

/// Retain the factory's hierarchy, controls and focus policies. Supply only
/// computed bounds that normally come from layout, and a hierarchy-ordered stack.
pub(crate) fn finish(mut app: App) -> Fixture {
    app.update();
    let world = app.world_mut();
    let mut slots: Vec<_> = world
        .query::<(Entity, &Slot)>()
        .iter(world)
        .map(|(entity, slot)| (slot.0, entity))
        .collect();
    slots.sort_by_key(|(index, _)| *index);
    assert_eq!(slots.len(), 10);
    let nodes: Vec<_> = world
        .query_filtered::<Entity, With<Node>>()
        .iter(world)
        .collect();
    for entity in nodes {
        let (center, size) = if let Some(slot) = world.get::<Slot>(entity) {
            (point(slot.0), Vec2::splat(40.0))
        } else if let Some(button) = world.get::<super::super::page::Button>(entity) {
            (
                Vec2::new(if button.0 { 80.0 } else { 30.0 }, 120.0),
                Vec2::splat(24.0),
            )
        } else if world.get::<crate::windows::DragHandle>(entity).is_some() {
            (Vec2::new(270.0, 15.0), Vec2::new(540.0, 30.0))
        } else if world.get::<crate::windows::Frame>(entity).is_some()
            || world
                .get::<Node>(entity)
                .is_some_and(|node| !node.overflow.is_visible())
        {
            (Vec2::new(270.0, 80.0), Vec2::new(540.0, 160.0))
        } else {
            (Vec2::ZERO, Vec2::ZERO)
        };
        world.entity_mut(entity).insert((
            ComputedNode {
                size,
                inverse_scale_factor: 1.0,
                ..default()
            },
            UiGlobalTransform::from(Affine2::from_translation(center)),
            InheritedVisibility::VISIBLE,
        ));
    }
    let roots: Vec<_> = world
        .query_filtered::<Entity, (With<Node>, Without<ChildOf>)>()
        .iter(world)
        .collect();
    let mut stack = Vec::new();
    for root in roots {
        walk(world, root, &mut stack);
    }
    let length = stack.len();
    *world.resource_mut::<UiStack>() = UiStack {
        partition: std::iter::once(0..length).collect(),
        uinodes: stack,
    };
    app.update();
    Fixture {
        app,
        slots: slots.into_iter().map(|(_, entity)| entity).collect(),
    }
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

fn point(slot: usize) -> Vec2 {
    Vec2::new(30.0 + f32::from(u16::try_from(slot).unwrap()) * 50.0, 60.0)
}

impl Fixture {
    pub(crate) fn pointer(&mut self, at: Vec2, down: Option<bool>) {
        let world = self.app.world_mut();
        world
            .query_filtered::<&mut Window, With<PrimaryWindow>>()
            .single_mut(world)
            .unwrap()
            .set_physical_cursor_position(Some(DVec2::new(f64::from(at.x), f64::from(at.y))));
        if let Some(down) = down {
            let mut mouse = world.resource_mut::<ButtonInput<MouseButton>>();
            if down {
                mouse.press(MouseButton::Left);
            } else {
                mouse.release(MouseButton::Left);
            }
        }
        self.app.update();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    }

    pub(crate) fn press(&mut self, slot: usize) {
        self.pointer(point(slot), Some(true));
    }
    pub(crate) fn release(&mut self, slot: usize) {
        self.pointer(point(slot), Some(false));
    }
    pub(crate) fn hold(&mut self) {
        self.app.world_mut().resource_mut::<Presses>().hold_past();
        self.app.update();
    }
    pub(crate) fn carry(&self) -> Option<Action> {
        self.app.world().resource::<Carry>().hotkey()
    }
    pub(crate) fn binding(&self, slot: usize) -> Option<Action> {
        self.app.world().resource::<Bindings>().0[slot]
    }
    pub(crate) fn requests(&self) -> &[Action] {
        &self.app.world().resource::<Requests>().0
    }
}

/// Shared behavioral contract for both real ordinary and skin factories.
pub(crate) fn pickup_move_remove(mut fixture: Fixture) {
    fixture.press(0);
    assert_eq!(
        fixture.app.world().get::<Interaction>(fixture.slots[0]),
        Some(&Interaction::Pressed)
    );
    assert!(fixture.app.world().resource::<Presses>().pending());
    assert_eq!(fixture.requests(), []);
    fixture.release(0);
    fixture.app.update();
    assert_eq!(fixture.requests(), [Action::Gem(0)]);

    // An empty gem's stored shortcut is still editable and visibly carried.
    fixture.press(4);
    fixture.hold();
    assert_eq!(fixture.binding(4), None);
    assert_eq!(fixture.carry(), Some(Action::Gem(4)));
    let world = fixture.app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Node, With<crate::inventory::cursor::Overlay>>()
            .single(world)
            .unwrap()
            .display,
        Display::Flex
    );
    assert!(
        world
            .query::<&Text>()
            .iter(world)
            .any(|text| text.0 == "Spell gem 5")
    );
    fixture.release(4);
    assert_eq!(fixture.carry(), Some(Action::Gem(4)));
    fixture.press(5);
    assert_eq!(fixture.binding(5), Some(Action::Gem(4)));
    assert_eq!(fixture.carry(), Some(Action::Gem(5)));
    fixture.release(5);
    fixture.pointer(Vec2::new(700.0, 300.0), Some(true));
    fixture.pointer(Vec2::new(700.0, 300.0), Some(false));
    assert_eq!(fixture.carry(), None);

    fixture.press(0);
    fixture.hold();
    fixture.release(8);
    assert_eq!(fixture.binding(0), None);
    assert_eq!(fixture.binding(8), Some(Action::Gem(0)));
    assert_eq!(fixture.carry(), Some(Action::Sit));
    fixture.press(0);
    fixture.release(0);
    assert_eq!(fixture.binding(0), Some(Action::Sit));
    assert_eq!(fixture.carry(), None);
    fixture.press(8);
    fixture.hold();
    fixture.pointer(Vec2::new(700.0, 300.0), Some(false));
    assert_eq!(fixture.binding(8), None);
    assert_eq!(fixture.carry(), None);
    assert_eq!(fixture.requests(), [Action::Gem(0)]);
}

#[test]
fn ordinary_buttons_pick_up_show_swap_and_remove_shortcuts() {
    pickup_move_remove(ordinary());
}

#[test]
fn native_hold_boundary_is_strict_and_a_slow_release_never_activates() {
    let since = Instant::now();
    assert!(!expired(
        since,
        since + HOLD.checked_sub(Duration::from_nanos(1)).unwrap()
    ));
    assert!(!expired(since, since + HOLD));
    assert!(expired(since, since + HOLD + Duration::from_nanos(1)));
    let mut fixture = ordinary();
    fixture.press(0);
    // No update between moving the clock and releasing the button.
    fixture
        .app
        .world_mut()
        .resource_mut::<Presses>()
        .hold_past();
    fixture.release(0);
    assert_eq!(fixture.binding(0), None);
    assert_eq!(fixture.carry(), Some(Action::Gem(0)));
    assert_eq!(fixture.requests(), []);
}

#[test]
fn bevy_pressed_state_outside_the_button_cannot_pick_up_or_activate() {
    let mut fixture = ordinary();
    fixture.press(0);
    fixture.pointer(Vec2::new(700.0, 300.0), None);
    // Bevy deliberately retains Pressed while outside the button.
    assert_eq!(
        fixture.app.world().get::<Interaction>(fixture.slots[0]),
        Some(&Interaction::Pressed)
    );
    assert!(
        !fixture
            .app
            .world()
            .get::<RelativeCursorPosition>(fixture.slots[0])
            .unwrap()
            .cursor_over()
    );
    fixture.hold();
    fixture.release(0);
    assert_eq!(fixture.binding(0), Some(Action::Gem(0)));
    assert_eq!(fixture.carry(), None);
    assert_eq!(fixture.requests(), []);
}

#[test]
fn focus_input_reset_rebuild_and_layout_replacement_cancel_the_pending_press() {
    for cause in 0..4 {
        let mut fixture = ordinary();
        fixture.press(0);
        match cause {
            0 => {
                let world = fixture.app.world_mut();
                world
                    .query_filtered::<&mut Window, With<PrimaryWindow>>()
                    .single_mut(world)
                    .unwrap()
                    .focused = false;
            }
            1 => fixture
                .app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .reset_all(),
            2 => {
                fixture.app.world_mut().despawn(fixture.slots[0]);
            }
            3 => fixture.app.world_mut().resource_mut::<Bindings>().0[0] = Some(Action::Stand),
            _ => unreachable!(),
        }
        fixture.hold();
        assert!(
            !fixture.app.world().resource::<Presses>().pending(),
            "cause {cause}"
        );
        assert_eq!(fixture.carry(), None);
        fixture.release(0);
        assert_eq!(fixture.requests(), []);
        assert_eq!(
            fixture.binding(0),
            Some(if cause == 3 {
                Action::Stand
            } else {
                Action::Gem(0)
            })
        );
    }
}

#[test]
fn escape_cancels_pending_or_carried_shortcuts_before_closing_a_window() {
    for picked in [false, true] {
        let mut fixture = ordinary();
        let world = fixture.app.world_mut();
        world
            .resource_mut::<crate::windows::Shown>()
            .open(crate::windows::WindowId::Inventory);
        world.spawn((
            crate::windows::WindowId::Inventory,
            crate::windows::Frame::default(),
            Node::default(),
        ));
        fixture.press(0);
        if picked {
            fixture.hold();
            fixture.release(0);
        }
        fixture
            .app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        fixture.app.update();
        assert_eq!(
            *fixture.app.world().resource::<crate::escape::Escape>(),
            crate::escape::Escape::Hotbar
        );
        assert!(
            fixture
                .app
                .world()
                .resource::<crate::windows::Shown>()
                .is_open(crate::windows::WindowId::Inventory)
        );
        assert_eq!(fixture.carry(), None);
        assert!(!fixture.app.world().resource::<Presses>().pending());
        assert_eq!(
            fixture.binding(0),
            if picked { None } else { Some(Action::Gem(0)) }
        );
        assert_eq!(fixture.requests(), []);
    }
}

#[test]
fn item_shortcut_drag_never_moves_the_actual_item_and_inventory_cursor_wins() {
    let mut fixture = ordinary();
    let mut item = crate::preview::items().remove(0);
    item.slot = InventorySlot(23);
    let id = item.details.id;
    testing::inventory(
        &mut fixture.app.world_mut().resource_mut::<OnlineState>(),
        eq_client_core::inventory::InventoryUpdate::Snapshot(vec![item.clone()]),
    );
    fixture.app.world_mut().resource_mut::<Bindings>().0[0] = Some(Action::Item {
        slot: item.slot,
        id,
    });
    let revision = fixture
        .app
        .world()
        .resource::<OnlineState>()
        .world()
        .inventory()
        .revision();
    fixture.press(0);
    fixture.hold();
    fixture.pointer(Vec2::new(700.0, 300.0), Some(false));
    let inventory = fixture
        .app
        .world()
        .resource::<OnlineState>()
        .world()
        .inventory();
    assert_eq!(inventory.revision(), revision);
    assert_eq!(inventory.items()[&item.slot].details.id, id);
    assert_eq!(inventory.items().len(), 1);
    assert_eq!(fixture.binding(0), None);
    assert_eq!(fixture.requests(), []);

    fixture.press(8);
    fixture.hold();
    item.slot = InventorySlot::CURSOR;
    testing::inventory(
        &mut fixture.app.world_mut().resource_mut::<OnlineState>(),
        eq_client_core::inventory::InventoryUpdate::Snapshot(vec![item]),
    );
    fixture.app.update();
    assert_eq!(fixture.carry(), None);
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<OnlineState>()
            .world()
            .inventory()
            .items()[&InventorySlot::CURSOR]
            .details
            .id,
        id
    );
}

#[test]
fn an_inner_button_does_not_drag_its_window_and_keyboard_activation_still_works() {
    let mut fixture = ordinary();
    let world = fixture.app.world_mut();
    let frame = world
        .query_filtered::<Entity, With<crate::windows::Frame>>()
        .single(world)
        .unwrap();
    let before = world.get::<Node>(frame).unwrap().clone();
    fixture.press(0);
    fixture.pointer(point(0) + Vec2::new(5.0, 0.0), None);
    let node = fixture.app.world().get::<Node>(frame).unwrap();
    assert_eq!((node.left, node.top), (before.left, before.top));
    fixture.release(0);
    fixture
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Digit9);
    fixture.app.update();
    assert_eq!(fixture.requests(), [Action::Gem(0), Action::Sit]);
    fixture
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    // The title's actual drag handle still starts a window move.
    fixture.pointer(Vec2::new(270.0, 15.0), Some(true));
    fixture.pointer(Vec2::new(290.0, 25.0), None);
    let node = fixture.app.world().get::<Node>(frame).unwrap();
    assert_eq!((node.left, node.top), (px(20.0), px(10.0)));
}

fn enter(fixture: &mut Fixture, admission: u64, name: &str) {
    let mut online = OnlineState::new(true);
    let mut player = testing::player(1);
    player.name = name.into();
    testing::admit(&mut online, admission, player);
    testing::news(
        &mut online,
        [eq_client_core::WorldEvent::WorldName {
            short_name: "Sample".into(),
        }],
    );
    fixture.app.insert_resource(online);
}

#[test]
fn replacing_the_admission_cancels_even_when_profile_and_layout_are_identical() {
    let mut fixture = ordinary();
    let name = fixture
        .app
        .world()
        .resource::<OnlineState>()
        .world()
        .player()
        .unwrap()
        .name
        .clone();
    fixture.press(4);
    enter(&mut fixture, 2, &name);
    fixture.hold();
    fixture.release(4);
    assert_eq!(fixture.binding(4), Some(Action::Gem(4)));
    assert_eq!(fixture.carry(), None);
    assert_eq!(fixture.requests(), []);
}

#[test]
fn pickup_persists_only_the_source_profile_and_cannot_edit_the_next_profile() {
    for picked in [false, true] {
        let directory = std::env::temp_dir().join(format!(
            "eq-hotbar-drag-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut fixture = ordinary();
        fixture
            .app
            .world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .settings_directory = Some(directory.clone());
        fixture.app.add_systems(
            Update,
            super::super::persist
                .after(crate::profile_files::follow)
                .before(crate::escape::route),
        );
        enter(&mut fixture, 3, "First");
        fixture.app.update();
        fixture.press(4);
        if picked {
            fixture.hold();
            fixture.release(4);
        }
        enter(&mut fixture, 4, "Second");
        fixture.hold();
        fixture.release(4);
        assert_eq!(fixture.binding(4), Some(Action::Gem(4)));
        assert_eq!(fixture.carry(), None);
        assert_eq!(fixture.requests(), []);
        let first = directory.join(super::super::file_name("Sample", "First"));
        if picked {
            let saved = eq_client_core::hotbar::Hotbar::read(
                &std::fs::read_to_string(first).unwrap(),
                eq_client_core::hotbar::Hotbar::default(),
            );
            assert_eq!(saved.0[4], None);
        } else {
            assert!(!first.exists());
        }
        assert!(
            !directory
                .join(super::super::file_name("Sample", "Second"))
                .exists()
        );
        if directory.exists() {
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
}
