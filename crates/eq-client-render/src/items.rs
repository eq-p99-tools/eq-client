//! Clickable chat item links and a read-only server-backed details panel.
use super::{online::OnlineState, target::CommandsToServer};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, ItemDetails, ItemLink};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Component)]
pub(super) struct ItemButton(pub ItemLink);
#[derive(Component)]
pub(super) struct ItemPanel;
#[derive(Component)]
pub(super) struct ItemText;
#[derive(Component)]
pub(super) struct CloseItem;
#[derive(Resource, Default)]
pub(super) struct ItemState {
    pub cache: BTreeMap<u32, ItemDetails>,
    pub hovered: bool,
    session: Option<u64>,
    selected: Option<(u32, String)>,
    pending: Option<Instant>,
    status: String,
}

impl ItemState {
    /// Opens a received inventory definition without making an inspection request.
    pub(super) fn open_received(&mut self, item: ItemDetails) {
        self.selected = Some((item.id, item.name.clone()));
        self.pending = None;
        self.status.clear();
        if self.cache.len() >= 128 {
            self.cache.pop_first();
        }
        self.cache.insert(item.id, item);
    }
}

/// Creates an initially hidden item panel; closing it has no server-side effect.
pub(super) fn spawn(commands: &mut Commands) {
    let frame = commands
        .spawn((
            super::hud::HudRoot,
            ItemPanel,
            ScrollPosition::default(),
            GlobalZIndex(30),
            Node {
                position_type: PositionType::Absolute,
                right: px(20),
                top: px(105),
                width: px(310),
                max_height: percent(68),
                overflow: Overflow::scroll_y(),
                padding: UiRect::all(px(12)),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgb(0.025, 0.032, 0.04)),
        ))
        .id();
    super::windows::interactive(commands, frame);
    commands.entity(frame).with_children(|panel| {
        super::windows::title_bar(panel, frame, "ITEM");
        panel.spawn((
            Button,
            CloseItem,
            Text::new("Close item"),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(Color::srgb(0.85, 0.8, 0.6)),
        ));
        panel.spawn((
            ItemText,
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.9, 0.9)),
        ));
    });
}

/// Queues one inspection at a time, tied to the active session, and uses cached replies.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn input(
    settings: Res<super::ViewerSettings>,
    chat: Res<super::chat::ChatState>,
    mut attempted: Local<bool>,
    buttons: Query<(&Interaction, &ItemButton), Changed<Interaction>>,
    close: Query<&Interaction, (With<CloseItem>, Changed<Interaction>)>,
    online: Res<OnlineState>,
    sender: Res<CommandsToServer>,
    mut state: ResMut<ItemState>,
) {
    if state.session != online.session_id {
        *state = ItemState {
            session: online.session_id,
            ..default()
        };
    }
    if close.iter().any(|i| *i == Interaction::Pressed) {
        state.selected = None;
    }
    if state
        .pending
        .is_some_and(|t| t.elapsed() > Duration::from_secs(8))
    {
        state.pending = None;
        state.status = "No item definition received. Click the link to retry.".into();
    }
    if state
        .selected
        .as_ref()
        .is_some_and(|(id, _)| state.cache.contains_key(id))
    {
        state.pending = None;
    }
    let automatic = if !*attempted
        && settings.0.validation == Some(super::ValidationAction::InspectFirstItem)
        && online.connected
    {
        chat.history
            .lines(eq_client_core::chat::ChatTab::All)
            .into_iter()
            .flat_map(|(_, line)| &line.message.item_links)
            .find(|link| link.item_id != 0 && link.item_id != 0xfffff)
            .cloned()
    } else {
        None
    };
    if automatic.is_some() {
        *attempted = true;
    }
    for link in buttons
        .iter()
        .filter(|(i, _)| **i == Interaction::Pressed)
        .map(|(_, b)| &b.0)
        .chain(automatic.as_ref())
    {
        if state.pending.is_some() {
            continue;
        }
        state.selected = Some((link.item_id, link.text.clone()));
        if state.cache.contains_key(&link.item_id) {
            continue;
        }
        if !online.connected || online.death.is_some() {
            state.status = "Connect to inspect this item.".into();
            continue;
        }
        let Some(session_id) = online.session_id else {
            continue;
        };
        let command = ClientCommand::InspectItem {
            session_id,
            link_body: link.body.clone(),
        };
        if sender
            .0
            .as_ref()
            .is_some_and(|s| s.try_send(command).is_ok())
        {
            eprintln!("Item inspection requested: ID {}", link.item_id);
            state.pending = Some(Instant::now());
            state.status = "Loading item from server...".into();
        } else {
            state.status = "Item request could not be queued.".into();
        }
    }
}

/// Shows only received statistics; a missing response never becomes fabricated stats.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    state: Res<ItemState>,
    mut panels: Query<&mut Node, With<ItemPanel>>,
    mut texts: Query<&mut Text, With<ItemText>>,
) {
    for mut panel in &mut panels {
        panel.display = if state.selected.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    let Some((id, name)) = &state.selected else {
        return;
    };
    let text = state.cache.get(id).map_or_else(
        || format!("{name}\n\n{}", state.status),
        |item| {
            let properties = item
                .stats
                .iter()
                .map(|s| format!("{}: {}", s.label, s.value))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{}\n{}\nWeight: {}.{}\n\n{}\n\nClasses: {}\nRaces: {}\nSlots: {}",
                item.name,
                item.flags.join(" / "),
                item.weight_tenths / 10,
                item.weight_tenths % 10,
                properties,
                mask(
                    item.classes,
                    &[
                        "WAR", "CLR", "PAL", "RNG", "SHD", "DRU", "MNK", "BRD", "ROG", "SHM",
                        "NEC", "WIZ", "MAG", "ENC", "BST", "BER"
                    ]
                ),
                mask(
                    item.races,
                    &[
                        "HUM", "BAR", "ERU", "ELF", "HIE", "DEF", "HEF", "DWF", "TRL", "OGR",
                        "HFL", "GNM", "IKS", "VAH", "FRG", "DRK"
                    ]
                ),
                mask(
                    item.slots,
                    &[
                        "Charm",
                        "Ear",
                        "Head",
                        "Face",
                        "Ear",
                        "Neck",
                        "Shoulders",
                        "Arms",
                        "Back",
                        "Wrist",
                        "Wrist",
                        "Ranged",
                        "Hands",
                        "Primary",
                        "Secondary",
                        "Finger",
                        "Finger",
                        "Chest",
                        "Legs",
                        "Feet",
                        "Waist",
                        "Ammo"
                    ]
                )
            )
        },
    );
    for mut label in &mut texts {
        label.0.clone_from(&text);
    }
}
fn mask(bits: u32, labels: &[&str]) -> String {
    let known = (1u32 << labels.len()) - 1;
    if bits & known == known {
        return "ALL".into();
    }
    let mut names = Vec::new();
    for (bit, label) in labels.iter().enumerate() {
        if bits & (1 << bit) != 0 && !names.contains(label) {
            names.push(*label);
        }
    }
    if names.is_empty() {
        "None".into()
    } else {
        names.join(" ")
    }
}

/// Renders links as purple text spans within the same wrapping text paragraph.
pub(super) fn spawn_message(
    parent: &mut ChildSpawnerCommands,
    prefix: String,
    message: &eq_client_core::chat::Message,
    color: Color,
) {
    parent
        .spawn((
            Text::new(prefix),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(color),
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                ..default()
            },
        ))
        .with_children(|text| {
            let mut end = 0;
            for link in &message.item_links {
                if link.text_start < end
                    || message.text.get(end..link.text_start).is_none()
                    || message.text.get(link.text_start..link.text_end).is_none()
                {
                    continue;
                }
                text.spawn((
                    TextSpan::new(&message.text[end..link.text_start]),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(color),
                ));
                text.spawn((
                    TextSpan::new(&message.text[link.text_start..link.text_end]),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(Color::srgb_u8(190, 80, 255)),
                    ItemButton(link.clone()),
                    Interaction::None,
                ));
                end = link.text_end;
            }
            text.spawn((
                TextSpan::new(&message.text[end..]),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(color),
            ));
        });
}

/// Hit-tests the actual shaped text runs, including each wrapped part of a link.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(super) fn link_input(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    paragraphs: Query<(
        &UiGlobalTransform,
        &ComputedNode,
        &bevy::text::TextLayoutInfo,
        &bevy::text::ComputedTextBlock,
        &InheritedVisibility,
        Option<&bevy::ui::CalculatedClip>,
    )>,
    mut links: Query<&mut Interaction, With<ItemButton>>,
) {
    for mut interaction in &mut links {
        if *interaction != Interaction::None {
            *interaction = Interaction::None;
        }
    }
    let Some(cursor) = windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position)
    else {
        return;
    };
    for (transform, node, layout, block, visible, clip) in &paragraphs {
        if !visible.get() || node.is_empty() || clip.is_some_and(|c| !c.clip.contains(cursor)) {
            continue;
        }
        let Some(inverse) = transform.try_inverse() else {
            continue;
        };
        // Match Bevy's decoration transform: physical UI coordinates, relative to content origin.
        let point = inverse.transform_point2(cursor) - node.content_box().min;
        for section in hit_sections(&layout.run_geometry, point) {
            if let Some(span) = block.entities().get(section)
                && let Ok(mut interaction) = links.get_mut(span.entity)
            {
                *interaction = if mouse.just_pressed(MouseButton::Left) {
                    Interaction::Pressed
                } else {
                    Interaction::Hovered
                };
                return;
            }
        }
    }
}

fn hit_sections(runs: &[bevy::text::RunGeometry], point: Vec2) -> impl Iterator<Item = usize> + '_ {
    runs.iter()
        .filter(move |run| run.bounds.contains(point))
        .map(|run| run.section_index)
}

/// Scrolls long definitions while keeping the camera still beneath the panel.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn scroll(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut panels: Query<(&UiGlobalTransform, &ComputedNode, &mut ScrollPosition), With<ItemPanel>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut state: ResMut<ItemState>,
) {
    state.hovered = false;
    let delta: f32 = wheel
        .read()
        .map(|e| match e.unit {
            bevy::input::mouse::MouseScrollUnit::Line => e.y * 24.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => e.y,
        })
        .sum();
    let Some(cursor) = windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position)
    else {
        return;
    };
    if state.selected.is_none() {
        return;
    }
    for (transform, node, mut position) in &mut panels {
        if transform.try_inverse().is_some_and(|inverse| {
            inverse
                .transform_point2(cursor)
                .abs()
                .cmple(node.size() * 0.5)
                .all()
        }) {
            state.hovered = true;
            position.y = (position.y - delta).max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapped_link_uses_each_line_without_linking_the_gap() {
        let runs = [
            bevy::text::RunGeometry {
                section_index: 1,
                bounds: Rect::new(0.0, 0.0, 40.0, 12.0),
                ..default()
            },
            bevy::text::RunGeometry {
                section_index: 2,
                bounds: Rect::new(40.0, 0.0, 80.0, 12.0),
                ..default()
            },
            bevy::text::RunGeometry {
                section_index: 2,
                bounds: Rect::new(0.0, 12.0, 25.0, 24.0),
                ..default()
            },
        ];
        assert_eq!(
            hit_sections(&runs, Vec2::new(50.0, 6.0)).collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(
            hit_sections(&runs, Vec2::new(10.0, 18.0)).collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(
            hit_sections(&runs, Vec2::new(10.0, 6.0)).collect::<Vec<_>>(),
            [1]
        );
        assert!(hit_sections(&runs, Vec2::new(50.0, 18.0)).next().is_none());
    }

    #[test]
    fn inline_link_preserves_unicode_and_message_order_without_duplicate_labels() {
        let mut app = App::new();
        app.add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Node::default()).with_children(|parent| {
                spawn_message(
                    parent,
                    "[Auc] ".into(),
                    &eq_client_core::chat::Message {
                        message: None,
                        message_hex: None,
                        text: "WTS Épée now".into(),
                        item_links: vec![ItemLink {
                            body: "synthetic".into(),
                            text: "Épée".into(),
                            item_id: 42,
                            start: 0,
                            end: 0,
                            text_start: 4,
                            text_end: 10,
                        }],
                    },
                    Color::WHITE,
                );
            });
        });
        app.update();
        let world = app.world_mut();
        let mut paragraphs = world.query::<(&Text, &Children)>();
        let (text, children) = paragraphs.single(world).unwrap();
        let mut combined = text.0.clone();
        for child in children {
            combined.push_str(&world.get::<TextSpan>(*child).unwrap().0);
        }
        assert_eq!(combined, "[Auc] WTS Épée now");
        let mut links = world.query::<(&ItemButton, &TextColor)>();
        let (link, color) = links.single(world).unwrap();
        assert_eq!(link.0.item_id, 42);
        assert_eq!(color.0, Color::srgb_u8(190, 80, 255));
    }

    #[test]
    fn clicking_an_item_queues_inspection_and_reconnect_clears_the_panel() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        let mut app = App::new();
        let mut online = OnlineState::new(true);
        online.connected = true;
        online.session_id = Some(77);
        app.insert_resource(online)
            .insert_resource(super::super::ViewerSettings(
                super::super::ViewerConfig::default(),
            ))
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<ItemState>()
            .insert_resource(CommandsToServer(Some(sender)))
            .add_systems(Update, input);
        let body = format!("00002A{}1234ABCD", "0".repeat(31));
        app.world_mut().spawn((
            Interaction::Pressed,
            ItemButton(ItemLink {
                body: body.clone(),
                text: "Synthetic blade".into(),
                item_id: 42,
                start: 0,
                end: 0,
                text_start: 0,
                text_end: 0,
            }),
        ));
        app.update();
        assert_eq!(
            receiver.try_recv().unwrap(),
            ClientCommand::InspectItem {
                session_id: 77,
                link_body: body
            }
        );
        assert_eq!(
            app.world()
                .resource::<ItemState>()
                .selected
                .as_ref()
                .unwrap()
                .0,
            42
        );
        app.update();
        assert!(receiver.try_recv().is_err());
        app.world_mut().resource_mut::<OnlineState>().session_id = Some(78);
        app.update();
        assert!(app.world().resource::<ItemState>().selected.is_none());
        assert!(app.world().resource::<ItemState>().pending.is_none());
    }
}
