//! Compact slots and bag-content grids.
use super::{SlotButton, Tab, View, label, visible_slots};
use crate::sheets::Art;
use bevy::prelude::*;
use eq_client_assets::ui::{Area, EquipmentLayout};
use eq_client_core::inventory::InventorySlot;

const CELL: f32 = 34.0;

/// Names shown in empty equipment slots, short enough for one; hovering gives
/// the full name, and the paperdoll's arrangement shows left from right.
const EQUIPMENT_CAPTIONS: [&str; 22] = [
    "Charm", "Ear", "Head", "Face", "Ear", "Neck", "Shoulder", "Arms", "Back", "Wrist", "Wrist",
    "Range", "Hands", "Primary", "Second", "Finger", "Finger", "Chest", "Legs", "Feet", "Waist",
    "Ammo",
];

/// The installed skin's equipment layout, and the character image for the area
/// the skin draws its character in.
#[derive(Clone, Copy)]
pub(super) struct Paperdoll<'a> {
    pub layout: &'a EquipmentLayout,
    pub figure: Option<&'a crate::paperdoll::PaperdollImage>,
}

/// Keeps equipment and the cursor beside the carried bag grids. Equipment sits
/// around the paperdoll when the installed skin's layout could be read.
pub(super) fn contents(
    parent: &mut ChildSpawnerCommands,
    view: View<'_>,
    paperdoll: Option<Paperdoll<'_>>,
    art: &mut Art<'_>,
) {
    quantity_picker(parent, view);
    storage_columns(parent, view, paperdoll, art);
}

/// Keeps the quantity picker available for carried and bank stacks alike.
fn quantity_picker(parent: &mut ChildSpawnerCommands, view: View<'_>) {
    if let Some(selection) = &view.state.actions.split {
        use super::interaction::SplitAction;
        parent
            .spawn(Node {
                padding: UiRect::all(px(8)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                ..default()
            })
            .with_children(|picker| {
                label(
                    picker,
                    &format!("PICK UP {} / {}", selection.amount, selection.available),
                    12.0,
                );
                picker
                    .spawn(Node {
                        column_gap: px(5),
                        ..default()
                    })
                    .with_children(|row| {
                        for (action, title) in [
                            (SplitAction::Minimum, "Min"),
                            (SplitAction::Less, "-"),
                            (SplitAction::More, "+"),
                            (SplitAction::Maximum, "Max"),
                            (SplitAction::Confirm, "Pick up"),
                            (SplitAction::Cancel, "Cancel"),
                        ] {
                            row.spawn((
                                Button,
                                action,
                                Node {
                                    padding: UiRect::axes(px(10), px(6)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.08, 0.10, 0.13)),
                            ))
                            .with_children(|button| label(button, title, 11.0));
                        }
                    });
            });
    }
}

/// Shows equipment, cursor, carried bags and any requested bank storage together.
fn storage_columns(
    parent: &mut ChildSpawnerCommands,
    view: View<'_>,
    paperdoll: Option<Paperdoll<'_>>,
    art: &mut Art<'_>,
) {
    parent
        .spawn(Node {
            column_gap: px(16),
            align_items: AlignItems::Start,
            ..default()
        })
        .with_children(|columns| {
            columns
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|equipment| {
                    label(equipment, "EQUIPMENT", 10.0);
                    if let Some(paperdoll) = paperdoll {
                        placed(equipment, paperdoll, view, art);
                    } else {
                        let slots: Vec<_> = (0..=21).map(InventorySlot).collect();
                        grid(equipment, &slots, view, art);
                    }
                });
            columns
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|bags| {
                    // The cursor heads the storage column, beside the paperdoll.
                    label(bags, "CURSOR", 10.0);
                    bags.spawn(Node {
                        column_gap: px(8),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|cursor| {
                        square(cursor, InventorySlot(30), view, art);
                        cursor
                            .spawn((
                                Button,
                                super::StoreCursor,
                                Node {
                                    padding: UiRect::axes(px(10), px(6)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.08, 0.10, 0.13)),
                            ))
                            .with_children(|button| label(button, "Auto inventory", 11.0));
                    });
                    if view.state.tab == Tab::Bank {
                        carried(bags, view, Tab::Bank, art);
                    }
                    carried(bags, view, Tab::Inventory, art);
                });
        });
}

/// Builds bag rows and loose carried slots without duplicating equipment.
fn carried(parent: &mut ChildSpawnerCommands, view: View<'_>, tab: Tab, art: &mut Art<'_>) {
    let slots: Vec<_> = visible_slots(view.inventory, tab)
        .into_iter()
        .filter(|slot| tab == Tab::Bank || matches!(slot.0,22..=29|251..=340))
        .collect();
    let mut remaining = slots.clone();
    for slot in &slots {
        let Some(item) = view
            .inventory
            .items()
            .get(slot)
            .filter(|item| item.bag_slots > 0)
        else {
            continue;
        };
        if slot.parent().is_some() {
            continue;
        }
        let children: Vec<_> = (0..item.bag_slots)
            .filter_map(|index| slot.child(index))
            .collect();
        remaining.retain(|other| *other != *slot && !children.contains(other));
        parent
            .spawn((
                Node {
                    padding: UiRect::all(px(6)),
                    border_radius: BorderRadius::all(px(4)),
                    column_gap: px(8),
                    align_items: AlignItems::Start,
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(view.state.colors.tint(*slot)),
            ))
            .with_children(|row| {
                row.spawn(Node {
                    width: px(112),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(5),
                    ..default()
                })
                .with_children(|bag| {
                    bag.spawn(Node {
                        column_gap: px(8),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|heading| {
                        square(heading, *slot, view, art);
                        super::colors::controls(heading, &view.state.colors, *slot);
                    });
                    label(bag, &item.details.name, 10.0);
                });
                grid(row, &children, view, art);
            });
    }
    if !remaining.is_empty() {
        label(
            parent,
            match tab {
                Tab::Inventory => "CARRIED",
                Tab::Bank => "BANK",
            },
            10.0,
        );
        grid(parent, &remaining, view, art);
    } else if slots.is_empty() {
        label(parent, "No bank items received", 12.0);
    }
}

/// Equipment slots where the skin's inventory window puts them, scaled so each
/// slot is one of this window's cells, with the character where the skin draws it.
fn placed(
    parent: &mut ChildSpawnerCommands,
    Paperdoll { layout, figure }: Paperdoll<'_>,
    view: View<'_>,
    art: &mut Art<'_>,
) {
    let size = layout
        .slots
        .iter()
        .map(|p| p.width.max(p.height))
        .fold(1.0, f32::max);
    let scale = CELL / size;
    let (width, height) = layout
        .slots
        .iter()
        .map(|p| (p.x + size, p.y + size))
        .chain(layout.character.map(|a| (a.x + a.width, a.y + a.height)))
        .fold((0.0_f32, 0.0_f32), |(width, height), (right, bottom)| {
            (width.max(right), height.max(bottom))
        });
    parent
        .spawn(Node {
            width: px(width * scale),
            height: px(height * scale),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|doll| {
            if let (Some(figure), Some(area)) = (figure, layout.character) {
                let fitted = fit(area, figure.size);
                doll.spawn((
                    ImageNode::new(figure.handle.clone()),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(fitted.x * scale),
                        top: px(fitted.y * scale),
                        width: px(fitted.width * scale),
                        height: px(fitted.height * scale),
                        ..default()
                    },
                ));
            }
            for placement in &layout.slots {
                doll.spawn(Node {
                    position_type: PositionType::Absolute,
                    left: px(placement.x * scale),
                    top: px(placement.y * scale),
                    ..default()
                })
                .with_children(|cell| {
                    square(cell, InventorySlot(i32::from(placement.slot)), view, art);
                });
            }
        });
}

/// The largest centered rectangle inside `area` with the proportions of `size`.
fn fit(area: Area, size: Vec2) -> Area {
    let scale = (area.width / size.x).min(area.height / size.y);
    let (width, height) = (size.x * scale, size.y * scale);
    Area {
        x: area.x + (area.width - width) / 2.0,
        y: area.y + (area.height - height) / 2.0,
        width,
        height,
    }
}

fn grid(
    parent: &mut ChildSpawnerCommands,
    slots: &[InventorySlot],
    view: View<'_>,
    art: &mut Art<'_>,
) {
    parent
        .spawn(Node {
            display: Display::Grid,
            grid_template_columns: RepeatedGridTrack::px(5, CELL),
            column_gap: px(4),
            row_gap: px(4),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|grid| {
            for slot in slots {
                square(grid, *slot, view, art);
            }
        });
}

fn square(
    parent: &mut ChildSpawnerCommands,
    slot: InventorySlot,
    view: View<'_>,
    art: &mut Art<'_>,
) {
    let item = view.inventory.items().get(&slot);
    parent
        .spawn((
            Button,
            SlotButton(slot),
            Node {
                width: px(CELL),
                height: px(CELL),
                flex_shrink: 0.0,
                border: UiRect::all(px(1)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.045, 0.055, 0.065)),
            BorderColor::all(if item.is_some() {
                Color::srgb(0.48, 0.43, 0.31)
            } else {
                Color::srgb(0.22, 0.25, 0.28)
            }),
        ))
        .with_children(|cell| {
            if let Some(item) = item {
                if let Some(icon) = art.item(item.icon) {
                    cell.spawn((
                        icon,
                        Node {
                            width: px(CELL - 2.0),
                            height: px(CELL - 2.0),
                            ..default()
                        },
                    ));
                } else {
                    let initials: String = item
                        .details
                        .name
                        .split_whitespace()
                        .filter_map(|w| w.chars().next())
                        .take(2)
                        .collect();
                    label(cell, &initials, 12.0);
                }
                if let Some(count) = item.stack_count {
                    let mut quantity = cell.spawn((
                        Text::new(count.to_string()),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                        Node {
                            position_type: PositionType::Absolute,
                            right: px(1),
                            bottom: px(0),
                            padding: UiRect::axes(px(4), px(2)),
                            ..default()
                        },
                    ));
                    if count > 1 && slot != InventorySlot(30) {
                        quantity.insert((Button, super::SplitStack(slot)));
                    }
                }
            } else if !view.inventory.received() || view.inventory.stale() {
                label(cell, "?", 12.0);
            } else if let Some(caption) = usize::try_from(slot.0)
                .ok()
                .and_then(|index| EQUIPMENT_CAPTIONS.get(index))
            {
                cell.spawn((
                    Text::new(*caption),
                    TextFont {
                        font_size: FontSize::Px(7.5),
                        ..default()
                    },
                    TextColor(Color::srgb(0.45, 0.49, 0.53)),
                    TextLayout::justify(Justify::Center),
                ));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{inventory::InventoryState, online::OnlineState};
    use eq_client_core::inventory::InventoryUpdate;

    /// Draws the window's contents once, from these resources.
    fn draw(app: &mut App, paperdoll: Option<EquipmentLayout>) {
        app.init_resource::<Assets<Image>>()
            .init_resource::<crate::sheets::Sheets>()
            .init_resource::<crate::skin::UiSkin>()
            .init_resource::<crate::ViewerSettings>()
            .add_systems(
                Startup,
                move |mut commands: Commands,
                      state: Res<InventoryState>,
                      online: Res<OnlineState>,
                      mut art: Art| {
                    let view = View {
                        state: &state,
                        inventory: online.world.inventory(),
                    };
                    commands.spawn(Node::default()).with_children(|parent| {
                        contents(
                            parent,
                            view,
                            paperdoll.as_ref().map(|layout| Paperdoll {
                                layout,
                                figure: None,
                            }),
                            &mut art,
                        );
                    });
                },
            );
        app.update();
    }

    #[test]
    fn quantity_click_opens_picker_without_also_picking_up_the_stack() {
        let mut online = OnlineState::new(false);
        crate::online::testing::inventory(
            &mut online,
            InventoryUpdate::Snapshot(
                super::super::demo_items()
                    .into_iter()
                    .filter(|item| item.slot != InventorySlot(30))
                    .collect(),
            ),
        );
        let before = online.world.inventory().clone();
        let inventory = InventoryState {
            open: true,
            demo: true,
            ..InventoryState::default()
        };
        let mut app = App::new();
        app.insert_resource(inventory)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<crate::chat::ChatState>()
            .init_resource::<crate::escape::Escape>()
            .init_resource::<crate::items::ItemState>()
            .insert_resource(online)
            .insert_resource(crate::target::CommandsToServer(None))
            .add_systems(Update, (super::super::input, super::super::settle).chain());
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        app.world_mut()
            .spawn((SlotButton(InventorySlot(251)), Interaction::Pressed));
        let count_button = app
            .world_mut()
            .spawn((
                super::super::SplitStack(InventorySlot(251)),
                Interaction::Pressed,
            ))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let items = app.world().resource::<OnlineState>().world.inventory();
        assert_eq!(items, &before);
        let inventory = app.world().resource::<InventoryState>();
        let selection = inventory.actions.split.as_ref().unwrap();
        assert_eq!((selection.slot, selection.amount), (InventorySlot(251), 1));
        // With an item on the cursor, the count is a placement target like its icon.
        let mut contents: Vec<_> = before.items().values().cloned().collect();
        let stack = contents
            .iter_mut()
            .find(|item| item.slot == InventorySlot(251))
            .unwrap();
        stack.stack_count = Some(19);
        let mut cursor = stack.clone();
        cursor.slot = InventorySlot(30);
        cursor.stack_count = Some(1);
        contents.push(cursor);
        crate::online::testing::inventory(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            InventoryUpdate::Snapshot(contents),
        );
        *app.world_mut()
            .get_mut::<Interaction>(count_button)
            .unwrap() = Interaction::Pressed;
        app.update();
        let items = app.world().resource::<OnlineState>().world.inventory();
        assert_eq!(items.items()[&InventorySlot(251)].stack_count, Some(20));
        assert!(!items.items().contains_key(&InventorySlot(30)));
        assert!(
            app.world()
                .resource::<InventoryState>()
                .actions
                .split
                .is_none()
        );
    }

    #[test]
    fn bank_layout_keeps_cursor_carried_bags_and_quantity_controls() {
        let mut state = InventoryState::default();
        let mut items = super::super::demo_items();
        items.retain(|item| item.slot != InventorySlot(30));
        let bank_stack = items
            .iter_mut()
            .find(|item| item.slot == InventorySlot(2000))
            .unwrap();
        bank_stack.stack_count = Some(12);
        let mut online = OnlineState::new(false);
        crate::online::testing::inventory(&mut online, InventoryUpdate::Snapshot(items));
        state.bank_open = true;
        state.tab = Tab::Bank;
        state.select_split(InventorySlot(2000), online.world.inventory());
        let mut app = App::new();
        app.insert_resource(state).insert_resource(online);
        draw(&mut app, None);
        let world = app.world_mut();
        let slots: Vec<_> = world
            .query::<&SlotButton>()
            .iter(world)
            .map(|button| button.0)
            .collect();
        for slot in [0, 13, 22, 29, 30, 251, 254, 2000, 2007] {
            assert_eq!(
                slots
                    .iter()
                    .filter(|candidate| **candidate == InventorySlot(slot))
                    .count(),
                1
            );
        }
        assert!(
            world
                .query::<&super::super::interaction::SplitAction>()
                .iter(world)
                .any(|action| matches!(action, super::super::interaction::SplitAction::Confirm))
        );
        assert!(
            world
                .query::<&Text>()
                .iter(world)
                .any(|text| text.0.starts_with("PICK UP "))
        );
        assert_eq!(
            world
                .query::<&super::super::StoreCursor>()
                .iter(world)
                .count(),
            1
        );
        let counts: Vec<_> = world
            .query::<&super::super::SplitStack>()
            .iter(world)
            .map(|button| button.0)
            .collect();
        assert!(counts.contains(&InventorySlot(2000)));
        assert!(counts.contains(&InventorySlot(251)));
        assert!(!counts.contains(&InventorySlot(30)));
    }

    #[test]
    fn a_skin_layout_places_every_equipment_slot_and_labels_the_empty_ones() {
        // A synthetic skin: two columns of 42-pixel slots, 11 rows each.
        let layout = EquipmentLayout {
            slots: (0..22_u16)
                .map(|slot| eq_client_assets::ui::SlotPlacement {
                    slot,
                    x: f32::from(slot % 2) * 130.0,
                    y: f32::from(slot / 2) * 43.0,
                    width: 42.0,
                    height: 42.0,
                })
                .collect(),
            character: None,
        };
        let mut online = OnlineState::new(false);
        crate::online::testing::inventory(
            &mut online,
            InventoryUpdate::Snapshot(
                super::super::demo_items()
                    .into_iter()
                    .filter(|item| item.slot == InventorySlot(13))
                    .collect(),
            ),
        );
        let mut app = App::new();
        app.init_resource::<InventoryState>()
            .insert_resource(online);
        draw(&mut app, Some(layout));
        let world = app.world_mut();
        let placed: Vec<(i32, Val, Val)> = world
            .query::<(&SlotButton, &ChildOf)>()
            .iter(world)
            .filter(|(button, _)| button.0.0 <= 21)
            .map(|(button, parent)| {
                let node = world.get::<Node>(parent.parent()).unwrap();
                (button.0.0, node.left, node.top)
            })
            .collect();
        assert_eq!(placed.len(), 22);
        // Slot 21 is in the second column of the last row, scaled to 34-pixel cells.
        let scale = CELL / 42.0;
        let (_, left, top) = placed.iter().find(|(slot, ..)| *slot == 21).unwrap();
        assert_eq!((*left, *top), (px(130.0 * scale), px(430.0 * scale)));
        let captions: Vec<String> = world
            .query::<&Text>()
            .iter(world)
            .map(|text| text.0.clone())
            .collect();
        // Every empty equipment slot is named; the occupied primary slot is not.
        assert_eq!(captions.iter().filter(|text| *text == "Ear").count(), 2);
        assert!(captions.iter().any(|text| text == "Shoulder"));
        assert!(!captions.iter().any(|text| text == "Primary"));
    }

    #[test]
    fn the_figure_keeps_its_proportions_inside_the_skins_character_area() {
        let area = Area {
            x: 46.0,
            y: 56.0,
            width: 80.0,
            height: 128.0,
        };
        // A narrow figure fills the height and centers across.
        let tall = fit(area, Vec2::new(32.0, 128.0));
        assert_eq!(
            (tall.x, tall.y, tall.width, tall.height),
            (70.0, 56.0, 32.0, 128.0)
        );
        // A wide one fills the width and centers down.
        let wide = fit(area, Vec2::new(128.0, 32.0));
        assert_eq!(
            (wide.x, wide.y, wide.width, wide.height),
            (46.0, 110.0, 80.0, 20.0)
        );
    }
}
