//! Workbench chrome and views; no file access or script execution lives here.

use bevy::{
    prelude::*,
    text::{EditableText, TextCursorStyle, TextEdit},
    ui::widget::TextScroll,
};

use super::{Tab, Workbench, help, icons, syntax};
use crate::{
    script::{Phase, RunStatus},
    theme,
    tooltip::Tooltip,
    windows::{
        self, Shown, WindowId,
        pointer::{TakesWheel, Wheel},
    },
};

#[derive(Component)]
pub(super) struct Root;

#[derive(Component)]
pub(super) struct LibraryPane;

#[derive(Component)]
pub(super) enum Field {
    Path,
    Source,
}

#[derive(Component, Clone, Copy)]
pub(super) enum Action {
    ToggleLibrary,
    New,
    Open,
    Reload,
    Save,
    Run,
    RunToCursor,
    Pause,
    Resume,
    Step,
    StepOut,
    Stop,
    Discard,
    Cancel,
    Tab(Tab),
    Help(usize),
}

impl Action {
    /// Whether the control can be pressed now: an icon and a text button
    /// for one action always agree.
    pub(super) fn usable(self, phase: Phase, busy: bool) -> bool {
        let active = phase.active();
        match self {
            Self::Run | Self::New | Self::Open | Self::Save | Self::Reload | Self::Discard => {
                !active && !busy
            }
            // Before a run it starts one; while paused it resumes to the line.
            Self::RunToCursor => (!active && !busy) || phase == Phase::Paused,
            Self::Pause => active && phase != Phase::Paused,
            Self::Resume | Self::Step | Self::StepOut => phase == Phase::Paused,
            Self::Stop => active,
            _ => true,
        }
    }
}

/// A run control's icon: tinted while its action can happen, dimmed while not.
#[derive(Component, Clone, Copy)]
pub(super) struct IconOf(Action, icons::Icon);

impl IconOf {
    /// The colour its icon shows with, usable or not.
    pub(super) fn tint(self, usable: bool) -> Color {
        if usable {
            self.1.tint()
        } else {
            self.1.dimmed()
        }
    }
}

#[derive(Component)]
pub(super) struct Page(Tab);
#[derive(Component)]
pub(super) struct Status;
#[derive(Component)]
pub(super) struct Message;
#[derive(Component)]
pub(super) struct Confirmation;
#[derive(Component)]
pub(super) struct SourceLines;
#[derive(Component)]
pub(super) struct History;
#[derive(Component)]
pub(super) struct FlowPane;
#[derive(Component)]
pub(super) struct FlowText;
#[derive(Component)]
pub(super) struct HelpBody;
#[derive(Component)]
pub(super) struct Breadcrumbs;
#[derive(Component)]
pub(super) struct HelpScroll;
#[derive(Component)]
pub(super) struct Validation;
#[derive(Resource)]
pub(super) struct Mono(Handle<Font>);

/// Cached source-derived views; elapsed-time updates do not reparse a document.
#[derive(Default)]
pub(super) struct Cache {
    document_revision: Option<u64>,
    run: Option<(Option<usize>, Phase, String, usize)>,
    help_page: Option<usize>,
    validation: Option<(u64, String)>,
}

pub(super) fn font(mono: &Mono, size: f32) -> TextFont {
    TextFont {
        font: mono.0.clone().into(),
        font_size: FontSize::Px(size),
        ..default()
    }
}

pub(super) fn label(
    commands: &mut Commands,
    text: impl Into<String>,
    size: f32,
    color: Color,
) -> Entity {
    commands
        .spawn((
            Text::new(text),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
        ))
        .id()
}

pub(super) fn button(commands: &mut Commands, title: &str, action: Action) -> Entity {
    let text = label(commands, title, 13.0, theme::INK_BRIGHT);
    commands
        .spawn((
            Button,
            action,
            Node {
                padding: UiRect::axes(px(9), px(5)),
                border: UiRect::all(px(1)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme::BUTTON),
            BorderColor::all(theme::EDGE),
        ))
        .add_child(text)
        .id()
}

/// A run control drawn as an icon; its name shows as the tooltip.
fn icon_button(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    icon: icons::Icon,
    title: &str,
    action: Action,
) -> Entity {
    let image = commands
        .spawn((
            IconOf(action, icon),
            ImageNode::new(images.add(icon.image())).with_color(icon.tint()),
            Node {
                width: px(14),
                height: px(14),
                ..default()
            },
        ))
        .id();
    commands
        .spawn((
            Button,
            action,
            Tooltip(title.to_owned()),
            Node {
                padding: UiRect::axes(px(7), px(6)),
                border: UiRect::all(px(1)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme::BUTTON),
            BorderColor::all(theme::EDGE),
        ))
        .add_child(image)
        .id()
}

/// The run and debugging controls as icons, for the right end of the row
/// above a page's script area.
fn run_controls(commands: &mut Commands, images: &mut Assets<Image>) -> Entity {
    let children = [
        (icons::Icon::Run, "Run", Action::Run),
        (
            icons::Icon::RunToCursor,
            "Run to cursor",
            Action::RunToCursor,
        ),
        (icons::Icon::Pause, "Pause", Action::Pause),
        (icons::Icon::Resume, "Resume", Action::Resume),
        (icons::Icon::Step, "Step", Action::Step),
        (icons::Icon::StepOut, "Step out", Action::StepOut),
        (icons::Icon::Stop, "Stop", Action::Stop),
    ]
    .map(|(icon, title, action)| icon_button(commands, images, icon, title, action));
    commands
        .spawn(Node {
            column_gap: px(4),
            flex_shrink: 0.0,
            margin: UiRect::left(Val::Auto),
            ..default()
        })
        .add_children(&children)
        .id()
}

/// A row holding `left` and the run controls at its right end.
fn with_run_controls(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    left: Option<Entity>,
) -> Entity {
    let controls = run_controls(commands, images);
    let row = commands
        .spawn(Node {
            width: percent(100),
            column_gap: px(8),
            row_gap: px(5),
            // In a narrow window the icons wrap below the file buttons,
            // still at the right.
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            ..default()
        })
        .id();
    if let Some(left) = left {
        commands.entity(row).add_child(left);
    }
    commands.entity(row).add_child(controls);
    row
}

fn row(commands: &mut Commands, actions: &[(&str, Action)]) -> Entity {
    let children: Vec<_> = actions
        .iter()
        .map(|(text, action)| button(commands, text, *action))
        .collect();
    commands
        .spawn(Node {
            column_gap: px(5),
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(5),
            ..default()
        })
        .add_children(&children)
        .id()
}

/// Create once so hiding or minimizing a window cannot discard an edited buffer.
#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
pub(super) fn spawn(
    mut commands: Commands,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    state: Res<Workbench>,
) {
    let mono = Mono(fonts.add(Font::from_bytes(bevy::text::DEFAULT_FONT_DATA.to_vec())));
    // Resizable from any edge; the script and source areas take the room.
    let frame = windows::resizable_frame(
        &mut commands,
        WindowId::Scripts,
        Node {
            display: Display::None,
            min_width: px(520),
            min_height: px(360),
            // A small game window still shows the whole frame; a larger
            // saved size comes back when the screen has room for it.
            max_width: percent(94),
            max_height: percent(92),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        Vec2::new(1040.0, 620.0),
        u64::from_be_bytes(*b"scripts1"),
    );
    commands.entity(frame).insert(Root);
    let body = commands
        .spawn((
            Node {
                padding: UiRect::all(px(12)),
                row_gap: px(8),
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                min_height: px(0),
                ..default()
            },
            BackgroundColor(theme::COVER),
        ))
        .id();
    let tabs = row(
        &mut commands,
        &[
            ("Editor", Action::Tab(Tab::Editor)),
            ("Execution", Action::Tab(Tab::Execution)),
            ("Help", Action::Tab(Tab::Help)),
        ],
    );
    let status = label(&mut commands, "Ready", 13.0, theme::FOCUS);
    commands.entity(status).insert(Status);
    let editor = commands
        .spawn((
            Page(Tab::Editor),
            Node {
                column_gap: px(10),
                flex_grow: 1.0,
                min_height: px(0),
                ..default()
            },
        ))
        .id();
    let path = commands
        .spawn((
            Field::Path,
            Interaction::default(),
            windows::KeepsPress,
            EditableText {
                max_characters: Some(2048),
                ..EditableText::new(&state.path)
            },
            Node {
                height: px(30),
                width: percent(100),
                padding: UiRect::all(px(5)),
                border: UiRect::all(px(1)),
                ..default()
            },
            font(&mono, 13.0),
            TextColor(theme::INK_BRIGHT),
            TextCursorStyle::default(),
            TextLayout::no_wrap(),
            BackgroundColor(theme::WELL),
            BorderColor::all(theme::EDGE),
        ))
        .id();
    let path_label = label(
        &mut commands,
        "Name in Scripts  ·  Save As (folders allowed)",
        12.0,
        theme::INK_DIM,
    );
    let file_buttons = row(
        &mut commands,
        &[
            ("Files", Action::ToggleLibrary),
            ("New", Action::New),
            ("Open", Action::Open),
            ("Save", Action::Save),
            ("Reload", Action::Reload),
        ],
    );
    let file_controls = with_run_controls(&mut commands, &mut images, Some(file_buttons));
    let source = commands
        .spawn((
            Field::Source,
            Interaction::default(),
            windows::KeepsPress,
            TakesWheel,
            super::editor::Surface,
            Node {
                flex_grow: 1.0,
                min_height: px(120),
                width: percent(100),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::WELL),
            BorderColor::all(theme::EDGE),
        ))
        .id();
    let validation = label(&mut commands, "", 12.0, theme::INK_DIM);
    commands.entity(validation).insert(Validation);
    let document = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            min_width: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            ..default()
        })
        .add_children(&[path_label, path, file_controls, source, validation])
        .id();
    let library = super::library::spawn(&mut commands);
    commands.entity(library).insert(LibraryPane);
    commands.entity(editor).add_children(&[library, document]);
    let execution = commands
        .spawn((
            Page(Tab::Execution),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                flex_grow: 1.0,
                min_height: px(0),
                ..default()
            },
        ))
        .id();
    let lines = commands
        .spawn((
            SourceLines,
            Node {
                min_height: px(120),
                min_width: px(0),
                flex_grow: 1.0,
                padding: UiRect::all(px(8)),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::WELL),
        ))
        .id();
    let history = label(
        &mut commands,
        "Run a script to see its source and progress here.",
        12.0,
        theme::INK,
    );
    commands.entity(history).insert(History);
    let flow = label(&mut commands, "", 12.0, theme::INK);
    commands.entity(flow).insert((
        FlowText,
        font(&mono, 12.0),
        Node {
            flex_shrink: 0.0,
            ..default()
        },
    ));
    let flow_pane = commands
        .spawn((
            FlowPane,
            TakesWheel,
            ScrollPosition::default(),
            Node {
                display: Display::None,
                width: percent(32),
                min_width: px(120),
                padding: UiRect::all(px(8)),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(theme::WELL),
        ))
        .add_child(flow)
        .id();
    let source_row = commands
        .spawn(Node {
            width: percent(100),
            column_gap: px(8),
            flex_grow: 1.0,
            // Its height comes from the window, never from the rows shown,
            // which follow the height.
            flex_basis: px(0),
            min_height: px(0),
            ..default()
        })
        .add_children(&[lines, flow_pane])
        .id();
    let execution_controls = with_run_controls(&mut commands, &mut images, None);
    commands
        .entity(execution)
        .add_children(&[execution_controls, source_row, history]);
    let help_page = commands
        .spawn((
            Page(Tab::Help),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                flex_grow: 1.0,
                min_height: px(0),
                ..default()
            },
        ))
        .id();
    let breadcrumbs = commands
        .spawn((
            Breadcrumbs,
            Node {
                column_gap: px(5),
                row_gap: px(5),
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .id();
    let help_scroll = commands
        .spawn((
            HelpScroll,
            TakesWheel,
            ScrollPosition::default(),
            Node {
                flex_grow: 1.0,
                min_height: px(0),
                overflow: Overflow::scroll_y(),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .id();
    let help_body = commands
        .spawn((
            HelpBody,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::right(px(8)),
                flex_shrink: 0.0,
                ..default()
            },
        ))
        .id();
    commands.entity(help_scroll).add_child(help_body);
    commands
        .entity(help_page)
        .add_children(&[breadcrumbs, help_scroll]);
    let message = label(&mut commands, "", 12.0, theme::INK_WARM);
    commands.entity(message).insert(Message);
    let confirmation = row(
        &mut commands,
        &[
            ("Discard edits", Action::Discard),
            ("Cancel", Action::Cancel),
        ],
    );
    commands.entity(confirmation).insert(Confirmation);
    commands.entity(body).add_children(&[
        tabs,
        status,
        editor,
        execution,
        help_page,
        message,
        confirmation,
    ]);
    commands.entity(frame).add_child(body);
    commands.insert_resource(mono);
}

fn phase_text(phase: Phase) -> &'static str {
    match phase {
        Phase::Idle => "Ready",
        Phase::Running => "Running",
        Phase::WaitingForFocus => "Waiting for the client to regain focus",
        Phase::WaitingForInput => "Background run: leave the client unfocused to continue",
        Phase::WaitingForPointer => "Move the real pointer outside the window to continue",
        Phase::Pausing => "Pausing after the current action",
        Phase::Paused => "Paused between actions",
        Phase::Following => "Waiting for appended lines",
        Phase::Completed => "Completed",
        Phase::Stopped => "Stopped",
        Phase::Failed => "Failed",
    }
}

/// Visibility and dynamic text, without rebuilding the editor on every keystroke.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::too_many_lines
)]
pub(super) fn refresh(
    mut commands: Commands,
    mut state: ResMut<Workbench>,
    status: Res<RunStatus>,
    shown: Res<Shown>,
    mono: Res<Mono>,
    mut nodes: Query<
        (
            &mut Node,
            Option<&Page>,
            Has<Root>,
            Has<Confirmation>,
            Has<LibraryPane>,
            Has<FlowPane>,
        ),
        Or<(
            With<Page>,
            With<Root>,
            With<Confirmation>,
            With<LibraryPane>,
            With<FlowPane>,
        )>,
    >,
    mut texts: Query<
        (
            &mut Text,
            Has<Status>,
            Has<Message>,
            Has<History>,
            Has<Validation>,
            Has<FlowText>,
        ),
        Or<(
            With<Status>,
            With<Message>,
            With<History>,
            With<Validation>,
            With<FlowText>,
        )>,
    >,
    mut fields: Query<(&Field, &mut EditableText, &mut BorderColor, &mut TextScroll)>,
    focus: Res<bevy::input_focus::InputFocus>,
    field_ids: Query<(Entity, &Field)>,
    mut buttons: Query<(&Action, &Interaction, &mut BackgroundColor)>,
    mut icons: Query<(&IconOf, &mut ImageNode)>,
    lines: Query<(Entity, Option<&ComputedNode>), With<SourceLines>>,
    help_body: Query<(Entity, Has<Breadcrumbs>), Or<(With<HelpBody>, With<Breadcrumbs>)>>,
    mut previous: Local<Cache>,
) {
    let active = status.phase().active();
    for (mut node, page, root, confirmation, library, flow_pane) in &mut nodes {
        let visible = if root {
            shown.displayed(WindowId::Scripts)
        } else if confirmation {
            state.confirm.is_some()
        } else if library {
            !state.library_collapsed
        } else if flow_pane {
            status.flow().is_some()
        } else if let Some(Page(tab)) = page {
            *tab == state.tab && (*tab != Tab::Editor || !active)
        } else {
            continue;
        };
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let (done, total) = status.progress();
    let line_count = state.document.text.lines().count();
    let check = state.bypass_change_detection().document.check();
    if previous
        .validation
        .as_ref()
        .is_none_or(|(generation, _)| *generation != check.generation)
    {
        let result = match (check.counts, check.errors.first()) {
            (Some((statements, actions)), _) => {
                format!("{statements} statements  ·  {actions} actions  ·  {line_count} lines")
            }
            (None, Some(error)) => format!(
                "{} error(s) · {error} · Listed under the editor",
                check.errors.len()
            ),
            (None, None) => String::new(),
        };
        previous.validation = Some((check.generation, result));
    }
    for (mut text, is_status, message, history, validation, flow_text) in &mut texts {
        let value = if is_status {
            let progress = status.flow().map_or_else(
                || format!("{done}/{total} actions"),
                |flow| {
                    format!(
                        "{} statements executed  ·  {done} actions finished",
                        flow.statements()
                    )
                },
            );
            format!(
                "{}  ·  {progress}  ·  {:.1}s{}",
                phase_text(status.phase()),
                status.elapsed().as_secs_f32(),
                if state.document.dirty() {
                    "  ·  Unsaved edits"
                } else {
                    ""
                }
            )
        } else if message {
            state.message.clone()
        } else if history {
            status
                .events()
                .iter()
                .rev()
                .take(5)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|event| {
                    format!(
                        "{}{}",
                        event
                            .line
                            .map_or_else(String::new, |line| format!("L{line}  ")),
                        event.message
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else if flow_text {
            flow_summary(&status)
        } else if validation {
            previous
                .validation
                .as_ref()
                .map_or_else(String::new, |(_, result)| result.clone())
        } else {
            continue;
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    let revision_changed = previous.document_revision != Some(state.revision);
    for (field, mut editor, mut border, mut scroll) in &mut fields {
        if revision_changed {
            // Preserve Bevy's resolved font, width and layout generation. A fresh
            // editor loses those while the unchanged style components are cached.
            editor.clear();
            editor.editor_mut().set_text(match field {
                Field::Path => &state.path,
                Field::Source => &state.document.text,
            });
            editor.queue_edit(TextEdit::TextStart(false));
            scroll.0 = Vec2::ZERO;
        }
        let focused = focus.get().is_some_and(|entity| {
            field_ids.get(entity).is_ok_and(|(_, other)| {
                std::mem::discriminant(field) == std::mem::discriminant(other)
            })
        });
        *border = BorderColor::all(if focused { theme::FOCUS } else { theme::EDGE });
    }
    previous.document_revision = Some(state.revision);
    for (action, interaction, mut color) in &mut buttons {
        let usable = action.usable(status.phase(), state.busy());
        let on = matches!(action, Action::Tab(tab) if *tab == state.tab)
            || matches!(action, Action::Help(page) if *page == state.help_page)
            || matches!(action, Action::ToggleLibrary) && !state.library_collapsed;
        *color = BackgroundColor(theme::button(usable, on, *interaction));
    }
    for (icon, mut image) in &mut icons {
        let tint = icon.tint(icon.0.usable(status.phase(), state.busy()));
        if image.color != tint {
            image.color = tint;
        }
    }
    // As many source rows as the pane's laid-out height holds.
    let rows = lines
        .single()
        .ok()
        .and_then(|(_, node)| node)
        .map_or(DEFAULT_ROWS, |node| {
            visible_rows(node.size().y * node.inverse_scale_factor())
        });
    // Rebuild at action boundaries or a new height, not at frame rate or
    // every elapsed-time tick.
    if previous
        .run
        .as_ref()
        .is_none_or(|(line, phase, source, shown)| {
            *line != status.line()
                || *phase != status.phase()
                || source != status.source()
                || *shown != rows
        })
    {
        if let Ok((parent, _)) = lines.single() {
            commands.entity(parent).despawn_children();
            let start = first_row(status.line().unwrap_or(1), rows);
            let symbols = syntax::Symbols::scan(status.source());
            for (index, line) in status.source().lines().enumerate().skip(start).take(rows) {
                source_line(
                    &mut commands,
                    parent,
                    line,
                    Some(&symbols),
                    Some(index + 1),
                    status.line() == Some(index + 1),
                    status.breakpoints().contains(&(index + 1)),
                    &mono,
                );
            }
        }
        previous.run = Some((
            status.line(),
            status.phase(),
            status.source().to_owned(),
            rows,
        ));
    }
    if previous.help_page != Some(state.help_page) {
        for (parent, breadcrumbs) in &help_body {
            commands.entity(parent).despawn_children();
            if breadcrumbs {
                help::breadcrumbs(&mut commands, parent, state.help_page);
            } else {
                help::draw(&mut commands, parent, state.help_page, &mono);
            }
        }
        previous.help_page = Some(state.help_page);
    }
}

fn flow_summary(status: &RunStatus) -> String {
    let Some(flow) = status.flow() else {
        return String::new();
    };
    let mut lines = vec!["Execution state".into()];
    if let Some((line, taken)) = flow.branch() {
        lines.push(format!("If L{line} · {taken}"));
    }
    for (line, iteration, limit) in flow.loops() {
        lines.push(format!("Loop L{line} · {iteration}/{limit}"));
    }
    lines.push(String::new());
    lines.push("Variables".into());
    // A loop's variable that has not run yet holds an empty line.
    let variables = flow.variables().iter().filter(|line| !line.is_empty());
    if variables.clone().next().is_none() {
        lines.push("None initialized".into());
    } else {
        lines.extend(variables.cloned());
    }
    lines.join("\n")
}

/// One source row's height, and the pane's padding above and below.
const ROW: f32 = 18.0;
const PANE_PADDING: f32 = 8.0;
/// Rows shown before the pane has been laid out.
const DEFAULT_ROWS: usize = 14;

/// How many whole rows fit a source pane this tall, at least three.
fn visible_rows(height: f32) -> usize {
    if height <= 0.0 || !height.is_finite() {
        return DEFAULT_ROWS;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a finite, positive pane height in rows"
    )]
    let rows = ((height - 2.0 * PANE_PADDING) / ROW).floor().max(0.0) as usize;
    rows.max(3)
}

/// The first row index shown: the current line, counted from 1, sits
/// after about three sevenths of the rows, as it sat 7th of 14.
const fn first_row(current: usize, rows: usize) -> usize {
    current.saturating_sub(rows * 3 / 7)
}

/// A read-only source line with stable syntax colors, an execution marker
/// and a breakpoint dot.
#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "one row's independent presentation facts"
)]
pub(super) fn source_line(
    commands: &mut Commands,
    parent: Entity,
    source: &str,
    symbols: Option<&syntax::Symbols>,
    line: Option<usize>,
    active: bool,
    dot: bool,
    mono: &Mono,
) {
    let row = commands
        .spawn((
            Node {
                width: percent(100),
                height: px(ROW),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(if active {
                theme::BUTTON_ON
            } else {
                Color::NONE
            }),
        ))
        .id();
    let text = commands
        .spawn((
            Text::new(line.map_or_else(String::new, |line| {
                format!(
                    "{}{}{line:>4}  ",
                    if active { "▶" } else { " " },
                    if dot { "●" } else { " " }
                )
            })),
            font(mono, 13.0),
            TextColor(theme::INK_DIM),
            TextLayout::no_wrap(),
        ))
        .id();
    // Without symbols the line is another language's example: drawn plain.
    let tokens = symbols.map_or_else(
        || vec![(source, syntax::Kind::Argument)],
        |symbols| syntax::tokens(source, symbols),
    );
    for (token, kind) in tokens {
        let color = kind.color();
        let span = commands
            .spawn((TextSpan::new(token), font(mono, 13.0), TextColor(color)))
            .id();
        commands.entity(text).add_child(span);
    }
    commands.entity(row).add_child(text);
    commands.entity(parent).add_child(row);
}

/// Give the wheel to the editor or help pane without zooming the game camera.
#[allow(clippy::needless_pass_by_value)]
#[allow(
    clippy::type_complexity,
    reason = "the query shares scrolling across two Bevy panes"
)]
pub(super) fn scroll(
    wheel: Res<Wheel>,
    state: Res<Workbench>,
    mut page: Local<Option<usize>>,
    mut panels: Query<
        (Entity, &ComputedNode, &mut ScrollPosition, Has<HelpScroll>),
        Or<(With<HelpScroll>, With<FlowPane>)>,
    >,
    mut editors: Query<
        (
            Entity,
            &ComputedNode,
            &bevy::text::TextLayoutInfo,
            &mut TextScroll,
        ),
        With<Field>,
    >,
) {
    for (entity, node, mut scroll, help) in &mut panels {
        if help && *page != Some(state.help_page) {
            scroll.0.y = 0.0;
        }
        if wheel.surface == Some(entity) {
            windows::scroll_by(&mut scroll, node, wheel.pixels);
        }
    }
    *page = Some(state.help_page);
    for (entity, node, text, mut scroll) in &mut editors {
        if wheel.surface == Some(entity) {
            let maximum = (text.size.y - node.content_box().height()).max(0.0);
            scroll.0.y =
                (scroll.0.y - wheel.pixels / node.inverse_scale_factor()).clamp(0.0, maximum);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_execution_source_shows_as_many_rows_as_its_pane_holds() {
        assert_eq!(visible_rows(0.0), DEFAULT_ROWS);
        assert_eq!(visible_rows(f32::NAN), DEFAULT_ROWS);
        assert_eq!(visible_rows(2.0f32.mul_add(PANE_PADDING, 14.0 * ROW)), 14);
        assert_eq!(visible_rows(2.0f32.mul_add(PANE_PADDING, 40.5 * ROW)), 40);
        assert_eq!(visible_rows(30.0), 3);
        // Line 7 of 14 rows starts the view at line 1; later lines scroll.
        assert_eq!(first_row(7, 14), 1);
        assert_eq!(first_row(1, 14), 0);
        assert_eq!(first_row(100, 40), 83);
        assert!((first_row(100, 40)..first_row(100, 40) + 40).contains(&99));
    }

    #[test]
    fn run_controls_are_usable_exactly_when_their_action_can_happen() {
        let controls = [
            Action::Run,
            Action::RunToCursor,
            Action::Pause,
            Action::Resume,
            Action::Step,
            Action::StepOut,
            Action::Stop,
        ];
        let usable = |phase, busy| controls.map(|action| action.usable(phase, busy));
        // Run, run to cursor, pause, resume, step, step out, stop.
        let (t, f) = (true, false);
        assert_eq!(usable(Phase::Idle, false), [t, t, f, f, f, f, f]);
        assert_eq!(usable(Phase::Idle, true), [false; 7]);
        assert_eq!(usable(Phase::Running, false), [f, f, t, f, f, f, t]);
        assert_eq!(usable(Phase::Pausing, false), [f, f, t, f, f, f, t]);
        // While paused, run to cursor resumes to the line asked for.
        assert_eq!(usable(Phase::Paused, false), [f, t, f, t, t, t, t]);
        for done in [Phase::Completed, Phase::Stopped, Phase::Failed] {
            assert_eq!(usable(done, false), [t, t, f, f, f, f, f]);
        }
    }
}
