//! The in-world view of the harness's reports.
use bevy::{
    prelude::*,
    text::{Justify, LineBreak},
};

use super::{super::Bridge, Row, State, admission_id};
use crate::{
    login::{FrontEnd, Screen},
    online::OnlineState,
    theme::{self, Size},
    windows::{
        self, Shown, WindowId,
        pointer::{TakesWheel, Wheel},
    },
};

#[derive(Component)]
struct Root;
#[derive(Component)]
enum Heading {
    Title,
    Progress,
    Health,
    Export,
}
#[derive(Component)]
struct Rows;
#[derive(Component)]
struct ExportButton;

pub(in crate::script::control) fn install(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, export.in_set(crate::Stage::Input))
        .add_systems(Update, refresh.in_set(crate::Stage::Present));
}

#[allow(clippy::needless_pass_by_value)]
fn spawn(mut commands: Commands) {
    let frame = windows::frame(
        &mut commands,
        WindowId::Runbook,
        Node {
            display: Display::None,
            width: px(380),
            height: px(360),
            max_width: percent(90),
            max_height: percent(70),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    commands.entity(frame).insert(Root).with_children(|frame| {
        frame
            .spawn(Node {
                flex_grow: 1.0,
                min_height: px(0),
                padding: UiRect::all(px(8)),
                row_gap: px(6),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|body| {
                body.spawn((
                    Heading::Title,
                    theme::text("No checklist loaded", Size::Heading, theme::INK_BRIGHT),
                    TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                ));
                body.spawn((
                    Heading::Progress,
                    theme::text("Harness reports appear here.", Size::Small, theme::INK),
                    TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                ));
                body.spawn((
                    Heading::Health,
                    theme::text(
                        "Waiting for the control channel",
                        Size::Small,
                        theme::INK_DIM,
                    ),
                    TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                ));
                body.spawn((
                    Heading::Export,
                    theme::text("", Size::Small, theme::INK_DIM),
                    TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                ));
                body.spawn((
                    Rows,
                    TakesWheel,
                    windows::KeepsPress,
                    ScrollPosition::default(),
                    Node {
                        flex_grow: 1.0,
                        min_height: px(0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ));
                body.spawn(Node {
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: px(6),
                    ..default()
                })
                .with_children(|footer| {
                    theme::button_with(footer, ExportButton, "Export JSON", Size::Small);
                    theme::button_with(
                        footer,
                        crate::skinned::Closes(WindowId::Runbook),
                        "Close",
                        Size::Small,
                    );
                });
            });
    });
}

#[allow(clippy::needless_pass_by_value)]
fn export(
    clicks: crate::clicks::Clicks<ExportButton>,
    mut bridge: ResMut<Bridge>,
    (front, online, shown): (Res<FrontEnd>, Res<OnlineState>, Res<Shown>),
    mut button: Query<(&Interaction, &mut BackgroundColor), With<ExportButton>>,
) {
    let runbook_id = bridge.runbook.as_ref().map(|run| run.id);
    let usable = runbook_id.is_some() && !bridge.export.busy();
    if let Ok((interaction, mut color)) = button.single_mut() {
        color.0 = theme::button(usable, false, *interaction);
    }
    if usable
        && shown.displayed(WindowId::Runbook)
        && front.screen(&online) == Screen::World
        && clicks.any()
        && let Some(id) = runbook_id
    {
        let _ = bridge.export_runbook(id, None, admission_id(&online));
    }
}

#[allow(clippy::needless_pass_by_value)]
fn refresh(
    mut commands: Commands,
    bridge: Res<Bridge>,
    (front, online, shown): (Res<FrontEnd>, Res<OnlineState>, Res<Shown>),
    (wheel, mut previous): (Res<Wheel>, Local<Option<(u64, u64)>>),
    mut root: Query<&mut Node, With<Root>>,
    mut headings: Query<(&Heading, &mut Text)>,
    mut list: Query<(Entity, &ComputedNode, &mut ScrollPosition), With<Rows>>,
) {
    let Ok(mut root) = root.single_mut() else {
        return;
    };
    let open = shown.displayed(WindowId::Runbook) && front.screen(&online) == Screen::World;
    let display = if open { Display::Flex } else { Display::None };
    if root.display != display {
        root.display = display;
    }
    let Ok((parent, node, mut scroll)) = list.single_mut() else {
        return;
    };
    if !open {
        return;
    }
    if wheel.surface == Some(parent) {
        windows::scroll_by(&mut scroll, node, wheel.pixels);
    }
    let run = bridge.runbook.as_ref();
    for (heading, mut text) in &mut headings {
        match heading {
            Heading::Health => {
                let health = match bridge.channel_health() {
                    "ready" => "Control channel ready",
                    "io_error" => "Control file I/O failed",
                    "request_error" => "Invalid control request",
                    "stalled" => "Control exchange stalled",
                    "closed" => "Control channel closed",
                    _ => "Waiting for the control channel",
                };
                let value = admission_id(&online).map_or_else(
                    || health.to_owned(),
                    |id| format!("{health} · Admission #{id}"),
                );
                if text.0 != value {
                    text.0 = value;
                }
            }
            Heading::Export => {
                let status = run.map_or("", |run| bridge.export.label(run));
                if text.0 != status {
                    status.clone_into(&mut text.0);
                }
            }
            _ => (),
        }
    }
    let Some(run) = run else { return };
    let key = (run.id, run.revision);
    if *previous == Some(key) {
        return;
    }
    if previous.is_none_or(|(id, _)| id != run.id) {
        *scroll = ScrollPosition::default();
    }
    *previous = Some(key);
    let counts = run.counts();
    for (heading, mut text) in &mut headings {
        match heading {
            Heading::Title => text.0.clone_from(&run.title),
            Heading::Progress => {
                text.0 = format!(
                    "Harness run #{} · {} pending · {} running · {} blocked · {} skipped · {} passed · {} failed",
                    run.id,
                    counts.pending,
                    counts.running,
                    counts.blocked,
                    counts.skipped,
                    counts.passed,
                    counts.failed,
                );
            }
            _ => (),
        }
    }
    commands
        .entity(parent)
        .despawn_children()
        .with_children(|list| {
            for row in &run.rows {
                draw_row(list, row);
            }
        });
}

fn draw_row(list: &mut ChildSpawnerCommands, row: &Row) {
    let report = row.report();
    let (status, ink) = match report.map(|report| report.state) {
        Some(State::Running) => ("[…] Running", theme::INK_BRIGHT),
        Some(State::Blocked) => ("[!] Blocked", theme::INK_WARM),
        Some(State::Skipped) => ("[–] Skipped", theme::INK_DIM),
        Some(State::Passed) => ("[x] Passed", theme::AGREED),
        Some(State::Failed) => ("[!] Failed", theme::INK_WARM),
        None => ("[ ] Pending", theme::INK),
    };
    list.spawn((
        Node {
            width: percent(100),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(6), px(4)),
            ..default()
        },
        BackgroundColor(theme::INSET),
    ))
    .with_children(|entry| {
        entry.spawn((
            theme::text(format!("{status} · {}", row.item.label), Size::Body, ink),
            TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
        ));
        if let Some(note) = report.and_then(|report| report.note.as_ref()) {
            entry.spawn((
                theme::text(note.as_str(), Size::Small, theme::INK),
                TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
            ));
        }
        if let Some(evidence) = report.and_then(|report| report.evidence.as_ref()) {
            entry.spawn((
                theme::text(
                    format!(
                        "Reported evidence · {} · {}",
                        evidence.kind.label(),
                        evidence.reference
                    ),
                    Size::Small,
                    theme::INK_DIM,
                ),
                TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
            ));
        }
        if let Some(id) = report.and_then(|report| report.admission_id) {
            entry.spawn(theme::text(
                format!("Report scoped to admission #{id}"),
                Size::Small,
                theme::INK_DIM,
            ));
        }
    });
}
