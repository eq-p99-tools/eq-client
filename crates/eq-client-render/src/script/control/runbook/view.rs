//! The in-world view of the harness's reports.
use bevy::{
    prelude::*,
    text::{Justify, LineBreak},
};

use super::{super::Bridge, Outcome, Row};
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
struct Title;
#[derive(Component)]
struct Progress;
#[derive(Component)]
struct Rows;

pub(in crate::script::control) fn install(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, refresh.in_set(crate::Stage::Present));
}

#[allow(clippy::needless_pass_by_value)]
fn spawn(mut commands: Commands, bridge: Option<Res<Bridge>>) {
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
                    Title,
                    theme::text("No checklist loaded", Size::Heading, theme::INK_BRIGHT),
                    TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                ));
                let hint = if bridge.is_some() {
                    "Waiting for the local test harness."
                } else {
                    "Launch with --script-control to load a checklist."
                };
                body.spawn((Progress, theme::text(hint, Size::Small, theme::INK)));
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
                    footer.spawn(theme::text(
                        "A new harness run resets the checklist.",
                        Size::Small,
                        theme::INK_DIM,
                    ));
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

type Headings<'w, 's> =
    Query<'w, 's, (&'static mut Text, Has<Title>), Or<(With<Title>, With<Progress>)>>;

#[allow(clippy::needless_pass_by_value)]
fn refresh(
    mut commands: Commands,
    bridge: Option<Res<Bridge>>,
    (front, online, shown): (Res<FrontEnd>, Res<OnlineState>, Res<Shown>),
    (wheel, mut previous): (Res<Wheel>, Local<Option<(u64, usize)>>),
    mut root: Query<&mut Node, With<Root>>,
    mut headings: Headings,
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
    let Some(run) = bridge.as_ref().and_then(|bridge| bridge.runbook.as_ref()) else {
        return;
    };
    let (mut passed, mut failed) = (0, 0);
    for row in &run.rows {
        match row.report.as_ref().map(|report| report.outcome) {
            Some(Outcome::Passed) => passed += 1,
            Some(Outcome::Failed) => failed += 1,
            None => (),
        }
    }
    let reported = passed + failed;
    // Reports are terminal within a run, so the count changes on every new row result.
    let key = (run.id, reported);
    if *previous == Some(key) {
        return;
    }
    if previous.is_none_or(|(id, _)| id != run.id) {
        *scroll = ScrollPosition::default();
    }
    *previous = Some(key);
    let total = run.rows.len();
    for (mut text, title) in &mut headings {
        text.0 = if title {
            run.title.clone()
        } else {
            format!(
                "Harness run #{} · {reported}/{total} reported · {passed} passed · {failed} failed",
                run.id
            )
        };
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
    let (status, ink) = match row.report.as_ref().map(|report| report.outcome) {
        Some(Outcome::Passed) => ("[x] Passed", theme::AGREED),
        Some(Outcome::Failed) => ("[!] Failed", theme::INK_WARM),
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
        if let Some(note) = row.report.as_ref().and_then(|report| report.note.as_ref()) {
            entry.spawn((
                theme::text(note.as_str(), Size::Small, theme::INK),
                TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
            ));
        }
    });
}
