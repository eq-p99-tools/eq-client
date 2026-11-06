//! Tabbed chat, with mobile palette colors and independent scroll positions.
use crate::{
    theme::{self, Size},
    windows::WindowId,
};
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    window::PrimaryWindow,
};
use eq_client_core::{
    ClientCommand, OutboundChat,
    chat::{Arrival, ChannelName, ChatHistory, ChatLine, ChatTab, Message, Source, channel_rgb},
    qol::{Fix, Settings},
};
use std::collections::{BTreeMap, HashSet};

mod drag;
mod groups;
pub(super) mod links;
mod paste;

pub(super) use drag::{TabDrag, drag_tabs, paint as paint_drag};
pub(crate) use groups::Groups;

/// Words the client says in the chat, and whose they are: the official
/// client's, read from its string table, or this client's own, which the
/// official client's log never takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Said {
    pub words: eq_client_core::chat::RichText,
    pub source: Source,
    /// The message type the official client gives this line, by which the
    /// chat colours it, where a read names one for the line itself.
    pub message_type: Option<u32>,
}

impl Said {
    /// The official client's words.
    pub(crate) fn official(text: impl Into<String>) -> Self {
        Self {
            words: text.into().into(),
            source: Source::Official,
            message_type: None,
        }
    }

    /// This client's own words.
    pub(crate) fn own(text: impl Into<String>) -> Self {
        Self {
            words: text.into().into(),
            source: Source::Client,
            message_type: None,
        }
    }

    /// The same words, in the colour of message type `kind`.
    pub(crate) const fn typed(mut self, kind: u32) -> Self {
        self.message_type = Some(kind);
        self
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
    let Said {
        words,
        source,
        message_type,
    } = said.into();
    ChatLine {
        channel: ChannelName::System,
        message_type,
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
    pub(crate) groups: Groups,
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
    /// An explicit disconnect to handle before other gameplay input.
    pub departure: Option<super::login::Departure>,
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
    /// The line being typed, for tests outside the chat.
    #[cfg(test)]
    pub(crate) fn draft(&self) -> &str {
        &self.draft
    }

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
    /// The chat scrolled `offset` logical pixels down its lines, away from
    /// the newest, for tests of its place.
    pub(crate) fn scrolled_for_tests(offset: f32) -> Self {
        let mut state = Self::default();
        let view = state.views.entry(ChatTab::All).or_default();
        view.follow = false;
        view.offset = offset;
        state
    }

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
/// The chat surface a control or text box belongs to.
#[derive(Component, Clone, Copy)]
pub(super) struct InGroup(WindowId);

fn group(marker: Option<&InGroup>) -> WindowId {
    marker.map_or(WindowId::Chat, |group| group.0)
}

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
pub(super) struct TabButton(pub(super) ChatTab, WindowId);
#[derive(Component)]
pub(super) struct TabLabel(ChatTab, WindowId);
#[derive(Component)]
pub(super) struct Latest(WindowId);
#[derive(Component)]
pub(super) struct InputBox;
#[derive(Component)]
pub(super) struct InputLabel;
#[derive(Component)]
pub(super) struct Send(WindowId);
#[derive(Component)]
pub(crate) struct CloseGroup(pub(crate) WindowId);
/// A part of the chat drawn in the skin: it keeps the skin's look, with no
/// fill, border or words of the client's own.
#[derive(Component)]
pub(super) struct Bare;

/// Creates a clipped scrollback window; changing tabs never destroys stored
/// messages. Where the installed skin has a chat window, it is drawn from
/// the skin instead, with the same tabs, lines and input in its boxes
/// ([`skinned_output`], [`skinned_input`]).
pub(super) fn spawn(commands: &mut Commands) {
    spawn_group(commands, WindowId::Chat);
}

fn spawn_group(commands: &mut Commands, id: WindowId) -> Entity {
    let detached = id != WindowId::Chat;
    let frame = super::windows::frame(
        commands,
        id,
        Node {
            width: px(420),
            max_width: if detached { Val::Auto } else { percent(95) },
            height: px(202),
            max_height: if detached { Val::Auto } else { percent(45) },
            padding: UiRect::all(px(8)),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            ..default()
        },
    );
    commands.entity(frame).insert((
        super::hud::HudRoot,
        Panel,
        InGroup(id),
        super::windows::pointer::TakesWheel,
    ));
    if detached {
        // The fallback uses flex layout; the skin will replace the sizing
        // scope with its own when it supplies authored anchors.
        commands.entity(frame).queue(|mut entity: EntityWorldMut| {
            entity
                .get_mut::<super::windows::Frame>()
                .unwrap()
                .sizing(Some(u64::MAX), Vec2::new(420.0, 202.0));
        });
        super::windows::drag_anywhere(commands, frame, id);
    }
    commands.entity(frame).with_children(|root| {
        tab_strip(root, id, false, None);
        lines(root, false, id);
        root.spawn(Node {
            column_gap: px(5),
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|row| {
            typing_box(
                row,
                id,
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    padding: UiRect::axes(px(7), px(5)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
            );
            theme::button_with(row, Send(id), "Send", Size::Body);
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
            theme::button_with(footer, Latest(id), "Latest", Size::Small);
        });
    });
    frame
}

/// The close box and Escape both return detached tabs to the main dock.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn close_groups(
    mut state: ResMut<ChatState>,
    buttons: crate::clicks::Clicks<CloseGroup>,
    escape: Res<crate::escape::Escape>,
) {
    for CloseGroup(id) in buttons.iter() {
        state.groups.close(*id);
    }
    if let crate::escape::Escape::Close(id @ WindowId::ChatGroup(_)) = *escape {
        state.groups.close(id);
    }
}

/// Detached groups exist only while the world HUD is present. Their
/// membership and tab views survive rebuilding it after zoning.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn reconcile(
    mut commands: Commands,
    state: Res<ChatState>,
    frames: Query<(Entity, &WindowId), With<Panel>>,
    mut stack: ResMut<super::windows::Stack>,
) {
    stack.sync_chat_groups(&state.groups);
    if !frames.iter().any(|(_, id)| *id == WindowId::Chat) {
        return;
    }
    for (entity, id) in &frames {
        if !state.groups.contains(*id) {
            commands.entity(entity).despawn();
        }
    }
    for id in state.groups.detached() {
        if !frames.iter().any(|(_, frame)| *frame == id) {
            let frame = spawn_group(&mut commands, id);
            commands.entity(frame).insert(GlobalZIndex(stack.z(id)));
        }
    }
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
    id: WindowId,
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
            let skin = tab_frame.map(|look| (look.clone(), art.drawing_skin()));
            tab_strip(output, id, true, skin);
            viewport = lines(output, true, id);
        });
    viewport
}

/// A retained tab strip. Only membership changes rebuild its controls.
#[derive(Component)]
pub(super) struct TabStrip {
    id: WindowId,
    painted: Option<Vec<ChatTab>>,
    /// Drawn inside skinned output, which may have no authored tab frame.
    bare: bool,
    skin: Option<(eq_client_assets::sidl::FrameLook, Option<String>)>,
}

fn tab_strip(
    parent: &mut ChildSpawnerCommands,
    id: WindowId,
    bare: bool,
    skin: Option<(eq_client_assets::sidl::FrameLook, Option<String>)>,
) {
    let gap = if skin.is_some() { 2.0 } else { 3.0 };
    parent.spawn((
        TabStrip {
            id,
            painted: None,
            bare,
            skin,
        },
        super::windows::KeepsPress,
        Interaction::None,
        BackgroundColor(Color::NONE),
        Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(gap),
            row_gap: px(gap),
            min_height: px(20),
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

/// Creates the tabs with the strip's skin and visual order after a move.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn sync_tabs(
    mut commands: Commands,
    state: Res<ChatState>,
    mut strips: Query<(Entity, &mut TabStrip)>,
    mut art: crate::sheets::Art,
) {
    for (entity, mut strip) in &mut strips {
        let Some(group) = state.groups.get(strip.id) else {
            continue;
        };
        if strip.painted.as_deref() == Some(group.tabs.as_slice()) {
            continue;
        }
        commands
            .entity(entity)
            .despawn_children()
            .with_children(|parent| {
                if group.tabs.is_empty() && !strip.bare {
                    parent.spawn(theme::text("Drop a chat tab here", Size::Small, theme::INK));
                }
                let art_skin = strip.skin.as_ref().and_then(|(_, skin)| skin.clone());
                art.draw_in(art_skin);
                for &tab in &group.tabs {
                    let mut cell = parent.spawn((Button, TabButton(tab, strip.id)));
                    if let Some((look, _)) = &strip.skin {
                        cell.insert(Bare);
                        super::skinned::frame_tab(
                            &mut cell,
                            &mut art,
                            look,
                            Node {
                                height: px(super::skinned::WORD_TAB_HEIGHT),
                                align_items: AlignItems::Center,
                                flex_shrink: 0.0,
                                ..default()
                            },
                        );
                    } else {
                        cell.insert((
                            Node {
                                padding: UiRect::axes(px(3), px(3)),
                                border: UiRect::bottom(px(2)),
                                ..default()
                            },
                            BackgroundColor(theme::BUTTON),
                            BorderColor::all(theme::EDGE),
                        ));
                    }
                    cell.with_children(|button| {
                        let mut words = button.spawn((
                            TabLabel(tab, strip.id),
                            theme::text(tab.label(), Size::Small, theme::INK),
                        ));
                        if strip.skin.is_some() {
                            words.insert(Bare);
                        }
                    });
                }
                art.draw_in(None);
            });
        strip.painted = Some(group.tabs.clone());
    }
}

/// The skin's input box, with a top edge where its layout has no divider.
pub(super) fn skinned_input(
    parent: &mut ChildSpawnerCommands,
    node: Node,
    divider: bool,
    id: WindowId,
) {
    let divider = if divider { 1.0 } else { 0.0 };
    parent
        .spawn((
            Button,
            InputBox,
            InGroup(id),
            Bare,
            BorderColor::all(theme::EDGE_LIGHT.with_alpha(0.6)),
            Node {
                border: UiRect::top(px(divider)),
                padding: UiRect {
                    top: px(1.0 - divider),
                    ..UiRect::axes(px(4), px(1))
                },
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..node
            },
        ))
        .with_children(|input| {
            input.spawn((
                InputLabel,
                InGroup(id),
                Bare,
                Text::new(""),
                theme::font(Size::Body),
                TextColor(theme::INK_BRIGHT),
            ));
        });
}

/// The active tab's lines, which scroll on their own; drawn in the skin
/// (`bare`), an empty tab is an empty box.
fn lines(parent: &mut ChildSpawnerCommands, bare: bool, id: WindowId) -> Entity {
    parent
        .spawn((
            Viewport,
            InGroup(id),
            super::windows::KeepsPress,
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
                InGroup(id),
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
fn typing_box(parent: &mut ChildSpawnerCommands, id: WindowId, node: Node) {
    parent
        .spawn((
            Button,
            InputBox,
            InGroup(id),
            node,
            BackgroundColor(theme::WELL),
            BorderColor::all(theme::EDGE),
        ))
        .with_children(|input| {
            input.spawn((
                InputLabel,
                InGroup(id),
                Text::new("Press Enter to chat"),
                theme::font(Size::Body),
                TextColor(theme::INK),
            ));
        });
}

/// Routes tab presses and wheel input, reserving the wheel from the camera over chat.
#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::too_many_lines
)]
pub(super) fn input(
    mut state: ResMut<ChatState>,
    mut online: ResMut<super::online::OnlineState>,
    (outbox, options): (
        Res<crate::outbox::Outbox>,
        Option<Res<crate::options::OptionsState>>,
    ),
    mut typing: ResMut<crate::keys::Typing>,
    (mut keyboard, mouse): (MessageReader<KeyboardInput>, Res<ButtonInput<MouseButton>>),
    keys: Option<Res<ButtonInput<Key>>>,
    mut clipboard: Option<ResMut<bevy::clipboard::Clipboard>>,
    mut paste: Local<paste::Paste>,
    (windows, pointer): (
        Query<&Window, With<PrimaryWindow>>,
        super::windows::pointer::PointerUi,
    ),
    panels: Query<(Entity, Option<&InGroup>), With<Panel>>,
    wheel: Res<super::windows::pointer::Wheel>,
    (tabs, latest): (
        crate::clicks::Clicks<TabButton>,
        crate::clicks::Clicks<Latest>,
    ),
    input_box: Query<(&Interaction, Option<&InGroup>), (With<InputBox>, Changed<Interaction>)>,
    send: crate::clicks::Clicks<Send>,
    viewports: Query<(&ComputedNode, Option<&InGroup>), With<Viewport>>,
    drag: Option<Res<TabDrag>>,
) {
    typing.escape_consumed = drag.as_ref().is_some_and(|drag| drag.escaped);
    if typing.editing_script {
        paste.clear();
        keyboard.clear();
        typing.composing = false;
        state.hovered = false;
        return;
    }
    // Character selection owns keyboard input until the zone admits the player.
    if online.enabled && online.world().session_id().is_none() {
        paste.clear();
        keyboard.clear();
        typing.composing = false;
        state.hovered = false;
        return;
    }
    state.hovered = drag.as_deref().is_some_and(TabDrag::dragging)
        || windows
            .single()
            .ok()
            .and_then(Window::physical_cursor_position)
            .and_then(|cursor| pointer.top_at(cursor))
            .is_some_and(|top| {
                panels
                    .iter()
                    .any(|(panel, _)| pointer.descends_from(top, panel))
            });
    // A click on the game, anywhere outside the chat window, acts as any
    // click does and gives the keyboard back to the game; the line keeps
    // what was typed, which Enter brings back (Titanium per Adam, 10-19;
    // EQMac inferred, recording checklist 6.243).
    if typing.composing
        && !state.hovered
        && mouse.any_just_pressed([MouseButton::Left, MouseButton::Right])
    {
        typing.composing = false;
    }
    for TabButton(tab, id) in tabs.iter() {
        state.groups.select(*id, *tab);
    }
    if let Some((_, marker)) = input_box
        .iter()
        .find(|(value, _)| **value == Interaction::Pressed)
        && state.groups.focus(group(marker))
    {
        typing.composing = true;
        typing.counting = false;
    }
    let send = send
        .iter()
        .next()
        .is_some_and(|Send(id)| state.groups.focus(*id));
    // Local text fields keep their keys out of chat.
    if typing.counting || typing.map_search || typing.spell_search_claimed {
        keyboard.clear();
        paste.clear();
    }
    let events: Vec<_> = keyboard
        .read_with_id()
        .filter(|(event, id)| {
            !(typing.spell_search_captured(*id)
                || typing.escape_consumed && event.logical_key == Key::Escape)
        })
        .map(|(event, _)| event.clone())
        .collect();
    let empty = ButtonInput::default();
    let submit = compose(
        &mut state,
        &mut typing,
        &events,
        keys.as_deref().unwrap_or(&empty),
        clipboard.as_deref_mut(),
        &mut paste,
        online.world().session_id(),
    ) || send;
    if submit {
        paste.clear();
        if state.draft.trim().is_empty() {
            // Enter on an empty line closes the input, as in the official client.
            typing.composing = false;
        } else {
            let qol = options
                .as_ref()
                .map_or_else(Settings::default, |options| options.options.qol);
            submit_draft(&mut state, &mut typing, &mut online, &outbox, &qol);
        }
    }
    for Latest(id) in latest.iter() {
        if let Some(tab) = state.groups.active(*id) {
            state.views.entry(tab).or_default().follow = true;
        }
    }
    if wheel.pixels != 0.0
        && let Some((_, marker)) = wheel.surface.and_then(|surface| panels.get(surface).ok())
        && let Some(active) = state.groups.active(group(marker))
        && let Some((node, _)) = viewports
            .iter()
            .find(|(_, owner)| group(*owner) == group(marker))
        && !node.is_empty()
    {
        let maximum =
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        let view = state.views.entry(active).or_default();
        if view.follow {
            view.offset = maximum;
        }
        view.offset = (view.offset.min(maximum) - wheel.pixels).clamp(0.0, maximum);
        view.follow = view.offset >= maximum - 1.0;
    }
}

/// Edit one chat draft with ordered modifier snapshots; clipboard input never submits.
fn compose(
    state: &mut ChatState,
    typing: &mut crate::keys::Typing,
    events: &[KeyboardInput],
    keys: &ButtonInput<Key>,
    mut clipboard: Option<&mut bevy::clipboard::Clipboard>,
    paste: &mut paste::Paste,
    session: Option<u64>,
) -> bool {
    let mut submit = false;
    let modifiers = crate::keys::modifiers::snapshots(events, keys);
    for (event, held) in events.iter().zip(modifiers) {
        if !event.state.is_pressed() {
            continue;
        }
        if crate::keys::modifiers::chorded(held) {
            if typing.composing
                && held[0]
                && !held[2]
                && !held[3]
                && matches!(&event.logical_key, Key::Character(key) if key.eq_ignore_ascii_case("v"))
                && let Some(clipboard) = clipboard.as_mut()
            {
                paste.request(clipboard.fetch_text(), session, &state.draft);
            }
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
            // A slash opens the line with only itself typed, so a command is
            // one keystroke away, as in the official client (Titanium checked;
            // EQMac inferred, recording checklist 6.241). A line left with
            // Escape gives way to it, as Enter alone brings that line back.
            Key::Character(value) if !typing.composing && value.as_str() == "/" => {
                typing.composing = true;
                "/".clone_into(&mut state.draft);
            }
            // The line types what the key types, as the window reports it:
            // the space bar is a named key, not a character. Chords are
            // handled above using their event-time modifiers.
            _ if typing.composing && state.draft.len() < 512 => {
                let text = event.text.as_deref().unwrap_or_default();
                state.draft.extend(text.chars().filter(|c| !c.is_control()));
                let boundary = state.draft.floor_char_boundary(512);
                state.draft.truncate(boundary);
            }
            _ => (),
        }
    }
    if paste
        .apply(&mut state.draft, typing.composing, session)
        .is_err()
    {
        state.refuse("Could not read clipboard text.");
    }
    submit
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
    mut contents: Query<(
        Entity,
        &mut Content,
        Option<&Children>,
        Option<&InGroup>,
        Option<&ComputedNode>,
    )>,
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
    mut input_labels: Query<
        (&mut Text, Has<Bare>, Option<&InGroup>),
        (With<InputLabel>, Without<TabLabel>),
    >,
    mut input_boxes: Query<
        (&mut BorderColor, Option<&InGroup>),
        (With<InputBox>, Without<TabButton>, Without<Bare>),
    >,
    (look, options): (Option<Res<ChatLook>>, Res<super::options::OptionsState>),
    (online, messages): (
        Res<super::online::OnlineState>,
        Option<Res<crate::hud::messages::Messages>>,
    ),
) {
    let revision = state.history.revision();
    let ChatState { groups, views, .. } = &mut *state;
    for (_, _, _, marker, node) in &contents {
        if node.is_some_and(ComputedNode::is_empty) {
            continue;
        }
        let Some(group) = groups.get(group(marker)) else {
            continue;
        };
        let Some(active) = group.active else { continue };
        if views.entry(active).or_default().follow {
            if active == ChatTab::All {
                for &tab in &group.tabs {
                    views.entry(tab).or_default().seen = revision;
                }
            } else {
                views.entry(active).or_default().seen = revision;
            }
        }
    }
    for (TabLabel(tab, id), mut text, mut color, bare) in &mut labels {
        // A skinned tab has no fill to show it is the one shown: its words
        // are lit instead.
        let wanted = if bare && Some(*tab) == state.groups.active(*id) {
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
    for (TabButton(tab, id), interaction, mut background, mut border) in &mut buttons {
        let active = Some(*tab) == state.groups.active(*id);
        background.0 = theme::button(true, active, *interaction);
        *border = BorderColor::all(if active {
            theme::EDGE_HOVER
        } else {
            theme::EDGE
        });
    }
    for (mut text, bare, marker) in &mut input_labels {
        let id = group(marker);
        let owner = state.groups.owner() == id;
        let wanted = if owner && typing.composing {
            format!("{}|", state.draft)
        } else if owner && !state.draft.is_empty() {
            state.draft.clone()
        } else if bare || state.groups.active(id).is_none() {
            String::new()
        } else {
            "Press Enter to chat".into()
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    for (mut border, marker) in &mut input_boxes {
        *border = BorderColor::all(
            if typing.composing && state.groups.owner() == group(marker) {
                theme::FOCUS
            } else {
                theme::EDGE
            },
        );
    }
    // Each group only lays out its active tab's lines.
    // The skinned chat's lines take the player's colours and font, which
    // can arrive after its first lines: those are drawn again.
    let restyled = look.as_ref().is_some_and(Res::is_changed);
    // Lines drawn with their times, or without, are drawn again once the
    // player turns the times on or off.
    let times = options.options.qol.on(Fix::ShowTimesInChat);
    for (column, mut content, children, marker, _) in &mut contents {
        let Some(active) = state.groups.active(group(marker)) else {
            if content.tab.take().is_some() {
                commands.entity(column).despawn_children();
            }
            continue;
        };
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
/// time ([`Fix::ShowTimesInChat`]), with item and web links clickable; in its
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
    if message.item_links.is_empty() && !links::contains_url(&message.text) {
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
    online: &mut super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
    qol: &Settings,
) {
    let result = (|| -> Result<(), Unsent> {
        let draft = typed_command(state.draft.trim()).into_owned();
        if let Some(request) = departure_request(&draft) {
            state.departure = Some(request?);
            return Ok(());
        }
        if let Some(request) = client_request(&draft, state) {
            return Ok(request?);
        }
        if let Some(commands) = game_commands(&draft, online, outbox, qol) {
            for command in commands? {
                outbox.send(online.world(), command)?;
            }
            if let Some(to) = camp_destination(&draft) {
                online.camp_to(to);
            }
            return Ok(());
        }
        let active = state
            .groups
            .active(state.groups.owner())
            .unwrap_or_default();
        let message = outbound(active, state.draft.trim())?;
        outbox.send(online.world(), ClientCommand::SendChat(message))?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            state.draft.clear();
            // Sent lines return the keyboard to the game, and the chat to its
            // newest line; a line sent says nothing more.
            typing.composing = false;
            if let Some(active) = state.groups.active(state.groups.owner()) {
                state.views.entry(active).or_default().follow = true;
            }
        }
        // The draft stays, to mend and send again. Each line typed is
        // answered, however soon the same one comes again.
        Err(Unsent::Mistake(mistake)) => state.history.push(system_line(mistake)),
        // The draft stays, to send again.
        Err(Unsent::Refused) => (),
    }
}

/// These local commands never go through chat, the game queue, or camp.
fn departure_request(input: &str) -> Option<Result<super::login::Departure, String>> {
    use super::login::Departure;
    let command = input.strip_prefix('/')?;
    let (name, words) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    let (destination, command) = match name.to_ascii_lowercase().as_str() {
        "ex" | "exi" | "exit" => (Departure::Desktop, "/exit"),
        "q" | "qu" | "qui" | "quit" => (Departure::Login, "/quit"),
        _ => return None,
    };
    Some(if words.trim().is_empty() {
        Ok(destination)
    } else {
        Err(format!("Use {command} without arguments"))
    })
}

/// Sends a game slash command for scripts and buttons; ordinary chat is
/// never sent this way.
pub(super) fn submit_game_command(
    input: &str,
    online: &mut super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
    qol: &Settings,
) -> Result<(), String> {
    let commands = game_commands(input, online, outbox, qol)
        .ok_or_else(|| format!("{input} is not a game command"))??;
    for command in commands {
        outbox
            .send(online.world(), command)
            .map_err(|refusal| refusal.text().to_owned())?;
    }
    if let Some(to) = camp_destination(input) {
        online.camp_to(to);
    }
    Ok(())
}

/// Runs a button's or a hotbutton's game slash command, as typing it would;
/// a refusal shows in the chat once, though the outbox says it too
/// (`outbox::show`).
pub(super) fn run_game_command(
    command: &str,
    online: &mut super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
    chat: &mut ChatState,
    qol: &Settings,
) {
    if let Err(reason) = submit_game_command(command, online, outbox, qol) {
        chat.refuse(reason);
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
        "follow" => ClientCommand::FollowGroup {
            session_id,
            inviter: online
                .world()
                .group_invitation()
                .unwrap_or_default()
                .to_owned(),
            invitation_id: online.world().group_invitation_id(),
        },
        _ => match online.world().group_invitation() {
            Some(inviter) => ClientCommand::DeclineGroup {
                session_id,
                inviter: inviter.to_owned(),
                invitation_id: online.world().group_invitation_id(),
            },
            None => ClientCommand::Disband { session_id },
        },
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

/// A typed line as the commands read it: a bare `/`, alone or with words,
/// is `/who` with the same words, as in the official client (the project
/// owner's report, 11-01). That the words after it are `/who`'s is
/// inferred (checklist 6.328). Every other line is unchanged.
fn typed_command(input: &str) -> std::borrow::Cow<'_, str> {
    match input.strip_prefix('/') {
        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
            format!("/who{rest}").into()
        }
        _ => input.into(),
    }
}

/// Where a typed `/camp` ends, for a line that is one and whose words
/// [`game_commands`] took; None for any other line.
fn camp_destination(input: &str) -> Option<eq_client_core::camp::CampTo> {
    let command = input.strip_prefix('/')?.trim();
    let (name, words) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    name.eq_ignore_ascii_case("camp")
        .then(|| eq_client_core::camp::destination(words).ok())
        .flatten()
}

/// Builds the Bard's explicit song stop after checking the input and admission.
fn stop_song_command(
    words: &str,
    world: &eq_client_core::world::ClientWorld,
    stamp: impl FnOnce() -> Result<crate::outbox::Stamp, String>,
) -> Result<ClientCommand, String> {
    if !words.trim().is_empty() {
        return Err("Use /stopsong without arguments".into());
    }
    let player = world.player().ok_or("Enter the world first")?;
    if player.class != Some(8) {
        return Err("Only Bards can stop a song".into());
    }
    let stamp = stamp()?;
    Ok(ClientCommand::StopSong {
        session_id: stamp.session_id,
        created: stamp.created,
    })
}

/// Builds a pet order for the current admission and target.
fn pet_order(
    words: &str,
    world: &eq_client_core::world::ClientWorld,
    stamp: impl FnOnce() -> Result<crate::outbox::Stamp, String>,
) -> Result<ClientCommand, String> {
    let command = eq_client_core::pet::command(words)
        .ok_or_else(|| format!("Unknown pet command: {words}"))?;
    Ok(ClientCommand::Pet {
        session_id: stamp()?.session_id,
        command,
        target: world.target().selected,
    })
}

/// Slash commands that are game actions rather than chat; None means ordinary chat.
fn game_commands(
    input: &str,
    online: &super::online::OnlineState,
    outbox: &crate::outbox::Outbox,
    qol: &Settings,
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
        "stopsong" => stop_song_command(words, online.world(), stamp).map(|command| vec![command]),
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
        "pet" => pet_order(words, online.world(), stamp).map(|command| vec![command]),
        // The ordinary camp sits and prepares first. Immediate exit is an
        // explicit opt-in, still completed by the session's logout reply.
        "camp" => eq_client_core::camp::destination(words).and_then(|_| {
            if qol.on(Fix::ExitAtOnce) {
                let stamp = stamp()?;
                return Ok(vec![ClientCommand::ExitAtOnce {
                    session_id: stamp.session_id,
                    created: stamp.created,
                }]);
            }
            let sit = posture(eq_client_core::Posture::Sitting)?;
            let stamp = stamp()?;
            Ok(vec![
                sit,
                ClientCommand::Camp {
                    session_id: stamp.session_id,
                    created: stamp.created,
                },
            ])
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
        "ranged" => {
            if words.trim().is_empty() {
                stamp().and_then(|stamp| {
                    super::combat::ranged_command(online.world(), stamp)
                        .map(|command| vec![command])
                })
            } else {
                Err("Use /ranged without arguments".into())
            }
        }
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
    mut query: Query<(&ComputedNode, &mut ScrollPosition, Option<&InGroup>), With<Viewport>>,
) {
    for (node, mut scroll, marker) in &mut query {
        let view = state
            .groups
            .active(group(marker))
            .and_then(|tab| state.views.get(&tab))
            .copied()
            .unwrap_or_default();
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
    use bevy::{
        ecs::system::RunSystemOnce,
        text::{
            ComputedTextBlock, LineBreak, RunGeometry, TextBounds, TextLayoutInfo, TextPipeline,
        },
        ui::ComputedStackIndex,
    };

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
    fn a_bare_slash_is_who_with_the_same_words() {
        assert_eq!(typed_command("/"), "/who");
        assert_eq!(typed_command("/ wiz"), "/who wiz");
        assert_eq!(typed_command("/ all wiz 50 60"), "/who all wiz 50 60");
        for unchanged in ["//", "/say hi", "/who", "hello", "/camp desktop"] {
            assert_eq!(typed_command(unchanged), unchanged);
        }
        assert_eq!(
            zone_who_request(&typed_command("/")),
            Some(Ok(eq_client_core::who::WhoFilter::default()))
        );
        assert_eq!(
            zone_who_request(&typed_command("/ wiz")),
            Some(Ok(eq_client_core::who::WhoFilter {
                class: Some(12),
                ..eq_client_core::who::WhoFilter::default()
            }))
        );

        let mut online = super::super::online::OnlineState::new(true);
        let (queue, received) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        crate::online::testing::admit(&mut online, 4, crate::online::testing::player(12));
        assert!(matches!(
            game_commands(
                &typed_command("/ all wiz 50 60"),
                &online,
                &outbox,
                &Settings::default()
            )
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

        // Typed, a bare `/` lists the zone as `/who` does, with no refusal.
        let mut state = ChatState::default();
        let mut typing = crate::keys::Typing::default();
        "/".clone_into(&mut state.draft);
        submit_draft(
            &mut state,
            &mut typing,
            &mut online,
            &outbox,
            &Settings::default(),
        );
        assert_eq!(
            state.zone_who,
            Some(eq_client_core::who::WhoFilter::default())
        );
        assert!(state.history.lines(ChatTab::All).is_empty());
        assert_eq!(state.draft, "");
        assert!(received.try_recv().is_err());
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
    fn departure_requires_an_exact_alias_without_arguments() {
        use crate::login::Departure;

        for name in ["/ex", "/EXI", "/exit"] {
            assert_eq!(departure_request(name), Some(Ok(Departure::Desktop)));
        }
        for name in ["/q", "/Qu", "/qui", "/QUIT"] {
            assert_eq!(departure_request(name), Some(Ok(Departure::Login)));
        }
        for unrelated in [
            "/e",
            "/exits",
            "/exitnow",
            "/quick",
            "/quitter",
            "/camp desktop",
        ] {
            assert_eq!(departure_request(unrelated), None);
        }
        for invalid in ["/exit desktop", "/qu now", "/QUIT\tnow"] {
            assert!(matches!(departure_request(invalid), Some(Err(_))));
        }
    }

    /// The front end must move on before this worker finishes.
    struct Stopping(std::sync::Arc<std::sync::atomic::AtomicBool>);

    impl crate::login::Worker for Stopping {
        fn stop(&self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        fn finished(&self) -> bool {
            false
        }
        fn reason(&self) -> Option<String> {
            None
        }
        fn closed(&self) -> bool {
            false
        }
    }

    fn enter_draft(app: &mut App, draft: &str) {
        let world = app.world_mut();
        draft.clone_into(&mut world.resource_mut::<ChatState>().draft);
        world.resource_mut::<crate::keys::Typing>().composing = true;
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        world.write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }

    #[test]
    fn stop_song_is_a_scoped_bard_action_through_the_game_command_path() {
        let mut online = super::super::online::OnlineState::new(true);
        let (queue, received) = std::sync::mpsc::sync_channel(2);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        let qol = Settings::default();
        let mut player = super::super::online::testing::player(7);
        super::super::online::testing::admit(&mut online, 4, player.clone());
        assert!(submit_game_command("/stopsong", &mut online, &outbox, &qol).is_err());

        player.class = Some(8);
        super::super::online::testing::admit(&mut online, 5, player);
        assert!(submit_game_command("/stopsong now", &mut online, &outbox, &qol).is_err());
        assert!(received.try_recv().is_err());
        submit_game_command("/StopSong", &mut online, &outbox, &qol).unwrap();
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::StopSong { session_id: 5, .. })
        ));
    }

    #[test]
    fn typed_departure_detaches_the_admission_before_login_input_or_exit() {
        use crate::login::{
            FrontEnd, Screen, Session,
            testing::{Fake, Heard, server},
        };
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc::{TryRecvError, sync_channel},
        };
        for (alias, closes) in [("/qu", false), ("/EXI", true)] {
            let heard = Arc::new(Mutex::new(Heard::default()));
            let stopped = Arc::new(AtomicBool::new(false));
            let (news, updates) = sync_channel(4);
            let (commands, queue) = sync_channel(4);
            let (front, updates, commands) = FrontEnd::launched(
                Box::new(Fake {
                    servers: vec![server("Example", "remembered")],
                    heard: heard.clone(),
                }),
                Some(Session {
                    inventory_move_rules: None,
                    spell_provider: eq_client_core::spell_tables::Binding::new(
                        eq_client_core::spell_tables::Policy::Installed,
                    ),
                    updates,
                    commands,
                    worker: Box::new(Stopping(stopped.clone())),
                }),
            );
            let mut online = crate::online::OnlineState::new(true);
            crate::online::testing::admit(&mut online, 4, crate::online::testing::player(12));
            online.camp_to(eq_client_core::camp::CampTo::Desktop);
            crate::online::testing::news(
                &mut online,
                [eq_client_core::WorldEvent::Camp(
                    eq_client_core::CampStatus::LoggingOut,
                )],
            );
            assert_eq!(front.screen(&online), Screen::World);

            let mut app = crate::testing::app();
            app.insert_resource(front)
                .insert_resource(online)
                .insert_resource(crate::online::Updates(Mutex::new(updates)))
                .insert_resource(crate::outbox::Outbox::new(commands))
                .init_resource::<Assets<Mesh>>()
                .init_resource::<Assets<StandardMaterial>>()
                .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
                .add_message::<KeyboardInput>()
                .add_message::<AppExit>()
                .add_systems(
                    Update,
                    (
                        input,
                        crate::login::depart,
                        crate::login::form,
                        crate::online::leave,
                    )
                        .chain(),
                );
            let old_scene = app.world_mut().spawn(crate::SceneEntity).id();
            app.world_mut()
                .resource_mut::<crate::options::OptionsState>()
                .options
                .qol
                .set(Fix::ExitAtOnce, !closes);
            enter_draft(&mut app, alias);

            let world = app.world();
            let online = world.resource::<crate::online::OnlineState>();
            let front = world.resource::<FrontEnd>();
            assert!(stopped.load(Ordering::SeqCst));
            assert!(!front.running());
            assert_eq!(front.screen(online), Screen::Login);
            assert_eq!(front.account, "remembered");
            assert_eq!(
                heard.lock().unwrap().connects,
                [] as [(usize, String, String); 0]
            );
            assert!(online.world().ended());
            assert_eq!(online.world().session_id(), None);
            assert!(online.world().player().is_none());
            assert!(online.world().camp_completion().is_none());
            assert!(world.get_entity(old_scene).is_err());
            assert!(world.resource::<crate::keys::Typing>().logging_in);
            assert!(!world.resource::<crate::keys::Typing>().composing);
            assert!(matches!(queue.try_recv(), Err(TryRecvError::Disconnected)));
            assert!(
                news.try_send(eq_client_core::WorldUpdate::Connection(
                    eq_client_core::world::Link::Ended,
                ))
                .is_err()
            );
            assert_eq!(
                world.resource::<Messages<AppExit>>().len(),
                usize::from(closes)
            );
        }
    }

    #[test]
    fn camp_sits_first_and_game_commands_never_become_chat() {
        let mut online = super::super::online::OnlineState::new(true);
        let (queue, _received) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        assert!(game_commands("/say hello", &online, &outbox, &Settings::default()).is_none());
        assert!(game_commands("hello", &online, &outbox, &Settings::default()).is_none());
        assert!(
            game_commands("/camp", &online, &outbox, &Settings::default())
                .unwrap()
                .is_err()
        );
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
                languages: None,
                disciplines: None,
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
        let commands = game_commands("/CAMP", &online, &outbox, &Settings::default())
            .unwrap()
            .unwrap();
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
            game_commands("/stand", &online, &outbox, &Settings::default())
                .unwrap()
                .unwrap()
                .as_slice(),
            [ClientCommand::SetPosture {
                posture: eq_client_core::Posture::Standing,
                ..
            }]
        ));
        // Words after a command that takes none make it chat.
        assert!(game_commands("/sit down", &online, &outbox, &Settings::default()).is_none());
    }

    #[test]
    fn camp_desktop_camps_as_camp_does_and_other_words_are_refused_not_chat() {
        use eq_client_core::camp::CampTo;
        let mut online = super::super::online::OnlineState::new(true);
        let (queue, received) = std::sync::mpsc::sync_channel(8);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        crate::online::testing::admit(&mut online, 4, crate::online::testing::player(12));
        for typed in ["/camp desktop", "/CAMP Desktop"] {
            assert!(matches!(
                game_commands(typed, &online, &outbox, &Settings::default())
                    .unwrap()
                    .unwrap()
                    .as_slice(),
                [
                    ClientCommand::SetPosture {
                        posture: eq_client_core::Posture::Sitting,
                        ..
                    },
                    ClientCommand::Camp { session_id: 4, .. }
                ]
            ));
            assert_eq!(camp_destination(typed), Some(CampTo::Desktop));
        }
        assert_eq!(camp_destination("/camp"), Some(CampTo::CharacterSelect));
        assert_eq!(camp_destination("/camping desktop"), None);
        // Another word is refused in the client's words, never sent as chat.
        let refusal = game_commands("/camp server", &online, &outbox, &Settings::default())
            .unwrap()
            .unwrap_err();
        assert!(refusal.contains("/camp desktop"), "{refusal}");
        assert_eq!(camp_destination("/camp server"), None);

        // Typed, the camp it asks for ends on the desktop once the session
        // says it began; nothing is sent for the refused word.
        let mut state = ChatState::default();
        let mut typing = crate::keys::Typing::default();
        "/camp server".clone_into(&mut state.draft);
        submit_draft(
            &mut state,
            &mut typing,
            &mut online,
            &outbox,
            &Settings::default(),
        );
        assert!(received.try_recv().is_err());
        "/camp desktop".clone_into(&mut state.draft);
        submit_draft(
            &mut state,
            &mut typing,
            &mut online,
            &outbox,
            &Settings::default(),
        );
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::SetPosture { .. })
        ));
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::Camp { .. })
        ));
        online.tell(
            &eq_client_core::WorldUpdate::Game(eq_client_core::WorldEvent::Camp(
                eq_client_core::CampStatus::Preparing,
            )),
            std::time::Instant::now(),
            &eq_client_core::world::NoSpells,
        );
        assert_eq!(
            online.world().camp().map(|camp| camp.to),
            Some(CampTo::Desktop)
        );
    }

    #[test]
    fn immediate_camp_sends_one_logout_intent_and_waits_for_the_session() {
        use eq_client_core::camp::CampTo;
        let mut online = super::super::online::OnlineState::new(true);
        let (queue, received) = std::sync::mpsc::sync_channel(8);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        crate::online::testing::admit(&mut online, 4, crate::online::testing::player(12));
        let mut qol = Settings::default();
        qol.set(Fix::ExitAtOnce, true);
        for typed in ["/camp", "/CAMP Desktop"] {
            assert!(matches!(
                game_commands(typed, &online, &outbox, &qol)
                    .unwrap()
                    .unwrap()
                    .as_slice(),
                [ClientCommand::ExitAtOnce { session_id: 4, .. }]
            ));
        }
        assert!(
            game_commands("/camp server", &online, &outbox, &qol)
                .unwrap()
                .is_err()
        );
        assert!(received.try_recv().is_err());

        // Buttons and script slash steps share this submission path. The
        // destination is remembered, but neither queueing nor LoggingOut is a
        // server acknowledgement that the character left.
        submit_game_command("/camp desktop", &mut online, &outbox, &qol).unwrap();
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::ExitAtOnce { session_id: 4, .. })
        ));
        assert!(received.try_recv().is_err());
        assert!(online.world().connected() && !online.world().leaving());
        crate::online::testing::news(
            &mut online,
            [eq_client_core::WorldEvent::Camp(
                eq_client_core::CampStatus::LoggingOut,
            )],
        );
        assert_eq!(
            online.world().camp().map(|camp| camp.to),
            Some(CampTo::Desktop)
        );
        assert!(!online.world().leaving());
        crate::online::testing::news(
            &mut online,
            [eq_client_core::WorldEvent::Camp(
                eq_client_core::CampStatus::Camped,
            )],
        );
        assert!(online.world().leaving());
    }

    #[test]
    fn immediate_camp_still_refuses_dead_disconnected_or_unoffered_requests() {
        let mut online = super::super::online::OnlineState::new(true);
        let (queue, received) = std::sync::mpsc::sync_channel(8);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        let mut qol = Settings::default();
        qol.set(Fix::ExitAtOnce, true);
        assert!(submit_game_command("/camp", &mut online, &outbox, &qol).is_err());
        crate::online::testing::news(
            &mut online,
            [eq_client_core::WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 3,
                zone: "qeytoqrg".into(),
                player: Box::new(crate::online::testing::player(12)),
                far_clip: None,
                spell_landing_flag: None,
            }],
        );
        crate::online::testing::connect(&mut online, true);
        assert!(submit_game_command("/camp", &mut online, &outbox, &qol).is_err());
        crate::online::testing::admit(&mut online, 4, crate::online::testing::player(12));
        crate::online::testing::connect(&mut online, false);
        assert!(submit_game_command("/camp", &mut online, &outbox, &qol).is_err());
        crate::online::testing::connect(&mut online, true);
        crate::online::testing::news(
            &mut online,
            [eq_client_core::WorldEvent::Death(eq_client_core::Death {
                spawn_id: 12,
                killer_id: 0,
                corpse_id: 0,
                bind_zone_id: 0,
                corpse_name: None,
            })],
        );
        assert!(submit_game_command("/camp", &mut online, &outbox, &qol).is_err());
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn who_consent_corpse_and_pet_commands_reach_the_game() {
        let mut online = super::super::online::OnlineState::new(true);
        let (queue, _received) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        crate::online::testing::admit(&mut online, 4, crate::online::testing::player(12));
        assert!(matches!(
            game_commands("/who all wiz 50 60", &online, &outbox, &Settings::default())
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
        assert!(
            game_commands("/who", &online, &outbox, &Settings::default())
                .unwrap()
                .is_err()
        );
        // Consent names a player; the corpse commands want a corpse targeted,
        // but a drop without one drops them all.
        assert!(matches!(
            game_commands("/consent Helper", &online, &outbox, &Settings::default())
                .unwrap()
                .unwrap()
                .as_slice(),
            [ClientCommand::Consent { session_id: 4, name, given: true }] if name == "Helper"
        ));
        assert!(matches!(
            game_commands("/deny Helper", &online, &outbox, &Settings::default())
                .unwrap()
                .unwrap()
                .as_slice(),
            [ClientCommand::Consent { given: false, .. }]
        ));
        assert_eq!(
            game_commands("/corpsedrag", &online, &outbox, &Settings::default()).unwrap(),
            Err("You must first target a corpse.".into())
        );
        // A pet command aims at the target; unknown words say so.
        assert!(matches!(
            game_commands("/pet back off", &online, &outbox, &Settings::default())
                .unwrap()
                .unwrap()
                .as_slice(),
            [ClientCommand::Pet {
                command: eq_client_core::pets::PetCommand::BackOff,
                ..
            }]
        ));
        assert_eq!(
            game_commands("/pet dance", &online, &outbox, &Settings::default()).unwrap(),
            Err("Unknown pet command: dance".into())
        );
        assert!(matches!(
            game_commands("/corpsedrop", &online, &outbox, &Settings::default())
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
            .init_resource::<ButtonInput<MouseButton>>()
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
    fn a_complete_paste_chord_in_one_frame_never_types_v_or_submits() {
        let (mut app, receiver) = slash_app(true);
        app.world_mut().resource_mut::<ChatState>().draft = "kept".into();
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        for (key, state) in [
            (Key::Control, bevy::input::ButtonState::Pressed),
            (
                Key::Character("v".into()),
                bevy::input::ButtonState::Pressed,
            ),
            (
                Key::Character("v".into()),
                bevy::input::ButtonState::Released,
            ),
            (Key::Control, bevy::input::ButtonState::Released),
        ] {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::KeyV,
                text: Some("v".into()),
                logical_key: key,
                state,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        }
        app.update();
        assert_eq!(app.world().resource::<ChatState>().draft, "kept");
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
            .init_resource::<ButtonInput<MouseButton>>()
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

    /// A chat app for the slash tests, admitted to the zone or not, with the
    /// outbox's receiver.
    fn slash_app(admitted: bool) -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let mut online = super::super::online::OnlineState::new(true);
        if admitted {
            crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        }
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.init_resource::<ChatState>()
            .init_resource::<super::super::windows::pointer::Wheel>()
            .add_message::<KeyboardInput>()
            .insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, input);
        (app, receiver)
    }

    fn press_slash(app: &mut App) {
        press_slash_on(app, KeyCode::Slash);
    }

    fn press_slash_on(app: &mut App, key_code: KeyCode) {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Character("/".into()),
            state: bevy::input::ButtonState::Pressed,
            text: Some("/".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }

    #[test]
    fn a_slash_opens_the_line_with_itself_typed() {
        let (mut app, receiver) = slash_app(true);
        // A line left with Escape gives way to the slash.
        app.world_mut().resource_mut::<ChatState>().draft = "hello".into();
        press_slash(&mut app);
        assert!(app.world().resource::<crate::keys::Typing>().composing);
        assert_eq!(app.world().resource::<ChatState>().draft, "/");
        assert!(receiver.try_recv().is_err());
        // In the line, a slash types as any character does.
        press_slash(&mut app);
        assert_eq!(app.world().resource::<ChatState>().draft, "//");
    }

    #[test]
    fn the_keypads_slash_opens_the_line_too() {
        let (mut app, _receiver) = slash_app(true);
        press_slash_on(&mut app, KeyCode::NumpadDivide);
        assert!(app.world().resource::<crate::keys::Typing>().composing);
        assert_eq!(app.world().resource::<ChatState>().draft, "/");
    }

    #[test]
    fn the_line_types_what_the_key_types() {
        let (mut app, _receiver) = slash_app(true);
        press_slash(&mut app);
        let key = |key_code, logical_key, text: &str| KeyboardInput {
            key_code,
            logical_key,
            state: bevy::input::ButtonState::Pressed,
            text: Some(text.into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        for event in [
            // The window reports the space bar as a named key.
            key(KeyCode::KeyT, Key::Character("t".into()), "t"),
            key(KeyCode::Space, Key::Space, " "),
            // Shift's letter, as the window types it.
            key(KeyCode::KeyA, Key::Character("A".into()), "A"),
            key(KeyCode::NumpadDivide, Key::Character("/".into()), "/"),
            // Ctrl+A types only a control character, which is dropped.
            key(KeyCode::KeyA, Key::Character("a".into()), "\u{1}"),
        ] {
            app.world_mut().write_message(event);
        }
        app.update();
        assert_eq!(app.world().resource::<ChatState>().draft, "/t A/");
    }

    #[test]
    fn a_click_on_the_game_gives_the_keyboard_back_and_keeps_the_line() {
        let (mut app, receiver) = slash_app(true);
        let mut window = Window {
            focused: true,
            resolution: bevy::window::WindowResolution::new(800, 600),
            ..default()
        };
        window.set_physical_cursor_position(Some(bevy::math::DVec2::new(100.0, 100.0)));
        app.world_mut().spawn((window, PrimaryWindow));
        // The chat window: 300 by 200 at (24, 110).
        app.world_mut().spawn((
            Panel,
            super::super::windows::KeepsPress,
            ComputedNode {
                size: Vec2::new(300.0, 200.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::new(
                174.0, 210.0,
            ))),
        ));
        press_slash(&mut app);
        let click = |app: &mut App, at: (f64, f64)| {
            app.world_mut()
                .query::<&mut Window>()
                .single_mut(app.world_mut())
                .unwrap()
                .set_physical_cursor_position(Some(bevy::math::DVec2::new(at.0, at.1)));
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release(MouseButton::Left);
            mouse.clear();
            mouse.press(MouseButton::Left);
            app.update();
        };
        let composing = |app: &App| app.world().resource::<crate::keys::Typing>().composing;
        // A click in the chat window keeps the line open.
        click(&mut app, (100.0, 200.0));
        assert!(composing(&app));
        // A click on the game outside it closes the line, keeping its text.
        click(&mut app, (600.0, 500.0));
        assert!(!composing(&app));
        assert_eq!(app.world().resource::<ChatState>().draft, "/");
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn a_slash_leaves_the_quantity_windows_number_and_character_select_alone() {
        let (mut app, _receiver) = slash_app(true);
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .counting = true;
        press_slash(&mut app);
        let typing = app.world().resource::<crate::keys::Typing>();
        assert!(typing.counting && !typing.composing);
        assert_eq!(app.world().resource::<ChatState>().draft, "");

        let (mut app, _receiver) = slash_app(false);
        press_slash(&mut app);
        assert!(!app.world().resource::<crate::keys::Typing>().composing);
        assert_eq!(app.world().resource::<ChatState>().draft, "");
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
            .init_resource::<ButtonInput<MouseButton>>()
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
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
            .init_resource::<ChatState>()
            .add_systems(Update, sync_tabs);
        app.world_mut()
            .run_system_once(|mut commands: Commands, mut art: crate::sheets::Art| {
                let tab_frame = eq_client_assets::sidl::FrameLook::default();
                commands.spawn(Node::default()).with_children(|window| {
                    skinned_output(
                        window,
                        Node::default(),
                        (&mut art, Some(&tab_frame)),
                        WindowId::Chat,
                    );
                    skinned_input(window, Node::default(), false, WindowId::Chat);
                });
            })
            .unwrap();
        app.update();
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
            .init_resource::<ButtonInput<MouseButton>>()
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

    fn received_chat_app() -> (App, std::sync::mpsc::Sender<eq_client_core::WorldUpdate>) {
        use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = crate::testing::app();
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(7));
        online.open_loot(9);
        app.add_message::<KeyboardInput>()
            .insert_resource(online)
            .insert_resource(crate::online::Updates(std::sync::Mutex::new(Some(rx))))
            .insert_resource(crate::hud::messages::Messages::parse(
                "EQST0002\n0 1\n12239 Synthetic assessment.\n12216 Synthetic standing.\n12227 Synthetic level.\n12072 Synthetic loot %1.\n60001 Synthetic typed system.\n",
            ))
            .insert_resource(ChatLook { colors: [(289, [4, 5, 6])].into(), font: None })
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
            .add_systems(Update, (crate::online::receive, input, refresh).chain());
        app.world_mut().spawn(Content {
            bare: true,
            ..default()
        });
        (app, tx)
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered receive and tab-switch rendering scenario together"
    )]
    fn received_consider_loot_and_system_lines_render_in_all_after_tab_switches() {
        use eq_client_core::{
            Coins, WorldEvent, WorldUpdate,
            combat::{ConColor, Consideration},
            loot::{LootResponse, LootUpdate},
        };
        let (mut app, tx) = received_chat_app();
        for update in [
            WorldUpdate::Chat(ChatLine {
                channel: ChannelName::Guild,
                source: Source::Server,
                sender: Some("Speaker".into()),
                ..system_line("Synthetic guild.")
            }),
            WorldUpdate::Game(WorldEvent::Consideration(Consideration {
                target_id: 9,
                faction: 5,
                color: ConColor::Blue,
                hit_points: None,
            })),
            WorldUpdate::Game(WorldEvent::Loot(LootUpdate::Opened {
                response: LootResponse::Normal,
                coins: Coins {
                    gold: 1,
                    ..Coins::default()
                },
            })),
            WorldUpdate::ServerMessage {
                string_id: 60_001,
                arguments: Vec::new(),
                message_type: Some(289),
            },
        ] {
            tx.send(update).unwrap();
        }
        app.update();
        let expected = [
            "Speaker (guild): Synthetic guild.",
            "Synthetic assessment.",
            "Synthetic loot 1g.",
            "Synthetic typed system.",
        ];
        assert_eq!(shown(&mut app), expected);
        let history = &app.world().resource::<ChatState>().history;
        assert_eq!(history.lines(ChatTab::System).len(), 3);
        assert!(
            history.lines(ChatTab::Say).is_empty(),
            "assessment and loot are not NPC/player speech"
        );
        assert!(
            history
                .lines(ChatTab::System)
                .iter()
                .all(|(_, line)| line.channel == ChannelName::System
                    && line.source == Source::Official)
        );
        let button = app
            .world_mut()
            .spawn((
                TabButton(ChatTab::System, WindowId::Chat),
                Interaction::None,
            ))
            .id();
        crate::clicks::testing::click(&mut app, button);
        assert_eq!(shown(&mut app), expected[1..]);
        app.world_mut()
            .entity_mut(button)
            .insert(TabButton(ChatTab::Guild, WindowId::Chat));
        crate::clicks::testing::click(&mut app, button);
        assert_eq!(shown(&mut app), expected[..1]);
        // Receive another system line while a different filter is selected.
        tx.send(WorldUpdate::Chat(system_line("Synthetic local refusal.")))
            .unwrap();
        app.update();
        assert_eq!(shown(&mut app), expected[..1]);
        app.world_mut()
            .entity_mut(button)
            .insert(TabButton(ChatTab::All, WindowId::Chat));
        crate::clicks::testing::click(&mut app, button);
        let mut all = expected.to_vec();
        all.push("Synthetic local refusal.");
        assert_eq!(shown(&mut app), all);
        let typed = column(&mut app)
            .into_iter()
            .find(|entity| {
                app.world()
                    .get::<Text>(*entity)
                    .is_some_and(|text| text.0 == "Synthetic typed system.")
            })
            .unwrap();
        assert_eq!(
            app.world().get::<TextColor>(typed).unwrap().0,
            Color::srgb_u8(4, 5, 6)
        );
        let history = &app.world().resource::<ChatState>().history;
        let ids: Vec<_> = history
            .lines(ChatTab::All)
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(ids, [1, 2, 3, 4, 5]);
        assert_eq!(history.lines(ChatTab::System).len(), 4);
        assert_eq!(
            history.lines(ChatTab::System).last().unwrap().1.source,
            Source::Client
        );
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
        crate::clicks::testing::install(&mut app);
        app.world_mut().spawn(Content::default());
        let button = app
            .world_mut()
            .spawn((TabButton(ChatTab::Guild, WindowId::Chat), Interaction::None))
            .id();
        for line in crate::preview::chat_lines() {
            app.world_mut()
                .resource_mut::<ChatState>()
                .history
                .push(line);
        }
        crate::clicks::testing::click(&mut app, button);
        assert_eq!(
            app.world()
                .resource::<ChatState>()
                .groups
                .active(WindowId::Chat),
            Some(ChatTab::Guild)
        );
        // Each line reads in its log's words: without the installed table,
        // in this client's words for who spoke and where.
        let messages = shown(&mut app);
        assert_eq!(
            messages,
            ["Preview (guild): Meet by the tunnel when everyone is ready."]
        );
        app.world_mut()
            .entity_mut(button)
            .insert(TabButton(ChatTab::Auction, WindowId::Chat));
        crate::clicks::testing::click(&mut app, button);
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
    fn closing_a_detached_frame_returns_its_tabs_to_the_main_strip() {
        for via_escape in [false, true] {
            let mut app = crate::testing::app();
            app.add_systems(Startup, |mut commands: Commands| spawn(&mut commands))
                .add_systems(Update, (close_groups, reconcile, sync_tabs).chain());
            let detached = app
                .world_mut()
                .resource_mut::<ChatState>()
                .groups
                .detach(WindowId::Chat, ChatTab::Guild)
                .unwrap();
            app.update();

            let world = app.world_mut();
            let frame = hud_frame(world, detached);
            let tab_group = |world: &mut World| {
                world
                    .query::<&TabButton>()
                    .iter(world)
                    .find(|button| button.0 == ChatTab::Guild)
                    .unwrap()
                    .1
            };
            assert_eq!(tab_group(world), detached);
            if via_escape {
                world.insert_resource(crate::escape::Escape::Close(detached));
                app.update();
            } else {
                let close = world
                    .query::<(Entity, &CloseGroup)>()
                    .iter(world)
                    .find(|(_, close)| close.0 == detached)
                    .unwrap()
                    .0;
                crate::clicks::testing::click(&mut app, close);
            }
            assert!(app.world().get_entity(frame).is_err());
            let world = app.world_mut();
            assert_eq!(tab_group(world), WindowId::Chat);
            assert_eq!(
                world.resource::<ChatState>().groups.active(WindowId::Chat),
                Some(ChatTab::Guild)
            );
        }
    }

    fn hud_frame(world: &mut World, id: WindowId) -> Entity {
        world
            .query_filtered::<(Entity, &WindowId), With<crate::hud::HudRoot>>()
            .iter(world)
            .find(|(_, window)| **window == id)
            .unwrap()
            .0
    }

    #[test]
    fn rebuilding_detached_chat_preserves_its_place_in_the_window_stack() {
        use crate::windows::{Shown, Stack, raise_opened, restack};
        use bevy::{ecs::system::RunSystemOnce, mesh::skinning::SkinnedMeshInverseBindposes};

        let inventory = WindowId::Inventory;
        let build_hud = |mut commands: Commands| crate::hud::spawn(&mut commands);
        for inventory_in_front in [false, true] {
            let mut app = crate::testing::app();
            app.init_resource::<Assets<Mesh>>()
                .init_resource::<Assets<StandardMaterial>>()
                .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
                .add_systems(Startup, build_hud)
                .add_systems(
                    Update,
                    (raise_opened, restack, crate::inventory::update, reconcile).chain(),
                );
            app.world_mut().resource_mut::<Shown>().open(inventory);
            // Route observes the inventory after presentation has shown it.
            app.update();
            app.update();
            let detached = app
                .world_mut()
                .resource_mut::<ChatState>()
                .groups
                .detach(WindowId::Chat, ChatTab::Guild)
                .unwrap();
            app.update();
            if inventory_in_front {
                app.world_mut().resource_mut::<Stack>().raise(inventory);
            }
            app.update();
            let ids = [inventory, detached];
            let z = |world: &World, id| world.get::<GlobalZIndex>(id).unwrap().0;
            let frames = ids.map(|id| hud_frame(app.world_mut(), id));
            assert_eq!(
                z(app.world(), frames[0]) > z(app.world(), frames[1]),
                inventory_in_front
            );

            // Leave through the production root cleanup, then rebuild through
            // the same HUD builder as a newly loaded zone.
            app.world_mut()
                .run_system_once(|mut commands: Commands, mut scene: crate::zone::Scene| {
                    scene.leave(&mut commands, &mut default());
                })
                .unwrap();
            assert!(
                frames
                    .iter()
                    .all(|&frame| app.world().get_entity(frame).is_err())
            );
            app.update();
            app.world_mut().run_system_once(build_hud).unwrap();
            let rebuilt_inventory = hud_frame(app.world_mut(), inventory);
            assert_eq!(
                app.world().get::<Node>(rebuilt_inventory).unwrap().display,
                Display::None
            );
            app.update();
            app.update();
            let rebuilt = ids.map(|id| hud_frame(app.world_mut(), id));
            assert_ne!(rebuilt[0], frames[0]);
            assert_ne!(rebuilt[1], frames[1]);
            assert_eq!(
                app.world().get::<Node>(rebuilt[0]).unwrap().display,
                Display::Flex
            );
            assert_eq!(
                z(app.world(), rebuilt[0]) > z(app.world(), rebuilt[1]),
                inventory_in_front
            );

            let world = app.world_mut();
            world.resource_mut::<ChatState>().groups.close(detached);
            app.update();
            assert!(
                !app.world()
                    .resource::<Stack>()
                    .front_to_back()
                    .any(|id| id == detached)
            );
            let reopened = app
                .world_mut()
                .resource_mut::<ChatState>()
                .groups
                .detach(WindowId::Chat, ChatTab::Tell)
                .unwrap();
            assert_eq!(reopened, detached);
            app.update();
            assert_eq!(
                app.world().resource::<Stack>().front_to_back().next(),
                Some(reopened)
            );
        }
    }

    #[test]
    fn an_empty_skinned_chat_strip_keeps_its_drop_target_without_a_client_hint() {
        let mut app = crate::testing::app();
        app.add_systems(
            Startup,
            |mut commands: Commands, mut art: crate::sheets::Art| {
                commands.spawn_empty().with_children(|parent| {
                    tab_strip(parent, WindowId::Chat, false, None);
                    for tab_frame in [None, Some(default())] {
                        skinned_output(
                            parent,
                            Node::default(),
                            (&mut art, tab_frame.as_ref()),
                            WindowId::Chat,
                        );
                    }
                });
            },
        )
        .add_systems(Update, sync_tabs);
        let world = app.world_mut();
        for tab in ChatTab::ALL {
            world
                .resource_mut::<ChatState>()
                .groups
                .detach(WindowId::Chat, tab)
                .unwrap();
        }
        app.update();
        let world = app.world_mut();
        let strips: BTreeMap<_, _> = world
            .query_filtered::<(Entity, &TabStrip), With<crate::windows::KeepsPress>>()
            .iter(world)
            .map(|(entity, strip)| ((strip.bare, strip.skin.is_some()), entity))
            .collect();
        assert_eq!(
            strips.keys().copied().collect::<Vec<_>>(),
            [(false, false), (true, false), (true, true)]
        );
        let labels: Vec<_> = world
            .query::<(&Text, &ChildOf)>()
            .iter(world)
            .filter(|(text, _)| text.0 == "Drop a chat tab here")
            .map(|(_, parent)| parent.parent())
            .collect();
        assert_eq!(labels, [strips[&(false, false)]]);
    }

    #[test]
    fn a_nested_chat_line_only_links_in_its_frontmost_viewport() {
        let mut app = crate::testing::app();
        app.add_systems(Update, crate::items::link_input);
        let world = app.world_mut();
        world
            .query_filtered::<&mut Window, With<PrimaryWindow>>()
            .single_mut(world)
            .unwrap()
            .set_physical_cursor_position(Some(bevy::math::DVec2::splat(50.0)));
        let node = ComputedNode {
            size: Vec2::splat(100.0),
            ..default()
        };
        let transform = UiGlobalTransform::from_translation(Vec2::splat(50.0));
        let viewport = world
            .spawn((
                Viewport,
                crate::windows::KeepsPress,
                node,
                transform,
                ComputedStackIndex(1),
            ))
            .id();
        let column = world.spawn(ChildOf(viewport)).id();
        let wrapper = world.spawn(ChildOf(column)).id();
        let paragraph = world
            .run_system_once(move |mut commands: Commands| {
                let mut paragraph = commands.spawn((
                    LineId(1),
                    node,
                    transform,
                    InheritedVisibility::VISIBLE,
                    ChildOf(wrapper),
                ));
                paragraph.with_children(|parent| {
                    links::spawn_text(parent, "https://example.com", Color::WHITE, Size::Small);
                });
                paragraph.id()
            })
            .unwrap();
        let span = world
            .query_filtered::<Entity, With<links::WebLink>>()
            .single(world)
            .unwrap();
        let mut block = ComputedTextBlock::default();
        // Register the span without depending on a font or glyph shaping.
        // The synthetic run below supplies the laid-out link's bounds.
        TextPipeline::default()
            .update_buffer(
                &Assets::default(),
                std::iter::once((
                    span,
                    1,
                    "",
                    &TextFont::default(),
                    Color::WHITE,
                    default(),
                    default(),
                )),
                LineBreak::NoWrap,
                Justify::Left,
                TextBounds::UNBOUNDED,
                1.0,
                &mut block,
                &mut default(),
                &mut default(),
                Vec2::splat(100.0),
                16.0,
            )
            .unwrap();
        world.entity_mut(paragraph).insert((
            block,
            TextLayoutInfo {
                run_geometry: vec![RunGeometry {
                    section_index: 0,
                    bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
                    ..default()
                }],
                ..default()
            },
        ));
        app.update();
        assert_eq!(
            app.world().get::<Interaction>(span),
            Some(&Interaction::Hovered)
        );
        app.world_mut().spawn((
            Viewport,
            crate::windows::KeepsPress,
            node,
            transform,
            ComputedStackIndex(2),
        ));
        app.update();
        assert_eq!(
            app.world().get::<Interaction>(span),
            Some(&Interaction::None)
        );
    }

    #[test]
    fn detached_chat_keeps_its_view_links_and_outgoing_channel() {
        let (mut app, receiver) = slash_app(true);
        app.init_resource::<crate::options::OptionsState>()
            .add_systems(Update, refresh.after(input))
            .add_systems(PostUpdate, scroll);
        let world = app.world_mut();
        let mut chat = world.resource_mut::<ChatState>();
        for line in crate::preview::chat_lines() {
            chat.history.push(line);
        }
        chat.groups.select(WindowId::Chat, ChatTab::Guild);
        let detached = chat
            .groups
            .detach(WindowId::Chat, ChatTab::Auction)
            .unwrap();
        chat.views.insert(
            ChatTab::Auction,
            TabView {
                offset: 35.0,
                follow: false,
                seen: 0,
            },
        );
        chat.draft = "synthetic auction".into();
        let main_input = world.spawn((InputLabel, Bare, Text::default())).id();
        let float_input = world
            .spawn((InputLabel, Bare, InGroup(detached), Text::default()))
            .id();
        let main = world.spawn(Content::default()).id();
        let floating = world.spawn((Content::default(), InGroup(detached))).id();
        let viewport = world
            .spawn((
                Viewport,
                InGroup(detached),
                ScrollPosition::default(),
                ComputedNode {
                    size: Vec2::new(200.0, 100.0),
                    content_size: Vec2::new(200.0, 220.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
            ))
            .id();
        world.resource_mut::<crate::keys::Typing>().composing = true;
        app.update();

        let world = app.world();
        let line = |column| world.get::<Children>(column).unwrap()[0];
        assert!(
            world
                .get::<Text>(line(main))
                .unwrap()
                .0
                .contains("Meet by the tunnel")
        );
        let auction = line(floating);
        let spans = world.get::<Children>(auction).unwrap();
        assert!(spans.iter().any(|span| {
            world
                .get::<TextSpan>(span)
                .is_some_and(|text| text.0.contains("WTS"))
        }));
        assert!(spans.iter().any(|span| {
            world
                .get::<crate::items::ItemButton>(span)
                .is_some_and(|link| link.0.item_id == 42)
        }));
        assert_eq!(world.get::<Text>(main_input).unwrap().0, "");
        assert_eq!(
            world.get::<Text>(float_input).unwrap().0,
            "synthetic auction|"
        );
        assert!((world.get::<ScrollPosition>(viewport).unwrap().y - 35.0).abs() < f32::EPSILON);
        assert_eq!(world.resource::<ChatState>().history.revision(), 7);
        assert_eq!(
            world.resource::<ChatState>().views[&ChatTab::Auction].seen,
            0
        );

        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ClientCommand::SendChat(OutboundChat::Auction(text)) if text == "synthetic auction"
        ));
        let chat = app.world().resource::<ChatState>();
        assert!(chat.views[&ChatTab::Auction].follow);
        assert_eq!(chat.history.revision(), 7);
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
    #[test]
    fn group_buttons_bind_answers_to_the_displayed_invitation() {
        use crate::online::{OnlineState, testing};
        use eq_client_core::{
            WorldEvent,
            group::{GroupUpdate, InvitationId, InvitationKind},
        };
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(9));
        let offer = |id| {
            WorldEvent::Group(GroupUpdate::Invited {
                inviter: "Leader".into(),
                player: "Example".into(),
                kind: InvitationKind::Existing,
                id: InvitationId::new(id),
            })
        };
        testing::news(&mut online, [offer(1)]);
        let accept = group_command("follow", "", &online, 7);
        let decline = group_command("disband", "", &online, 7);
        assert!(
            matches!(&accept, ClientCommand::FollowGroup { session_id: 7, inviter, invitation_id }
            if inviter == "Leader" && *invitation_id == InvitationId::new(1))
        );
        assert!(
            matches!(&decline, ClientCommand::DeclineGroup { session_id: 7, inviter, invitation_id }
            if inviter == "Leader" && *invitation_id == InvitationId::new(1))
        );
        testing::news(
            &mut online,
            [
                WorldEvent::Group(GroupUpdate::Declining {
                    inviter: "Leader".into(),
                }),
                offer(2),
            ],
        );
        assert_ne!(group_command("follow", "", &online, 7), accept);
        assert_ne!(group_command("disband", "", &online, 7), decline);
    }
}
