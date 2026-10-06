//! Tabbed receive-only chat, with mobile palette colors and independent scroll positions.
use crate::theme::{self, Size};
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    window::PrimaryWindow,
};
use eq_client_core::{
    ClientCommand, OutboundChat,
    chat::{Arrival, ChannelName, ChatHistory, ChatLine, ChatTab, Message, Source, channel_rgb},
    qol::Fix,
};
use std::collections::{BTreeMap, HashSet};

/// Words the client says in the chat, and whose they are: the official
/// client's, read from its string table, or this client's own, which the
/// official client's log never takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Said {
    pub words: eq_client_core::chat::RichText,
    pub source: Source,
}

impl Said {
    /// The official client's words.
    pub(crate) fn official(text: impl Into<String>) -> Self {
        Self {
            words: text.into().into(),
            source: Source::Official,
        }
    }

    /// This client's own words.
    pub(crate) fn own(text: impl Into<String>) -> Self {
        Self {
            words: text.into().into(),
            source: Source::Client,
        }
    }
}

/// Words with nothing said of whose they are are this client's own.
impl From<String> for Said {
    fn from(text: String) -> Self {
        Self::own(text)
    }
}

impl From<&str> for Said {
    fn from(text: &str) -> Self {
        Self::own(text)
    }
}

/// A locally produced line in the System channel.
pub(super) fn system_line(said: impl Into<Said>) -> ChatLine {
    let Said { words, source } = said.into();
    ChatLine {
        channel: ChannelName::System,
        message_type: None,
        sender: None,
        target: None,
        message: Message {
            message: None,
            message_hex: None,
            text: words.text,
            item_links: words.item_links,
        },
        source,
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
    /// A `/target Name` request waiting for target selection to resolve it.
    pub requested_target: Option<String>,
    /// A plain `/who`, waiting to list the zone's players.
    pub zone_who: Option<eq_client_core::who::WhoFilter>,
    /// A `/loc` or `/time`, waiting to be answered.
    pub asked: Option<super::whereabouts::Asked>,
    /// A `/log`, waiting to turn the chat log on or off.
    pub log_toggle: bool,
    /// A `/shownames`, waiting to set how much of players' names shows.
    pub show_names: Option<eq_client_core::names::ShowNames>,
    /// A `/shownames` the client could not read, answered with the usage
    /// line.
    pub show_names_usage: bool,
    /// A window a slash command opens or closes, as `/raidwindow` does the
    /// Raid window, waiting for the toggle.
    pub toggled: Option<super::windows::WindowId>,
    draft: String,
    refusals: Refusals,
}

/// The refusals said, so the same one again, as a held key repeats it each
/// frame, is not said too soon.
#[derive(Default)]
struct Refusals {
    /// The last refusal said and when.
    last: Option<(String, std::time::Instant)>,
    /// Whether the same refusal is said again however soon, as it is unless
    /// the quality-of-life fix keeps repeats quiet ([`Fix::QuietRepeats`]);
    /// kept in step with the options.
    says_repeats: bool,
}

/// How soon the same refusal again goes unsaid.
const REPEATED: std::time::Duration = std::time::Duration::from_secs(3);

impl ChatState {
    /// Says a refusal in the chat as a system line, where the official client
    /// says its refusals; the same refusal again within [`REPEATED`], as from
    /// a held key, is not said twice.
    pub(super) fn refuse(&mut self, said: impl Into<Said>) {
        self.refuse_at(said.into(), std::time::Instant::now());
    }

    fn refuse_at(&mut self, said: Said, now: std::time::Instant) {
        if said.words.text.is_empty() {
            return;
        }
        let refusals = &mut self.refusals;
        if !refusals.says_repeats
            && refusals.last.as_ref().is_some_and(|(text, at)| {
                *text == said.words.text && now.saturating_duration_since(*at) < REPEATED
            })
        {
            return;
        }
        refusals.last = Some((said.words.text.clone(), now));
        self.history.push(system_line(said));
    }
}

#[cfg(test)]
impl ChatState {
    /// The newest line's words, for tests of what the chat says.
    pub(crate) fn newest(&self) -> String {
        self.history
            .lines(ChatTab::All)
            .last()
            .map(|(_, line)| line.message.text.clone())
            .unwrap_or_default()
    }
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
    /// Drawn in the skin: an empty tab is an empty box.
    bare: bool,
    /// Whether the lines start with the time they arrived
    /// ([`Fix::ShowTimesInChat`]).
    times: bool,
}
/// The history entry a chat line shows.
#[derive(Component)]
pub(super) struct LineId(u64);
/// Stands in for the lines of a tab that has none yet.
#[derive(Component)]
pub(super) struct Placeholder;
#[derive(Component)]
pub(super) struct TabButton(pub(super) ChatTab);
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
/// A part of the chat drawn in the skin: it keeps the skin's look, with no
/// fill, border or words of the client's own.
#[derive(Component)]
pub(super) struct Bare;

/// Creates a clipped scrollback window; changing tabs never destroys stored
/// messages. Where the installed skin has a chat window, it is drawn from
/// the skin instead, with the same tabs, lines and input in its boxes
/// ([`skinned_output`], [`skinned_input`]).
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
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            ..default()
        },
    );
    commands.entity(frame).insert((
        super::hud::HudRoot,
        Panel,
        super::windows::pointer::TakesWheel,
    ));
    commands.entity(frame).with_children(|root| {
        tab_row(root);
        lines(root, false);
        root.spawn(Node {
            column_gap: px(5),
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|row| {
            typing_box(
                row,
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    padding: UiRect::axes(px(7), px(5)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
            );
            theme::button_with(row, Send, "Send", Size::Body);
        });
        root.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|footer| {
            footer.spawn(theme::text(
                "Wheel: history | Enter: chat",
                Size::Small,
                theme::INK,
            ));
            theme::button_with(footer, Latest, "Latest", Size::Small);
        });
    });
}

/// The skin's output box, filled with the chat's tabs along its top, in
/// the skin's tab frame where it has one, and the active tab's lines below
/// them, which it returns: they scroll.
pub(super) fn skinned_output(
    parent: &mut ChildSpawnerCommands,
    node: Node,
    (art, tab_frame): (
        &mut crate::sheets::Art,
        Option<&eq_client_assets::sidl::FrameLook>,
    ),
) -> Entity {
    let mut viewport = Entity::PLACEHOLDER;
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::all(px(2)),
            ..node
        })
        .with_children(|output| {
            match tab_frame {
                Some(look) => skinned_tabs(output, art, look),
                None => tab_row(output),
            }
            viewport = lines(output, true);
        });
    viewport
}

/// The chat's tabs in the skin's tab frame, as a skinned window's tabs of
/// words are drawn; the shown tab's words are lit.
fn skinned_tabs(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    look: &eq_client_assets::sidl::FrameLook,
) {
    parent
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(2),
            row_gap: px(2),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|tabs| {
            for tab in ChatTab::ALL {
                let mut cell = tabs.spawn((Button, TabButton(tab), Bare));
                super::skinned::frame_tab(
                    &mut cell,
                    art,
                    look,
                    Node {
                        height: px(super::skinned::WORD_TAB_HEIGHT),
                        align_items: AlignItems::Center,
                        flex_shrink: 0.0,
                        ..default()
                    },
                );
                cell.with_child((
                    TabLabel(tab),
                    Bare,
                    Text::new(tab.label()),
                    theme::font(Size::Small),
                    TextColor(theme::INK),
                ));
            }
        });
}

/// The skin's input box, with nothing of the client's own in it: the line
/// the player types, and the caret alone while typing.
pub(super) fn skinned_input(parent: &mut ChildSpawnerCommands, node: Node) {
    parent
        .spawn((
            Button,
            InputBox,
            Bare,
            Node {
                padding: UiRect::axes(px(4), px(1)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..node
            },
        ))
        .with_children(|input| {
            input.spawn((
                InputLabel,
                Bare,
                Text::new(""),
                theme::font(Size::Body),
                TextColor(theme::INK_BRIGHT),
            ));
        });
}

/// The chat's tabs, one for each group of channels, with their unread
/// counts.
fn tab_row(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
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
                    BackgroundColor(theme::BUTTON),
                    BorderColor::all(theme::EDGE),
                ))
                .with_children(|button| {
                    button.spawn((
                        TabLabel(tab),
                        Text::new(tab.label()),
                        theme::font(Size::Small),
                        TextColor(theme::INK),
                    ));
                });
            }
        });
}

/// The active tab's lines, which scroll on their own; drawn in the skin
/// (`bare`), an empty tab is an empty box.
fn lines(parent: &mut ChildSpawnerCommands, bare: bool) -> Entity {
    parent
        .spawn((
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
                Content { bare, ..default() },
                Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
        })
        .id()
}

/// The line the player types into, which shows the draft.
fn typing_box(parent: &mut ChildSpawnerCommands, node: Node) {
    parent
        .spawn((
            Button,
            InputBox,
            node,
            BackgroundColor(theme::WELL),
            BorderColor::all(theme::EDGE),
        ))
        .with_children(|input| {
            input.spawn((
                InputLabel,
                Text::new("Press Enter to chat"),
                theme::font(Size::Body),
                TextColor(theme::INK),
            ));
        });
}

/// Routes tab presses and wheel input, reserving the wheel from the camera over chat.
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
pub(super) fn input(
    mut state: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    outbox: Res<crate::outbox::Outbox>,
    mut typing: ResMut<crate::keys::Typing>,
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
    typing.escape_consumed = false;
    // Character selection owns keyboard input until the zone admits the player.
    if online.enabled && online.world().session_id().is_none() {
        keyboard.clear();
        typing.composing = false;
        state.hovered = false;
        return;
    }
    if input_box.iter().any(|value| *value == Interaction::Pressed) {
        typing.composing = true;
        typing.counting = false;
    }
    // The quantity window's number has the keyboard: its keys are its own.
    if typing.counting {
        keyboard.clear();
    }
    let mut submit = send.iter().any(|value| *value == Interaction::Pressed);
    for event in keyboard.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                if typing.composing {
                    submit = true;
                } else {
                    typing.composing = true;
                }
            }
            Key::Escape if typing.composing => {
                typing.composing = false;
                typing.escape_consumed = true;
            }
            Key::Backspace if typing.composing => {
                state.draft.pop();
            }
            Key::Character(value) if typing.composing && state.draft.len() < 512 => {
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
            typing.composing = false;
        } else {
            submit_draft(&mut state, &mut typing, &online, &outbox);
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

/// The chat's colours for its kinds of text, and its font, as the player
/// set them in the official client: drawn in the skinned chat only.
#[derive(Resource, Default)]
pub(super) struct ChatLook {
    /// Each kind of text's colour, by the kind's number
    /// (`eq_client_core::chat::color_kind`).
    colors: std::collections::BTreeMap<u16, [u8; 3]>,
    /// The main chat window's font, by the client's font number.
    font: Option<u8>,
}

impl ChatLook {
    /// A line's colour: the player's for the message type the server gave
    /// it, else the player's for its channel's kind of text, else the
    /// client's. A type below the Colors page's kinds (a fixed colour in the
    /// official client, not checked) takes its channel's.
    fn color(&self, line: &ChatLine) -> Color {
        let [red, green, blue] = line
            .message_type
            .and_then(|kind| u16::try_from(kind).ok())
            .and_then(|kind| self.colors.get(&kind).copied())
            .or_else(|| {
                eq_client_core::chat::color_kind(line.channel)
                    .and_then(|kind| self.colors.get(&kind).copied())
            })
            .unwrap_or_else(|| channel_rgb(line.channel));
        Color::srgb_u8(red, green, blue)
    }
}

/// Keeps whether the same refusal is said again however soon in step with
/// the options ([`Fix::QuietRepeats`]).
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn follow_options(
    options: Res<super::options::OptionsState>,
    mut state: ResMut<ChatState>,
) {
    let says = !options.options.qol.on(Fix::QuietRepeats);
    if state.refusals.says_repeats != says {
        state.refusals.says_repeats = says;
    }
}

/// Reads the chat's colours and font from the official settings again when
/// the character or world changes, and only then.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn look(
    profile: Res<super::profile_files::Profile>,
    settings: Res<super::ViewerSettings>,
    mut look: ResMut<ChatLook>,
) {
    if !profile.is_changed() {
        return;
    }
    let current = profile.names();
    let official = settings.0.official_settings();
    look.colors = official
        .as_ref()
        .map(|official| official.text_colors().into_iter().collect())
        .unwrap_or_default();
    look.font = current
        .zip(official)
        .and_then(|((character, world), official)| official.chat_font(character, world));
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
    typing: Res<crate::keys::Typing>,
    mut contents: Query<(Entity, &mut Content, Option<&Children>)>,
    rendered: Query<(Option<&LineId>, Has<Placeholder>)>,
    mut labels: Query<(&TabLabel, &mut Text, &mut TextColor, Has<Bare>), Without<InputLabel>>,
    mut buttons: Query<
        (
            &TabButton,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (Without<InputBox>, Without<Bare>),
    >,
    mut input_labels: Query<(&mut Text, Has<Bare>), (With<InputLabel>, Without<TabLabel>)>,
    mut input_boxes: Query<&mut BorderColor, (With<InputBox>, Without<TabButton>, Without<Bare>)>,
    (look, options): (Option<Res<ChatLook>>, Res<super::options::OptionsState>),
    (online, messages): (
        Res<super::online::OnlineState>,
        Option<Res<crate::hud::messages::Messages>>,
    ),
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
    for (TabLabel(tab), mut text, mut color, bare) in &mut labels {
        // A skinned tab has no fill to show it is the one shown: its words
        // are lit instead.
        let wanted = if bare && *tab == active {
            theme::INK_BRIGHT
        } else {
            theme::INK
        };
        if color.0 != wanted {
            color.0 = wanted;
        }
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
        background.0 = theme::button(true, *tab == active, *interaction);
        *border = BorderColor::all(if *tab == active {
            theme::EDGE_HOVER
        } else {
            theme::EDGE
        });
    }
    for (mut text, bare) in &mut input_labels {
        text.0 = if state.draft.is_empty() {
            if typing.composing {
                "|"
            } else if bare {
                ""
            } else {
                "Press Enter to chat"
            }
            .into()
        } else if typing.composing {
            format!("{}|", state.draft)
        } else {
            state.draft.clone()
        };
    }
    for mut border in &mut input_boxes {
        *border = BorderColor::all(if typing.composing {
            theme::FOCUS
        } else {
            theme::EDGE
        });
    }
    // Only the active tab's lines exist, as laying out every tab's lines each frame
    // would cost more than redrawing one tab when it is chosen.
    // The skinned chat's lines take the player's colours and font, which
    // can arrive after its first lines: those are drawn again.
    let restyled = look.as_ref().is_some_and(Res::is_changed);
    // Lines drawn with their times, or without, are drawn again once the
    // player turns the times on or off.
    let times = options.options.qol.on(Fix::ShowTimesInChat);
    for (column, mut content, children) in &mut contents {
        if restyled && content.bare {
            content.tab = None;
        }
        if content.times != times {
            content.times = times;
            content.tab = None;
        }
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
        let lines = state.history.arrivals(active);
        // A line reads in its log's words, which tell the player's own
        // speech from others'.
        let player = online
            .world()
            .player()
            .map_or("", |player| player.name.as_str());
        sync_lines(
            &mut commands,
            (column, content.bare),
            children,
            (&lines, look.as_deref().filter(|_| content.bare), times),
            (player, messages.as_deref()),
            &rendered,
        );
    }
}

/// Brings the shown lines up to the history: lines it let go are dropped and new
/// ones appended, so a busy channel never lays its kept lines out again.
fn sync_lines(
    commands: &mut Commands,
    (column, bare): (Entity, bool),
    children: Option<&Children>,
    (lines, look, times): (&[Arrival], Option<&ChatLook>, bool),
    reading: (&str, Option<&crate::hud::messages::Messages>),
    rendered: &Query<(Option<&LineId>, Has<Placeholder>)>,
) {
    let kept: HashSet<u64> = lines.iter().map(|arrival| arrival.id).collect();
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
        (None, true) if !bare => {
            commands.entity(column).with_child((
                Placeholder,
                Text::new("No messages in this channel yet."),
                theme::font(Size::Label),
                TextColor(theme::INK),
            ));
        }
        _ => (),
    }
    // History ids only grow, so every line newer than the last one shown is new.
    commands.entity(column).with_children(|parent| {
        for arrival in lines.iter().filter(|arrival| arrival.id > newest) {
            spawn_line(parent, arrival, (look, times), reading);
        }
    });
}

/// Appends one chat line in the official client's words, as its log
/// writes it without the time (`logs::shown`) unless the player wants the
/// time ([`Fix::ShowTimesInChat`]), with item links clickable; in its
/// channel's colour, the player's and in the player's font where the chat
/// has them.
fn spawn_line(
    parent: &mut ChildSpawnerCommands,
    &Arrival { id, at, line }: &Arrival,
    (look, times): (Option<&ChatLook>, bool),
    (player, messages): (&str, Option<&crate::hud::messages::Messages>),
) {
    let color = look.map_or_else(
        || {
            let [r, g, b] = channel_rgb(line.channel);
            Color::srgb_u8(r, g, b)
        },
        |look| look.color(line),
    );
    let size = look
        .and_then(|look| look.font)
        .map_or(Size::Label, |font| super::skinned::font(Some(font)));
    let mut message = crate::logs::shown(line, player, messages);
    if times {
        stamp(&mut message, at);
    }
    if message.item_links.is_empty() {
        parent.spawn((
            LineId(id),
            Text::new(message.text),
            theme::font(size),
            TextColor(color),
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                ..default()
            },
        ));
    } else {
        let message = super::items::spawn_message(parent, &message, color, size);
        parent.commands().entity(message).insert(LineId(id));
    }
}

/// Starts a line's words with the time it arrived, by the computer's clock,
/// as the chat log writes the time of day ([`Fix::ShowTimesInChat`]); its
/// item links move along with the words.
fn stamp(message: &mut Message, at: chrono::NaiveDateTime) {
    let time = at.format("[%H:%M:%S] ").to_string();
    for link in &mut message.item_links {
        link.text_start += time.len();
        link.text_end += time.len();
    }
    message.text.insert_str(0, &time);
}

/// Why a draft did not go: a mistake in it, or a refusal, which the outbox
/// says itself. The chat says either as a system line.
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
    typing: &mut crate::keys::Typing,
    online: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
) {
    let result = (|| -> Result<(), Unsent> {
        let draft = state.draft.trim().to_owned();
        if let Some(request) = client_request(&draft, state) {
            return Ok(request?);
        }
        if let Some(commands) = game_commands(state.draft.trim(), online, outbox) {
            for command in commands? {
                outbox.send(online.world(), command)?;
            }
            return Ok(());
        }
        let message = outbound(state.active, state.draft.trim())?;
        outbox.send(online.world(), ClientCommand::SendChat(message))?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            state.draft.clear();
            // Sent lines return the keyboard to the game, and the chat to its
            // newest line; a line sent says nothing more.
            typing.composing = false;
            let active = state.active;
            state.views.entry(active).or_default().follow = true;
        }
        // The draft stays, to mend and send again. Each line typed is
        // answered, however soon the same one comes again.
        Err(Unsent::Mistake(mistake)) => state.history.push(system_line(mistake)),
        // The draft stays, to send again.
        Err(Unsent::Refused) => (),
    }
}

/// Sends a game slash command for scripts and buttons; ordinary chat is
/// never sent this way.
pub(super) fn submit_game_command(
    input: &str,
    online: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
) -> Result<(), String> {
    let commands = game_commands(input, online, outbox)
        .ok_or_else(|| format!("{input} is not a game command"))??;
    for command in commands {
        outbox
            .send(online.world(), command)
            .map_err(|refusal| refusal.text().to_owned())?;
    }
    Ok(())
}

/// Runs a button's or a hotbutton's game slash command, as typing it would;
/// a refusal shows in the chat.
pub(super) fn run_game_command(
    command: &str,
    online: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
    chat: &mut ChatState,
) {
    if let Err(reason) = submit_game_command(command, online, outbox) {
        chat.history.push(system_line(reason));
    }
}

/// A slash command the client answers itself, noted for the system that
/// answers it: `/target Name`, a plain `/who`, `/loc`, `/time`, `/log`,
/// `/shownames` or `/raidwindow`. None for any other line.
pub(super) fn client_request(input: &str, state: &mut ChatState) -> Option<Result<(), String>> {
    if let Some(request) = target_request(input) {
        return Some(request.map(|name| state.requested_target = Some(name)));
    }
    if let Some(asked) = super::whereabouts::Asked::typed(input) {
        state.asked = Some(asked);
        return Some(Ok(()));
    }
    if let Some(request) = zone_who_request(input) {
        return Some(request.map(|filter| state.zone_who = Some(filter)));
    }
    if input.eq_ignore_ascii_case("/log") {
        state.log_toggle = true;
        return Some(Ok(()));
    }
    if input.eq_ignore_ascii_case("/raidwindow") {
        state.toggled = Some(super::windows::WindowId::Raid);
        return Some(Ok(()));
    }
    let (command, word) = input.split_once(' ').unwrap_or((input, ""));
    if command.eq_ignore_ascii_case("/shownames") {
        use eq_client_core::names::ShowNames;
        match ShowNames::parse(word) {
            Some(level) => state.show_names = Some(level),
            None => state.show_names_usage = true,
        }
        return Some(Ok(()));
    }
    None
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

/// The name of the player the player targets, themself included; empty
/// when the target is no player.
fn targeted_player(online: &super::online::OnlineState) -> String {
    let world = online.world();
    let Some(id) = world.target().selected else {
        return String::new();
    };
    if world.is_player(id) {
        return world
            .player()
            .map(|player| player.name.clone())
            .unwrap_or_default();
    }
    world
        .spawn(id)
        .filter(|spawn| spawn.state.kind == eq_client_core::SpawnKind::Player)
        .map(|spawn| spawn.state.name.clone())
        .unwrap_or_default()
}

/// A group command by its name: an invitation for the player named, or else
/// the targeted player, which the session refuses when it names no one;
/// joining the group of whoever invited the player last; or leaving the
/// group, or as its leader removing the target or disbanding it, as the
/// server decides, which with an invitation waiting declines it.
fn group_command(
    name: &str,
    words: &str,
    online: &super::online::OnlineState,
    session_id: u64,
) -> ClientCommand {
    match name {
        "invite" => ClientCommand::InviteToGroup {
            session_id,
            name: words
                .split_whitespace()
                .next()
                .map_or_else(|| targeted_player(online), str::to_owned),
        },
        "follow" => ClientCommand::FollowGroup { session_id },
        _ => ClientCommand::Disband { session_id },
    }
}

/// The range `/random` rolls in: 0 to 100 without words, 0 to one number,
/// or between two.
fn random_range(words: &str) -> Result<(u32, u32), String> {
    let numbers = words
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Use /random, /random high or /random low high".to_owned())?;
    match numbers[..] {
        [] => Ok((0, 100)),
        [high] => Ok((0, high)),
        [low, high] => Ok((low, high)),
        _ => Err("Use /random, /random high or /random low high".to_owned()),
    }
}

/// Whose target `/assist` takes: the player named, among those in the zone,
/// or else the player's target.
fn assisted(words: &str, online: &super::online::OnlineState) -> Result<u16, String> {
    let world = online.world();
    let Some(name) = words.split_whitespace().next() else {
        return world
            .target()
            .selected
            .ok_or_else(|| "Target someone to assist first.".to_owned());
    };
    if let Some(player) = world
        .player()
        .filter(|player| player.name.eq_ignore_ascii_case(name))
    {
        return Ok(player.spawn_id);
    }
    world
        .spawns()
        .values()
        .find(|spawn| {
            spawn.state.kind == eq_client_core::SpawnKind::Player
                && spawn.state.name.eq_ignore_ascii_case(name)
        })
        .map(|spawn| spawn.state.spawn_id)
        .ok_or_else(|| format!("No one named {name} is in the zone."))
}

/// The player corpse the player targets.
fn targeted_corpse(online: &super::online::OnlineState) -> Result<u16, String> {
    let world = online.world();
    world
        .target()
        .selected
        .filter(|id| {
            world
                .spawn(*id)
                .is_some_and(|spawn| spawn.state.kind == eq_client_core::SpawnKind::PlayerCorpse)
        })
        .ok_or_else(|| "You must first target a corpse.".to_owned())
}

/// The words of a plain `/who`, which lists the zone's players; None for
/// anything else, `/who all` among it.
pub(super) fn zone_who_request(
    input: &str,
) -> Option<Result<eq_client_core::who::WhoFilter, String>> {
    let command = input.strip_prefix('/')?.trim();
    let (name, words) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    if !name.eq_ignore_ascii_case("who") {
        return None;
    }
    match eq_client_core::who::parse(words) {
        Ok(request) if request.everywhere => None,
        Ok(request) => Some(Ok(request.filter)),
        Err(error) => Some(Err(error)),
    }
}

/// Slash commands about the player and those around them: how `/who`
/// lists the player, dice, emotes, assisting and groups. None for any other
/// line.
fn social_commands(
    name: &str,
    words: &str,
    online: &super::online::OnlineState,
    stamp: &dyn Fn() -> Result<crate::outbox::Stamp, String>,
) -> Option<Result<Vec<ClientCommand>, String>> {
    Some(match name {
        // How `/who` lists the player: away, which takes a message the client
        // does not keep yet, anonymous or roleplaying.
        "afk" => stamp().map(|stamp| {
            vec![ClientCommand::ToggleAway {
                session_id: stamp.session_id,
            }]
        }),
        "anonymous" | "anon" | "roleplay" if words.is_empty() => stamp().map(|stamp| {
            let session_id = stamp.session_id;
            vec![if name == "roleplay" {
                ClientCommand::ToggleRoleplay { session_id }
            } else {
                ClientCommand::ToggleAnonymous { session_id }
            }]
        }),
        // A die from 0 to 100, from 0 to the number given, or between the
        // two given; the server orders them.
        "random" => random_range(words).and_then(|(low, high)| {
            Ok(vec![ClientCommand::Random {
                session_id: stamp()?.session_id,
                low,
                high,
            }])
        }),
        // An emote, in the player's words after their name.
        "emote" | "em" if !words.is_empty() => stamp().map(|stamp| {
            vec![ClientCommand::Emote {
                session_id: stamp.session_id,
                text: words.to_owned(),
            }]
        }),
        // The target of the player named, or else of the player's target.
        "assist" => assisted(words, online).and_then(|spawn_id| {
            Ok(vec![ClientCommand::Assist {
                session_id: stamp()?.session_id,
                spawn_id,
            }])
        }),
        // Groups: an invitation takes a name, the rest no words.
        // Raids: an invitation takes a name or the targeted player, the rest
        // no words.
        "raidinvite" => stamp().map(|stamp| {
            vec![ClientCommand::RaidInvite {
                session_id: stamp.session_id,
                name: words
                    .split_whitespace()
                    .next()
                    .map_or_else(|| targeted_player(online), str::to_owned),
            }]
        }),
        // The lead, handed to the player named or else to the target.
        "makeraidleader" => stamp().map(|stamp| {
            vec![ClientCommand::RaidMakeLeader {
                session_id: stamp.session_id,
                name: words
                    .split_whitespace()
                    .next()
                    .map_or_else(|| targeted_player(online), str::to_owned),
            }]
        }),
        "raidaccept" | "raiddecline" | "raiddisband" if words.is_empty() => stamp().map(|stamp| {
            let session_id = stamp.session_id;
            vec![match name {
                "raidaccept" => ClientCommand::RaidAccept { session_id },
                "raiddecline" => ClientCommand::RaidDecline { session_id },
                _ => ClientCommand::RaidLeave { session_id },
            }]
        }),
        "invite" | "follow" | "disband" if name == "invite" || words.is_empty() => {
            stamp().map(|stamp| vec![group_command(name, words, online, stamp.session_id)])
        }
        _ => return None,
    })
}

/// Slash commands that are game actions rather than chat; None means ordinary chat.
fn game_commands(
    input: &str,
    online: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
) -> Option<Result<Vec<ClientCommand>, String>> {
    let command = input.strip_prefix('/')?.trim();
    let (name, words) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    let name = name.to_ascii_lowercase();
    let stamp = || {
        outbox
            .stamp(online.world())
            .map_err(|refusal| refusal.text().to_owned())
    };
    let posture = |posture| {
        let stamp = stamp()?;
        let player = online.world().player().ok_or("Enter the world first")?;
        Ok(ClientCommand::SetPosture {
            session_id: stamp.session_id,
            spawn_id: player.spawn_id,
            posture,
            created: stamp.created,
        })
    };
    if let Some(commands) = social_commands(&name, words, online, &stamp) {
        return Some(commands);
    }
    Some(match name.as_str() {
        "who" => eq_client_core::who::parse(words).and_then(|request| {
            if !request.everywhere {
                return Err("The zone's /who is listed by chat, not sent".into());
            }
            Ok(vec![ClientCommand::WhoAll {
                session_id: stamp()?.session_id,
                filter: request.filter,
            }])
        }),
        // Who may drag the player's corpses; the session refuses no name.
        "consent" | "deny" => stamp().map(|stamp| {
            vec![ClientCommand::Consent {
                session_id: stamp.session_id,
                name: words.to_owned(),
                given: name == "consent",
            }]
        }),
        // A command to the player's pet, aimed at the target where it takes one.
        "pet" => eq_client_core::pet::command(words)
            .ok_or_else(|| format!("Unknown pet command: {words}"))
            .and_then(|command| {
                Ok(vec![ClientCommand::Pet {
                    session_id: stamp()?.session_id,
                    command,
                    target: online.world().target().selected,
                }])
            }),
        // The rest take no words; with words, they are chat.
        _ if !words.is_empty() => return None,
        // A player's corpse the player targets, pulled close or dragged.
        "corpse" | "corpsedrag" => targeted_corpse(online).and_then(|spawn_id| {
            let session_id = stamp()?.session_id;
            Ok(vec![if name == "corpse" {
                ClientCommand::SummonCorpse {
                    session_id,
                    spawn_id,
                }
            } else {
                ClientCommand::DragCorpse {
                    session_id,
                    spawn_id,
                }
            }])
        }),
        // The targeted corpse, or every corpse when none is targeted.
        "corpsedrop" => stamp().map(|stamp| {
            vec![ClientCommand::DropCorpse {
                session_id: stamp.session_id,
                spawn_id: targeted_corpse(online).ok(),
            }]
        }),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_rolls_to_100_to_one_number_or_between_two() {
        assert_eq!(random_range(""), Ok((0, 100)));
        assert_eq!(random_range("6"), Ok((0, 6)));
        assert_eq!(random_range(" 10  20 "), Ok((10, 20)));
        assert!(random_range("1 2 3").is_err());
        assert!(random_range("six").is_err());
    }

    #[test]
    fn a_refusal_repeated_at_once_is_said_once() {
        let mut state = ChatState::default();
        let said = |state: &ChatState| state.history.lines(ChatTab::All).len();
        let now = std::time::Instant::now();
        state.refuse_at("Too far away".into(), now);
        state.refuse_at("Too far away".into(), now + REPEATED / 2);
        assert_eq!(said(&state), 1);
        state.refuse_at("Too far away".into(), now + REPEATED);
        assert_eq!(said(&state), 2);
        state.refuse_at("Locked".into(), now + REPEATED);
        state.refuse_at("".into(), now + REPEATED);
        assert_eq!(said(&state), 3);
        // A refusal keeps whose words it is in.
        state.refuse_at(Said::official("Closer, please."), now + REPEATED);
        assert_eq!(
            state
                .history
                .lines(ChatTab::All)
                .last()
                .map(|(_, line)| line.source),
            Some(Source::Official)
        );
    }

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
        crate::keys::testing::install(&mut app);
        app.init_resource::<ChatState>()
            .init_resource::<crate::options::OptionsState>()
            .insert_resource(crate::online::OnlineState::new(false))
            .add_systems(Update, refresh);
        app.world_mut().spawn(Content::default());
        app.update();
        let empty = column(&mut app);
        assert_eq!(empty.len(), 1);
        assert!(app.world().get::<Placeholder>(empty[0]).is_some());

        let push = |app: &mut App, count| {
            let mut state = app.world_mut().resource_mut::<ChatState>();
            for _ in 0..count {
                state.history.push(system_line("Synthetic line"));
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
    fn with_times_on_each_line_starts_with_the_time_it_arrived() {
        use eq_client_core::options::Toggle;
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.init_resource::<ChatState>()
            .init_resource::<crate::options::OptionsState>()
            .insert_resource(crate::online::OnlineState::new(false))
            .add_systems(Update, refresh);
        app.world_mut().spawn(Content::default());
        let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 3)
            .and_then(|day| day.and_hms_opt(17, 42, 5))
            .unwrap();
        let auction = crate::preview::chat_lines()
            .into_iter()
            .find(|line| line.channel == ChannelName::Auction)
            .unwrap();
        app.world_mut()
            .resource_mut::<ChatState>()
            .history
            .push_at(auction, at);
        app.update();
        let line = "Preview (auction): WTS Fine Steel Long Sword - send a tell.";
        assert_eq!(shown(&mut app), [line]);
        let times = |app: &mut App, on| {
            app.world_mut()
                .resource_mut::<crate::options::OptionsState>()
                .options
                .set(Toggle::Qol(Fix::ShowTimesInChat), on);
            app.update();
            shown(app)
        };
        // Turned on, the lines already shown are drawn again with their
        // times, and a link keeps the item's words.
        assert_eq!(times(&mut app, true), [format!("[17:42:05] {line}")]);
        let mut links = app
            .world_mut()
            .query::<(&TextSpan, &super::super::items::ItemButton)>();
        let spans: Vec<_> = links
            .iter(app.world())
            .map(|(span, link)| (span.0.clone(), link.0.item_id))
            .collect();
        assert_eq!(spans, [("Fine Steel Long Sword".to_owned(), 42)]);
        // Turned off, they read as the official client's do.
        assert_eq!(times(&mut app, false), [line]);
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
    fn plain_who_lists_the_zone_and_who_all_asks_the_world() {
        assert_eq!(
            zone_who_request("/who wiz"),
            Some(Ok(eq_client_core::who::WhoFilter {
                class: Some(12),
                ..eq_client_core::who::WhoFilter::default()
            }))
        );
        assert_eq!(
            zone_who_request("/WHO"),
            Some(Ok(eq_client_core::who::WhoFilter::default()))
        );
        assert_eq!(zone_who_request("/who all"), None);
        assert_eq!(zone_who_request("/whoever"), None);
        assert!(zone_who_request("/who 1 2 3").unwrap().is_err());
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
                practice_points: None,
                spell_refresh_ms: None,
                memorized_spells: [None; 8],
                size: 6.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                hp_percent: Some(100),
                appearance: eq_client_core::outfit::Appearance::default(),
                listing: eq_client_core::listing::Listing::default(),
                name_parts: eq_client_core::names::NameParts::default(),
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
        // Words after a command that takes none make it chat.
        assert!(game_commands("/sit down", &online, &outbox).is_none());
    }

    #[test]
    fn who_consent_corpse_and_pet_commands_reach_the_game() {
        let mut online = super::super::online::OnlineState::new(true);
        let (queue, _received) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        crate::online::testing::admit(&mut online, 4, crate::online::testing::player(12));
        assert!(matches!(
            game_commands("/who all wiz 50 60", &online, &outbox)
                .unwrap()
                .unwrap()
                .as_slice(),
            [ClientCommand::WhoAll {
                session_id: 4,
                filter: eq_client_core::who::WhoFilter {
                    class: Some(12),
                    levels: Some((50, 60)),
                    ..
                },
            }]
        ));
        assert!(game_commands("/who", &online, &outbox).unwrap().is_err());
        // Consent names a player; the corpse commands want a corpse targeted,
        // but a drop without one drops them all.
        assert!(matches!(
            game_commands("/consent Helper", &online, &outbox)
                .unwrap()
                .unwrap()
                .as_slice(),
            [ClientCommand::Consent { session_id: 4, name, given: true }] if name == "Helper"
        ));
        assert!(matches!(
            game_commands("/deny Helper", &online, &outbox)
                .unwrap()
                .unwrap()
                .as_slice(),
            [ClientCommand::Consent { given: false, .. }]
        ));
        assert_eq!(
            game_commands("/corpsedrag", &online, &outbox).unwrap(),
            Err("You must first target a corpse.".into())
        );
        // A pet command aims at the target; unknown words say so.
        assert!(matches!(
            game_commands("/pet back off", &online, &outbox)
                .unwrap()
                .unwrap()
                .as_slice(),
            [ClientCommand::Pet {
                command: eq_client_core::pets::PetCommand::BackOff,
                ..
            }]
        ));
        assert_eq!(
            game_commands("/pet dance", &online, &outbox).unwrap(),
            Err("Unknown pet command: dance".into())
        );
        assert!(matches!(
            game_commands("/corpsedrop", &online, &outbox)
                .unwrap()
                .unwrap()
                .as_slice(),
            [ClientCommand::DropCorpse { spawn_id: None, .. }]
        ));
    }
    #[test]
    fn sending_a_line_or_an_empty_enter_returns_the_keyboard_to_the_game() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let mut online = super::super::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
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
            let mut state = app.world_mut().resource_mut::<ChatState>();
            state.draft = "hello".into();
            state.views.entry(ChatTab::All).or_default().follow = false;
        }
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.world_mut().write_message(enter.clone());
        app.update();
        assert!(receiver.try_recv().is_ok());
        assert!(!app.world().resource::<crate::keys::Typing>().composing);
        assert_eq!(app.world().resource::<ChatState>().draft, "");
        // A line sent brings the chat back to its newest line.
        assert!(app.world().resource::<ChatState>().views[&ChatTab::All].follow);
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.world_mut().write_message(enter);
        app.update();
        assert!(!app.world().resource::<crate::keys::Typing>().composing);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn a_mistake_is_said_in_the_chat_and_the_draft_stays() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let mut online = super::super::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.init_resource::<ChatState>()
            .init_resource::<super::super::windows::pointer::Wheel>()
            .add_message::<KeyboardInput>()
            .insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, input);
        app.world_mut().resource_mut::<ChatState>().draft = "/tell Friend".into();
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(receiver.try_recv().is_err());
        let state = app.world().resource::<ChatState>();
        assert_eq!(state.draft, "/tell Friend");
        assert_eq!(
            state
                .history
                .lines(ChatTab::All)
                .last()
                .map(|(_, line)| (line.channel, line.message.text.as_str())),
            Some((ChannelName::System, "Use /tell Name message"))
        );
        assert!(app.world().resource::<crate::keys::Typing>().composing);
    }

    #[test]
    fn the_chat_leaves_the_keys_to_the_quantity_windows_number() {
        let mut online = super::super::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.init_resource::<ChatState>()
            .init_resource::<super::super::windows::pointer::Wheel>()
            .add_message::<KeyboardInput>()
            .insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(None))
            .add_systems(Update, input);
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .counting = true;
        for (key_code, logical_key) in [
            (KeyCode::Digit5, Key::Character("5".into())),
            (KeyCode::Enter, Key::Enter),
        ] {
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key,
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        }
        app.update();
        let typing = app.world().resource::<crate::keys::Typing>();
        assert!(typing.counting && !typing.composing);
        assert_eq!(app.world().resource::<ChatState>().draft, "");
    }

    #[test]
    fn the_skinned_chat_takes_the_players_colour_for_a_kind_of_text() {
        let look = ChatLook {
            colors: [(256, [1, 2, 3]), (289, [4, 5, 6])].into(),
            font: None,
        };
        let line = |channel, message_type| ChatLine {
            channel,
            message_type,
            ..system_line("Synthetic line")
        };
        let client = |channel| {
            let [red, green, blue] = channel_rgb(channel);
            Color::srgb_u8(red, green, blue)
        };
        assert_eq!(
            look.color(&line(ChannelName::Say, None)),
            Color::srgb_u8(1, 2, 3)
        );
        // A kind the player set no colour for, and the system lines, which
        // are of many kinds, keep the client's.
        for channel in [ChannelName::Shout, ChannelName::System] {
            assert_eq!(look.color(&line(channel, None)), client(channel));
        }
        // A line the server gave a message type takes the player's colour
        // for that type, whatever its channel.
        assert_eq!(
            look.color(&line(ChannelName::System, Some(289))),
            Color::srgb_u8(4, 5, 6)
        );
        // A type with no colour set, or below the Colors page's kinds, takes
        // its channel's.
        for message_type in [290, 15] {
            assert_eq!(
                look.color(&line(ChannelName::Say, Some(message_type))),
                Color::srgb_u8(1, 2, 3)
            );
            assert_eq!(
                look.color(&line(ChannelName::System, Some(message_type))),
                client(ChannelName::System)
            );
        }
    }

    #[test]
    fn the_skinned_input_shows_only_the_line_and_the_caret() {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .init_resource::<crate::options::OptionsState>()
            .init_resource::<crate::keys::Typing>()
            .insert_resource(crate::online::OnlineState::new(false))
            .add_systems(Update, refresh);
        let bare = app
            .world_mut()
            .spawn((InputLabel, Bare, Text::new("x")))
            .id();
        let own = app.world_mut().spawn((InputLabel, Text::new("x"))).id();
        let text = |app: &App, entity| app.world().get::<Text>(entity).unwrap().0.clone();
        app.update();
        // Idle, the skin's box is empty; the client's own asks for Enter.
        assert_eq!(text(&app, bare), "");
        assert_eq!(text(&app, own), "Press Enter to chat");
        // Typing, the caret alone shows it.
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.update();
        assert_eq!(text(&app, bare), "|");
    }

    #[test]
    fn the_skins_boxes_hold_the_tabs_lines_and_input() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<crate::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()));
        app.world_mut()
            .run_system_once(|mut commands: Commands, mut art: crate::sheets::Art| {
                let tab_frame = eq_client_assets::sidl::FrameLook::default();
                commands.spawn(Node::default()).with_children(|window| {
                    skinned_output(window, Node::default(), (&mut art, Some(&tab_frame)));
                    skinned_input(window, Node::default());
                });
            })
            .unwrap();
        let world = app.world_mut();
        let count = |world: &mut World, filter: fn(EntityRef) -> bool| {
            world
                .iter_entities()
                .filter(|entity| filter(*entity))
                .count()
        };
        assert_eq!(
            count(world, |entity| entity.contains::<TabButton>()),
            ChatTab::ALL.len()
        );
        // Drawn in the skin, with none of the client's own look: its tabs in
        // the skin's tab frame, its input box bare, and an empty tab empty.
        assert_eq!(
            count(world, |entity| entity.contains::<TabButton>()
                && entity.contains::<Bare>()),
            ChatTab::ALL.len()
        );
        assert_eq!(
            count(world, |entity| entity.contains::<InputBox>()
                && entity.contains::<Bare>()),
            1
        );
        assert_eq!(
            count(world, |entity| entity
                .get::<Content>()
                .is_some_and(|content| content.bare)),
            1
        );
        for part in [
            |entity: EntityRef| entity.contains::<Viewport>(),
            |entity: EntityRef| entity.contains::<Content>(),
            |entity: EntityRef| entity.contains::<InputBox>(),
            |entity: EntityRef| entity.contains::<InputLabel>(),
        ] {
            assert_eq!(count(world, part), 1);
        }
        // Enter sends, as in the official client: the skin's window has no
        // Send button or footer.
        assert_eq!(count(world, |entity| entity.contains::<Send>()), 0);
        assert_eq!(count(world, |entity| entity.contains::<Latest>()), 0);
    }

    #[test]
    fn character_selection_enter_does_not_open_chat_or_leak_after_admission() {
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
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
        assert!(!app.world().resource::<crate::keys::Typing>().composing);
        crate::online::testing::admit(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            1,
            crate::online::testing::player(1),
        );
        app.update();
        assert!(!app.world().resource::<crate::keys::Typing>().composing);
    }

    #[test]
    fn pressing_tabs_filters_text_without_losing_history() {
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.init_resource::<ChatState>()
            .init_resource::<crate::options::OptionsState>()
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
        for line in crate::preview::chat_lines() {
            app.world_mut()
                .resource_mut::<ChatState>()
                .history
                .push(line);
        }
        app.update();
        assert_eq!(app.world().resource::<ChatState>().active, ChatTab::Guild);
        // Each line reads in its log's words: without the installed table,
        // in this client's words for who spoke and where.
        let messages = shown(&mut app);
        assert_eq!(
            messages,
            ["Preview (guild): Meet by the tunnel when everyone is ready."]
        );
        app.world_mut()
            .entity_mut(button)
            .insert((TabButton(ChatTab::Auction), Interaction::Pressed));
        app.update();
        let messages = shown(&mut app);
        assert_eq!(
            messages,
            ["Preview (auction): WTS Fine Steel Long Sword - send a tell."]
        );
        // Its link is still the item's words.
        let mut links = app
            .world_mut()
            .query::<(&TextSpan, &super::super::items::ItemButton)>();
        let spans: Vec<_> = links
            .iter(app.world())
            .map(|(span, link)| (span.0.clone(), link.0.item_id))
            .collect();
        assert_eq!(spans, [("Fine Steel Long Sword".to_owned(), 42)]);
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
        crate::keys::testing::install(&mut app);
        app.init_resource::<ChatState>()
            .init_resource::<crate::options::OptionsState>()
            .insert_resource(crate::online::OnlineState::new(false))
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
            for line in crate::preview::chat_lines() {
                state.history.push(line);
            }
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
