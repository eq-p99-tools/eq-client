use super::*;
use crate::tasks::{
    OfferedTask, Outcome, TaskCompletion, TaskDescription, TaskKind, TaskOffers, TaskSlot,
};
use eq_network_game::tasks::Rewards;

const QUEST_3: TaskSlot = TaskSlot {
    kind: TaskKind::Quest,
    slot: 3,
};

fn described(session_id: u64, task_id: u32) -> WorldEvent {
    WorldEvent::TaskDescribed {
        session_id,
        description: TaskDescription {
            at: QUEST_3,
            task_id,
            open_journal: false,
            title: "Synthetic title".into(),
            duration_seconds: 0,
            duration_code: 0,
            started_at: 0,
            description: String::new(),
            rewards: Rewards {
                listed: true,
                coin_copper: 0,
                experience: false,
                faction: false,
                text: String::new(),
                item_link: String::new(),
            },
        },
    }
}

fn offered(session_id: u64, tasks: &[u32]) -> WorldEvent {
    WorldEvent::TasksOffered {
        session_id,
        offers: TaskOffers {
            kind: TaskKind::Quest,
            giver: 77,
            tasks: tasks
                .iter()
                .map(|task_id| OfferedTask {
                    task_id: *task_id,
                    duration_seconds: 0,
                    duration_code: 2,
                    title: "Synthetic offer".into(),
                    description: String::new(),
                    steps: vec![],
                })
                .collect(),
        },
    }
}

fn offered_ids(world: &ClientWorld) -> Option<Vec<u32>> {
    world
        .task_offer()
        .map(|offer| offer.offers.tasks.iter().map(|task| task.task_id).collect())
}

#[test]
fn the_journal_takes_only_this_admissions_events_and_starts_empty_with_the_next() {
    let mut world = admitted();
    assert!(game(&mut world, described(9, 1000)).ignored);
    assert_eq!(world.task_journal().entries(), []);
    assert!(!game(&mut world, described(1, 1000)).ignored);
    assert!(world.task_journal().holds(1000));
    game(&mut world, entered(2));
    assert_eq!(world.task_journal().entries(), []);
    // The old admission's news is stale.
    assert!(game(&mut world, described(1, 1001)).ignored);
    assert_eq!(world.task_journal().entries(), []);
}

#[test]
fn an_offer_is_replaced_closed_by_its_answer_and_cleared_with_the_admission() {
    let mut world = admitted();
    assert!(game(&mut world, offered(9, &[1000])).ignored);
    assert!(world.task_offer().is_none());
    game(&mut world, offered(1, &[1000]));
    let first = world.task_offer().unwrap().serial;
    game(&mut world, offered(1, &[1001, 1002]));
    let second = world.task_offer().unwrap().serial;
    assert_ne!(first, second);
    assert_eq!(offered_ids(&world), Some(vec![1001, 1002]));
    // An answer to the replaced offer leaves the new one.
    world.close_task_offer(first);
    assert!(world.task_offer().is_some());
    world.close_task_offer(second);
    assert!(world.task_offer().is_none());

    game(&mut world, offered(1, &[1000]));
    game(&mut world, entered(2));
    assert!(world.task_offer().is_none());
    game(&mut world, offered(2, &[1000]));
    connection(&mut world, false, false);
    assert!(world.task_offer().is_none());
}

#[test]
fn a_refusal_is_a_notice_and_a_conclusion_adds_no_line_of_ours() {
    let mut world = admitted();
    let reason = "That task isn't offered.";
    let changes = game(
        &mut world,
        WorldEvent::TaskRefused {
            session_id: 1,
            reason: reason.into(),
        },
    );
    assert_eq!(changes.notices, [Notice::TaskRefused(reason.into())]);
    let stale = game(
        &mut world,
        WorldEvent::TaskRefused {
            session_id: 9,
            reason: reason.into(),
        },
    );
    assert!(stale.ignored && stale.notices.is_empty());

    game(&mut world, described(1, 1000));
    for (outcome, stage_completed) in [
        (Outcome::Completed, true),
        (Outcome::Completed, false),
        (Outcome::Failed, false),
    ] {
        let changes = game(
            &mut world,
            WorldEvent::TaskConcluded {
                session_id: 1,
                completion: TaskCompletion {
                    at: QUEST_3,
                    task_id: 1000,
                    activity_id: 0,
                    outcome,
                    stage_completed,
                },
            },
        );
        assert!(!changes.ignored && changes.notices.is_empty());
    }
    // The server's removal takes the entry out, not the conclusion.
    assert!(world.task_journal().holds(1000));
}
