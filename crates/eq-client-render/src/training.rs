//! Training at a guildmaster, as the official client does: the trade key on
//! the player's own guildmaster asks to train, the answer opens the skin's
//! Training window with the skills the guildmaster teaches, Train practices
//! the chosen one, and Done or closing the window leaves. The session
//! decides what may be practiced; this module keeps the window with the
//! world's training.
use crate::{
    online::OnlineState,
    outbox::Outbox,
    theme::{self, Size},
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, Coins,
    training::{TrainingRequest, practice_cost},
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
pub(crate) struct Chosen(pub(crate) Option<u32>);

/// The Training window's Train button.
#[derive(Component)]
pub(crate) struct TrainButton;

/// A row of the Training window's list: the skill it offers, and its place
/// from the top.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SkillRow {
    pub(crate) skill: u32,
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
}

impl SkillRows {
    pub(crate) fn new(columns: Vec<f32>) -> Self {
        Self {
            columns,
            drawn: None,
        }
    }
}

/// A row as drawn: the skill, its name, its value and the next practice's
/// cost, and whether it is the one chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    skill: u32,
    name: String,
    value: u32,
    cost: Coins,
    chosen: bool,
}

/// The skills the guildmaster teaches, by name, with the player's value and
/// what the next practice costs.
fn rows(world: &ClientWorld, chosen: Option<u32>) -> Vec<Row> {
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
            let value = skills.get(index).copied().unwrap_or(0);
            Some(Row {
                skill,
                name: eq_client_core::skills::name(skill)
                    .map_or_else(|| format!("Skill {skill}"), str::to_owned),
                value,
                cost: in_coins(practice_cost(value)),
                chosen: chosen == Some(skill),
            })
        })
        .collect();
    rows.sort_by(|left, right| left.name.cmp(&right.name));
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
) {
    let training = online.world().training().map(|offer| offer.trainer);
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
    train: Query<&Interaction, (Changed<Interaction>, With<TrainButton>)>,
) {
    for (interaction, row) in &rows {
        if *interaction == Interaction::Pressed && chosen.0 != Some(row.skill) {
            chosen.0 = Some(row.skill);
        }
    }
    let pressed = train
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);
    if pressed
        && online.world().training().is_some()
        && let Some(skill) = chosen.0
    {
        ask(TrainingRequest::Train { skill }, online.world(), &outbox);
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
    mut lists: Query<(Entity, &mut SkillRows, Option<&Children>)>,
) {
    let wanted = rows(online.world(), chosen.0);
    for (entity, mut list, children) in &mut lists {
        if list.drawn.as_ref() == Some(&wanted) {
            continue;
        }
        if let Some(children) = children {
            for child in children {
                commands.entity(*child).despawn();
            }
        }
        for (index, row) in wanted.iter().enumerate() {
            let cells = [
                row.name.clone(),
                // The official client's rank for a value is not known yet.
                String::new(),
                row.value.to_string(),
                row.cost.platinum.to_string(),
                row.cost.gold.to_string(),
                row.cost.silver.to_string(),
                row.cost.copper.to_string(),
            ];
            let line = commands
                .spawn((
                    Button,
                    SkillRow {
                        skill: row.skill,
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
            for (words, width) in cells.into_iter().zip(&list.columns) {
                commands.spawn((
                    theme::text(words, Size::Small, theme::INK_BRIGHT),
                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    Node {
                        width: px(*width),
                        flex_shrink: 0.0,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    ChildOf(line),
                ));
            }
        }
        list.drawn = Some(wanted.clone());
    }
}

/// The player's practice points, as the Training window counts them.
pub(crate) fn practice_points(world: &ClientWorld) -> String {
    world
        .player()
        .and_then(|player| player.practice_points)
        .map_or_else(String::new, |points| points.to_string())
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
        assert_eq!(rows(online.world(), None).len(), 0);
        let mut caps = vec![0; 100];
        caps[30] = 200;
        caps[10] = 100;
        testing::news(
            &mut online,
            [WorldEvent::Training(TrainingUpdate::Offered(
                TrainingOffer { trainer: 42, caps },
            ))],
        );
        let rows = rows(online.world(), Some(30));
        let names: Vec<_> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["Bash", "Kick"]);
        // Kick at 20: ten over ten, cubed, over a hundred is ten copper.
        assert_eq!(rows[1].cost, in_coins(10));
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
        assert_eq!(rows[0].cost, Coins::default());
        assert!(is_guildmaster(Some(20)) && !is_guildmaster(Some(41)));
        assert!(!is_guildmaster(None));
    }
}
