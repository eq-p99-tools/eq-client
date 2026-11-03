//! The skin's Skills window, following the installed generation's list,
//! rank and numeric-column rules. Installed strings supply the words;
//! unknown profile/session inputs remain unknown.
use crate::online::OnlineState;
use bevy::prelude::*;
use eq_client_assets::{
    skills::{LookAhead, Presence, RankText, SkillInputs, SkillRules},
    ui::InstalledClient,
};
use eq_client_core::world::ClientWorld;

/// Optional facts supplied only when their authoritative session fields are
/// mapped. Missing facts never acquire a guessed level bound.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SkillWindowInputs {
    pub(crate) look_ahead: LookAhead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Projection {
    revision: u64,
    client: InstalledClient,
    inputs: SkillWindowInputs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    id: u32,
    name: String,
    rank: String,
    value: Option<i32>,
}

/// The Skills window's list, which the client fills from the player's
/// skills.
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct SkillsList {
    /// Each column's width, from the left.
    columns: Vec<f32>,
    /// The rows last drawn, to redraw only when one changes.
    drawn: Option<Vec<Row>>,
    /// Inputs last projected; None also covers a newly created window.
    checked: Option<Projection>,
}

impl SkillsList {
    pub(crate) fn new(columns: Vec<f32>) -> Self {
        Self {
            columns,
            drawn: None,
            checked: None,
        }
    }
}

/// Values servers use to mark a skill as reset rather than learned.
pub(crate) const RESET: u32 = 254;

/// Rows established by the known class/profile inputs. A row dependent on
/// an unknown look-ahead bound stays withheld until that fact is supplied.
fn rows(
    world: &ClientWorld,
    client: InstalledClient,
    facts: SkillWindowInputs,
    messages: &crate::hud::messages::Messages,
) -> Vec<Row> {
    let Some(player) = world.player() else {
        return Vec::new();
    };
    let rules = client.skill_rules();
    let inputs = SkillInputs {
        race: player.race,
        class: player.class,
        level: player.level,
        look_ahead: facts.look_ahead,
    };
    let mut rows = Vec::new();
    for (index, raw) in player
        .skills
        .as_deref()
        .unwrap_or_default()
        .iter()
        .take(100)
        .enumerate()
    {
        let id = u32::try_from(index).expect("skill index below 100");
        if rules.presence(id, *raw, inputs) != Presence::Shown {
            continue;
        }
        let name = rules
            .name_string(id)
            .and_then(|name| messages.argument_free(name))
            .map_or_else(
                || {
                    eq_client_core::skills::name(id)
                        .map_or_else(|| format!("Skill {id}"), str::to_owned)
                },
                str::to_owned,
            );
        if let Some(row) = row(id, name, *raw, inputs, rules, messages) {
            rows.push(row);
        }
    }
    let common_variant = match client {
        InstalledClient::Titanium => world
            .language_zero_selector()
            .map(eq_client_core::rules::LanguageZeroSelector::alternate_name),
        InstalledClient::EqMac => None,
    };
    for (index, raw) in player
        .languages
        .as_deref()
        .unwrap_or_default()
        .iter()
        .take(32)
        .enumerate()
    {
        let language = u8::try_from(index).expect("language index below 32");
        if !SkillRules::language_present(language, *raw) {
            continue;
        }
        let name = SkillRules::language_name_string(language, common_variant)
            .and_then(|name| messages.argument_free(name))
            .map_or_else(|| format!("Language {language}"), str::to_owned);
        let id = 100 + u32::from(language);
        if let Some(row) = row(id, name, u32::from(*raw), inputs, rules, messages) {
            rows.push(row);
        }
    }
    rows.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    rows
}

fn row(
    id: u32,
    name: String,
    raw: u32,
    inputs: SkillInputs,
    rules: &SkillRules,
    messages: &crate::hud::messages::Messages,
) -> Option<Row> {
    let text = rules.row_text(id, raw, inputs)?;
    let rank = match text.rank {
        RankText::String(id) => messages.text(id, "Rank unavailable"),
        RankText::TrainableAt(level) => {
            messages.official(0x871, &[level.to_string()], &format!("Level {level}"))
        }
        RankText::Unknown => "Rank unavailable".into(),
    };
    Some(Row {
        id,
        name,
        rank,
        value: text.value,
    })
}

/// The height of a row of the list.
const ROW_HEIGHT: f32 = 16.0;

/// Fills the Skills window's list again whenever a skill or its value
/// changes.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn fill(
    mut commands: Commands,
    online: Res<OnlineState>,
    settings: Option<Res<crate::ViewerSettings>>,
    facts: Option<Res<SkillWindowInputs>>,
    messages: Res<crate::hud::messages::Messages>,
    mut lists: Query<(Entity, &mut SkillsList, Option<&Children>)>,
) {
    let input = Projection {
        revision: online.world().skills_revision(),
        client: settings.map_or_else(Default::default, |settings| settings.0.installed_client),
        inputs: facts.as_deref().copied().unwrap_or_default(),
    };
    // No list, or every existing list is current: no row allocation or sorting.
    if !messages.is_changed() && lists.iter().all(|(_, list, _)| list.checked == Some(input)) {
        return;
    }
    let wanted = rows(online.world(), input.client, input.inputs, &messages);
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
        for row in &wanted {
            let cells = [
                row.name.clone(),
                row.rank.clone(),
                row.value
                    .map_or_else(String::new, |value| value.to_string()),
            ];
            let line = commands
                .spawn((
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ChildOf(entity),
                ))
                .id();
            crate::list_cells::text(&mut commands, line, cells, list.columns.iter().copied());
        }
        list.drawn = Some(wanted.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;

    fn messages(client: InstalledClient) -> crate::hud::messages::Messages {
        let rules = client.skill_rules();
        crate::hud::messages::Messages::parse(&format!(
            "EQST0002\n12\n{} Alpha\n{} Beta\n3224 Low\n3225 Next\n3228 Middle\n3234 Last\n2161 Later %1\n3200 Test Language\n3114 Default speech\n3113 Alternate speech\n",
            rules.name_string(10).unwrap(),
            rules.name_string(30).unwrap()
        ))
    }

    #[test]
    fn names_and_rank_words_come_from_strings_and_values_keep_their_column() {
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let mut online = OnlineState::new(true);
            let mut player = testing::player(7);
            player.level = 65;
            player.class = Some(1);
            player.race = 1;
            let mut skills = vec![0; 100];
            skills[30] = 20;
            skills[10] = 5;
            skills[29] = RESET;
            player.skills = Some(skills);
            testing::admit(&mut online, 1, player);
            let rows = rows(
                online.world(),
                client,
                SkillWindowInputs::default(),
                &messages(client),
            );
            let alpha = rows.iter().find(|row| row.id == 10).unwrap();
            let beta = rows.iter().find(|row| row.id == 30).unwrap();
            assert_eq!(
                (&*alpha.name, &*alpha.rank, alpha.value),
                ("Alpha", "Low", Some(5))
            );
            assert_eq!(
                (&*beta.name, &*beta.rank, beta.value),
                ("Beta", "Next", Some(20))
            );
            assert!(!rows.iter().any(|row| row.id == 29));
        }
    }

    #[test]
    fn skill_lists_refresh_on_changes_and_recreation_but_not_unrelated_news() {
        use eq_client_core::WorldEvent;
        let mut app = crate::testing::app();
        app.add_systems(Update, fill);
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.level = 65;
        player.class = Some(1);
        player.race = 1;
        player.skills = Some(vec![1; 100]);
        testing::admit(&mut online, 1, player);
        app.insert_resource(online);
        let list = app.world_mut().spawn(SkillsList::new(vec![100.0; 3])).id();
        app.update();
        let children = app.world().get::<Children>(list).unwrap().to_vec();
        let revision = app.world().get::<SkillsList>(list).unwrap().checked;
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Coins(eq_client_core::Coins::default())],
        );
        app.update();
        assert_eq!(
            app.world().get::<Children>(list).unwrap().to_vec(),
            children
        );
        assert_eq!(
            app.world().get::<SkillsList>(list).unwrap().checked,
            revision
        );

        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Skill {
                skill_id: 30,
                value: 7,
            }],
        );
        app.update();
        assert!(
            app.world()
                .get::<SkillsList>(list)
                .unwrap()
                .drawn
                .as_ref()
                .unwrap()
                .iter()
                .any(|row| row.id == 30 && row.value == Some(7))
        );
        assert_ne!(
            app.world().get::<Children>(list).unwrap().to_vec(),
            children
        );

        let new_list = app.world_mut().spawn(SkillsList::new(vec![100.0; 3])).id();
        app.update();
        assert_eq!(
            app.world().get::<SkillsList>(new_list).unwrap().drawn,
            app.world().get::<SkillsList>(list).unwrap().drawn
        );
        assert!(app.world().get::<Children>(new_list).is_some());
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Skill {
                skill_id: 10,
                value: 254,
            }],
        );
        app.update();
        assert!(
            app.world()
                .get::<SkillsList>(list)
                .unwrap()
                .drawn
                .as_ref()
                .unwrap()
                .iter()
                .any(|row| row.id == 10 && row.value == Some(254))
        );
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = InstalledClient::EqMac;
        app.update();
        assert!(
            !app.world()
                .get::<SkillsList>(list)
                .unwrap()
                .drawn
                .as_ref()
                .unwrap()
                .iter()
                .any(|row| row.id == 10)
        );
        app.world_mut().resource_mut::<OnlineState>().restart();
        app.update();
        for list in [list, new_list] {
            assert!(
                app.world()
                    .get::<Children>(list)
                    .is_none_or(Children::is_empty)
            );
        }
    }

    #[test]
    fn known_level_changes_and_explicit_look_ahead_refresh_zero_rows() {
        use eq_client_core::WorldEvent;
        let client = InstalledClient::EqMac;
        let rules = client.skill_rules();
        let skill = (0..100)
            .find(|skill| {
                rules.class_rule(*skill, 1).is_some_and(|rule| {
                    rule.usable() && rule.train_level > 1 && rule.train_level <= 65
                })
            })
            .unwrap();
        let train_level = rules.class_rule(skill, 1).unwrap().train_level;
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = client;
        app.add_systems(Update, fill);
        let mut player = testing::player(7);
        player.class = Some(1);
        player.race = 1;
        player.level = train_level - 1;
        player.skills = Some(vec![0; 100]);
        testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            1,
            player,
        );
        let list = app.world_mut().spawn(SkillsList::default()).id();
        let includes = |app: &App| {
            app.world()
                .get::<SkillsList>(list)
                .unwrap()
                .drawn
                .as_ref()
                .unwrap()
                .iter()
                .any(|row| row.id == skill && row.value == Some(0))
        };
        app.update();
        assert!(!includes(&app));
        let revision = app
            .world()
            .resource::<OnlineState>()
            .world()
            .skills_revision();
        app.insert_resource(SkillWindowInputs {
            look_ahead: LookAhead {
                player_spawn: Some(true),
                bound: Some(train_level),
            },
        });
        app.update();
        assert!(includes(&app));
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .skills_revision(),
            revision
        );
        app.world_mut().remove_resource::<SkillWindowInputs>();
        app.update();
        assert!(!includes(&app));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Level {
                current: train_level,
                previous: train_level - 1,
                experience: 1,
            }],
        );
        app.update();
        assert!(includes(&app));
    }

    fn language_list(client: InstalledClient) -> (App, Entity) {
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = client;
        app.insert_resource(messages(client))
            .add_systems(Update, fill);
        let mut player = testing::player(7);
        player.languages = Some(vec![0; 32]);
        player.languages.as_mut().unwrap()[0] = 100;
        testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            1,
            player,
        );
        let list = app.world_mut().spawn(SkillsList::default()).id();
        app.update();
        (app, list)
    }

    fn language_name(app: &App, list: Entity) -> &str {
        &app.world()
            .get::<SkillsList>(list)
            .unwrap()
            .drawn
            .as_ref()
            .unwrap()
            .iter()
            .find(|row| row.id == 100)
            .unwrap()
            .name
    }

    fn language_rules(client: InstalledClient, raw: u32) -> eq_client_core::WorldEvent {
        let mut body = vec![
            0;
            if client == InstalledClient::Titanium {
                264
            } else {
                244
            }
        ];
        body[..4].copy_from_slice(&raw.to_le_bytes());
        eq_client_core::WorldEvent::Rules(match client {
            InstalledClient::Titanium => eq_client_core::rules::titanium_rules(&body).unwrap(),
            InstalledClient::EqMac => eq_client_core::rules::eqmac_rules(&body).unwrap(),
        })
    }

    #[test]
    fn language_events_optional_selectors_and_string_reload_refresh_the_same_list() {
        use eq_client_core::WorldEvent;
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let (mut app, list) = language_list(client);
            let expected = match client {
                InstalledClient::Titanium => ["Default speech", "Alternate speech", "Changed"],
                InstalledClient::EqMac => ["Language 0"; 3],
            };
            assert_eq!(language_name(&app, list), "Language 0");
            testing::news(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                [language_rules(client, 0)],
            );
            app.update();
            assert_eq!(language_name(&app, list), expected[0]);
            testing::news(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                [language_rules(client, 0x100)],
            );
            app.update();
            assert_eq!(language_name(&app, list), expected[1]);
            testing::news(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                [WorldEvent::Skill {
                    skill_id: 101,
                    value: 10,
                }],
            );
            app.update();
            assert!(
                app.world()
                    .get::<SkillsList>(list)
                    .unwrap()
                    .drawn
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|row| row.id == 101
                        && row.name == "Test Language"
                        && row.rank == "Low"
                        && row.value == Some(10))
            );
            let children = app.world().get::<Children>(list).unwrap().to_vec();
            testing::news(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                [
                    language_rules(client, 0x100),
                    WorldEvent::Skill {
                        skill_id: 101,
                        value: 10,
                    },
                ],
            );
            app.update();
            assert_eq!(
                app.world().get::<Children>(list).unwrap().to_vec(),
                children
            );
            app.insert_resource(crate::hud::messages::Messages::parse(
                "EQST0002\n1\n3113 Changed\n",
            ));
            app.update();
            assert_eq!(language_name(&app, list), expected[2]);
        }
    }

    #[test]
    fn skills_language_choice_survives_admission_but_is_unknown_after_restart() {
        let client = InstalledClient::Titanium;
        let (mut app, list) = language_list(client);
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [language_rules(client, u32::MAX)],
        );
        app.update();
        assert_eq!(language_name(&app, list), "Alternate speech");
        let player = app
            .world()
            .resource::<OnlineState>()
            .world()
            .player()
            .unwrap()
            .clone();
        testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            2,
            player.clone(),
        );
        app.update();
        assert_eq!(language_name(&app, list), "Alternate speech");
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [language_rules(client, 0)],
        );
        app.update();
        assert_eq!(language_name(&app, list), "Default speech");

        app.world_mut().resource_mut::<OnlineState>().restart();
        testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            3,
            player,
        );
        app.update();
        assert_eq!(language_name(&app, list), "Language 0");
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [language_rules(client, 0x8000_0000)],
        );
        app.update();
        assert_eq!(language_name(&app, list), "Alternate speech");
        // The installed generation remains part of the projection. A
        // contrived retained Titanium fact cannot enable an EQMac mapping.
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = InstalledClient::EqMac;
        app.update();
        assert_eq!(language_name(&app, list), "Language 0");
    }
}
