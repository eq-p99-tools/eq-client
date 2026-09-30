//! Compact slots and bag-content grids.
use super::{InventoryState, SlotButton, Tab, icons::Icons, label, visible_slots};
use bevy::prelude::*;
use eq_client_assets::ui::SlotPlacement;
use eq_client_core::inventory::InventorySlot;

const CELL: f32 = 34.0;

/// Names shown in empty equipment slots, short enough for one; hovering gives
/// the full name, and the paperdoll's arrangement shows left from right.
const EQUIPMENT_CAPTIONS: [&str; 22] = [
    "Charm", "Ear", "Head", "Face", "Ear", "Neck", "Shoulder", "Arms", "Back", "Wrist", "Wrist",
    "Range", "Hands", "Primary", "Second", "Finger", "Finger", "Chest", "Legs", "Feet", "Waist",
    "Ammo",
];

/// The installed skin's equipment layout, and the character image for its middle.
#[derive(Clone, Copy)]
pub(super) struct Paperdoll<'a> {
    pub layout: &'a [SlotPlacement],
    pub figure: Option<&'a Handle<Image>>,
}

/// Keeps equipment and the cursor beside the carried bag grids. Equipment sits
/// around the paperdoll when the installed skin's layout could be read.
pub(super) fn contents(
    parent: &mut ChildSpawnerCommands,
    state: &InventoryState,
    paperdoll: Option<Paperdoll<'_>>,
    icons: &mut Icons,
    directory: Option<&std::path::Path>,
    images: &mut Assets<Image>,
) {
    quantity_picker(parent, state);
    storage_columns(parent, state, paperdoll, icons, directory, images);
}

/// Keeps the quantity picker available for carried and bank stacks alike.
fn quantity_picker(parent: &mut ChildSpawnerCommands, state: &InventoryState) {
    if let Some(selection) = &state.actions.split {
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
    state: &InventoryState,
    paperdoll: Option<Paperdoll<'_>>,
    icons: &mut Icons,
    directory: Option<&std::path::Path>,
    images: &mut Assets<Image>,
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
                        placed(equipment, paperdoll, state, icons, directory, images);
                    } else {
                        let slots: Vec<_> = (0..=21).map(InventorySlot).collect();
                        grid(equipment, &slots, state, icons, directory, images);
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
                        square(cursor, InventorySlot(30), state, icons, directory, images);
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
                    if state.tab == Tab::Bank {
                        carried(bags, state, Tab::Bank, icons, directory, images);
                    }
                    carried(bags, state, Tab::Inventory, icons, directory, images);
                });
        });
}

/// Builds bag rows and loose carried slots without duplicating equipment.
fn carried(
    parent: &mut ChildSpawnerCommands,
    state: &InventoryState,
    tab: Tab,
    icons: &mut Icons,
    directory: Option<&std::path::Path>,
    images: &mut Assets<Image>,
) {
    let slots: Vec<_> = visible_slots(state, tab)
        .into_iter()
        .filter(|slot| tab == Tab::Bank || matches!(slot.0,22..=29|251..=340))
        .collect();
    let mut remaining = slots.clone();
    for slot in &slots {
        let Some(item) = state
            .data
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
                BackgroundColor(state.colors.tint(*slot)),
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
                        square(heading, *slot, state, icons, directory, images);
                        super::colors::controls(heading, &state.colors, *slot);
                    });
                    label(bag, &item.details.name, 10.0);
                });
                grid(row, &children, state, icons, directory, images);
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
        grid(parent, &remaining, state, icons, directory, images);
    } else if slots.is_empty() {
        label(parent, "No bank items received", 12.0);
    }
}

/// Equipment slots where the skin's inventory window puts them around the
/// paperdoll, scaled so each slot is one of this window's cells, with the
/// character drawn in the middle.
fn placed(
    parent: &mut ChildSpawnerCommands,
    Paperdoll { layout, figure }: Paperdoll<'_>,
    state: &InventoryState,
    icons: &mut Icons,
    directory: Option<&std::path::Path>,
    images: &mut Assets<Image>,
) {
    let size = layout
        .iter()
        .map(|p| p.width.max(p.height))
        .fold(1.0, f32::max);
    let scale = CELL / size;
    let (width, height) = layout
        .iter()
        .fold((0.0_f32, 0.0_f32), |(width, height), p| {
            (width.max(p.x + size), height.max(p.y + size))
        });
    parent
        .spawn(Node {
            width: px(width * scale),
            height: px(height * scale),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|doll| {
            if let (Some(figure), Some(middle)) = (figure, middle(layout, size)) {
                doll.spawn((
                    ImageNode::new(figure.clone()),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(middle.min.x * scale),
                        top: px(middle.min.y * scale),
                        width: px(middle.width() * scale),
                        height: px(middle.height() * scale),
                        ..default()
                    },
                ));
            }
            for placement in layout {
                doll.spawn(Node {
                    position_type: PositionType::Absolute,
                    left: px(placement.x * scale),
                    top: px(placement.y * scale),
                    ..default()
                })
                .with_children(|cell| {
                    square(
                        cell,
                        InventorySlot(i32::from(placement.slot)),
                        state,
                        icons,
                        directory,
                        images,
                    );
                });
            }
        });
}

/// The empty middle of a paperdoll layout, where the character goes: between
/// the outermost columns, from under the top row down to the first slot that
/// sits between those columns again.
fn middle(layout: &[SlotPlacement], size: f32) -> Option<Rect> {
    let xs = || layout.iter().map(|placement| placement.x);
    let left = xs().fold(f32::INFINITY, f32::min) + size;
    let right = xs().fold(f32::NEG_INFINITY, f32::max);
    let top = layout.iter().map(|p| p.y).fold(f32::INFINITY, f32::min) + size;
    let bottom = layout
        .iter()
        .filter(|p| p.x >= left && p.x < right && p.y >= top)
        .map(|p| p.y)
        .fold(f32::INFINITY, f32::min);
    (right > left && bottom.is_finite() && bottom > top)
        .then(|| Rect::new(left, top, right, bottom))
}

fn grid(
    parent: &mut ChildSpawnerCommands,
    slots: &[InventorySlot],
    state: &InventoryState,
    icons: &mut Icons,
    directory: Option<&std::path::Path>,
    images: &mut Assets<Image>,
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
                square(grid, *slot, state, icons, directory, images);
            }
        });
}

fn square(
    parent: &mut ChildSpawnerCommands,
    slot: InventorySlot,
    state: &InventoryState,
    icons: &mut Icons,
    directory: Option<&std::path::Path>,
    images: &mut Assets<Image>,
) {
    let item = state.data.items().get(&slot);
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
                if let Some(icon) = icons.get(item.icon, directory, images) {
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
            } else if !state.data.received() || state.data.stale() {
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
    use eq_client_core::inventory::InventoryUpdate;

    #[test]
    fn quantity_click_opens_picker_without_also_picking_up_the_stack() {
        let mut inventory = InventoryState::default();
        inventory.apply(InventoryUpdate::Snapshot(
            super::super::demo_items()
                .into_iter()
                .filter(|item| item.slot != InventorySlot(30))
                .collect(),
        ));
        inventory.open = true;
        inventory.demo = true;
        let before = inventory.data.clone();
        let mut app = App::new();
        app.insert_resource(inventory)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<crate::chat::ChatState>()
            .init_resource::<crate::items::ItemState>()
            .insert_resource(crate::online::OnlineState::new(false))
            .insert_resource(crate::target::CommandsToServer(None))
            .add_systems(Update, super::super::input);
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
        let inventory = app.world().resource::<InventoryState>();
        assert_eq!(inventory.data, before);
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
        app.world_mut()
            .resource_mut::<InventoryState>()
            .apply(InventoryUpdate::Snapshot(contents));
        *app.world_mut()
            .get_mut::<Interaction>(count_button)
            .unwrap() = Interaction::Pressed;
        app.update();
        let inventory = app.world().resource::<InventoryState>();
        assert_eq!(
            inventory.data.items()[&InventorySlot(251)].stack_count,
            Some(20)
        );
        assert!(!inventory.data.items().contains_key(&InventorySlot(30)));
        assert!(inventory.actions.split.is_none());
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
        state.apply(InventoryUpdate::Snapshot(items));
        state.bank_open = true;
        state.tab = Tab::Bank;
        state.select_split(InventorySlot(2000));
        let mut app = App::new();
        app.insert_resource(state)
            .init_resource::<Assets<Image>>()
            .add_systems(
                Startup,
                |mut commands: Commands,
                 state: Res<InventoryState>,
                 mut images: ResMut<Assets<Image>>| {
                    commands.spawn(Node::default()).with_children(|parent| {
                        contents(
                            parent,
                            &state,
                            None,
                            &mut Icons::default(),
                            None,
                            &mut images,
                        );
                    });
                },
            );
        app.update();
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
        let layout: Vec<SlotPlacement> = (0..22_u16)
            .map(|slot| SlotPlacement {
                slot,
                x: f32::from(slot % 2) * 130.0,
                y: f32::from(slot / 2) * 43.0,
                width: 42.0,
                height: 42.0,
            })
            .collect();
        let mut state = InventoryState::default();
        state.apply(InventoryUpdate::Snapshot(Vec::new()));
        state.apply(InventoryUpdate::Snapshot(
            super::super::demo_items()
                .into_iter()
                .filter(|item| item.slot == InventorySlot(13))
                .collect(),
        ));
        let mut app = App::new();
        app.insert_resource(state)
            .init_resource::<Assets<Image>>()
            .add_systems(
                Startup,
                move |mut commands: Commands,
                      state: Res<InventoryState>,
                      mut images: ResMut<Assets<Image>>| {
                    let layout = layout.clone();
                    commands.spawn(Node::default()).with_children(|parent| {
                        contents(
                            parent,
                            &state,
                            Some(Paperdoll {
                                layout: &layout,
                                figure: None,
                            }),
                            &mut Icons::default(),
                            None,
                            &mut images,
                        );
                    });
                },
            );
        app.update();
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
    fn the_figure_fills_the_middle_between_the_outer_columns() {
        let at = |slot, x, y| SlotPlacement {
            slot,
            x,
            y,
            width: 42.0,
            height: 42.0,
        };
        // A top row, one side slot per column, and a lower row in the middle.
        let layout = [
            at(1, 0.0, 0.0),
            at(2, 43.0, 0.0),
            at(3, 87.0, 0.0),
            at(4, 130.0, 0.0),
            at(17, 0.0, 43.0),
            at(5, 130.0, 43.0),
            at(12, 43.0, 216.0),
            at(0, 87.0, 216.0),
        ];
        assert_eq!(
            middle(&layout, 42.0),
            Some(Rect::new(42.0, 42.0, 130.0, 216.0))
        );
        // A single column leaves no middle to draw in.
        assert_eq!(middle(&layout[..1], 42.0), None);
    }
}
