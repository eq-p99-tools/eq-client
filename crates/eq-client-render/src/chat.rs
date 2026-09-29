//! Tabbed receive-only chat, with mobile palette colors and independent scroll positions.
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    window::PrimaryWindow,
};
use eq_client_core::{
    ClientCommand, OutboundChat,
    chat::{ChannelName, ChatHistory, ChatLine, ChatTab, Message, channel_rgb},
};
use std::collections::BTreeMap;

/// A locally produced line in the System channel.
pub(super) fn system_line(text: String) -> ChatLine {
    ChatLine {
        channel: ChannelName::System,
        sender: None,
        target: None,
        message: Message {
            message: None,
            message_hex: None,
            text,
            item_links: Vec::new(),
        },
    }
}

#[derive(Clone, Copy)]
struct TabView {
    offset: f32,
    follow: bool,
    seen: u64,
}
impl Default for TabView {
    fn default() -> Self {
        Self {
            offset: 0.0,
            follow: true,
            seen: 0,
        }
    }
}
#[derive(Resource, Default)]
pub(super) struct ChatState {
    pub history: ChatHistory,
    active: ChatTab,
    views: BTreeMap<ChatTab, TabView>,
    pub hovered: bool,
    pub composing: bool,
    pub escape_consumed: bool,
    /// A `/target Name` request waiting for target selection to resolve it.
    pub requested_target: Option<String>,
    draft: String,
    status: String,
}
#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(super) struct Viewport;
#[derive(Component)]
pub(super) struct Content;
#[derive(Component)]
pub(super) struct TabButton(ChatTab);
#[derive(Component)]
pub(super) struct TabLabel(ChatTab);
#[derive(Component)]
pub(super) struct Latest;
#[derive(Component)]
pub(super) struct InputBox;
#[derive(Component)]
pub(super) struct InputLabel;
#[derive(Component)]
pub(super) struct Send;
#[derive(Component)]
pub(super) struct InputStatus;

const PANEL: Color = Color::srgba(0.025, 0.032, 0.04, 0.94);
const EDGE: Color = Color::srgb(0.23, 0.25, 0.26);
const INK: Color = Color::srgb(0.72, 0.75, 0.77);

/// Creates a clipped scrollback window; changing tabs never destroys stored messages.
#[allow(clippy::too_many_lines)] // Declarative UI tree.
pub(super) fn spawn(commands: &mut Commands) {
    let frame = commands
        .spawn((
            super::hud::HudRoot,
            Panel,
            GlobalZIndex(15),
            Node {
                position_type: PositionType::Absolute,
                left: px(20),
                bottom: px(16),
                width: px(420),
                max_width: percent(95),
                height: px(202),
                max_height: percent(45),
                padding: UiRect::all(px(8)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(EDGE),
        ))
        .id();
    super::windows::interactive(commands, frame);
    commands.entity(frame).with_children(|root| {
        super::windows::title_bar(root, frame, "CHAT");
        root.spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(3),
            row_gap: px(3),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|tabs| {
            for tab in ChatTab::ALL {
                tabs.spawn((
                    Button,
                    TabButton(tab),
                    Node {
                        padding: UiRect::axes(px(3), px(3)),
                        border: UiRect::bottom(px(2)),
                        ..default()
                    },
                    BackgroundColor(PANEL),
                    BorderColor::all(EDGE),
                ))
                .with_children(|button| {
                    button.spawn((
                        TabLabel(tab),
                        Text::new(tab.label()),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(INK),
                    ));
                });
            }
        });
        root.spawn((
            Viewport,
            ScrollPosition::default(),
            Node {
                overflow: Overflow::scroll_y(),
                flex_grow: 1.0,
                min_height: px(0),
                ..default()
            },
        ))
        .with_children(|viewport| {
            viewport.spawn((
                Content,
                Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
        });
        root.spawn(Node {
            column_gap: px(5),
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Button,
                InputBox,
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    padding: UiRect::axes(px(7), px(5)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.04, 0.05, 0.06)),
                BorderColor::all(EDGE),
            ))
            .with_children(|input| {
                input.spawn((
                    InputLabel,
                    Text::new("Press Enter to chat"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(INK),
                ));
            });
            row.spawn((
                Button,
                Send,
                Node {
                    padding: UiRect::axes(px(9), px(5)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.13, 0.16)),
            ))
            .with_children(|button| {
                button.spawn((
                    Text::new("Send"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(INK),
                ));
            });
        });
        root.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|footer| {
            footer.spawn((
                InputStatus,
                Text::new("Scroll for history / Enter to chat"),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(INK),
            ));
            footer
                .spawn((
                    Button,
                    Latest,
                    Node {
                        padding: UiRect::axes(px(8), px(3)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.10, 0.13, 0.16)),
                ))
                .with_children(|button| {
                    button.spawn((
                        Text::new("Latest"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(INK),
                    ));
                });
        });
    });
}

/// Routes tab presses and wheel input, reserving the wheel from the camera over chat.
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
pub(super) fn input(
    mut state: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    sender: Res<super::target::CommandsToServer>,
    mut keyboard: MessageReader<KeyboardInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    panels: Query<(&UiGlobalTransform, &ComputedNode), With<Panel>>,
    mut wheel: MessageReader<MouseWheel>,
    tabs: Query<(&Interaction, &TabButton), Changed<Interaction>>,
    latest: Query<&Interaction, (With<Latest>, Changed<Interaction>)>,
    input_box: Query<&Interaction, (With<InputBox>, Changed<Interaction>)>,
    send: Query<&Interaction, (With<Send>, Changed<Interaction>)>,
    viewport: Query<&ComputedNode, With<Viewport>>,
) {
    state.escape_consumed = false;
    // Character selection owns keyboard input until the zone admits the player.
    if online.enabled && online.session_id.is_none() {
        keyboard.clear();
        wheel.clear();
        state.composing = false;
        state.hovered = false;
        return;
    }
    if input_box.iter().any(|value| *value == Interaction::Pressed) {
        state.composing = true;
    }
    let mut submit = send.iter().any(|value| *value == Interaction::Pressed);
    for event in keyboard.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                if state.composing {
                    submit = true;
                } else {
                    state.composing = true;
                }
            }
            Key::Escape if state.composing => {
                state.composing = false;
                state.escape_consumed = true;
            }
            Key::Backspace if state.composing => {
                state.draft.pop();
            }
            Key::Character(value) if state.composing && state.draft.len() < 512 => {
                state.draft.push_str(value);
                let boundary = state.draft.floor_char_boundary(512);
                state.draft.truncate(boundary);
            }
            _ => (),
        }
    }
    if submit {
        submit_draft(&mut state, &online, &sender);
    }
    state.hovered = windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position)
        .is_some_and(|cursor| {
            panels.iter().any(|(transform, node)| {
                transform.try_inverse().is_some_and(|inverse| {
                    let local = inverse.transform_point2(cursor).abs();
                    local.cmple(node.size() * 0.5).all()
                })
            })
        });
    for (interaction, TabButton(tab)) in &tabs {
        if *interaction == Interaction::Pressed {
            state.active = *tab;
        }
    }
    let active = state.active;
    if latest.iter().any(|v| *v == Interaction::Pressed) {
        state.views.entry(active).or_default().follow = true;
    }
    for event in wheel.read() {
        if !state.hovered {
            continue;
        }
        let maximum = viewport.single().map_or(0.0, |node| {
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0)
        });
        let view = state.views.entry(active).or_default();
        if view.follow {
            view.offset = maximum;
        }
        let delta = -event.y
            * if event.unit == MouseScrollUnit::Line {
                30.0
            } else {
                1.0
            };
        view.offset = (view.offset + delta).clamp(0.0, maximum);
        view.follow = view.offset >= maximum - 1.0;
    }
}

/// Rebuilds message nodes only when the selected history changes.
#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::type_complexity
)] // Bevy keeps disjoint UI queries explicit; one refresh owns the coherent chat view.
pub(super) fn refresh(
    mut commands: Commands,
    mut state: ResMut<ChatState>,
    content: Query<Entity, With<Content>>,
    mut labels: Query<(&TabLabel, &mut Text), (Without<InputLabel>, Without<InputStatus>)>,
    mut buttons: Query<
        (
            &TabButton,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Without<InputBox>,
    >,
    mut previous: Local<Option<(Entity, ChatTab, u64)>>,
    mut input_labels: Query<&mut Text, (With<InputLabel>, Without<TabLabel>, Without<InputStatus>)>,
    mut input_boxes: Query<&mut BorderColor, (With<InputBox>, Without<TabButton>)>,
    mut input_status: Query<&mut Text, (With<InputStatus>, Without<InputLabel>, Without<TabLabel>)>,
) {
    let Ok(content) = content.single() else {
        return;
    };
    let active = state.active;
    let revision = state.history.revision();
    let following = state.views.entry(active).or_default().follow;
    if following {
        if active == ChatTab::All {
            for tab in ChatTab::ALL {
                state.views.entry(tab).or_default().seen = revision;
            }
        } else {
            state.views.entry(active).or_default().seen = revision;
        }
    }
    for (TabLabel(tab), mut text) in &mut labels {
        let seen = state.views.get(tab).copied().unwrap_or_default().seen;
        let unread = state.history.unread(*tab, seen);
        let label = if unread == 0 {
            tab.label().to_owned()
        } else {
            format!(
                "{} {}",
                tab.label(),
                if unread > 99 {
                    "99+".into()
                } else {
                    unread.to_string()
                }
            )
        };
        if text.0 != label {
            text.0 = label;
        }
    }
    for (TabButton(tab), interaction, mut background, mut border) in &mut buttons {
        background.0 = if *tab == active {
            Color::srgb(0.16, 0.19, 0.22)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.10, 0.13, 0.16)
        } else {
            PANEL
        };
        *border = BorderColor::all(if *tab == active {
            Color::srgb(0.72, 0.62, 0.38)
        } else {
            EDGE
        });
    }
    for mut text in &mut input_labels {
        text.0 = if state.draft.is_empty() {
            if state.composing {
                "|"
            } else {
                "Press Enter to chat"
            }
            .into()
        } else if state.composing {
            format!("{}|", state.draft)
        } else {
            state.draft.clone()
        };
    }
    for mut border in &mut input_boxes {
        *border = BorderColor::all(if state.composing {
            Color::srgb(0.45, 0.72, 0.95)
        } else {
            EDGE
        });
    }
    for mut text in &mut input_status {
        text.0 = if state.status.is_empty() {
            "Scroll for history / Enter to chat".into()
        } else {
            state.status.clone()
        };
    }
    let key = (content, active, revision);
    if *previous == Some(key) {
        return;
    }
    *previous = Some(key);
    commands.entity(content).despawn_children();
    commands.entity(content).with_children(|parent| {
        let lines = state.history.lines(active);
        if lines.is_empty() {
            parent.spawn((
                Text::new("No messages in this channel yet."),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(INK),
            ));
        }
        for (_, line) in lines {
            let [r, g, b] = channel_rgb(line.channel);
            let sender = line
                .sender
                .as_deref()
                .filter(|s| !s.is_empty())
                .map_or(String::new(), |s| format!("{s}: "));
            let label = ChatTab::for_channel(line.channel).label();
            if line.message.item_links.is_empty() {
                parent.spawn((
                    Text::new(format!("[{label}] {sender}{}", line.message.text)),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(Color::srgb_u8(r, g, b)),
                    Node {
                        width: percent(100),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
            } else {
                super::items::spawn_message(
                    parent,
                    format!("[{label}] {sender}"),
                    &line.message,
                    Color::srgb_u8(r, g, b),
                );
            }
        }
    });
}

fn submit_draft(
    state: &mut ChatState,
    online: &super::online::OnlineState,
    sender: &super::target::CommandsToServer,
) {
    let result = (|| -> Result<(), String> {
        if !online.connected || online.death.is_some() {
            return Err("Connect before sending chat".into());
        }
        let sender = sender
            .0
            .as_ref()
            .ok_or_else(|| "Network worker is unavailable".to_owned())?;
        if let Some(request) = target_request(state.draft.trim()) {
            state.requested_target = Some(request?);
            return Ok(());
        }
        if let Some(commands) = game_commands(state.draft.trim(), online) {
            for command in commands? {
                sender
                    .try_send(command)
                    .map_err(|_| "Command could not be queued".to_owned())?;
            }
            return Ok(());
        }
        let message = outbound(state.active, state.draft.trim())?;
        sender
            .try_send(ClientCommand::SendChat(message))
            .map_err(|_| "Chat could not be queued".to_owned())?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            state.draft.clear();
            state.status = "Chat queued".into();
        }
        Err(error) => state.status = error,
    }
}

/// Queues a game slash command for scripts; ordinary chat is never sent this way.
pub(super) fn submit_game_command(
    input: &str,
    online: &super::online::OnlineState,
    sender: &super::target::CommandsToServer,
) -> Result<(), String> {
    let commands =
        game_commands(input, online).ok_or_else(|| format!("{input} is not a game command"))??;
    let sender = sender
        .0
        .as_ref()
        .ok_or_else(|| "Network worker is unavailable".to_owned())?;
    for command in commands {
        sender
            .try_send(command)
            .map_err(|_| "Command could not be queued".to_owned())?;
    }
    Ok(())
}

/// The name in a `/target Name` command; underscores match spaces as in spawn names.
pub(super) fn target_request(input: &str) -> Option<Result<String, String>> {
    let command = input.strip_prefix('/')?;
    let (name, rest) = command.split_once(' ').unwrap_or((command, ""));
    if !name.eq_ignore_ascii_case("target") {
        return None;
    }
    let rest = rest.trim().replace('_', " ");
    Some(if rest.is_empty() {
        Err("Use /target Name".into())
    } else {
        Ok(rest)
    })
}

/// Slash commands that are game actions rather than chat; None means ordinary chat.
fn game_commands(
    input: &str,
    online: &super::online::OnlineState,
) -> Option<Result<Vec<ClientCommand>, String>> {
    let name = input.strip_prefix('/')?.trim().to_ascii_lowercase();
    let posture = |posture| {
        let (Some(session_id), Some(player)) = (online.session_id, online.player.as_ref()) else {
            return Err("Enter the world first".to_owned());
        };
        Ok(ClientCommand::SetPosture {
            session_id,
            spawn_id: player.spawn_id,
            posture,
            created: std::time::Instant::now(),
        })
    };
    Some(match name.as_str() {
        "sit" => posture(eq_client_core::Posture::Sitting).map(|command| vec![command]),
        "stand" => posture(eq_client_core::Posture::Standing).map(|command| vec![command]),
        // Camping requires sitting, so sit first as a player would.
        "camp" => posture(eq_client_core::Posture::Sitting).map(|sit| {
            vec![
                sit,
                ClientCommand::Camp {
                    session_id: online.session_id.unwrap_or_default(),
                    created: std::time::Instant::now(),
                },
            ]
        }),
        _ => return None,
    })
}

fn outbound(tab: ChatTab, input: &str) -> Result<OutboundChat, String> {
    if input.is_empty() {
        return Err("Type a message first".into());
    }
    if let Some(command) = input.strip_prefix('/') {
        let (name, rest) = command.split_once(' ').unwrap_or((command, ""));
        let rest = rest.trim();
        if matches!(name.to_ascii_lowercase().as_str(), "tell" | "t") {
            let (recipient, message) = rest
                .split_once(' ')
                .ok_or_else(|| "Use /tell Name message".to_owned())?;
            if message.trim().is_empty() {
                return Err("Use /tell Name message".into());
            }
            return Ok(OutboundChat::Tell {
                recipient: recipient.into(),
                message: message.trim().into(),
            });
        }
        if rest.is_empty() {
            return Err("Type a message after the channel command".into());
        }
        return match name.to_ascii_lowercase().as_str() {
            "say" | "s" => Ok(OutboundChat::Say(rest.into())),
            "auction" | "auc" => Ok(OutboundChat::Auction(rest.into())),
            "ooc" => Ok(OutboundChat::Ooc(rest.into())),
            "guild" | "gu" => Ok(OutboundChat::Guild(rest.into())),
            "group" | "g" => Ok(OutboundChat::Group(rest.into())),
            "shout" => Ok(OutboundChat::Shout(rest.into())),
            "raid" => Ok(OutboundChat::Raid(rest.into())),
            _ => Err(format!("Unknown chat command /{name}")),
        };
    }
    match tab {
        ChatTab::Guild => Ok(OutboundChat::Guild(input.into())),
        ChatTab::Group => Ok(OutboundChat::Group(input.into())),
        ChatTab::Auction => Ok(OutboundChat::Auction(input.into())),
        ChatTab::Ooc => Ok(OutboundChat::Ooc(input.into())),
        ChatTab::Raid => Ok(OutboundChat::Raid(input.into())),
        ChatTab::Shout => Ok(OutboundChat::Shout(input.into())),
        ChatTab::Tell => Err("Use /tell Name message".into()),
        _ => Ok(OutboundChat::Say(input.into())),
    }
}

/// Applies following or the saved tab offset after layout knows wrapped text heights.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn scroll(
    state: Res<ChatState>,
    mut query: Query<(&ComputedNode, &mut ScrollPosition), With<Viewport>>,
) {
    let view = state.views.get(&state.active).copied().unwrap_or_default();
    for (node, mut scroll) in &mut query {
        let maximum =
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        scroll.y = if view.follow {
            maximum
        } else {
            view.offset.min(maximum)
        };
    }
}

/// Clearly synthetic messages for the existing offline preview; no packets are sent.
pub(super) fn seed_demo(history: &mut ChatHistory) {
    use eq_client_core::chat::{ChannelName, ChatLine, Message};
    for (channel, text) in [
        (
            ChannelName::System,
            "Offline preview - these are synthetic messages.",
        ),
        (
            ChannelName::Guild,
            "Meet by the tunnel when everyone is ready.",
        ),
        (ChannelName::Group, "Ready when you are."),
        (
            ChannelName::Auction,
            "WTS Fine Steel Long Sword - send a tell.",
        ),
        (ChannelName::Ooc, "Anyone heading toward the inn?"),
        (ChannelName::Tell, "I will wait here."),
        (ChannelName::Emote, "waves hello."),
    ] {
        history.push(ChatLine {
            channel,
            sender: Some("Preview".into()),
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: text.into(),
                item_links: if channel == ChannelName::Auction {
                    let label = "Fine Steel Long Sword";
                    let start = text.find(label).unwrap();
                    vec![eq_client_core::ItemLink {
                        body: format!("00002A{}1234ABCD", "0".repeat(31)),
                        text: label.into(),
                        item_id: 42,
                        start: 0,
                        end: 0,
                        text_start: start,
                        text_end: start + label.len(),
                    }]
                } else {
                    vec![]
                },
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_requests_are_names_not_chat() {
        assert_eq!(
            target_request("/target a_cave rat"),
            Some(Ok("a cave rat".into()))
        );
        assert_eq!(target_request("/TARGET Arias"), Some(Ok("Arias".into())));
        assert!(target_request("/target  ").unwrap().is_err());
        assert_eq!(target_request("/targetx rat"), None);
        assert_eq!(target_request("target rat"), None);
    }

    #[test]
    fn camp_sits_first_and_game_commands_never_become_chat() {
        let mut online = super::super::online::OnlineState::new(true);
        assert!(game_commands("/say hello", &online).is_none());
        assert!(game_commands("hello", &online).is_none());
        assert!(game_commands("/camp", &online).unwrap().is_err());
        online.session_id = Some(4);
        online.player = Some(eq_client_core::PlayerState {
            base_attributes: None,
            deity: None,
            class: Some(2),
            spawn_id: 12,
            race: 1,
            gender: 0,
            level: 1,
            position: eq_client_core::WorldPosition::default(),
            mana: 0,
            endurance: None,
            skills: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 6.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
        });
        let commands = game_commands("/CAMP", &online).unwrap().unwrap();
        assert!(matches!(
            commands.as_slice(),
            [
                ClientCommand::SetPosture {
                    session_id: 4,
                    spawn_id: 12,
                    posture: eq_client_core::Posture::Sitting,
                    ..
                },
                ClientCommand::Camp { session_id: 4, .. }
            ]
        ));
        assert!(matches!(
            game_commands("/stand", &online)
                .unwrap()
                .unwrap()
                .as_slice(),
            [ClientCommand::SetPosture {
                posture: eq_client_core::Posture::Standing,
                ..
            }]
        ));
    }
    #[test]
    fn character_selection_enter_does_not_open_chat_or_leak_after_admission() {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .add_message::<MouseWheel>()
            .add_message::<KeyboardInput>()
            .insert_resource(super::super::online::OnlineState::new(true))
            .insert_resource(super::super::target::CommandsToServer(None))
            .add_systems(Update, input);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(!app.world().resource::<ChatState>().composing);
        app.world_mut()
            .resource_mut::<super::super::online::OnlineState>()
            .session_id = Some(1);
        app.update();
        assert!(!app.world().resource::<ChatState>().composing);
    }

    #[test]
    fn pressing_tabs_filters_text_without_losing_history() {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .add_message::<MouseWheel>()
            .add_message::<KeyboardInput>()
            .insert_resource(super::super::online::OnlineState::new(false))
            .insert_resource(super::super::target::CommandsToServer(None))
            .add_systems(Update, (input, refresh).chain());
        app.world_mut().spawn(Content);
        let button = app
            .world_mut()
            .spawn((TabButton(ChatTab::Guild), Interaction::Pressed))
            .id();
        seed_demo(&mut app.world_mut().resource_mut::<ChatState>().history);
        app.update();
        assert_eq!(app.world().resource::<ChatState>().active, ChatTab::Guild);
        let messages: Vec<_> = app
            .world_mut()
            .query::<(&Text, Option<&Children>)>()
            .iter(app.world())
            .map(|(root, children)| {
                let mut text = root.0.clone();
                if let Some(children) = children {
                    for child in children {
                        if let Some(span) = app.world().get::<TextSpan>(*child) {
                            text.push_str(&span.0);
                        }
                    }
                }
                text
            })
            .collect();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("Meet by the tunnel"));
        app.world_mut()
            .entity_mut(button)
            .insert((TabButton(ChatTab::Auction), Interaction::Pressed));
        app.update();
        let messages: Vec<_> = app
            .world_mut()
            .query::<(&Text, Option<&Children>)>()
            .iter(app.world())
            .map(|(root, children)| {
                let mut text = root.0.clone();
                if let Some(children) = children {
                    for child in children {
                        if let Some(span) = app.world().get::<TextSpan>(*child) {
                            text.push_str(&span.0);
                        }
                    }
                }
                text
            })
            .collect();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("Fine Steel"));
        assert_eq!(
            app.world()
                .resource::<ChatState>()
                .history
                .lines(ChatTab::All)
                .len(),
            7
        );
    }
    #[test]
    fn messages_do_not_force_a_scrolled_tab_back_to_latest() {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .add_systems(Update, refresh);
        app.world_mut().spawn(Content);
        {
            let mut state = app.world_mut().resource_mut::<ChatState>();
            state.views.insert(
                ChatTab::All,
                TabView {
                    follow: false,
                    offset: 35.0,
                    seen: 0,
                },
            );
            seed_demo(&mut state.history);
        }
        app.update();
        let state = app.world().resource::<ChatState>();
        let view = state.views[&ChatTab::All];
        assert!(!view.follow);
        assert!((view.offset - 35.0).abs() < 0.001);
        assert_eq!(view.seen, 0);
        assert_eq!(state.history.unread(ChatTab::All, view.seen), 7);
    }
}
