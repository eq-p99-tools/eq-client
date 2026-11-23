use super::*;
use crate::online::testing;
use eq_client_core::{
    WorldEvent,
    tasks::{Objective, TaskActivity, TaskDescription},
};
use eq_network_game::tasks::Rewards;
use std::sync::mpsc::{Receiver, sync_channel};

const QUEST_3: TaskSlot = TaskSlot {
    kind: TaskKind::Quest,
    slot: 3,
};
const QUEST_5: TaskSlot = TaskSlot {
    kind: TaskKind::Quest,
    slot: 5,
};
const THE_TASK: TaskSlot = TaskSlot {
    kind: TaskKind::Task,
    slot: 0,
};

fn described(session_id: u64, at: TaskSlot, task_id: u32, open_journal: bool) -> WorldEvent {
    WorldEvent::TaskDescribed {
        session_id,
        description: TaskDescription {
            at,
            task_id,
            open_journal,
            title: format!("Synthetic task {task_id}"),
            duration_seconds: 0,
            duration_code: 3,
            started_at: 0,
            description: "Synthetic description".into(),
            rewards: Rewards {
                listed: true,
                coin_copper: 1234,
                experience: true,
                faction: false,
                text: "A synthetic reward".into(),
                item_link: String::new(),
            },
        },
    }
}

fn step(session_id: u64, at: TaskSlot, task_id: u32, activity_id: u32, shown: bool) -> WorldEvent {
    WorldEvent::TaskStepUpdated {
        session_id,
        activity: TaskActivity {
            at,
            task_id,
            activity_id,
            group: 0,
            step: if shown {
                Step::Shown(Objective {
                    activity_type: 2,
                    optional: false,
                    target_name: "a synthetic rat".into(),
                    item_list: String::new(),
                    goal: 5,
                    skill_id: 0,
                    spell_id: 0,
                    zone_id: 0,
                    description: String::new(),
                    done: 2,
                })
            } else {
                Step::Hidden { optional: true }
            },
        },
    }
}

/// An app with the journal open and the player admitted in session 5,
/// offered tasks or not.
fn app(tasks_offered: bool) -> (App, Receiver<ClientCommand>) {
    let mut app = crate::testing::app();
    let (sender, receiver) = sync_channel(8);
    app.insert_resource(Outbox::new(Some(sender)))
        .add_systems(Update, (buttons, window, fill).chain());
    let mut online = app.world_mut().resource_mut::<OnlineState>();
    if tasks_offered {
        testing::admit(&mut online, 5, testing::player(7));
    } else {
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: Capability::ALL
                    .into_iter()
                    .filter(|capability| *capability != Capability::Tasks)
                    .collect(),
                choices: Vec::new(),
                session_id: 5,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        testing::connect(&mut online, true);
    }
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::TaskJournal);
    (app, receiver)
}

fn hear(app: &mut App, events: impl IntoIterator<Item = WorldEvent>) {
    testing::news(&mut app.world_mut().resource_mut::<OnlineState>(), events);
    app.update();
}

fn button(app: &mut App, action: Action) -> Option<Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &JournalButton)>()
        .iter(world)
        .find(|(_, button)| button.0 == action)
        .map(|(entity, _)| entity)
}

fn click(app: &mut App, action: Action) {
    let control = button(app, action).unwrap_or_else(|| panic!("no {action:?} button"));
    crate::clicks::testing::click(app, control);
}

fn texts(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    world
        .query::<&Text>()
        .iter(world)
        .map(|text| text.0.clone())
        .collect()
}

fn has(app: &mut App, wanted: &str) -> bool {
    texts(app).iter().any(|text| text == wanted)
}

#[test]
fn the_active_page_lists_the_entries_and_shows_the_chosen_ones_steps() {
    let (mut app, _receiver) = app(true);
    app.update();
    assert!(has(&mut app, "No tasks"));
    hear(
        &mut app,
        [
            described(5, QUEST_3, 1000, false),
            described(5, THE_TASK, 7, false),
            step(5, THE_TASK, 7, 0, true),
            step(5, THE_TASK, 7, 1, false),
        ],
    );
    // The task comes first, and is shown until another is chosen.
    assert!(has(&mut app, "> Synthetic task 7"));
    assert!(has(&mut app, "Synthetic task 1000"));
    assert!(has(&mut app, "Task  ·  Duration: Long"));
    assert!(has(&mut app, "- Kill 5 a synthetic rat: 2 / 5"));
    assert!(has(&mut app, "- ???"));
    assert!(has(&mut app, "Reward: A synthetic reward"));
    assert!(has(&mut app, "Coin: 1p 2g 3s 4c"));
    assert!(has(&mut app, "Experience"));

    click(&mut app, Action::Entry(QUEST_3));
    assert!(has(&mut app, "> Synthetic task 1000"));
    assert!(!has(&mut app, "- ???"));

    // Another task in the slot replaces what is shown.
    hear(&mut app, [described(5, QUEST_3, 1001, false)]);
    assert!(has(&mut app, "> Synthetic task 1001"));
    assert!(!has(&mut app, "Synthetic task 1000"));

    // A new admission starts the journal empty.
    testing::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        6,
        testing::player(7),
    );
    app.update();
    assert!(has(&mut app, "No tasks"));
}

#[test]
fn cancel_asks_in_the_window_and_only_yes_sends_it_for_the_same_entry() {
    let (mut app, receiver) = app(true);
    hear(
        &mut app,
        [
            described(5, QUEST_3, 1000, false),
            described(5, QUEST_5, 1002, false),
        ],
    );
    click(&mut app, Action::Entry(QUEST_3));
    click(&mut app, Action::Cancel);
    assert!(has(&mut app, "Cancel Synthetic task 1000?"));
    assert!(receiver.try_recv().is_err());
    click(&mut app, Action::Confirm(false));
    assert!(receiver.try_recv().is_err());
    assert!(button(&mut app, Action::Confirm(true)).is_none());

    click(&mut app, Action::Cancel);
    click(&mut app, Action::Confirm(true));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ClientCommand::CancelTask {
            session_id: 5,
            at: QUEST_3,
            ..
        }
    ));
    assert!(receiver.try_recv().is_err());

    // A question is dropped with its admission.
    click(&mut app, Action::Cancel);
    testing::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        6,
        testing::player(7),
    );
    hear(
        &mut app,
        [
            described(6, QUEST_3, 1000, false),
            described(6, QUEST_5, 1002, false),
        ],
    );
    assert!(button(&mut app, Action::Confirm(true)).is_none());
    assert!(button(&mut app, Action::Cancel).is_some());

    // An entry that changed after the question sends nothing.
    click(&mut app, Action::Entry(QUEST_5));
    click(&mut app, Action::Cancel);
    hear(&mut app, [described(6, QUEST_5, 1003, false)]);
    click(&mut app, Action::Confirm(true));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn a_finished_task_asks_for_its_steps_and_shows_the_reply() {
    let (mut app, receiver) = app(true);
    hear(
        &mut app,
        [WorldEvent::CompletedTasks {
            session_id: 5,
            list: vec![
                CompletedTask {
                    task_id: 1,
                    title: "First finished".into(),
                    completed_at: 5,
                },
                CompletedTask {
                    task_id: 2,
                    title: "Second finished".into(),
                    completed_at: 6,
                },
            ],
        }],
    );
    click(&mut app, Action::Page(Page::Finished));
    assert!(has(&mut app, "First finished") && has(&mut app, "Second finished"));
    click(&mut app, Action::Finished(1));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ClientCommand::AskTaskHistory {
            session_id: 5,
            index: 1,
            ..
        }
    ));
    assert!(has(&mut app, "No steps received"));
    // Not asked again while the answer is on its way.
    click(&mut app, Action::Finished(0));
    click(&mut app, Action::Finished(1));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ClientCommand::AskTaskHistory { index: 0, .. }
    ));
    assert!(receiver.try_recv().is_err());
    hear(
        &mut app,
        [WorldEvent::TaskHistory {
            session_id: 5,
            history: TaskHistory {
                index: 1,
                steps: vec![eq_client_core::tasks::DoneStep {
                    activity_type: 4,
                    target_name: "the synthetic tanner".into(),
                    item_list: String::new(),
                    goal: 1,
                    zone_id: 0,
                    description: String::new(),
                }],
            },
        }],
    );
    assert!(has(&mut app, "- Speak with the synthetic tanner"));
    // A history held is not asked for again.
    click(&mut app, Action::Finished(1));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn without_tasks_offered_the_journal_is_read_only() {
    let (mut app, receiver) = app(false);
    hear(
        &mut app,
        [
            described(5, QUEST_3, 1000, false),
            WorldEvent::CompletedTasks {
                session_id: 5,
                list: vec![CompletedTask {
                    task_id: 1,
                    title: "First finished".into(),
                    completed_at: 5,
                }],
            },
        ],
    );
    assert!(button(&mut app, Action::Cancel).is_none());
    assert!(has(&mut app, "Tasks can't be changed on this server."));
    click(&mut app, Action::Page(Page::Finished));
    click(&mut app, Action::Finished(0));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn the_server_opens_the_journal_when_a_task_is_taken() {
    let (mut app, _receiver) = app(true);
    app.world_mut()
        .resource_mut::<Shown>()
        .close(WindowId::TaskJournal);
    hear(&mut app, [described(5, QUEST_3, 1000, false)]);
    assert!(
        !app.world()
            .resource::<Shown>()
            .is_open(WindowId::TaskJournal)
    );
    hear(&mut app, [described(5, QUEST_5, 1001, true)]);
    assert!(
        app.world()
            .resource::<Shown>()
            .is_open(WindowId::TaskJournal)
    );
    // Closed again, it stays closed until the server asks again.
    app.world_mut()
        .resource_mut::<Shown>()
        .close(WindowId::TaskJournal);
    app.update();
    assert!(
        !app.world()
            .resource::<Shown>()
            .is_open(WindowId::TaskJournal)
    );
}

#[test]
fn the_time_left_reads_to_the_minute() {
    let mut entry = TaskEntry {
        description: match described(5, QUEST_3, 1, false) {
            WorldEvent::TaskDescribed { description, .. } => description,
            _ => unreachable!(),
        },
        steps: std::collections::BTreeMap::new(),
    };
    assert_eq!(time_left(&entry, 100), None);
    // Without the start it counts from, there is no line.
    entry.description.duration_seconds = 3_600;
    assert_eq!(time_left(&entry, 100), None);
    entry.description.started_at = 1_000;
    entry.description.duration_seconds = 3_600;
    assert_eq!(time_left(&entry, 1_000).as_deref(), Some("Time left: 1 h"));
    assert_eq!(time_left(&entry, 1_030).as_deref(), Some("Time left: 1 h"));
    assert_eq!(
        time_left(&entry, 1_090).as_deref(),
        Some("Time left: 59 min")
    );
    assert_eq!(time_left(&entry, 4_600).as_deref(), Some("Time left: none"));
}
