use super::*;
use crate::{
    chat::ChatState,
    clicks::testing as clicks,
    navigation::{self, NavigationKeys},
    online::{OnlineState, testing as session},
    spellbook::{BookEntry, BookHand, BookPlace, BookSelection, Entry, GemChoice},
};
use bevy::input::ButtonState;
use eq_client_core::{ClientCommand, SpellUpdate};

fn write_slot(book: &mut SpellBook, slot: u32, spell_id: u32) {
    book.apply(&SpellUpdate::Slot {
        slot,
        spell_id,
        mode: 0,
    });
}

fn book() -> SpellBook {
    let mut book = SpellBook::default();
    for (slot, id) in [(4, 42), (32, 44), (63, 43), (88, 99)] {
        write_slot(&mut book, slot, id);
    }
    book
}

fn app() -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
    let mut app = crate::testing::app();
    let mut online = OnlineState::new(true);
    session::admit(&mut online, 7, session::player(1));
    session::book(&mut online, book());
    let (queue, sent) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(online)
        .insert_resource(crate::outbox::Outbox::new(Some(queue)))
        .insert_resource(SpellNames::parse("42^Zebra Ward\n43^Alpha Ward\n44^Peace"));
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::Spellbook);
    (app, sent)
}

fn query(app: &mut App, query: &str) {
    app.world_mut().resource_mut::<BookView>().search.query = query.into();
}

#[test]
fn skinned_results_keep_the_real_slot_and_clear_restores_the_open_spread() {
    let (mut app, sent) = app();
    app.insert_resource(crate::skinned::Skinned::of(&[WindowId::Spellbook]))
        .add_systems(
            Update,
            (
                refresh,
                crate::spellbook::book_clicks,
                crate::spellbook::book_present,
            )
                .chain(),
        );
    app.world_mut().resource_mut::<BookView>().spread = 2;
    let [first, second, blank] = [0, 1, 2].map(|index| {
        app.world_mut()
            .spawn((Button, BookPlace(index), Tooltip::default()))
            .id()
    });
    app.update();
    assert!(
        app.world()
            .get::<Tooltip>(first)
            .unwrap()
            .0
            .starts_with("Peace")
    );
    query(&mut app, " wArD ");
    app.update();
    let tip = |app: &App, entity| app.world().get::<Tooltip>(entity).unwrap().0.clone();
    assert!(tip(&app, first).starts_with("Zebra Ward · Book slot 5"));
    assert!(tip(&app, second).starts_with("Alpha Ward · Book slot 64"));
    assert_eq!(
        app.world().get::<Node>(blank).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world().resource::<OnlineState>().world().spell_book(),
        Some(&book())
    );
    clicks::click(&mut app, second);
    let held = app.world().resource::<BookHand>().held.unwrap();
    assert_eq!(
        held,
        Entry {
            slot: 63,
            spell: 43
        }
    );
    query(&mut app, "Spell 99");
    app.update();
    assert!(tip(&app, first).starts_with("Spell 99 · Book slot 89"));
    assert_eq!(app.world().resource::<BookHand>().held, Some(held));
    let memorize = |app: &App| {
        crate::spellbook::memorize_held(
            held,
            1,
            (
                app.world().resource::<OnlineState>(),
                app.world().resource::<crate::outbox::Outbox>(),
            ),
            (
                app.world().resource::<SpellNames>(),
                app.world().resource::<crate::hud::messages::Messages>(),
            ),
            None,
        )
    };
    assert!(memorize(&app).is_none());
    assert!(matches!(
        sent.try_recv().unwrap(),
        ClientCommand::MemorizeSpell {
            spell_id: 43,
            gem: 1,
            ..
        }
    ));

    // The spell now exists in a different slot. A click using the old held
    // entry is refused even before the next book frame can clear the cursor.
    let mut moved = book();
    moved.apply(&SpellUpdate::BookSwap { from: 63, to: 65 });
    session::book(&mut app.world_mut().resource_mut::<OnlineState>(), moved);
    assert!(memorize(&app).is_some());
    assert!(sent.try_recv().is_err());
    query(&mut app, "");
    app.update();
    assert!(tip(&app, first).starts_with("Peace"));
    assert_eq!(
        app.world().get::<Node>(blank).unwrap().display,
        Display::Flex
    );
    assert!(app.world().resource::<BookHand>().held.is_none());
}

#[test]
fn fallback_keeps_rows_and_gem_selection_on_the_same_authoritative_entry() {
    let (mut app, sent) = app();
    app.add_systems(Startup, |mut commands: Commands| {
        crate::spellbook::spawn(&mut commands);
    })
    .add_systems(Update, (refresh, crate::spellbook::update).chain());
    query(&mut app, "ward");
    app.update();
    let world = app.world_mut();
    let row = world
        .query::<(Entity, &BookEntry)>()
        .iter(world)
        .find(|(_, entry)| entry.0 == 1)
        .unwrap()
        .0;
    assert!(
        world
            .get::<Text>(row)
            .unwrap()
            .0
            .contains("64 | Alpha Ward")
    );
    let gem = world
        .query::<(Entity, &GemChoice)>()
        .iter(world)
        .find(|(_, choice)| choice.0 == 2)
        .unwrap()
        .0;
    clicks::click(&mut app, row);
    assert_eq!(
        app.world().resource::<BookSelection>().search_slot,
        Some(63)
    );
    let mut moved = book();
    moved.apply(&SpellUpdate::BookSwap { from: 63, to: 65 });
    session::book(&mut app.world_mut().resource_mut::<OnlineState>(), moved);
    clicks::click(&mut app, gem);
    assert!(sent.try_recv().is_err());
    assert!(app.world().resource::<BookSelection>().selected.is_none());
    assert!(
        app.world()
            .get::<Text>(row)
            .unwrap()
            .0
            .contains("66 | Alpha Ward")
    );
    clicks::click(&mut app, row);
    clicks::click(&mut app, gem);
    assert!(matches!(
        sent.try_recv().unwrap(),
        ClientCommand::MemorizeSpell {
            spell_id: 43,
            gem: 2,
            ..
        }
    ));
}

fn key(app: &mut App, key_code: KeyCode, logical_key: Key, text: Option<&str>, repeat: bool) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key_code);
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key,
        text: text.map(Into::into),
        repeat,
        state: ButtonState::Pressed,
        window: Entity::PLACEHOLDER,
    });
}

fn release(app: &mut App, key_code: KeyCode, logical_key: Key) {
    let world = app.world_mut();
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(key_code);
    world.write_message(KeyboardInput {
        key_code,
        logical_key,
        text: None,
        repeat: false,
        state: ButtonState::Released,
        window: Entity::PLACEHOLDER,
    });
}

#[test]
fn search_typing_and_repeats_never_reach_chat_or_movement() {
    let (mut app, sent) = app();
    navigation::install(&mut app);
    app.add_message::<KeyboardInput>()
        .add_systems(Startup, |mut commands: Commands| {
            crate::spellbook::spawn(&mut commands);
        })
        .add_systems(
            Update,
            (
                focus,
                capture,
                crate::chat::input,
                input,
                refresh,
                navigation::update,
                crate::spellbook::update,
            )
                .chain(),
        );
    app.update();
    let world = app.world_mut();
    let field = world
        .query_filtered::<Entity, With<Field>>()
        .single(world)
        .unwrap();
    clicks::click(&mut app, field);
    assert!(app.world().resource::<Typing>().spell_search);
    key(
        &mut app,
        KeyCode::Slash,
        Key::Character("/".into()),
        Some("/"),
        false,
    );
    key(&mut app, KeyCode::Numpad8, Key::ArrowUp, None, false);
    key(&mut app, KeyCode::Numpad2, Key::ArrowDown, None, false);
    key(&mut app, KeyCode::Escape, Key::Escape, None, false);
    app.update();
    let typing = app.world().resource::<Typing>();
    assert!(!typing.spell_search && typing.escape_consumed && !typing.composing);
    assert_eq!(app.world().resource::<BookView>().search.query, "/");
    key(
        &mut app,
        KeyCode::Slash,
        Key::Character("/".into()),
        Some("/"),
        true,
    );
    release(&mut app, KeyCode::Slash, Key::Character("/".into()));
    key(&mut app, KeyCode::Numpad8, Key::ArrowUp, None, true);
    release(&mut app, KeyCode::Numpad8, Key::ArrowUp);
    key(&mut app, KeyCode::Numpad2, Key::ArrowDown, None, true);
    release(&mut app, KeyCode::Numpad2, Key::ArrowDown);
    key(&mut app, KeyCode::Numpad2, Key::ArrowDown, None, false);
    release(&mut app, KeyCode::Numpad2, Key::ArrowDown);
    app.update();
    let world = app.world();
    let physical = world.resource::<ButtonInput<KeyCode>>();
    let sampled = world.resource::<NavigationKeys>().sample(physical);
    assert!(!sampled.just_pressed(KeyCode::ArrowUp) && !sampled.just_pressed(KeyCode::Numpad8));
    assert!(!sampled.pressed(KeyCode::ArrowUp) && !sampled.pressed(KeyCode::Numpad8));
    assert!(sampled.just_pressed(KeyCode::ArrowDown) && sampled.just_pressed(KeyCode::Numpad2));
    assert_eq!(world.resource::<ChatState>().draft(), "");
    assert!(!world.resource::<Typing>().composing);
    assert!(sent.try_recv().is_err());
    clicks::click(&mut app, field);
    app.world_mut()
        .resource_mut::<Shown>()
        .close(WindowId::Spellbook);
    app.update();
    assert!(!app.world().resource::<Typing>().spell_search);
}
