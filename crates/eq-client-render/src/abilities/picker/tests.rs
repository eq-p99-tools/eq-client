use super::*;
use crate::online::testing;
use eq_client_core::{abilities::Ability, hotbar::Binding};

/// A rogue with Fishing, Pick Lock and Sense Traps, in session 1.
fn rogue(session_id: u64) -> OnlineState {
    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.class = Some(9);
    player.level = 21;
    let mut skills = vec![0; 100];
    for skill in [35, 55, 62] {
        skills[skill] = 5;
    }
    player.skills = Some(skills);
    testing::admit(&mut online, session_id, player);
    online
}

fn app() -> App {
    let mut app = crate::testing::app();
    app.init_resource::<Choices>()
        .init_resource::<Picker>()
        .insert_resource(rogue(1))
        .add_systems(Update, (right_click, choose, window, fill, frame).chain());
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::ActionsWindow);
    app
}

fn slot_three() -> AbilityButton {
    AbilityButton {
        page: super::super::Page::Abilities,
        index: 2,
    }
}

fn open_for(app: &mut App, button: AbilityButton) {
    app.world_mut().resource_mut::<Picker>().ask(button);
    app.update();
    app.update();
}

fn shown(app: &App) -> bool {
    app.world()
        .resource::<Shown>()
        .is_open(WindowId::SkillSelect)
}

#[test]
fn a_right_click_opens_the_picker_in_front_for_the_button_under_the_pointer() {
    let mut app = app();
    app.world_mut()
        .spawn((slot_three(), Interaction::Hovered, Button));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    app.update();
    app.update();
    assert!(shown(&app));
    assert_eq!(
        app.world().resource::<Picker>().button(),
        Some(slot_three())
    );
    assert_eq!(
        app.world()
            .resource::<windows::Stack>()
            .front_to_back()
            .next(),
        Some(WindowId::SkillSelect)
    );
}

#[test]
fn a_row_puts_its_ability_on_the_button_and_closes_the_picker() {
    let mut app = app();
    open_for(&mut app, slot_three());
    let rows: Vec<(Entity, PickRow)> = app
        .world_mut()
        .query::<(Entity, &PickRow)>()
        .iter(app.world())
        .map(|(entity, row)| (entity, *row))
        .collect();
    let (fishing, _) = rows
        .iter()
        .find(|(_, row)| row.slot == Slot::Skill(Ability::Fishing.skill()))
        .copied()
        .unwrap();
    app.world_mut()
        .entity_mut(fishing)
        .insert(Interaction::Pressed);
    app.update();
    assert!(!shown(&app));
    let choices = app.world().resource::<Choices>();
    assert!(choices.kept());
    let online = app.world().resource::<OnlineState>();
    assert_eq!(
        super::super::assigned(online.world(), choices, slot_three()),
        Some(Binding::Ability(Ability::Fishing))
    );
}

#[test]
fn closing_without_a_choice_changes_nothing_and_a_new_session_or_the_actions_window_closes_it() {
    let mut app = app();
    open_for(&mut app, slot_three());
    app.world_mut()
        .resource_mut::<Shown>()
        .close(WindowId::SkillSelect);
    app.update();
    assert_eq!(app.world().resource::<Picker>().button(), None);
    assert!(!app.world().resource::<Choices>().kept());
    open_for(&mut app, slot_three());
    assert!(shown(&app));
    app.insert_resource(rogue(2));
    app.update();
    assert!(!shown(&app));
    open_for(&mut app, slot_three());
    app.world_mut()
        .resource_mut::<Shown>()
        .close(WindowId::ActionsWindow);
    app.update();
    assert!(!shown(&app));
    assert_eq!(app.world().resource::<Picker>().button(), None);
}

#[test]
fn the_rows_follow_the_players_skills_while_the_picker_is_open() {
    let mut app = app();
    open_for(&mut app, slot_three());
    let count = |app: &mut App| {
        app.world_mut()
            .query::<&PickRow>()
            .iter(app.world())
            .count()
    };
    let before = count(&mut app);
    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.class = Some(9);
    player.level = 21;
    let mut skills = vec![0; 100];
    for skill in [35, 48, 55, 62] {
        skills[skill] = 5;
    }
    player.skills = Some(skills);
    testing::admit(&mut online, 1, player);
    app.insert_resource(online);
    app.update();
    app.update();
    assert_eq!(count(&mut app), before + 1);
}

#[test]
fn nothing_is_saved_before_the_first_choice_and_all_ten_after_it() {
    let directory = std::env::temp_dir().join(format!(
        "eq-ability-choices-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let mut app = crate::testing::app();
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .settings_directory = Some(directory.clone());
    app.init_resource::<Choices>()
        .init_resource::<super::super::choices::Files>()
        .add_systems(
            Update,
            (crate::profile_files::follow, super::super::choices::persist).chain(),
        );
    let mut online = rogue(1);
    testing::news(
        &mut online,
        [eq_client_core::WorldEvent::WorldName {
            short_name: "ExampleWorld".into(),
        }],
    );
    app.insert_resource(online);
    app.update();
    app.update();
    let file = || {
        std::fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().starts_with("abilities"))
            .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
    };
    assert_eq!(file(), None);
    app.world_mut()
        .resource_scope(|world, mut choices: Mut<Choices>| {
            let online = world.resource::<OnlineState>();
            choices.choose(
                online.world(),
                slot_three(),
                Slot::Skill(Ability::PickLock.skill()),
            );
        });
    app.update();
    let saved = file().unwrap();
    assert!(saved.contains("Ability3=35"));
    assert_eq!(saved.matches('=').count(), 10);
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn a_left_click_opens_the_picker_only_on_an_empty_button() {
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let mut app = app();
    app.insert_resource(crate::outbox::Outbox::new(Some(tx)))
        .add_systems(Update, super::super::input.before(window));
    app.world_mut()
        .resource_scope(|world, mut choices: Mut<Choices>| {
            let online = world.resource::<OnlineState>();
            choices.choose(online.world(), slot_three(), Slot::Empty);
            let unusable = AbilityButton {
                page: super::super::Page::Abilities,
                index: 3,
            };
            choices.choose(online.world(), unusable, Slot::Skill(71));
        });
    let empty = app.world_mut().spawn((Button, slot_three())).id();
    crate::clicks::testing::click(&mut app, empty);
    app.update();
    assert!(shown(&app));
    app.world_mut()
        .resource_mut::<Shown>()
        .close(WindowId::SkillSelect);
    app.update();
    // One holding what the client cannot use does nothing and sends nothing.
    let unusable = app
        .world_mut()
        .spawn((
            Button,
            AbilityButton {
                page: super::super::Page::Abilities,
                index: 3,
            },
        ))
        .id();
    crate::clicks::testing::click(&mut app, unusable);
    app.update();
    assert!(!shown(&app));
    assert!(rx.try_recv().is_err());
}
