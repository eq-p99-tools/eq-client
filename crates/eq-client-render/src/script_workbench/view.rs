//! Workbench chrome and views; no file access or script execution lives here.

use bevy::{
    prelude::*,
    text::{EditableText, TextCursorStyle, TextEdit},
    ui::widget::TextScroll,
};

use super::{Tab, Workbench, help, syntax};
use crate::{
    script::{Phase, RunStatus},
    theme,
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
    Pause,
    Resume,
    Step,
    Stop,
    Discard,
    Cancel,
    Tab(Tab),
    Help(usize),
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
    run: Option<(Option<usize>, Phase, String)>,
    help_page: Option<usize>,
    validation: Option<(String, Option<std::path::PathBuf>, String)>,
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
    state: Res<Workbench>,
) {
    let mono = Mono(fonts.add(Font::from_bytes(bevy::text::DEFAULT_FONT_DATA.to_vec())));
    let frame = windows::frame(
        &mut commands,
        WindowId::Scripts,
        Node {
            display: Display::None,
            width: px(1040),
            max_width: percent(94),
            max_height: percent(92),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    commands.entity(frame).insert(Root);
    let body = commands
        .spawn((
            Node {
                padding: UiRect::all(px(12)),
                row_gap: px(8),
                flex_direction: FlexDirection::Column,
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
    let controls = row(
        &mut commands,
        &[
            ("Run", Action::Run),
            ("Pause", Action::Pause),
            ("Resume", Action::Resume),
            ("Step", Action::Step),
            ("Stop", Action::Stop),
        ],
    );
    let editor = commands
        .spawn((
            Page(Tab::Editor),
            Node {
                column_gap: px(10),
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
    let file_controls = row(
        &mut commands,
        &[
            ("Files", Action::ToggleLibrary),
            ("New", Action::New),
            ("Open", Action::Open),
            ("Save", Action::Save),
            ("Reload", Action::Reload),
        ],
    );
    let source = commands
        .spawn((
            Field::Source,
            Interaction::default(),
            windows::KeepsPress,
            TakesWheel,
            super::editor::Surface,
            Node {
                height: px(280),
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
                ..default()
            },
        ))
        .id();
    let lines = commands
        .spawn((
            SourceLines,
            Node {
                height: px(280),
                width: percent(100),
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
    commands.entity(execution).add_children(&[lines, history]);
    let help_page = commands
        .spawn((
            Page(Tab::Help),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
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
                height: px(380),
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
        controls,
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
    state: Res<Workbench>,
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
        ),
        Or<(
            With<Page>,
            With<Root>,
            With<Confirmation>,
            With<LibraryPane>,
        )>,
    >,
    mut texts: Query<
        (
            &mut Text,
            Has<Status>,
            Has<Message>,
            Has<History>,
            Has<Validation>,
        ),
        Or<(With<Status>, With<Message>, With<History>, With<Validation>)>,
    >,
    mut fields: Query<(&Field, &mut EditableText, &mut BorderColor, &mut TextScroll)>,
    focus: Res<bevy::input_focus::InputFocus>,
    field_ids: Query<(Entity, &Field)>,
    mut buttons: Query<(&Action, &Interaction, &mut BackgroundColor)>,
    lines: Query<Entity, With<SourceLines>>,
    help_body: Query<(Entity, Has<Breadcrumbs>), Or<(With<HelpBody>, With<Breadcrumbs>)>>,
    mut previous: Local<Cache>,
) {
    let active = status.phase().active();
    for (mut node, page, root, confirmation, library) in &mut nodes {
        let visible = if root {
            shown.displayed(WindowId::Scripts)
        } else if confirmation {
            state.confirm.is_some()
        } else if library {
            !state.library_collapsed
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
    if previous
        .validation
        .as_ref()
        .is_none_or(|(text, path, _)| *text != state.document.text || *path != state.document.path)
    {
        let result = match state.document.program() {
            Ok(program) => format!(
                "{} actions  ·  {} lines  ·  UTF-8  ·  # comments",
                program.instructions().len(),
                state.document.text.lines().count()
            ),
            Err(error) => {
                let count = crate::script::Program::diagnostics(
                    &state.document.text,
                    std::path::Path::new("."),
                )
                .len();
                format!("{count} error(s) · {error} · Hover an underlined line for details")
            }
        };
        previous.validation = Some((
            state.document.text.clone(),
            state.document.path.clone(),
            result,
        ));
    }
    for (mut text, is_status, message, history, validation) in &mut texts {
        let value = if is_status {
            format!(
                "{}  ·  {done}/{total} actions  ·  {:.1}s{}",
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
        } else if validation {
            previous
                .validation
                .as_ref()
                .map_or_else(String::new, |(_, _, result)| result.clone())
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
        let usable = match action {
            Action::Run
            | Action::New
            | Action::Open
            | Action::Save
            | Action::Reload
            | Action::Discard => !active && !state.busy(),
            Action::Pause => active && status.phase() != Phase::Paused,
            Action::Resume | Action::Step => status.phase() == Phase::Paused,
            Action::Stop => active,
            _ => true,
        };
        let on = matches!(action, Action::Tab(tab) if *tab == state.tab)
            || matches!(action, Action::Help(page) if *page == state.help_page)
            || matches!(action, Action::ToggleLibrary) && !state.library_collapsed;
        *color = BackgroundColor(theme::button(usable, on, *interaction));
    }
    // Rebuild at action boundaries, not at frame rate or every elapsed-time tick.
    if previous.run.as_ref().is_none_or(|(line, phase, source)| {
        *line != status.line() || *phase != status.phase() || source != status.source()
    }) {
        if let Ok(parent) = lines.single() {
            commands.entity(parent).despawn_children();
            let current = status.line().unwrap_or(1);
            let start = current.saturating_sub(6);
            for (index, line) in status.source().lines().enumerate().skip(start).take(14) {
                source_line(
                    &mut commands,
                    parent,
                    line,
                    Some(index + 1),
                    status.line() == Some(index + 1),
                    &mono,
                );
            }
        }
        previous.run = Some((status.line(), status.phase(), status.source().to_owned()));
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

/// A read-only source line with stable syntax colors and an execution marker.
pub(super) fn source_line(
    commands: &mut Commands,
    parent: Entity,
    source: &str,
    line: Option<usize>,
    active: bool,
    mono: &Mono,
) {
    let row = commands
        .spawn((
            Node {
                width: percent(100),
                min_height: px(18),
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
                format!("{}{line:>4}  ", if active { "▶" } else { " " })
            })),
            font(mono, 13.0),
            TextColor(theme::INK_DIM),
            TextLayout::no_wrap(),
        ))
        .id();
    for (token, kind) in syntax::tokens(source) {
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
pub(super) fn scroll(
    wheel: Res<Wheel>,
    state: Res<Workbench>,
    mut page: Local<Option<usize>>,
    mut panels: Query<(Entity, &ComputedNode, &mut ScrollPosition), With<HelpScroll>>,
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
    for (entity, node, mut scroll) in &mut panels {
        if *page != Some(state.help_page) {
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
