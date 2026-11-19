use super::super::{Action, LootWindow, TradeState, input};
use super::*;
use crate::online::{OnlineState, testing};
use eq_client_core::{SpawnState, WorldEvent, WorldUpdate, loot::LootUpdate, world::NoSpells};
use std::sync::mpsc::Receiver;

/// An item from the preview, NO DROP or not, that a warrior can or can't use.
fn item(no_drop: bool, warrior: bool) -> InventoryItem {
    let mut item = crate::preview::items().remove(0);
    if no_drop {
        item.details.flags.push("NO DROP".into());
    }
    if !warrior {
        item.details.classes = 1 << 1;
    }
    item
}

/// A corpse of this kind and name.
fn corpse(spawn_id: u16, kind: SpawnKind, name: &str) -> SpawnState {
    let mut corpse = testing::pet(spawn_id, 0);
    corpse.kind = kind;
    corpse.name = name.into();
    corpse.pet_owner = None;
    corpse
}

/// The player, a level 1 human warrior named Example, with an NPC's corpse
/// (9), their own (10, 12) and another player's (11), the first open with
/// this item at place 0.
fn world(loot: InventoryItem) -> OnlineState {
    let mut online = OnlineState::new(false);
    testing::admit(&mut online, 5, testing::player(7));
    testing::spawns(
        &mut online,
        vec![
            corpse(9, SpawnKind::NpcCorpse, "a_rat's_corpse9"),
            corpse(10, SpawnKind::PlayerCorpse, "Example's corpse0"),
            corpse(11, SpawnKind::PlayerCorpse, "Other's corpse0"),
            corpse(12, SpawnKind::PlayerCorpse, "Example`s_corpse3"),
        ],
    );
    online.open_loot(9);
    for event in [
        WorldEvent::Loot(LootUpdate::Item {
            place: 0,
            item: Box::new(loot),
        }),
        WorldEvent::Loot(LootUpdate::Listed { corpse_id: 9 }),
    ] {
        online.tell(
            &WorldUpdate::Game(event),
            std::time::Instant::now(),
            &NoSpells,
        );
    }
    online
}

#[test]
fn each_choice_asks_only_about_another_corpses_no_drop_item_as_the_click_does() {
    let online = world(item(false, true));
    let world = online.world();
    for (choice, asks_usable, asks_unusable) in [
        (LootNoDrop::Ask, true, true),
        (LootNoDrop::AskUnusable, false, true),
        (LootNoDrop::Take, false, false),
    ] {
        for corpse_id in [9, 11] {
            assert_eq!(
                asks(world, corpse_id, &item(true, true), choice),
                asks_usable
            );
            assert_eq!(
                asks(world, corpse_id, &item(true, false), choice),
                asks_unusable
            );
            assert!(!asks(world, corpse_id, &item(false, false), choice));
        }
        // The player's own corpse never asks, however the server spells it.
        for own in [10, 12] {
            assert!(!asks(world, own, &item(true, false), choice));
        }
    }
}

/// The loot window open on corpse 9 with this item, under this choice.
fn app(loot: InventoryItem, choice: LootNoDrop) -> (App, Receiver<ClientCommand>) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(8);
    let mut app = App::new();
    let mut options = OptionsState::default();
    options.options.loot_no_drop = choice;
    app.insert_resource(TradeState {
        loot: Some(LootWindow {
            name: "Corpse".into(),
            opening: None,
            pending: None,
            loot_all: false,
        }),
        ..TradeState::default()
    })
    .insert_resource(world(loot))
    .insert_resource(Outbox::new(Some(sender)))
    .insert_resource(options)
    .init_resource::<crate::windows::Shown>()
    .init_resource::<crate::notices::Lines>()
    .init_resource::<ChatState>()
    .init_resource::<crate::ViewerSettings>()
    .init_resource::<crate::inventory::InventoryState>()
    .init_resource::<Messages>()
    .init_resource::<crate::escape::Escape>()
    .init_resource::<Asked>()
    .init_resource::<ButtonInput<KeyCode>>()
    .add_systems(
        Update,
        (crate::online::carry_as, input, confirm, command).chain(),
    );
    crate::keys::testing::install(&mut app);
    crate::clicks::testing::install(&mut app);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        bevy::window::PrimaryWindow,
    ));
    (app, receiver)
}

fn click(app: &mut App, action: Action) {
    let button = app
        .world_mut()
        .spawn((Button, action, Interaction::None))
        .id();
    crate::clicks::testing::click(app, button);
    app.world_mut().despawn(button);
}

fn asked(app: &App) -> Option<Question> {
    app.world().resource::<Asked>().question()
}

#[test]
fn a_no_drop_click_asks_first_and_takes_only_on_yes() {
    let (mut app, sent) = app(item(true, true), LootNoDrop::Ask);
    let question = Question::LootNoDrop {
        session_id: 5,
        corpse_id: 9,
        place: 0,
        item_id: 1,
    };
    click(&mut app, Action::Take(0));
    assert!(sent.try_recv().is_err());
    assert_eq!(asked(&app), Some(question));
    let text = text(
        app.world().resource::<OnlineState>().world(),
        question,
        None,
    );
    assert!(text.starts_with("Preview sword is NO DROP"), "{text}");
    // No takes nothing.
    app.world_mut().resource_mut::<Asked>().give(false);
    app.update();
    assert!(sent.try_recv().is_err());
    assert_eq!(asked(&app), None);
    // Yes takes it, as the click would have.
    click(&mut app, Action::Take(0));
    app.world_mut().resource_mut::<Asked>().give(true);
    app.update();
    assert!(matches!(
        sent.try_recv(),
        Ok(ClientCommand::LootItem {
            corpse_id: 9,
            own_id: 7,
            place: 0,
            ..
        })
    ));
    assert!(sent.try_recv().is_err());
}

#[test]
fn twenty_seconds_unanswered_takes_the_question_back_and_takes_nothing() {
    let (mut app, sent) = app(item(true, true), LootNoDrop::Ask);
    click(&mut app, Action::Take(0));
    assert!(asked(&app).is_some());
    app.update();
    assert!(asked(&app).is_some());
    let since = &mut app.world_mut().resource_mut::<TradeState>().no_drop.asking;
    since.as_mut().unwrap().1 -= WAIT;
    app.update();
    assert_eq!(asked(&app), None);
    assert!(
        app.world()
            .resource::<TradeState>()
            .no_drop
            .asking
            .is_none()
    );
    // A late Yes finds nothing to answer.
    app.world_mut().resource_mut::<Asked>().give(true);
    app.update();
    assert!(sent.try_recv().is_err());
}

#[test]
fn a_click_the_choice_allows_takes_at_once() {
    for (loot, choice) in [
        (item(false, false), LootNoDrop::Ask),
        (item(true, true), LootNoDrop::AskUnusable),
        (item(true, false), LootNoDrop::Take),
    ] {
        let (mut app, sent) = app(loot, choice);
        click(&mut app, Action::Take(0));
        assert!(matches!(
            sent.try_recv(),
            Ok(ClientCommand::LootItem { place: 0, .. })
        ));
        assert_eq!(asked(&app), None);
    }
}

#[test]
fn the_mac_era_client_takes_at_once_as_before() {
    let (mut app, sent) = app(item(true, false), LootNoDrop::Ask);
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .installed_client = eq_client_assets::ui::InstalledClient::EqMac;
    click(&mut app, Action::Take(0));
    assert!(matches!(
        sent.try_recv(),
        Ok(ClientCommand::LootItem { place: 0, .. })
    ));
    assert_eq!(asked(&app), None);
}

#[test]
fn the_mac_era_client_answers_lootnodrop_as_before() {
    let (mut app, _sent) = app(item(false, true), LootNoDrop::Ask);
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .installed_client = eq_client_assets::ui::InstalledClient::EqMac;
    let mut chat = app.world_mut().resource_mut::<ChatState>();
    assert_eq!(
        crate::chat::client_request("/lootnodrop always", &mut chat),
        Some(Ok(()))
    );
    app.update();
    assert_eq!(
        app.world().resource::<OptionsState>().options.loot_no_drop,
        LootNoDrop::Ask
    );
    assert_eq!(
        app.world().resource::<ChatState>().newest(),
        "This client does not take /lootnodrop that way yet."
    );
}

#[test]
fn taking_all_leaves_an_item_that_asks_and_keeps_the_window() {
    let (mut app, sent) = app(item(true, false), LootNoDrop::AskUnusable);
    click(&mut app, Action::TakeAll);
    app.update();
    assert!(sent.try_recv().is_err());
    assert_eq!(asked(&app), None);
    let trade = app.world().resource::<TradeState>();
    assert!(trade.loot.as_ref().is_some_and(|window| !window.loot_all));
}

fn lines(app: &App) -> Vec<String> {
    app.world()
        .resource::<ChatState>()
        .history
        .lines(eq_client_core::chat::ChatTab::All)
        .into_iter()
        .map(|(_, line)| line.message.text.clone())
        .collect()
}

#[test]
fn lootnodrop_sets_the_choice_its_word_names_and_lists_them_otherwise() {
    let (mut app, _sent) = app(item(false, true), LootNoDrop::Ask);
    let ask = |app: &mut App, typed: &str| {
        let mut chat = app.world_mut().resource_mut::<ChatState>();
        assert_eq!(crate::chat::client_request(typed, &mut chat), Some(Ok(())));
        app.update();
    };
    ask(&mut app, "/lootnodrop never");
    assert_eq!(
        app.world().resource::<OptionsState>().options.loot_no_drop,
        LootNoDrop::Take
    );
    assert_eq!(lines(&app), [LootNoDrop::Take.line().1]);
    ask(&mut app, "/LootNoDrop Sometimes");
    assert_eq!(
        app.world().resource::<OptionsState>().options.loot_no_drop,
        LootNoDrop::AskUnusable
    );
    ask(&mut app, "/lootnodrop");
    ask(&mut app, "/lootnodrop maybe");
    let all: Vec<_> = LootNoDrop::ALL.map(|choice| choice.line().1).into();
    assert_eq!(lines(&app)[2..5], all[..]);
    assert_eq!(lines(&app)[5..], all[..]);
    assert_eq!(
        app.world().resource::<OptionsState>().options.loot_no_drop,
        LootNoDrop::AskUnusable
    );
}
