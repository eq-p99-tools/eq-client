//! The Quest Journal: the player's tasks and quests, each one's steps,
//! description, reward and time, the finished list and each finished
//! task's steps, and Cancel (checklist T4). It is this client's own window
//! until the skin's task window (`TaskWnd`) is listed; its words and the
//! time left are ours (inferred, R49-38).
//!
//! Cancel asks again inside this window, Yes or No. A script cancels with
//! `cancel_task` and asks for a finished task's steps with `task_history`,
//! since a script may do whatever the player can. Where the admission doesn't
//! offer [`Capability::Tasks`] the journal is read only. The task's words
//! are shown here only, never logged.
use crate::{
    clicks::Clicks,
    online::OnlineState,
    outbox::Outbox,
    theme::{self, Size},
    windows::{self, Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{
    Capability, ClientCommand,
    tasks::{self, CompletedTask, Step, TaskEntry, TaskHistory, TaskKind, TaskSlot},
    world::ClientWorld,
};
use std::time::{SystemTime, UNIX_EPOCH};

/// The journal's two pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Page {
    /// The tasks and quests held now.
    #[default]
    Active,
    /// The tasks finished.
    Finished,
}

/// What a journal button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    /// Shows a page.
    Page(Page),
    /// Shows a held entry.
    Entry(TaskSlot),
    /// Shows a finished task, by its index in the list, asking for its
    /// steps where the admission takes task requests.
    Finished(u32),
    /// Asks whether to cancel the shown entry.
    Cancel,
    /// Answers that question: Yes (true) or No.
    Confirm(bool),
}

#[derive(Clone, Copy, Component)]
pub(super) struct JournalButton(pub(super) Action);

#[derive(Component)]
pub(super) struct JournalFrame;

#[derive(Component)]
pub(super) struct JournalBody;

/// A cancel the player asked about, until they answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cancelling {
    session_id: u64,
    at: TaskSlot,
    task_id: u32,
}

/// The player's choices in the journal.
#[derive(Resource, Default)]
pub(super) struct JournalState {
    page: Page,
    entry: Option<TaskSlot>,
    finished: Option<u32>,
    cancelling: Option<Cancelling>,
    /// The admission and count of the last open the server asked for.
    opened: Option<(u64, u64)>,
    /// The finished tasks whose steps were asked for in this admission.
    histories_asked: (u64, std::collections::BTreeSet<u32>),
}

/// What the window shows; it is drawn again when this changes.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct View {
    page: Page,
    answerable: bool,
    entries: Vec<(TaskSlot, String)>,
    entry: Option<TaskEntry>,
    left: Option<String>,
    finished: Vec<CompletedTask>,
    chosen_finished: Option<(u32, Option<TaskHistory>)>,
    asking: bool,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The time an entry has left, in our words (inferred): its start and
/// duration against this machine's clock, to the minute. The server is the
/// judge of when it ends.
fn time_left(entry: &TaskEntry, now: u64) -> Option<String> {
    let description = &entry.description;
    // No line without a duration, or without the start it counts from.
    if description.duration_seconds == 0 || description.started_at == 0 {
        return None;
    }
    let ends = u64::from(description.started_at) + u64::from(description.duration_seconds);
    let left = ends.saturating_sub(now);
    if left == 0 {
        return Some("Time left: none".to_owned());
    }
    let minutes = left.div_ceil(60).saturating_mul(60);
    let minutes = u32::try_from(minutes).unwrap_or(u32::MAX);
    Some(format!("Time left: {}", tasks::duration_words(minutes, 0)))
}

fn chosen_entry(world: &ClientWorld, state: &JournalState) -> Option<TaskEntry> {
    let journal = world.task_journal();
    state
        .entry
        .and_then(|at| journal.entry(at))
        .or_else(|| journal.entries().first())
        .cloned()
}

fn view(world: &ClientWorld, state: &JournalState, now: u64) -> View {
    let journal = world.task_journal();
    let entry = chosen_entry(world, state);
    let finished = journal.completed().unwrap_or_default().to_vec();
    View {
        page: state.page,
        answerable: world.can(Capability::Tasks),
        entries: journal
            .entries()
            .iter()
            .map(|entry| (entry.description.at, entry.description.title.clone()))
            .collect(),
        left: entry.as_ref().and_then(|entry| time_left(entry, now)),
        entry,
        chosen_finished: state
            .finished
            .filter(|index| usize::try_from(*index).is_ok_and(|index| index < finished.len()))
            .map(|index| (index, journal.history(index).cloned())),
        finished,
        asking: state.cancelling.is_some(),
    }
}

/// Opens the journal when the server asks, closes it out of the world, and
/// keeps its frame while it is shown.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn window(
    mut commands: Commands,
    online: Res<OnlineState>,
    mut state: ResMut<JournalState>,
    mut shown: ResMut<Shown>,
    frames: Query<Entity, With<JournalFrame>>,
) {
    let world = online.world();
    if !world.connected() || world.player().is_none() {
        shown.close(WindowId::TaskJournal);
        state.cancelling = None;
    }
    // A question is dropped with its admission, and whenever the entry
    // shown is no longer the one it asks about.
    if let Some(asked) = state.cancelling {
        let shown = chosen_entry(world, &state)
            .map(|entry| (entry.description.at, entry.description.task_id));
        if world.session_id() != Some(asked.session_id) || shown != Some((asked.at, asked.task_id))
        {
            state.cancelling = None;
        }
    }
    if let Some(session_id) = world.session_id() {
        let asked = (session_id, world.task_journal().open_asked());
        if asked.1 != 0 && state.opened != Some(asked) {
            shown.open(WindowId::TaskJournal);
        }
        state.opened = Some(asked);
    }
    if !shown.displayed(WindowId::TaskJournal) {
        for frame in &frames {
            commands.entity(frame).despawn();
        }
        state.cancelling = None;
        return;
    }
    if !frames.is_empty() {
        return;
    }
    let frame = windows::frame(
        &mut commands,
        WindowId::TaskJournal,
        Node {
            width: px(600),
            max_width: percent(92),
            height: px(420),
            max_height: percent(80),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    commands.entity(frame).insert(JournalFrame).with_child((
        JournalBody,
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: UiRect::all(px(10)),
            ..default()
        },
    ));
}

/// Draws the journal again when what it shows changes.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn fill(
    mut commands: Commands,
    online: Res<OnlineState>,
    state: Res<JournalState>,
    bodies: Query<(Entity, Option<&Children>), With<JournalBody>>,
    mut drawn: Local<Option<(Entity, View)>>,
) {
    let Ok((body, children)) = bodies.single() else {
        *drawn = None;
        return;
    };
    let wanted = view(online.world(), &state, unix_now());
    if drawn
        .as_ref()
        .is_some_and(|(entity, view)| *entity == body && *view == wanted)
    {
        return;
    }
    for child in children.into_iter().flatten() {
        commands.entity(*child).despawn();
    }
    commands
        .entity(body)
        .with_children(|body| draw(body, &wanted));
    *drawn = Some((body, wanted));
}

fn tab(parent: &mut ChildSpawnerCommands, page: Page, shown: Page, label: &str) {
    let label = if page == shown {
        format!("> {label}")
    } else {
        label.to_owned()
    };
    theme::button_with(
        parent,
        JournalButton(Action::Page(page)),
        &label,
        Size::Small,
    );
}

fn column() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(4),
        overflow: Overflow::scroll_y(),
        ..default()
    }
}

fn draw(body: &mut ChildSpawnerCommands, view: &View) {
    body.spawn(Node {
        column_gap: px(6),
        ..default()
    })
    .with_children(|tabs| {
        tab(tabs, Page::Active, view.page, "Active");
        tab(tabs, Page::Finished, view.page, "Finished");
    });
    body.spawn(Node {
        flex_grow: 1.0,
        min_height: px(0),
        column_gap: px(8),
        ..default()
    })
    .with_children(|columns| match view.page {
        Page::Active => active(columns, view),
        Page::Finished => finished(columns, view),
    });
    if !view.answerable {
        body.spawn(theme::text(
            "Tasks can't be changed on this server.",
            Size::Small,
            theme::INK_DIM,
        ));
    }
    if view.page == Page::Active && view.answerable && view.entry.is_some() {
        body.spawn(Node {
            column_gap: px(8),
            justify_content: JustifyContent::End,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|buttons| {
            if view.asking {
                let title = view
                    .entry
                    .as_ref()
                    .map_or("", |entry| entry.description.title.as_str());
                buttons.spawn(theme::text(
                    format!("Cancel {title}?"),
                    Size::Body,
                    theme::INK_WARM,
                ));
                theme::button_with(
                    buttons,
                    JournalButton(Action::Confirm(true)),
                    "Yes",
                    Size::Body,
                );
                theme::button_with(
                    buttons,
                    JournalButton(Action::Confirm(false)),
                    "No",
                    Size::Body,
                );
            } else {
                theme::button_with(buttons, JournalButton(Action::Cancel), "Cancel", Size::Body);
            }
        });
    }
}

fn kind_word(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Task => "Task",
        TaskKind::Quest => "Quest",
    }
}

fn active(columns: &mut ChildSpawnerCommands, view: &View) {
    let chosen = view.entry.as_ref().map(|entry| entry.description.at);
    columns
        .spawn((
            Node {
                width: px(200),
                flex_shrink: 0.0,
                ..column()
            },
            windows::pointer::TakesWheel,
            ScrollPosition::default(),
        ))
        .with_children(|list| {
            if view.entries.is_empty() {
                list.spawn(theme::text("No tasks", Size::Small, theme::INK_DIM));
            }
            for (at, title) in &view.entries {
                let mark = if Some(*at) == chosen { "> " } else { "" };
                theme::button_with(
                    list,
                    JournalButton(Action::Entry(*at)),
                    &format!("{mark}{title}"),
                    Size::Small,
                );
            }
        });
    let Some(entry) = &view.entry else {
        return;
    };
    columns
        .spawn((
            Node {
                flex_grow: 1.0,
                padding: UiRect::all(px(6)),
                ..column()
            },
            BackgroundColor(theme::WELL),
            windows::pointer::TakesWheel,
            ScrollPosition::default(),
        ))
        .with_children(|details| {
            let description = &entry.description;
            details.spawn(theme::text(
                &description.title,
                Size::Heading,
                theme::INK_WARM,
            ));
            details.spawn(theme::text(
                format!(
                    "{}  ·  Duration: {}",
                    kind_word(description.at.kind),
                    tasks::duration_words(description.duration_seconds, description.duration_code)
                ),
                Size::Small,
                theme::INK,
            ));
            if let Some(left) = &view.left {
                details.spawn(theme::text(left, Size::Small, theme::INK));
            }
            if !description.description.trim().is_empty() {
                details.spawn(theme::text(
                    description.description.trim(),
                    Size::Body,
                    theme::INK_BRIGHT,
                ));
            }
            for line in reward_lines(entry) {
                details.spawn(theme::text(line, Size::Small, theme::INK));
            }
            for step in entry.steps.values() {
                details.spawn(theme::text(step_line(&step.step), Size::Body, theme::INK));
            }
        });
}

/// A step's line: its words, its count and whether it may be left undone,
/// or only `???` while hidden; the words are ours (inferred).
fn step_line(step: &Step) -> String {
    match step {
        // A step not yet open shows nothing of itself.
        Step::Hidden { .. } => "- ???".to_owned(),
        Step::Shown(objective) => {
            let words = tasks::objective_words(
                objective.activity_type,
                &objective.target_name,
                &objective.item_list,
                objective.goal,
                &objective.description,
            );
            let optional = if objective.optional {
                " (optional)"
            } else {
                ""
            };
            format!(
                "- {words}: {} / {}{optional}",
                objective.done.max(0),
                objective.goal.max(0)
            )
        }
    }
}

/// The reward's lines, in our words (inferred): its text, coin, and whether
/// it gives experience or changes faction. The reward item's link is left
/// for the read of how the program shows it.
fn reward_lines(entry: &TaskEntry) -> Vec<String> {
    let rewards = &entry.description.rewards;
    let mut lines = Vec::new();
    if !rewards.text.trim().is_empty() {
        lines.push(format!("Reward: {}", rewards.text.trim()));
    }
    if rewards.coin_copper != 0 {
        lines.push(format!("Coin: {}", coin_words(rewards.coin_copper)));
    }
    match (rewards.experience, rewards.faction) {
        (true, true) => lines.push("Experience and faction".to_owned()),
        (true, false) => lines.push("Experience".to_owned()),
        (false, true) => lines.push("Faction".to_owned()),
        (false, false) => {}
    }
    lines
}

fn coin_words(copper: u32) -> String {
    let pieces = [
        (copper / 1000, "p"),
        (copper / 100 % 10, "g"),
        (copper / 10 % 10, "s"),
        (copper % 10, "c"),
    ];
    pieces
        .iter()
        .filter(|(count, _)| *count != 0)
        .map(|(count, unit)| format!("{count}{unit}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn finished(columns: &mut ChildSpawnerCommands, view: &View) {
    let chosen = view.chosen_finished.as_ref().map(|(index, _)| *index);
    columns
        .spawn((
            Node {
                width: px(240),
                flex_shrink: 0.0,
                ..column()
            },
            windows::pointer::TakesWheel,
            ScrollPosition::default(),
        ))
        .with_children(|list| {
            if view.finished.is_empty() {
                list.spawn(theme::text(
                    "No finished tasks",
                    Size::Small,
                    theme::INK_DIM,
                ));
            }
            for (index, task) in view.finished.iter().enumerate().rev() {
                let Ok(index) = u32::try_from(index) else {
                    continue;
                };
                let mark = if Some(index) == chosen { "> " } else { "" };
                theme::button_with(
                    list,
                    JournalButton(Action::Finished(index)),
                    &format!("{mark}{}", task.title),
                    Size::Small,
                );
            }
        });
    let Some((index, history)) = &view.chosen_finished else {
        return;
    };
    columns
        .spawn((
            Node {
                flex_grow: 1.0,
                padding: UiRect::all(px(6)),
                ..column()
            },
            BackgroundColor(theme::WELL),
            windows::pointer::TakesWheel,
            ScrollPosition::default(),
        ))
        .with_children(|details| {
            if let Some(task) = usize::try_from(*index)
                .ok()
                .and_then(|index| view.finished.get(index))
            {
                details.spawn(theme::text(&task.title, Size::Heading, theme::INK_WARM));
            }
            match history {
                None => {
                    details.spawn(theme::text(
                        "No steps received",
                        Size::Small,
                        theme::INK_DIM,
                    ));
                }
                Some(history) => {
                    for step in &history.steps {
                        let words = tasks::objective_words(
                            i32::try_from(step.activity_type).unwrap_or(i32::MAX),
                            &step.target_name,
                            &step.item_list,
                            i32::try_from(step.goal).unwrap_or(i32::MAX),
                            &step.description,
                        );
                        details.spawn(theme::text(format!("- {words}"), Size::Body, theme::INK));
                    }
                }
            }
        });
}

/// Does what a journal button says.
pub(super) fn press(
    online: &OnlineState,
    outbox: &Outbox,
    state: &mut JournalState,
    action: Action,
) -> Result<(), &'static str> {
    let world = online.world();
    match action {
        Action::Page(page) => {
            state.page = page;
            state.cancelling = None;
        }
        Action::Entry(at) => {
            if world.task_journal().entry(at).is_none() {
                return Err("That journal entry is gone");
            }
            state.entry = Some(at);
            state.cancelling = None;
        }
        Action::Finished(index) => {
            let listed = world.task_journal().completed().map_or(0, <[_]>::len);
            if usize::try_from(index).map_or(true, |index| index >= listed) {
                return Err("That finished task is gone");
            }
            state.finished = Some(index);
            let Some(session_id) = world.session_id() else {
                return Ok(());
            };
            if state.histories_asked.0 != session_id {
                state.histories_asked = (session_id, std::collections::BTreeSet::new());
            }
            // Asked once per finished task in an admission, answered or not.
            if world.can(Capability::Tasks)
                && world.task_journal().history(index).is_none()
                && !state.histories_asked.1.contains(&index)
            {
                outbox
                    .try_post(world, |stamp| ClientCommand::AskTaskHistory {
                        session_id: stamp.session_id,
                        index,
                        created: stamp.created,
                    })
                    .map_err(crate::outbox::Refusal::text)?;
                state.histories_asked.1.insert(index);
            }
        }
        Action::Cancel => {
            if !world.can(Capability::Tasks) {
                return Err("Tasks can't be changed on this server");
            }
            let (Some(session_id), Some(entry)) = (world.session_id(), chosen_entry(world, state))
            else {
                return Err("No journal entry is shown");
            };
            // The entry asked about stays shown until the answer.
            state.entry = Some(entry.description.at);
            state.cancelling = Some(Cancelling {
                session_id,
                at: entry.description.at,
                task_id: entry.description.task_id,
            });
        }
        Action::Confirm(yes) => {
            let Some(asked) = state.cancelling.take() else {
                return Err("No cancel was asked");
            };
            if !yes {
                return Ok(());
            }
            let same = world.session_id() == Some(asked.session_id)
                && world
                    .task_journal()
                    .entry(asked.at)
                    .is_some_and(|entry| entry.description.task_id == asked.task_id);
            if !same {
                return Err("That journal entry changed");
            }
            if !world.can(Capability::Tasks) {
                return Err("Tasks can't be changed on this server");
            }
            outbox
                .try_post(world, |stamp| ClientCommand::CancelTask {
                    session_id: stamp.session_id,
                    at: asked.at,
                    created: stamp.created,
                })
                .map_err(crate::outbox::Refusal::text)?;
        }
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn buttons(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut state: ResMut<JournalState>,
    mut chat: ResMut<crate::chat::ChatState>,
    clicks: Clicks<JournalButton>,
) {
    if let Some(button) = clicks.iter().next().copied()
        && let Err(reason) = press(&online, &outbox, &mut state, button.0)
    {
        chat.refuse(reason);
    }
}

#[cfg(test)]
mod tests;
