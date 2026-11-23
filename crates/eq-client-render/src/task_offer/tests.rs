use super::*;
use crate::online::testing;
use bevy::ecs::system::RunSystemOnce;
use eq_client_core::{
    WorldEvent,
    tasks::{OfferedStep, TaskKind, TaskOffers},
};
use std::sync::mpsc::{Receiver, sync_channel};

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
                    title: format!("Synthetic task {task_id}"),
                    description: "Synthetic description".into(),
                    steps: vec![OfferedStep {
                        index: 0,
                        activity_type: 2,
                        target_name: "a synthetic rat".into(),
                        item_list: String::new(),
                        goal: 3,
                        skill_id: 0,
                        spell_id: 0,
                        zone_id: 0,
                        description: String::new(),
                    }],
                })
                .collect(),
        },
    }
}

/// An app that draws and answers offers, with the player admitted in
/// session 5, offered tasks or not.
fn app(tasks_offered: bool) -> (App, Receiver<ClientCommand>) {
    let mut app = crate::testing::app();
    let (sender, receiver) = sync_channel(8);
    app.insert_resource(Outbox::new(Some(sender)))
        .add_systems(Update, (buttons, present).chain());
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
    (app, receiver)
}

fn hear(app: &mut App, event: WorldEvent) {
    testing::news(&mut app.world_mut().resource_mut::<OnlineState>(), [event]);
    app.update();
}

fn find(app: &mut App, wanted: impl Fn(Answer) -> bool) -> Option<Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &OfferButton)>()
        .iter(world)
        .find(|(_, button)| wanted(button.answer))
        .map(|(entity, _)| entity)
}

fn answers(app: &mut App) -> Vec<Answer> {
    let world = app.world_mut();
    let mut answers: Vec<_> = world
        .query::<&OfferButton>()
        .iter(world)
        .map(|button| button.answer)
        .collect();
    answers.sort_by_key(|answer| format!("{answer:?}"));
    answers
}

fn texts(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    world
        .query::<&Text>()
        .iter(world)
        .map(|text| text.0.clone())
        .collect()
}

fn is_open(app: &App) -> bool {
    app.world().resource::<Shown>().is_open(WindowId::TaskOffer)
}

#[test]
fn accept_posts_the_chosen_task_with_the_current_admission_and_closes() {
    let (mut app, receiver) = app(true);
    app.update();
    assert!(!is_open(&app));
    hear(&mut app, offered(5, &[1000, 1001]));
    assert!(is_open(&app));
    let said = texts(&mut app);
    assert!(said.contains(&"Duration: Medium".to_owned()), "{said:?}");
    assert!(
        said.contains(&"- Kill 3 a synthetic rat".to_owned()),
        "{said:?}"
    );

    let second = find(&mut app, |answer| answer == Answer::Choose(1)).unwrap();
    crate::clicks::testing::click(&mut app, second);
    let accept = find(&mut app, |answer| answer == Answer::Accept(1001)).unwrap();
    crate::clicks::testing::click(&mut app, accept);
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ClientCommand::AcceptTask {
            session_id: 5,
            task_id: 1001,
            ..
        }
    ));
    assert!(receiver.try_recv().is_err());
    app.update();
    assert!(!is_open(&app));
    assert!(
        app.world()
            .resource::<OnlineState>()
            .world()
            .task_offer()
            .is_none()
    );
}

#[test]
fn decline_posts_decline_and_closes() {
    let (mut app, receiver) = app(true);
    hear(&mut app, offered(5, &[1000]));
    let decline = find(&mut app, |answer| answer == Answer::Decline).unwrap();
    crate::clicks::testing::click(&mut app, decline);
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ClientCommand::DeclineTasks { session_id: 5 }
    ));
    app.update();
    assert!(!is_open(&app));
}

#[test]
fn without_tasks_offered_the_offer_is_read_only() {
    let (mut app, receiver) = app(false);
    hear(&mut app, offered(5, &[1000]));
    assert!(is_open(&app));
    assert_eq!(answers(&mut app), [Answer::Choose(0), Answer::Close]);
    assert!(texts(&mut app).contains(&"Tasks can't be answered on this server.".to_owned()));
    // Even a stale press of Accept posts nothing here.
    let serial = app
        .world()
        .resource::<OnlineState>()
        .world()
        .task_offer()
        .unwrap()
        .serial;
    app.world_mut()
        .run_system_once(
            move |mut online: ResMut<OnlineState>,
                  outbox: Res<Outbox>,
                  mut chosen: ResMut<Chosen>| {
                answer(
                    &mut online,
                    &outbox,
                    &mut chosen,
                    serial,
                    Answer::Accept(1000),
                )
            },
        )
        .unwrap()
        .unwrap_err();
    let close = find(&mut app, |answer| answer == Answer::Close).unwrap();
    crate::clicks::testing::click(&mut app, close);
    assert!(receiver.try_recv().is_err());
    app.update();
    assert!(!is_open(&app));
}

#[test]
fn a_new_offer_replaces_the_window_and_a_new_zone_closes_it() {
    let (mut app, receiver) = app(true);
    hear(&mut app, offered(5, &[1000, 1001]));
    let second = find(&mut app, |answer| answer == Answer::Choose(1)).unwrap();
    crate::clicks::testing::click(&mut app, second);
    assert!(find(&mut app, |answer| answer == Answer::Accept(1001)).is_some());
    // The next offer starts again from its first task.
    hear(&mut app, offered(5, &[2000, 2001]));
    assert!(find(&mut app, |answer| answer == Answer::Accept(2000)).is_some());
    assert!(
        find(&mut app, |answer| matches!(
            answer,
            Answer::Accept(1000 | 1001)
        ))
        .is_none()
    );

    testing::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        6,
        testing::player(7),
    );
    app.update();
    assert!(!is_open(&app));
    assert!(find(&mut app, |_| true).is_none());
    assert!(receiver.try_recv().is_err());
}
