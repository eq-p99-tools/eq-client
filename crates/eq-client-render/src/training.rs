//! Training at a guildmaster, as the official client does: the trade key on
//! the player's own guildmaster asks to train, the answer opens the skin's
//! Training window with the skills the guildmaster teaches, Train practices
//! the chosen one, and Done or closing the window leaves. The session
//! decides what may be practiced; this module keeps the window with the
//! world's training.
use crate::{
    online::OnlineState,
    outbox::Outbox,
    theme,
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, Coins,
    training::{PracticePoints, TrainingRequest, TrainingSubject, practice_cost},
    world::ClientWorld,
};

/// Guildmasters' classes: the warrior's 20 to the berserker's 35.
const GUILDMASTERS: std::ops::RangeInclusive<u8> = 20..=35;

/// Whether a spawn of this class trains players: a guildmaster.
pub(crate) fn is_guildmaster(class: Option<u8>) -> bool {
    class.is_some_and(|class| GUILDMASTERS.contains(&class))
}

/// The skill chosen in the Training window's list, which Train practices.
#[derive(Resource, Default)]
pub(crate) struct Chosen(pub(crate) Option<TrainingSubject>);

/// The Training window's Train button.
#[derive(Component)]
pub(crate) struct TrainButton;

/// A row of the Training window's list: the skill it offers, and its place
/// from the top.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SkillRow {
    pub(crate) subject: TrainingSubject,
    pub(crate) index: usize,
}

/// The Training window's list of skills, which the client fills from the
/// guildmaster's answer and the player's skills.
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct SkillRows {
    /// Each column's width, from the left.
    columns: Vec<f32>,
    /// The rows last drawn, to redraw only when one changes.
    drawn: Option<Vec<Row>>,
    /// Inputs last projected; None also covers a newly created window.
    checked: Option<Inputs>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Inputs {
    skills: u64,
    training: u64,
    chosen: Option<TrainingSubject>,
}

impl SkillRows {
    pub(crate) fn new(columns: Vec<f32>) -> Self {
        Self {
            columns,
            drawn: None,
            checked: None,
        }
    }
}

/// A row as drawn: the skill, its name, its value and the next practice's
/// cost, and whether it is the one chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    subject: TrainingSubject,
    name: String,
    value: Option<u32>,
    cost: Option<Coins>,
    chosen: bool,
}

/// The skills the guildmaster teaches, by name, with the player's value and
/// what the next practice costs.
fn rows(
    world: &ClientWorld,
    chosen: Option<TrainingSubject>,
    messages: &crate::hud::messages::Messages,
) -> Vec<Row> {
    let Some(offer) = world.training() else {
        return Vec::new();
    };
    let skills = world
        .player()
        .and_then(|player| player.skills.as_deref())
        .unwrap_or_default();
    let mut rows: Vec<Row> = (0..offer.caps.len())
        .filter_map(|index| {
            let skill = u32::try_from(index).ok()?;
            if offer.cap(skill) == 0 {
                return None;
            }
            let value = skills.get(index).copied();
            Some(Row {
                subject: TrainingSubject::Skill(skill),
                name: eq_client_core::skills::name(skill)
                    .map_or_else(|| format!("Skill {skill}"), str::to_owned),
                value,
                cost: value.map(|value| in_coins(practice_cost(value))),
                chosen: chosen == Some(TrainingSubject::Skill(skill)),
            })
        })
        .collect();
    let common_variant = world
        .language_zero_selector()
        .map(eq_client_core::rules::LanguageZeroSelector::alternate_name);
    rows.extend(
        offer
            .language_caps
            .iter()
            .enumerate()
            .filter_map(|(index, cap)| {
                let language = u8::try_from(index).ok()?;
                let subject = TrainingSubject::Language(language);
                if *cap == 0 || language >= eq_client_core::training::LANGUAGES {
                    return None;
                }
                let value = world.player().and_then(|player| subject.value(player));
                let name = eq_client_assets::skills::SkillRules::language_name_string(
                    language,
                    common_variant,
                )
                .and_then(|id| messages.argument_free(id))
                .map_or_else(|| format!("Language {language}"), str::to_owned);
                Some(Row {
                    subject,
                    name,
                    value,
                    cost: value.map(|value| in_coins(practice_cost(value))),
                    chosen: chosen == Some(subject),
                })
            }),
    );
    rows.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.subject.update_id().cmp(&right.subject.update_id()))
    });
    rows
}

/// A price in copper as the fewest coins: platinum first.
fn in_coins(copper: u64) -> Coins {
    let take = |worth: u64, left: &mut u64| {
        let count = u32::try_from(*left / worth).unwrap_or(u32::MAX);
        *left %= worth;
        count
    };
    let mut left = copper;
    Coins {
        platinum: take(1000, &mut left),
        gold: take(100, &mut left),
        silver: take(10, &mut left),
        copper: take(1, &mut left),
    }
}

/// Sends one training request; a refusal shows in the chat.
fn ask(request: TrainingRequest, world: &ClientWorld, outbox: &Outbox) {
    let _ = outbox.post(world, |stamp| ClientCommand::Training {
        session_id: stamp.session_id,
        request,
        created: stamp.created,
    });
}

/// Where the Training window stands with the guildmaster it was opened for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Opened {
    /// No training.
    #[default]
    Closed,
    /// Open for this guildmaster.
    For(u16),
    /// Closed by the player, while leaving this guildmaster is under way.
    Leaving(u16),
}

/// Keeps the Training window with the training: it opens when the
/// guildmaster answers and closes when training ends. A window the player
/// closes, with Done or otherwise, leaves, and stays closed until the
/// training has ended.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut shown: ResMut<Shown>,
    mut chosen: ResMut<Chosen>,
    mut opened: Local<Opened>,
    mut seen_revision: Local<u64>,
) {
    let training = online.world().training().map(|offer| offer.trainer);
    let revision = online.world().training_revision();
    // Ended and a fresh offer can both arrive between display frames.
    // A new accepted offer must not inherit the old window's Leaving state.
    if training.is_some() && revision != *seen_revision && matches!(*opened, Opened::Leaving(_)) {
        *opened = Opened::Closed;
    }
    *seen_revision = revision;
    match (training, *opened) {
        (Some(trainer), Opened::For(open) | Opened::Leaving(open)) if open == trainer => {
            if *opened == Opened::For(trainer) && !shown.is_open(WindowId::Training) {
                ask(TrainingRequest::End, online.world(), &outbox);
                *opened = Opened::Leaving(trainer);
            }
        }
        (Some(trainer), _) => {
            shown.open(WindowId::Training);
            *opened = Opened::For(trainer);
            chosen.0 = None;
        }
        (None, Opened::For(_) | Opened::Leaving(_)) => {
            shown.close(WindowId::Training);
            *opened = Opened::Closed;
        }
        (None, Opened::Closed) => (),
    }
}

/// Chooses the clicked row's skill, and practices the chosen one when Train
/// is pressed.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut chosen: ResMut<Chosen>,
    rows: Query<(&Interaction, &SkillRow), Changed<Interaction>>,
    train: crate::clicks::Clicks<TrainButton>,
) {
    for (interaction, row) in &rows {
        if *interaction == Interaction::Pressed && chosen.0 != Some(row.subject) {
            chosen.0 = Some(row.subject);
        }
    }
    if train.any()
        && let Some(offer) = online.world().training()
        && let Some(subject) = chosen.0
    {
        ask(
            TrainingRequest::PracticeAt {
                trainer: offer.trainer,
                subject,
            },
            online.world(),
            &outbox,
        );
    }
}

/// The height of a row of the list.
const ROW_HEIGHT: f32 = 16.0;

/// Fills the Training window's list again whenever a row changes: a skill's
/// value, its cost or the choice.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn fill(
    mut commands: Commands,
    online: Res<OnlineState>,
    chosen: Res<Chosen>,
    messages: Res<crate::hud::messages::Messages>,
    mut lists: Query<(Entity, &mut SkillRows, Option<&Children>)>,
) {
    let input = Inputs {
        skills: online.world().skills_revision(),
        training: online.world().training_revision(),
        chosen: chosen.0,
    };
    // No list, or every existing list is current: no row allocation or sorting.
    if !messages.is_changed() && lists.iter().all(|(_, list, _)| list.checked == Some(input)) {
        return;
    }
    let wanted = rows(online.world(), chosen.0, &messages);
    for (entity, mut list, children) in &mut lists {
        if !messages.is_changed() && list.checked == Some(input) {
            continue;
        }
        list.checked = Some(input);
        if list.drawn.as_ref() == Some(&wanted) {
            continue;
        }
        if let Some(children) = children {
            for child in children {
                commands.entity(*child).despawn();
            }
        }
        for (index, row) in wanted.iter().enumerate() {
            let coin = |count: fn(Coins) -> u32| {
                row.cost
                    .map_or_else(String::new, |cost| count(cost).to_string())
            };
            let cells = [
                row.name.clone(),
                // The official client's rank for a value is not known yet.
                String::new(),
                row.value
                    .map_or_else(String::new, |value| value.to_string()),
                coin(|cost| cost.platinum),
                coin(|cost| cost.gold),
                coin(|cost| cost.silver),
                coin(|cost| cost.copper),
            ];
            let line = commands
                .spawn((
                    Button,
                    SkillRow {
                        subject: row.subject,
                        index,
                    },
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(if row.chosen {
                        theme::BUTTON
                    } else {
                        Color::NONE
                    }),
                    ChildOf(entity),
                ))
                .id();
            crate::list_cells::text(&mut commands, line, cells, list.columns.iter().copied());
        }
        list.drawn = Some(wanted.clone());
    }
}

/// The player's practice points, as the Training window counts them.
pub(crate) fn practice_points(world: &ClientWorld) -> String {
    match world.training_points() {
        PracticePoints::Balance(points) => points.to_string(),
        PracticePoints::Unknown => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{
        WorldEvent,
        training::{TrainingOffer, TrainingUpdate},
    };

    #[test]
    fn same_trainer_reopens_when_end_and_new_offer_arrive_between_frames() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(Outbox::new(Some(tx)))
            .init_resource::<Shown>()
            .init_resource::<Chosen>()
            .add_systems(Update, window);
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let offer = TrainingOffer {
            trainer: 42,
            caps: vec![100; 100],
            language_caps: Vec::new(),
            price_scale: None,
        };
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::Training(
                TrainingUpdate::Offered(offer.clone()),
            )],
        );
        testing::news(&mut online, [eq_client_core::WorldEvent::PracticePoints(2)]);
        app.insert_resource(online);
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Training));
        app.world_mut()
            .resource_mut::<Shown>()
            .close(WindowId::Training);
        app.update();
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::Training {
                request: TrainingRequest::End,
                ..
            }
        ));
        {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            testing::news(
                &mut online,
                [eq_client_core::WorldEvent::Training(TrainingUpdate::Ended)],
            );
            testing::news(
                &mut online,
                [eq_client_core::WorldEvent::Training(
                    TrainingUpdate::Offered(offer),
                )],
            );
        }
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Training));
        assert_eq!(
            practice_points(app.world().resource::<OnlineState>().world()),
            "2"
        );
        assert!(
            rx.try_recv().is_err(),
            "fresh offer must not send another Done"
        );
    }

    #[test]
    fn selecting_a_language_is_local_and_train_posts_its_typed_subject() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(Outbox::new(Some(tx)))
            .add_systems(Update, buttons);
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.skills = Some(vec![70; 100]);
        player.languages = Some(vec![20; 28]);
        testing::admit(&mut online, 1, player);
        testing::news(
            &mut online,
            [WorldEvent::Training(TrainingUpdate::Offered(
                TrainingOffer {
                    trainer: 42,
                    caps: vec![200; 100],
                    language_caps: vec![100; 26],
                    price_scale: None,
                },
            ))],
        );
        app.insert_resource(online);
        let row = app
            .world_mut()
            .spawn((
                Button,
                Interaction::None,
                SkillRow {
                    subject: TrainingSubject::Language(5),
                    index: 0,
                },
            ))
            .id();
        let train = app
            .world_mut()
            .spawn((Button, Interaction::None, TrainButton))
            .id();
        crate::clicks::testing::click(&mut app, row);
        assert!(rx.try_recv().is_err(), "selection alone sends no practice");
        assert_eq!(
            app.world().resource::<Chosen>().0,
            Some(TrainingSubject::Language(5))
        );
        crate::clicks::testing::click(&mut app, train);
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::Training {
                request: TrainingRequest::PracticeAt {
                    trainer: 42,
                    subject: TrainingSubject::Language(5)
                },
                ..
            })
        ));
        assert!(rx.try_recv().is_err());
        let online = app.world().resource::<OnlineState>();
        assert_eq!(
            online.world().player().unwrap().skills.as_ref().unwrap()[5],
            70
        );
        assert_eq!(
            online.world().player().unwrap().languages.as_ref().unwrap()[5],
            20
        );
    }

    #[test]
    fn language_rows_keep_their_bank_values_prices_and_received_revisions() {
        let mut app = crate::testing::app();
        app.add_systems(Update, fill);
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.skills = Some(vec![0; 100]);
        player.skills.as_mut().unwrap()[5] = 70;
        player.languages = Some(vec![20; 28]);
        player.languages.as_mut().unwrap()[25] = 0;
        testing::admit(&mut online, 1, player);
        let mut caps = vec![0; 100];
        caps[5] = 200;
        let mut language_caps = vec![0; 26];
        language_caps[5] = 100;
        language_caps[25] = 100;
        testing::news(
            &mut online,
            [WorldEvent::Training(TrainingUpdate::Offered(
                TrainingOffer {
                    trainer: 42,
                    caps,
                    language_caps,
                    price_scale: None,
                },
            ))],
        );
        app.insert_resource(online);
        let list = app.world_mut().spawn(SkillRows::new(vec![50.0; 7])).id();
        app.world_mut().resource_mut::<Chosen>().0 = Some(TrainingSubject::Language(5));
        app.update();
        let drawn = app
            .world()
            .get::<SkillRows>(list)
            .unwrap()
            .drawn
            .as_ref()
            .unwrap();
        assert_eq!(drawn.len(), 3);
        let language = drawn
            .iter()
            .find(|row| row.subject == TrainingSubject::Language(5))
            .unwrap();
        assert_eq!(language.value, Some(20));
        assert_eq!(language.cost, Some(in_coins(10)));
        assert!(language.chosen);
        assert!(
            !drawn
                .iter()
                .find(|row| row.subject == TrainingSubject::Skill(5))
                .unwrap()
                .chosen
        );
        // Unknown installed language strings remain identifiable; unlearned zero is still trainable.
        assert!(
            drawn
                .iter()
                .any(|row| row.subject == TrainingSubject::Language(25)
                    && row.name == "Language 25"
                    && row.value == Some(0)
                    && row.cost == Some(Coins::default()))
        );
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Skill {
                skill_id: 105,
                value: 21,
            }],
        );
        app.update();
        let drawn = app
            .world()
            .get::<SkillRows>(list)
            .unwrap()
            .drawn
            .as_ref()
            .unwrap();
        assert_eq!(
            drawn
                .iter()
                .find(|row| row.subject == TrainingSubject::Language(5))
                .unwrap()
                .value,
            Some(21)
        );
        assert_eq!(
            drawn
                .iter()
                .find(|row| row.subject == TrainingSubject::Skill(5))
                .unwrap()
                .value,
            Some(70)
        );
    }

    #[test]
    fn a_window_the_player_closes_stays_closed_until_training_ends() {
        let mut app = crate::testing::app();
        app.add_systems(Update, window);
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::news(
            &mut online,
            [WorldEvent::Training(TrainingUpdate::Offered(
                TrainingOffer {
                    trainer: 42,
                    caps: vec![0; 100],
                    language_caps: Vec::new(),
                    price_scale: None,
                },
            ))],
        );
        app.insert_resource(online);
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Training));
        app.world_mut()
            .resource_mut::<Shown>()
            .close(WindowId::Training);
        // Leaving is asked; the training lasts until the session says it
        // ended, and the window does not come back meanwhile.
        app.update();
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Training));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Training(TrainingUpdate::Ended)],
        );
        app.update();
        // A guildmaster's next answer opens it again.
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Training(TrainingUpdate::Offered(
                TrainingOffer {
                    trainer: 42,
                    caps: vec![0; 100],
                    language_caps: Vec::new(),
                    price_scale: None,
                },
            ))],
        );
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Training));
    }

    #[test]
    fn the_list_names_each_skill_the_guildmaster_teaches_with_its_cost() {
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        let mut skills = vec![0; 100];
        skills[30] = 20;
        skills[10] = 5;
        player.skills = Some(skills);
        testing::admit(&mut online, 1, player);
        assert_eq!(
            rows(
                online.world(),
                None,
                &crate::hud::messages::Messages::default()
            )
            .len(),
            0
        );
        let mut caps = vec![0; 100];
        caps[30] = 200;
        caps[10] = 100;
        testing::news(
            &mut online,
            [WorldEvent::Training(TrainingUpdate::Offered(
                TrainingOffer {
                    trainer: 42,
                    caps,
                    language_caps: Vec::new(),
                    price_scale: None,
                },
            ))],
        );
        let rows = rows(
            online.world(),
            Some(TrainingSubject::Skill(30)),
            &crate::hud::messages::Messages::default(),
        );
        let names: Vec<_> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["Bash", "Kick"]);
        // Kick at 20: ten over ten, cubed, over a hundred is ten copper.
        assert_eq!(rows[1].cost, Some(in_coins(10)));
        assert_eq!(
            in_coins(12_345),
            Coins {
                platinum: 12,
                gold: 3,
                silver: 4,
                copper: 5
            }
        );
        assert!(rows[1].chosen && !rows[0].chosen);
        assert_eq!(rows[0].cost, Some(Coins::default()));
        assert!(is_guildmaster(Some(20)) && !is_guildmaster(Some(41)));
        assert!(!is_guildmaster(None));
    }

    #[test]
    fn p99_spend_withdraws_displayed_points_until_fresh_admission() {
        use eq_client_core::training::PracticeUpdate;

        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.practice_points = Some(3);
        testing::admit(&mut online, 1, player);
        assert_eq!(practice_points(online.world()), "3");
        let practice = |update| WorldEvent::Training(TrainingUpdate::Practice(update));
        testing::news(
            &mut online,
            [practice(PracticeUpdate::Requested {
                skill: 30,
                estimated_cost: 0,
            })],
        );
        assert_eq!(practice_points(online.world()), "");
        testing::news(
            &mut online,
            [practice(PracticeUpdate::SkillRaised {
                skill: 30,
                value: 13,
            })],
        );
        assert_eq!(practice_points(online.world()), "");
        assert_eq!(online.world().player().unwrap().practice_points, Some(3));
        testing::news(
            &mut online,
            [practice(PracticeUpdate::Unanswered { skill: 30 })],
        );
        assert_eq!(practice_points(online.world()), "");
        let mut next = testing::player(7);
        next.practice_points = Some(2);
        testing::admit(&mut online, 2, next);
        assert_eq!(practice_points(online.world()), "2");
    }

    #[test]
    fn training_rows_follow_selection_skills_and_ended_offers() {
        let mut app = crate::testing::app();
        app.add_systems(Update, fill);
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.skills = Some(vec![0; 100]);
        testing::admit(&mut online, 1, player);
        let mut caps = vec![0; 100];
        caps[30] = 200;
        testing::news(
            &mut online,
            [WorldEvent::Training(TrainingUpdate::Offered(
                TrainingOffer {
                    trainer: 42,
                    caps,
                    language_caps: Vec::new(),
                    price_scale: None,
                },
            ))],
        );
        app.insert_resource(online);
        let list = app.world_mut().spawn(SkillRows::new(vec![50.0; 7])).id();
        app.update();
        let first = app.world().get::<Children>(list).unwrap()[0];
        app.update();
        assert_eq!(app.world().get::<Children>(list).unwrap()[0], first);
        app.world_mut().resource_mut::<Chosen>().0 = Some(TrainingSubject::Skill(30));
        app.update();
        let selected = app.world().get::<Children>(list).unwrap()[0];
        assert_eq!(
            app.world().get::<BackgroundColor>(selected).unwrap().0,
            theme::BUTTON
        );
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Training(TrainingUpdate::Trained {
                skill: 30,
                value: 3,
                cost: 0,
            })],
        );
        app.update();
        let row = &app
            .world()
            .get::<SkillRows>(list)
            .unwrap()
            .drawn
            .as_ref()
            .unwrap()[0];
        assert_eq!(row.value, Some(3));
        assert_eq!(row.cost, Some(in_coins(practice_cost(3))));
        assert!(row.chosen);
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Training(TrainingUpdate::Ended)],
        );
        app.update();
        assert!(
            app.world()
                .get::<Children>(list)
                .is_none_or(Children::is_empty)
        );
    }
}
