use super::*;
use eq_network_game::tasks::Rewards;

const QUEST_3: TaskSlot = TaskSlot {
    kind: TaskKind::Quest,
    slot: 3,
};
const THE_TASK: TaskSlot = TaskSlot {
    kind: TaskKind::Task,
    slot: 0,
};

fn description(at: TaskSlot, task_id: u32) -> TaskDescription {
    TaskDescription {
        at,
        task_id,
        open_journal: false,
        title: "Synthetic title".into(),
        duration_seconds: 0,
        duration_code: 2,
        started_at: 0,
        description: "Synthetic description".into(),
        rewards: Rewards {
            listed: true,
            coin_copper: 0,
            experience: false,
            faction: false,
            text: String::new(),
            item_link: String::new(),
        },
    }
}

fn described(session_id: u64, at: TaskSlot, task_id: u32) -> WorldEvent {
    WorldEvent::TaskDescribed {
        session_id,
        description: description(at, task_id),
    }
}

fn step(session_id: u64, at: TaskSlot, task_id: u32, activity_id: u32) -> WorldEvent {
    WorldEvent::TaskStepUpdated {
        session_id,
        activity: TaskActivity {
            at,
            task_id,
            activity_id,
            group: 0,
            step: Step::Hidden { optional: false },
        },
    }
}

fn completed(session_id: u64, count: u32) -> WorldEvent {
    WorldEvent::CompletedTasks {
        session_id,
        list: (0..count)
            .map(|index| CompletedTask {
                task_id: index + 1,
                title: "Done".into(),
                completed_at: 5,
            })
            .collect(),
    }
}

fn history(session_id: u64, index: u32) -> WorldEvent {
    WorldEvent::TaskHistory {
        session_id,
        history: TaskHistory {
            index,
            steps: vec![],
        },
    }
}

#[test]
fn the_journal_follows_the_events_in_order() {
    let mut journal = TaskJournal::default();
    assert!(journal.observe(5, &described(5, QUEST_3, 1000)));
    assert_eq!(journal.open_asked(), 0);
    let mut opening = described(5, THE_TASK, 7);
    if let WorldEvent::TaskDescribed { description, .. } = &mut opening {
        description.open_journal = true;
    }
    assert!(journal.observe(5, &opening));
    assert_eq!(journal.open_asked(), 1);
    // The task comes before the quests.
    let order: Vec<_> = journal
        .entries()
        .iter()
        .map(|entry| entry.description.task_id)
        .collect();
    assert_eq!(order, [7, 1000]);
    assert!(journal.observe(5, &step(5, QUEST_3, 1000, 0)));
    assert!(journal.observe(5, &step(5, QUEST_3, 1000, 1)));
    // A step of a task the slot doesn't hold is not taken.
    assert!(!journal.observe(5, &step(5, QUEST_3, 999, 2)));
    assert_eq!(journal.entry(QUEST_3).unwrap().steps.len(), 2);
    // The same task described again keeps its steps.
    assert!(journal.observe(5, &described(5, QUEST_3, 1000)));
    assert_eq!(journal.entry(QUEST_3).unwrap().steps.len(), 2);
    // Another task in the slot drops them.
    assert!(journal.observe(5, &described(5, QUEST_3, 1001)));
    assert!(journal.entry(QUEST_3).unwrap().steps.is_empty());
    assert!(journal.holds(1001) && !journal.holds(1000));
    // A removal empties the slot.
    let removed = WorldEvent::TaskRemoved {
        session_id: 5,
        at: QUEST_3,
    };
    assert!(journal.observe(5, &removed));
    assert!(!journal.observe(5, &removed));
    assert!(journal.entry(QUEST_3).is_none());
}

#[test]
fn a_history_is_kept_only_for_a_listed_index_and_a_new_list_drops_them() {
    let mut journal = TaskJournal::default();
    assert!(!journal.observe(5, &history(5, 0)));
    assert!(journal.observe(5, &completed(5, 2)));
    assert!(journal.observe(5, &history(5, 1)));
    assert!(!journal.observe(5, &history(5, 2)));
    assert!(journal.history(1).is_some());
    assert!(journal.observe(5, &completed(5, 3)));
    assert!(journal.history(1).is_none());
    assert_eq!(journal.completed().unwrap().len(), 3);
}

#[test]
fn a_new_admission_starts_the_journal_empty() {
    let mut journal = TaskJournal::default();
    journal.observe(5, &described(5, QUEST_3, 1000));
    journal.observe(5, &completed(5, 1));
    journal.observe(6, &step(6, QUEST_3, 1000, 0));
    assert_eq!(journal.entries(), []);
    assert!(journal.completed().is_none());
}

#[test]
fn durations_and_steps_read_in_our_words() {
    assert_eq!(duration_words(0, 1), "Short");
    assert_eq!(duration_words(0, 2), "Medium");
    assert_eq!(duration_words(0, 3), "Long");
    assert_eq!(duration_words(0, 0), "Unlimited");
    assert_eq!(duration_words(45, 2), "45 s");
    assert_eq!(duration_words(1800, 2), "30 min");
    assert_eq!(duration_words(7200, 2), "2 h");
    assert_eq!(duration_words(5400, 0), "1 h 30 min");

    assert_eq!(
        objective_words(2, "a gray wolf", "", 5, "Slay gray wolves"),
        "Slay gray wolves"
    );
    assert_eq!(
        objective_words(2, "a gray wolf", "", 5, " "),
        "Kill 5 a gray wolf"
    );
    assert_eq!(
        objective_words(1, "the tanner", "a wolf pelt", 1, ""),
        "Deliver a wolf pelt to the tanner"
    );
    assert_eq!(
        objective_words(3, "a gray wolf", "a pelt", 3, ""),
        "Loot 3 a pelt from a gray wolf"
    );
    assert_eq!(
        objective_words(4, "the tanner", "", 1, ""),
        "Speak with the tanner"
    );
    assert_eq!(
        objective_words(9, "a gnoll", "", 2, ""),
        "Cast 2 times on a gnoll"
    );
    assert_eq!(
        objective_words(100, "the tanner", "", 1, ""),
        "Give coin to the tanner"
    );
    assert_eq!(objective_words(55, "", "", 1, ""), "Complete");
}
