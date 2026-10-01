//! Tabbed receive-only chat, with mobile palette colors and independent scroll positions.
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    window::PrimaryWindow,
};
use eq_client_core::{
    ClientCommand, OutboundChat,
    chat::{ChannelName, ChatHistory, ChatLine, ChatTab, Message, channel_rgb},
};
use std::collections::{BTreeMap, HashSet};

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
/// The active tab's lines, kept between redraws.
#[derive(Component, Default)]
pub(super) struct Content {
    /// The tab the lines belong to, once drawn.
    tab: Option<ChatTab>,
    /// The history revision the lines match.
    revision: u64,
}
/// The history entry a chat line shows.
#[derive(Component)]
pub(super) struct LineId(u64);
/// Stands in for the lines of a tab that has none yet.
#[derive(Component)]
pub(super) struct Placeholder;
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
    let frame = super::windows::frame(
        commands,
        super::windows::WindowId::Chat,
        Node {
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
        PANEL,
    );
    commands.entity(frame).insert((
        super::hud::HudRoot,
        Panel,
        super::windows::pointer::TakesWheel,
        BorderColor::all(EDGE),
    ));
    commands.entity(frame).with_children(|root| {
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
            // Hovered only when no window drawn over the chat takes the pointer.
            Interaction::default(),
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
                Content::default(),
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
    outbox: Res<crate::outbox::Outbox>,
    mut keyboard: MessageReader<KeyboardInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    panels: Query<(Entity, &UiGlobalTransform, &ComputedNode), With<Panel>>,
    wheel: Res<super::windows::pointer::Wheel>,
    tabs: Query<(&Interaction, &TabButton), Changed<Interaction>>,
    latest: Query<&Interaction, (With<Latest>, Changed<Interaction>)>,
    input_box: Query<&Interaction, (With<InputBox>, Changed<Interaction>)>,
    send: Query<&Interaction, (With<Send>, Changed<Interaction>)>,
    viewport: Query<&ComputedNode, With<Viewport>>,
) {
    state.escape_consumed = false;
    // Character selection owns keyboard input until the zone admits the player.
    if online.enabled && online.world.session_id().is_none() {
        keyboard.clear();
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
        if state.draft.trim().is_empty() {
            // Enter on an empty line closes the input, as in the official client.
            state.composing = false;
        } else {
            submit_draft(&mut state, &online, &outbox);
        }
    }
    state.hovered = windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position)
        .is_some_and(|cursor| {
            panels.iter().any(|(_, transform, node)| {
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
    if wheel
        .surface
        .is_some_and(|surface| panels.contains(surface))
        && wheel.pixels != 0.0
    {
        let maximum = viewport.single().map_or(0.0, |node| {
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0)
        });
        let view = state.views.entry(active).or_default();
        if view.follow {
            view.offset = maximum;
        }
        view.offset = (view.offset - wheel.pixels).clamp(0.0, maximum);
        view.follow = view.offset >= maximum - 1.0;
    }
}

/// Keeps the shown lines in step with the active tab's history.
#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::type_complexity
)] // Bevy keeps disjoint UI queries explicit; one refresh owns the coherent chat view.
pub(super) fn refresh(
    mut commands: Commands,
    mut state: ResMut<ChatState>,
    mut contents: Query<(Entity, &mut Content, Option<&Children>)>,
    rendered: Query<(Option<&LineId>, Has<Placeholder>)>,
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
    mut input_labels: Query<&mut Text, (With<InputLabel>, Without<TabLabel>, Without<InputStatus>)>,
    mut input_boxes: Query<&mut BorderColor, (With<InputBox>, Without<TabButton>)>,
    mut input_status: Query<&mut Text, (With<InputStatus>, Without<InputLabel>, Without<TabLabel>)>,
) {
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
    // Only the active tab's lines exist, as laying out every tab's lines each frame
    // would cost more than redrawing one tab when it is chosen.
    for (column, mut content, children) in &mut contents {
        let children = if content.tab == Some(active) {
            if content.revision == revision {
                continue;
            }
            children
        } else {
            commands.entity(column).despawn_children();
            content.tab = Some(active);
            None
        };
        content.revision = revision;
        let lines = state.history.lines(active);
        sync_lines(&mut commands, column, children, &lines, &rendered);
    }
}

/// Brings the shown lines up to the history: lines it let go are dropped and new
/// ones appended, so a busy channel never lays its kept lines out again.
fn sync_lines(
    commands: &mut Commands,
    column: Entity,
    children: Option<&Children>,
    lines: &[(u64, &ChatLine)],
    rendered: &Query<(Option<&LineId>, Has<Placeholder>)>,
) {
    let kept: HashSet<u64> = lines.iter().map(|(id, _)| *id).collect();
    let mut newest = 0;
    let mut placeholder = None;
    for &child in children.into_iter().flatten() {
        match rendered.get(child) {
            Ok((Some(LineId(id)), _)) if kept.contains(id) => newest = newest.max(*id),
            Ok((Some(_), _)) => commands.entity(child).despawn(),
            Ok((None, true)) => placeholder = Some(child),
            _ => (),
        }
    }
    match (placeholder, lines.is_empty()) {
        (Some(placeholder), false) => commands.entity(placeholder).despawn(),
        (None, true) => {
            commands.entity(column).with_child((
                Placeholder,
                Text::new("No messages in this channel yet."),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(INK),
            ));
        }
        _ => (),
    }
    // History ids only grow, so every line newer than the last one shown is new.
    commands.entity(column).with_children(|parent| {
        for (id, line) in lines.iter().filter(|(id, _)| *id > newest) {
            spawn_line(parent, *id, line);
        }
    });
}

/// Appends one chat line: its channel, sender and text, with item links clickable.
fn spawn_line(parent: &mut ChildSpawnerCommands, id: u64, line: &ChatLine) {
    let [r, g, b] = channel_rgb(line.channel);
    let color = Color::srgb_u8(r, g, b);
    let sender = line
        .sender
        .as_deref()
        .filter(|s| !s.is_empty())
        .map_or(String::new(), |s| format!("{s}: "));
    let prefix = format!("[{}] {sender}", ChatTab::for_channel(line.channel).label());
    if line.message.item_links.is_empty() {
        parent.spawn((
            LineId(id),
            Text::new(format!("{prefix}{}", line.message.text)),
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
        ));
    } else {
        let message = super::items::spawn_message(parent, prefix, &line.message, color);
        parent.commands().entity(message).insert(LineId(id));
    }
}

/// Why a draft did not go: a mistake in it, which the chat's status line
/// shows, or a refusal, which the outbox shows in the feedback line.
enum Unsent {
    Mistake(String),
    Refused,
}

impl From<String> for Unsent {
    fn from(mistake: String) -> Self {
        Self::Mistake(mistake)
    }
}

impl From<crate::outbox::Refusal> for Unsent {
    fn from(_: crate::outbox::Refusal) -> Self {
        Self::Refused
    }
}

fn submit_draft(
    state: &mut ChatState,
    online: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
) {
    let result = (|| -> Result<(), Unsent> {
        if let Some(request) = target_request(state.draft.trim()) {
            state.requested_target = Some(request?);
            return Ok(());
        }
        if let Some(line) = location(state.draft.trim(), online) {
            state.history.push(system_line(line?));
            return Ok(());
        }
        if let Some(commands) = game_commands(state.draft.trim(), online, outbox) {
            for command in commands? {
                outbox.send(&online.world, command)?;
            }
            return Ok(());
        }
        let message = outbound(state.active, state.draft.trim())?;
        outbox.send(&online.world, ClientCommand::SendChat(message))?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            state.draft.clear();
            // Sent lines return the keyboard to the game; a line sent says
            // nothing more.
            state.composing = false;
            state.status.clear();
        }
        Err(Unsent::Mistake(mistake)) => state.status = mistake,
        // The draft stays, to send again.
        Err(Unsent::Refused) => (),
    }
}

/// Sends a game slash command for scripts; ordinary chat is never sent this way.
pub(super) fn submit_game_command(
    input: &str,
    online: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
) -> Result<(), String> {
    let commands = game_commands(input, online, outbox)
        .ok_or_else(|| format!("{input} is not a game command"))??;
    for command in commands {
        outbox
            .send(&online.world, command)
            .map_err(|refusal| refusal.text().to_owned())?;
    }
    Ok(())
}

/// `/loc`: where the player stands, worded as the official client words it,
/// north-south first.
fn location(input: &str, online: &super::online::OnlineState) -> Option<Result<String, String>> {
    let name = input.strip_prefix('/')?.trim();
    if !name.eq_ignore_ascii_case("loc") {
        return None;
    }
    Some(
        online
            .world
            .player()
            .map(|player| {
                let position = player.position;
                format!(
                    "Your Location is {:.2}, {:.2}, {:.2}",
                    position.y, position.x, position.z
                )
            })
            .ok_or_else(|| "Enter the world first".to_owned()),
    )
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
    outbox: &crate::outbox::Outbox,
) -> Option<Result<Vec<ClientCommand>, String>> {
    let name = input.strip_prefix('/')?.trim().to_ascii_lowercase();
    let stamp = || {
        outbox
            .stamp(&online.world)
            .map_err(|refusal| refusal.text().to_owned())
    };
    let posture = |posture| {
        let stamp = stamp()?;
        let player = online.world.player().ok_or("Enter the world first")?;
        Ok(ClientCommand::SetPosture {
            session_id: stamp.session_id,
            spawn_id: player.spawn_id,
            posture,
            created: stamp.created,
        })
    };
    Some(match name.as_str() {
        "sit" => posture(eq_client_core::Posture::Sitting).map(|command| vec![command]),
        "stand" => posture(eq_client_core::Posture::Standing).map(|command| vec![command]),
        // Camping requires sitting, so sit first as a player would.
        "camp" => posture(eq_client_core::Posture::Sitting).and_then(|sit| {
            let stamp = stamp()?;
            Ok(vec![
                sit,
                ClientCommand::Camp {
                    session_id: stamp.session_id,
                    created: stamp.created,
                },
            ])
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

    /// The entries shown in the chat window, in order.
    fn column(app: &mut App) -> Vec<Entity> {
        let mut query = app
            .world_mut()
            .query_filtered::<Option<&Children>, With<Content>>();
        query
            .single(app.world())
            .unwrap()
            .map(|children| children.to_vec())
            .unwrap_or_default()
    }

    /// The text of each line shown, item links included.
    fn shown(app: &mut App) -> Vec<String> {
        let lines = column(app);
        let world = app.world();
        lines
            .into_iter()
            .map(|line| {
                let mut text = world.get::<Text>(line).unwrap().0.clone();
                for child in world.get::<Children>(line).into_iter().flatten() {
                    if let Some(span) = world.get::<TextSpan>(*child) {
                        text.push_str(&span.0);
                    }
                }
                text
            })
            .collect()
    }

    #[test]
    fn new_lines_join_the_ones_shown_and_evicted_lines_leave() {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .add_systems(Update, refresh);
        app.world_mut().spawn(Content::default());
        app.update();
        let empty = column(&mut app);
        assert_eq!(empty.len(), 1);
        assert!(app.world().get::<Placeholder>(empty[0]).is_some());

        let push = |app: &mut App, count| {
            let mut state = app.world_mut().resource_mut::<ChatState>();
            for _ in 0..count {
                state.history.push(system_line("Synthetic line".into()));
            }
            app.update();
        };
        push(&mut app, 1);
        let first = column(&mut app);
        assert_eq!(first.len(), 1);
        assert!(app.world().get::<LineId>(first[0]).is_some());

        // A channel keeps 200 lines, so the first one leaves.
        push(&mut app, 200);
        let lines = column(&mut app);
        assert!(!lines.contains(&first[0]));
        let ids: Vec<u64> = lines
            .iter()
            .map(|line| app.world().get::<LineId>(*line).unwrap().0)
            .collect();
        let history: Vec<u64> = app
            .world()
            .resource::<ChatState>()
            .history
            .lines(ChatTab::All)
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(ids, history);

        // A new line keeps every line already drawn.
        push(&mut app, 1);
        let after = column(&mut app);
        assert_eq!(after.len(), 200);
        assert_eq!(after[..199], lines[1..]);
    }

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
        let (queue, _received) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        assert!(game_commands("/say hello", &online, &outbox).is_none());
        assert!(game_commands("hello", &online, &outbox).is_none());
        assert!(game_commands("/camp", &online, &outbox).unwrap().is_err());
        crate::online::testing::admit(
            &mut online,
            4,
            eq_client_core::PlayerState {
                name: "Example".into(),
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
                appearance: eq_client_core::outfit::Appearance::default(),
            },
        );
        let commands = game_commands("/CAMP", &online, &outbox).unwrap().unwrap();
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
            game_commands("/stand", &online, &outbox)
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
    fn sending_a_line_or_an_empty_enter_returns_the_keyboard_to_the_game() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let mut online = super::super::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .init_resource::<super::super::windows::pointer::Wheel>()
            .add_message::<KeyboardInput>()
            .insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, input);
        let enter = KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        {
            let mut chat = app.world_mut().resource_mut::<ChatState>();
            chat.composing = true;
            chat.draft = "hello".into();
        }
        app.world_mut().write_message(enter.clone());
        app.update();
        assert!(receiver.try_recv().is_ok());
        let chat = app.world().resource::<ChatState>();
        assert!(!chat.composing && chat.draft.is_empty());
        app.world_mut().resource_mut::<ChatState>().composing = true;
        app.world_mut().write_message(enter);
        app.update();
        assert!(!app.world().resource::<ChatState>().composing);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn character_selection_enter_does_not_open_chat_or_leak_after_admission() {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .init_resource::<super::super::windows::pointer::Wheel>()
            .add_message::<KeyboardInput>()
            .insert_resource(super::super::online::OnlineState::new(true))
            .insert_resource(crate::outbox::Outbox::new(None))
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
        crate::online::testing::admit(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            1,
            crate::online::testing::player(1),
        );
        app.update();
        assert!(!app.world().resource::<ChatState>().composing);
    }

    #[test]
    fn pressing_tabs_filters_text_without_losing_history() {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .init_resource::<super::super::windows::pointer::Wheel>()
            .add_message::<KeyboardInput>()
            .insert_resource(super::super::online::OnlineState::new(false))
            .insert_resource(crate::outbox::Outbox::new(None))
            .add_systems(Update, (input, refresh).chain());
        app.world_mut().spawn(Content::default());
        let button = app
            .world_mut()
            .spawn((TabButton(ChatTab::Guild), Interaction::Pressed))
            .id();
        seed_demo(&mut app.world_mut().resource_mut::<ChatState>().history);
        app.update();
        assert_eq!(app.world().resource::<ChatState>().active, ChatTab::Guild);
        let messages = shown(&mut app);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("Meet by the tunnel"));
        app.world_mut()
            .entity_mut(button)
            .insert((TabButton(ChatTab::Auction), Interaction::Pressed));
        app.update();
        let messages = shown(&mut app);
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
        app.world_mut().spawn(Content::default());
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
