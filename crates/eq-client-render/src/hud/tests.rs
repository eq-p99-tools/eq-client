use super::*;
use crate::{
    chat::ChatState,
    online::{OnlineState, testing},
    outbox::Outbox,
};
use bevy::ui::RelativeCursorPosition;
use eq_client_core::{
    ClientCommand, PlayerState, SpellUpdate, WorldEvent, WorldPosition, WorldUpdate,
    world::{ClientWorld, NoSpells},
};

/// A cleric with spell 73 in its first gem, admitted in session 7 as spawn 12.
fn caster() -> PlayerState {
    let mut gems = [None; 8];
    gems[0] = Some(73);
    PlayerState {
        name: "Example".into(),
        titanium_environment: None,
        base_attributes: None,
        deity: None,
        class: Some(2),
        spawn_id: 12,
        race: 1,
        gender: 0,
        level: 1,
        position: WorldPosition::default(),
        mana: 50,
        endurance: None,
        skills: None,
        practice_points: None,
        languages: None,
        disciplines: None,
        spell_refresh_ms: None,
        memorized_spells: gems,
        size: 6.0,
        walk_speed: 0.0,
        run_speed: 0.0,
        hp_percent: Some(100),
        appearance: eq_client_core::outfit::Appearance::default(),
        listing: eq_client_core::listing::Listing::default(),
        name_parts: eq_client_core::names::NameParts::default(),
        posture: None,
    }
}

/// The caster, admitted and connected.
fn admitted() -> OnlineState {
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 7, caster());
    online
}

fn world(app: &App) -> &ClientWorld {
    app.world().resource::<OnlineState>().world()
}

fn online(app: &mut App) -> Mut<'_, OnlineState> {
    app.world_mut().resource_mut::<OnlineState>()
}

#[test]
fn a_gem_clicked_with_a_spell_from_the_book_memorizes_it_instead_of_casting() {
    let mut app = App::new();
    crate::keys::testing::install(&mut app);
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let mut state = admitted();
    let mut book = eq_client_core::SpellBook::default();
    book.apply(&SpellUpdate::Slot {
        slot: 3,
        spell_id: 74,
        mode: 0,
    });
    testing::news(&mut state, [eq_client_core::WorldEvent::SpellBook(book)]);
    let mut hand = crate::spellbook::BookHand::default();
    hand.held = Some(crate::spellbook::Entry { slot: 3, spell: 74 });
    app.insert_resource(state)
        .insert_resource(crate::outbox::Outbox::new(Some(tx)))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ChatState>()
        .init_resource::<GemPress>()
        .init_resource::<HudState>()
        .init_resource::<hotbar::Bindings>()
        .init_resource::<crate::spellbook::SpellNames>()
        .init_resource::<crate::ViewerSettings>()
        .init_resource::<messages::Messages>()
        .init_resource::<action_bar::ActionRequests>()
        .insert_resource(hand)
        .add_systems(Update, actions);
    crate::clicks::testing::install(&mut app);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        bevy::window::PrimaryWindow,
    ));
    // The gem holds spell 73, which a click would cast.
    let gem = app.world_mut().spawn((SpellGem(0), Interaction::None)).id();
    crate::clicks::testing::click(&mut app, gem);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::MemorizeSpell {
            gem: 0,
            spell_id: 74,
            ..
        }
    ));
    assert!(rx.try_recv().is_err());
    assert!(
        app.world()
            .resource::<crate::spellbook::BookHand>()
            .held
            .is_none()
    );
}

/// Synthetic pointer and mouse edges as Bevy presents them to the HUD.
fn hud_pointer(app: &mut App, on: Option<Entity>, button: MouseButton, down: bool) {
    let world = app.world_mut();
    let mut interactions = world.query::<&mut Interaction>();
    for mut interaction in interactions.iter_mut(world) {
        interaction.set_if_neq(Interaction::None);
    }
    if let Some(on) = on {
        if let Some(mut relative) = app.world_mut().get_mut::<RelativeCursorPosition>(on) {
            *relative = RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            };
        }
        *app.world_mut().get_mut::<Interaction>(on).unwrap() =
            if down && button == MouseButton::Left {
                Interaction::Pressed
            } else {
                Interaction::Hovered
            };
    }
    let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    if down {
        mouse.press(button);
    } else {
        mouse.release(button);
    }
    app.update();
    let world = app.world_mut();
    world.resource_mut::<ButtonInput<MouseButton>>().clear();
}

#[test]
fn a_gem_casts_as_its_click_is_let_go_and_a_hold_puts_it_on_a_hotbutton_instead() {
    let mut app = crate::testing::app();
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(admitted())
        .insert_resource(Outbox::new(Some(tx)))
        .add_systems(Update, (hotbar::carry::route, actions).chain());
    let gem = app
        .world_mut()
        .spawn((
            SpellGem(0),
            hotbar::Pickable(hotbar::Source::Gem(0)),
            Interaction::None,
        ))
        .id();
    let fifth = app
        .world_mut()
        .spawn((
            hotbar::Slot(4),
            hotbar::Pickable(hotbar::Source::Slot(4)),
            Interaction::None,
        ))
        .id();
    // Bevy marks the control pressed, or the one the button is let go over
    // hovered.
    let button = |app: &mut App, on, down| hud_pointer(app, on, MouseButton::Left, down);
    button(&mut app, Some(gem), true);
    assert!(rx.try_recv().is_err());
    button(&mut app, Some(gem), false);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::CastSpell {
            gem: 0,
            spell_id: 73,
            ..
        }
    ));
    testing::pending_cast(&mut online(&mut app), None);
    // Let go off the gem, a quick press casts nothing.
    button(&mut app, Some(gem), true);
    button(&mut app, None, false);
    assert!(rx.try_recv().is_err());
    // Held, the gem casts nothing and its hotkey goes onto the fifth
    // hotbutton; neither press sends. Its stored fifth-gem shortcut comes
    // onto the cursor even though that gem has no memorized spell.
    button(&mut app, Some(gem), true);
    app.world_mut()
        .resource_mut::<hotbar::carry::Presses>()
        .hold_past();
    app.update();
    button(&mut app, None, false);
    button(&mut app, Some(fifth), true);
    button(&mut app, None, false);
    assert!(rx.try_recv().is_err());
    assert_eq!(
        app.world().resource::<hotbar::Bindings>().0[4],
        Some(hotbar::Action::Gem(0))
    );
    assert_eq!(
        app.world().resource::<hotbar::carry::Carry>().hotkey(),
        Some(hotbar::Action::Gem(4))
    );
    button(&mut app, None, true);
    button(&mut app, None, false);
    // The next quick click on the gem casts, and the fifth hotbutton now
    // casts the first gem's spell.
    button(&mut app, Some(gem), true);
    button(&mut app, Some(gem), false);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::CastSpell {
            gem: 0,
            spell_id: 73,
            ..
        }
    ));
    testing::pending_cast(&mut online(&mut app), None);
    button(&mut app, Some(fifth), true);
    button(&mut app, Some(fifth), false);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::CastSpell {
            gem: 0,
            spell_id: 73,
            ..
        }
    ));
}

/// The caster in the world with the bar's default keys, its actions run and
/// their refusals said, sending into this channel.
fn bar_app(tx: std::sync::mpsc::SyncSender<ClientCommand>) -> App {
    let mut app = App::new();
    crate::keys::testing::install(&mut app);
    app.insert_resource(admitted())
        .insert_resource(Outbox::new(Some(tx)))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ChatState>()
        .init_resource::<GemPress>()
        .init_resource::<hotbar::Bindings>()
        .init_resource::<crate::spellbook::SpellNames>()
        .init_resource::<crate::ViewerSettings>()
        .init_resource::<messages::Messages>()
        .init_resource::<crate::spellbook::BookHand>()
        .add_systems(Update, (actions, crate::outbox::show).chain());
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        bevy::window::PrimaryWindow,
    ));
    app
}

/// Presses one key alone.
fn tap(app: &mut App, key: KeyCode) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.reset_all();
    keys.press(key);
    app.update();
}

#[test]
fn native_sit_stand_chord_uses_reported_posture_and_respects_typing() {
    use eq_client_core::{Posture, PostureState};
    for client in [
        eq_client_assets::ui::InstalledClient::Titanium,
        eq_client_assets::ui::InstalledClient::EqMac,
    ] {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = bar_app(tx);
        app.insert_resource(crate::keys::KeyMap::for_client(client));
        let chord = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::KeyS);
            app.update();
        };
        for (reported, wanted) in [
            (PostureState::Standing, Posture::Sitting),
            (PostureState::Sitting, Posture::Standing),
            (PostureState::Ducking, Posture::Sitting),
        ] {
            testing::news(
                &mut online(&mut app),
                [WorldEvent::Posture {
                    spawn_id: 12,
                    posture: reported,
                }],
            );
            chord(&mut app);
            assert!(
                matches!(rx.try_recv(), Ok(ClientCommand::SetPosture { posture, .. }) if posture == wanted)
            );
            // A key does not manufacture the server's changed posture.
            assert_eq!(
                app.world().resource::<OnlineState>().world().posture(12),
                Some(reported)
            );
        }
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        chord(&mut app);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn the_duck_key_toggles_between_standing_and_ducking_and_leaves_other_stances() {
    use eq_client_core::{Posture, PostureState};
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let mut app = bar_app(tx);
    crate::keys::testing::bind(
        &mut app,
        crate::keys::Act::Duck,
        crate::keys::Chord::key(KeyCode::KeyC),
    );
    // Titanium 0x4619cd case 0x4623b1 and EQMac 0x54050c: a key-down ducks
    // a standing player and stands a ducking one (audit D1). The key acts
    // only from standing or ducking (read R19, key command 0xc), so it
    // leaves a sitting, lying or looting player alone and sends nothing.
    for (reported, wanted) in [
        (PostureState::Standing, Some(Posture::Ducking)),
        (PostureState::Ducking, Some(Posture::Standing)),
        (PostureState::Sitting, None),
        (PostureState::Lying, None),
        (PostureState::Looting, None),
    ] {
        testing::news(
            &mut online(&mut app),
            [WorldEvent::Posture {
                spawn_id: 12,
                posture: reported,
            }],
        );
        tap(&mut app, KeyCode::KeyC);
        match wanted {
            Some(wanted) => assert!(
                matches!(rx.try_recv(), Ok(ClientCommand::SetPosture { posture, .. }) if posture == wanted),
                "{reported:?}"
            ),
            None => assert!(rx.try_recv().is_err(), "{reported:?}"),
        }
    }
}

#[test]
fn a_hotbuttons_sit_and_stand_send_the_postures_their_keys_send_and_say_a_refusal_once() {
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let mut app = bar_app(tx);
    // The bar's ninth and tenth slots hold Sit and Stand by default, which
    // run `/sit` and `/stand`. Direct posture keys are explicit user choices.
    crate::keys::testing::bind(
        &mut app,
        crate::keys::Act::Sit,
        crate::keys::Chord::key(KeyCode::KeyX),
    );
    crate::keys::testing::bind(
        &mut app,
        crate::keys::Act::Stand,
        crate::keys::Chord::key(KeyCode::KeyV),
    );
    for (slot, key, wanted) in [
        (
            KeyCode::Digit9,
            KeyCode::KeyX,
            eq_client_core::Posture::Sitting,
        ),
        (
            KeyCode::Digit0,
            KeyCode::KeyV,
            eq_client_core::Posture::Standing,
        ),
    ] {
        for pressed in [slot, key] {
            tap(&mut app, pressed);
            let sent = rx.try_recv().unwrap();
            assert!(
                matches!(
                    sent,
                    ClientCommand::SetPosture {
                        session_id: 7,
                        spawn_id: 12,
                        posture,
                        ..
                    } if posture == wanted
                ),
                "{pressed:?}: {sent:?}"
            );
            assert!(rx.try_recv().is_err());
        }
    }
    // With no session to take it, the hotbutton's Sit is refused, and the
    // refusal is the one line said, though the outbox says it too.
    let lines = |app: &App| {
        let chat = app.world().resource::<ChatState>();
        chat.history.lines(eq_client_core::chat::ChatTab::All).len()
    };
    let before = lines(&app);
    app.insert_resource(Outbox::new(None));
    tap(&mut app, KeyCode::Digit9);
    assert_eq!(lines(&app), before + 1);
    assert_eq!(
        app.world().resource::<ChatState>().newest(),
        crate::outbox::Refusal::Offline.text()
    );
}

#[test]
fn a_hotbuttons_camp_and_group_kinds_send_what_their_buttons_send() {
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let mut app = bar_app(tx);
    let kinds = [
        hotbar::Action::Camp,
        hotbar::Action::Follow,
        hotbar::Action::Disband,
    ];
    let mut bindings = hotbar::Bindings::default();
    for (slot, kind) in bindings.0.iter_mut().zip(kinds) {
        *slot = Some(kind);
    }
    app.insert_resource(bindings);
    // Camping sits first, as `/camp` does.
    tap(&mut app, KeyCode::Digit1);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::SetPosture {
            posture: eq_client_core::Posture::Sitting,
            ..
        }
    ));
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::Camp { session_id: 7, .. }
    ));
    // Follow follows the target on foot, which movement starts; it sends
    // nothing of its own.
    tap(&mut app, KeyCode::Digit2);
    assert!(rx.try_recv().is_err());
    assert!(
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .follow_asked
            .take()
            .is_some()
    );
    tap(&mut app, KeyCode::Digit3);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::Disband { session_id: 7 }
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn opted_in_camp_hotbutton_sends_no_sit_and_flight_duck_sends_no_posture() {
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let mut app = bar_app(tx);
    app.init_resource::<crate::options::OptionsState>();
    {
        let mut options = app
            .world_mut()
            .resource_mut::<crate::options::OptionsState>();
        options
            .options
            .qol
            .set(eq_client_core::qol::Fix::ExitAtOnce, true);
        options.options.qol.set(eq_client_core::qol::Fix::Fly, true);
    }
    app.world_mut().resource_mut::<hotbar::Bindings>().0[0] = Some(hotbar::Action::Camp);
    tap(&mut app, KeyCode::Digit1);
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::ExitAtOnce { session_id: 7, .. })
    ));
    assert!(rx.try_recv().is_err());
    crate::keys::testing::bind(
        &mut app,
        crate::keys::Act::Duck,
        crate::keys::Chord::key(KeyCode::KeyC),
    );
    tap(&mut app, KeyCode::KeyC);
    assert!(
        rx.try_recv().is_err(),
        "flight owns Duck without changing posture"
    );
    app.world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(eq_client_core::qol::Fix::Fly, false);
    tap(&mut app, KeyCode::KeyC);
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SetPosture {
            posture: eq_client_core::Posture::Ducking,
            ..
        })
    ));
}

#[test]
fn estimated_resource_bars_fill_and_clear_with_their_maxima() {
    let mut app = App::new();
    let mut state = OnlineState::new(true);
    testing::resources(&mut state, 10, 15);
    app.insert_resource(HudState {
        resource_estimate: Some((20, 20)),
    })
    .insert_resource(state)
    .init_resource::<crate::spellbook::SpellNames>()
    .init_resource::<messages::Messages>()
    .add_systems(Update, update);
    let mana = app
        .world_mut()
        .spawn((Text::default(), HudLabel::Stat(super::Stat::Mana)))
        .id();
    let fill = app
        .world_mut()
        .spawn((Node::default(), HudFill(super::Stat::Mana)))
        .id();
    let stamina = app
        .world_mut()
        .spawn((Node::default(), HudFill(super::Stat::Stamina)))
        .id();
    app.update();
    assert_eq!(app.world().get::<Text>(mana).unwrap().0, "10 / ~20");
    assert_eq!(app.world().get::<Node>(fill).unwrap().width, percent(50));
    assert_eq!(app.world().get::<Node>(stamina).unwrap().width, percent(75));
    app.world_mut().resource_mut::<HudState>().resource_estimate = None;
    app.update();
    assert_eq!(app.world().get::<Text>(mana).unwrap().0, "10 / ?");
    assert_eq!(app.world().get::<Node>(fill).unwrap().width, percent(0));
}

#[test]
fn current_resources_do_not_claim_an_unknown_maximum_or_percentage() {
    let mut app = App::new();
    let mut state = OnlineState::new(true);
    testing::resources(&mut state, 25, 20);
    app.init_resource::<HudState>()
        .insert_resource(state)
        .init_resource::<crate::spellbook::SpellNames>()
        .init_resource::<crate::ViewerSettings>()
        .init_resource::<messages::Messages>()
        .add_systems(Update, update);
    let mana = app
        .world_mut()
        .spawn((Text::default(), HudLabel::Stat(super::Stat::Mana)))
        .id();
    let stamina = app
        .world_mut()
        .spawn((Text::default(), HudLabel::Stat(super::Stat::Stamina)))
        .id();
    app.update();
    assert_eq!(app.world().get::<Text>(mana).unwrap().0, "25 / ?");
    assert_eq!(app.world().get::<Text>(stamina).unwrap().0, "20 / ?");
    testing::news(&mut online(&mut app), [WorldEvent::Mana(0)]);
    app.update();
    assert_eq!(app.world().get::<Text>(mana).unwrap().0, "0 / ?");
}

#[test]
fn interruption_label_uses_local_text_then_expires() {
    let mut app = App::new();
    let interrupted = |message_id| SpellUpdate::Interrupted {
        caster_id: 12,
        message_id,
        caster_name: None,
    };
    let mut state = admitted();
    testing::spell(&mut state, interrupted(73));
    app.init_resource::<HudState>()
        .insert_resource(state)
        .init_resource::<crate::spellbook::SpellNames>()
        .insert_resource(messages::Messages::parse(
            "EQST0002\n0 1\n73 Synthetic failure",
        ))
        .add_systems(Update, update);
    let label = app
        .world_mut()
        .spawn((Text::default(), HudLabel::Casting))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<Text>(label).unwrap().0,
        "Synthetic failure"
    );
    testing::spell(&mut online(&mut app), interrupted(99));
    app.update();
    assert!(
        app.world()
            .get::<Text>(label)
            .unwrap()
            .0
            .contains("server reason 99")
    );
    let long_ago = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(4))
        .unwrap();
    testing::news_at(
        &mut online(&mut app),
        [WorldEvent::Spell(interrupted(73))],
        long_ago,
    );
    app.update();
    assert_eq!(app.world().get::<Text>(label).unwrap().0, "");
}

#[test]
fn pending_cast_is_labelled_as_awaiting_and_cleared_on_reset() {
    let mut app = App::new();
    let mut state = admitted();
    testing::pending_cast(&mut state, Some(73));
    app.init_resource::<HudState>()
        .insert_resource(state)
        .insert_resource(crate::spellbook::SpellNames::parse("73^Synthetic spell"))
        .init_resource::<messages::Messages>()
        .add_systems(Update, update);
    let label = app
        .world_mut()
        .spawn((Text::default(), HudLabel::Casting))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<Text>(label).unwrap().0,
        "Awaiting cast acknowledgement | Synthetic spell"
    );
    // A dropped connection forgets the request.
    testing::connect(&mut online(&mut app), false);
    app.update();
    assert_eq!(app.world().get::<Text>(label).unwrap().0, "");
}

#[test]
fn spell_hover_tracks_live_gem_contents_and_clears_when_pointer_leaves() {
    let mut app = App::new();
    crate::keys::testing::install(&mut app);
    let mut state = OnlineState::new(true);
    let mut player = caster();
    player.memorized_spells[0] = Some(42);
    testing::admit(&mut state, 7, player);
    app.init_resource::<HudState>()
        .insert_resource(state)
        .insert_resource(crate::spellbook::SpellNames::parse("42^Synthetic spell"))
        .add_systems(Update, spell_details);
    let gem = app
        .world_mut()
        .spawn((
            SpellGem(0),
            Interaction::Hovered,
            BackgroundColor::default(),
        ))
        .id();
    let details = app.world_mut().spawn((SpellDetails, Text::default())).id();
    app.update();
    let text = &app.world().get::<Text>(details).unwrap().0;
    assert!(text.contains("Synthetic spell"));
    // The gem's key is in its tooltip, written from the key map.
    assert!(!text.contains("Alt+1"));
    assert!(text.contains("timing unavailable"));
    // The server empties the gem.
    testing::spell(
        &mut online(&mut app),
        SpellUpdate::Slot {
            slot: 0,
            spell_id: 42,
            mode: 2,
        },
    );
    app.update();
    assert!(
        app.world()
            .get::<Text>(details)
            .unwrap()
            .0
            .contains("Gem 1 is empty")
    );
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
    app.update();
    assert_eq!(app.world().get::<Text>(details).unwrap().0, "");
}

#[test]
fn gems_and_action_slots_name_their_keys_from_the_key_map() {
    let mut app = App::new();
    crate::keys::testing::install(&mut app);
    let mut online = OnlineState::new(true);
    let mut player = caster();
    player.memorized_spells = [None; 8];
    player.memorized_spells[1] = Some(42);
    testing::admit(&mut online, 7, player);
    let mut bindings = hotbar::Bindings::default();
    bindings.0[3] = None;
    app.insert_resource(online)
        .insert_resource(crate::spellbook::SpellNames::parse("42^Synthetic spell"))
        .insert_resource(bindings)
        .add_systems(Update, key_help);
    let gem = app.world_mut().spawn(SpellGem(0)).id();
    let memorized = app.world_mut().spawn(SpellGem(1)).id();
    let slot = app.world_mut().spawn(hotbar::Slot(1)).id();
    let spellless = app.world_mut().spawn(hotbar::Slot(2)).id();
    let empty = app.world_mut().spawn(hotbar::Slot(3)).id();
    app.update();
    let tooltip = |entity| {
        app.world()
            .get::<crate::tooltip::Tooltip>(entity)
            .unwrap()
            .0
            .clone()
    };
    assert_eq!(tooltip(gem), "Alt+1: cast");
    // A gem with a spell names it first, and a hold picks it up.
    assert_eq!(
        tooltip(memorized),
        "Synthetic spell
Alt+2: cast | Right-click: forget | Hold: pick up"
    );
    // A stored shortcut can be picked up even if its gem is currently empty.
    assert_eq!(tooltip(slot), "2: use | Hold: pick up");
    assert_eq!(tooltip(spellless), "3: use | Hold: pick up");
    assert_eq!(tooltip(empty), "4: use");
}

#[test]
fn right_click_does_not_forget_an_empty_or_replaced_gem() {
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let mut app = bar_app(tx);
    let gem = app.world_mut().spawn((SpellGem(0), Interaction::None)).id();
    let empty = app.world_mut().spawn((SpellGem(1), Interaction::None)).id();
    let right = |app: &mut App, on, down| hud_pointer(app, on, MouseButton::Right, down);

    right(&mut app, Some(empty), true);
    right(&mut app, Some(empty), false);
    right(&mut app, None, true);
    right(&mut app, Some(gem), false);
    right(&mut app, Some(gem), true);
    right(&mut app, None, false);
    assert!(rx.try_recv().is_err());

    right(&mut app, Some(gem), true);
    testing::spell(
        &mut online(&mut app),
        SpellUpdate::Slot {
            slot: 0,
            spell_id: 74,
            mode: 1,
        },
    );
    right(&mut app, Some(gem), false);
    assert!(rx.try_recv().is_err());

    right(&mut app, Some(gem), true);
    assert!(rx.try_recv().is_err());
    right(&mut app, Some(gem), false);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::ForgetSpell {
            gem: 0,
            spell_id: 74,
            ..
        }
    ));
    right(&mut app, Some(gem), true);
    right(&mut app, Some(gem), false);
    assert!(rx.try_recv().is_err());
    assert_eq!(world(&app).gem(0), Some(74));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the ordered UI interaction scenario together"
)]
fn gem_clicks_cast_or_forget_without_predicting_slots_and_chat_blocks_actions() {
    let mut app = App::new();
    crate::keys::testing::install(&mut app);
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(admitted())
        .insert_resource(crate::outbox::Outbox::new(Some(tx)))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ChatState>()
        .init_resource::<GemPress>()
        .init_resource::<HudState>()
        .init_resource::<hotbar::Bindings>()
        .init_resource::<crate::spellbook::SpellNames>()
        .init_resource::<crate::ViewerSettings>()
        .init_resource::<messages::Messages>()
        .init_resource::<crate::spellbook::BookHand>()
        .add_systems(Update, actions);
    crate::clicks::testing::install(&mut app);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        bevy::window::PrimaryWindow,
    ));
    let gem = app.world_mut().spawn((SpellGem(0), Interaction::None)).id();
    crate::clicks::testing::click(&mut app, gem);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::CastSpell {
            session_id: 7,
            gem: 0,
            spell_id: 73,
            target_id: 12,
            ..
        }
    ));
    let bar = app
        .world_mut()
        .spawn((hotbar::Slot(0), Interaction::None))
        .id();
    crate::clicks::testing::click(&mut app, bar);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::CastSpell {
            gem: 0,
            spell_id: 73,
            ..
        }
    ));
    *app.world_mut().get_mut::<Interaction>(bar).unwrap() = Interaction::None;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Digit9);
    app.update();
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::SetPosture {
            posture: eq_client_core::Posture::Sitting,
            ..
        }
    ));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    let press = |app: &mut App| crate::clicks::testing::click(app, gem);
    let forget = |app: &mut App| {
        hud_pointer(app, Some(gem), MouseButton::Right, true);
        hud_pointer(app, Some(gem), MouseButton::Right, false);
    };
    let feedback = |app: &App| app.world().resource::<ChatState>().newest();
    // A submitted request blocks another click even before a server Begin notification.
    testing::pending_cast(&mut online(&mut app), Some(73));
    forget(&mut app);
    assert!(rx.try_recv().is_err());
    testing::pending_cast(&mut online(&mut app), None);
    assert!(feedback(&app).contains("acknowledge"));
    // A server-confirmed cast blocks further gem actions, but never predicts a new slot.
    testing::spell(
        &mut online(&mut app),
        SpellUpdate::Began {
            caster_id: 12,
            spell_id: 73,
            duration_ms: 2000,
        },
    );
    forget(&mut app);
    assert!(rx.try_recv().is_err());
    assert!(feedback(&app).contains("Already casting"));
    testing::news(
        &mut online(&mut app),
        [WorldEvent::SongStopUnconfirmed {
            session_id: 7,
            spell_id: Some(73),
        }],
    );
    assert!(rx.try_recv().is_err());
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    press(&mut app);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::CastSpell {
            gem: 0,
            spell_id: 73,
            ..
        }
    ));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    testing::spell(
        &mut online(&mut app),
        SpellUpdate::Began {
            caster_id: 12,
            spell_id: 73,
            duration_ms: 2000,
        },
    );
    testing::spell(
        &mut online(&mut app),
        SpellUpdate::Mana {
            spell_id: 73,
            keep_casting: false,
        },
    );
    let mut fields = vec!["0"; 16];
    fields[0] = "73";
    fields[1] = "Synthetic spell";
    fields[15] = "30000";
    let names = crate::spellbook::SpellNames::parse(&fields.join("^"));
    let now = std::time::Instant::now();
    {
        let mut state = online(&mut app);
        // A refresh for a changed slot cannot start a timer for an unrelated spell.
        let refresh = |spell_id| SpellUpdate::BarRefresh {
            slot: 0,
            spell_id,
            reduction_ms: 0,
        };
        testing::news_at(&mut state, [WorldEvent::Spell(refresh(74))], now);
        state.tick(now, &names);
        assert!(
            state
                .world()
                .casting()
                .cooldowns
                .remaining(73, now)
                .is_zero()
        );
        testing::news_at(&mut state, [WorldEvent::Spell(refresh(73))], now);
        state.tick(now, &names);
    }
    press(&mut app);
    assert!(rx.try_recv().is_err());
    forget(&mut app);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::ForgetSpell {
            session_id: 7,
            gem: 0,
            spell_id: 73,
            ..
        }
    ));
    assert_eq!(world(&app).player().unwrap().memorized_spells[0], Some(73));
    app.world_mut()
        .resource_mut::<crate::keys::Typing>()
        .composing = true;
    forget(&mut app);
    assert!(rx.try_recv().is_err());
    // A dropped connection forgets the gem timers.
    testing::connect(&mut online(&mut app), false);
    assert!(
        world(&app)
            .casting()
            .cooldowns
            .remaining(73, std::time::Instant::now())
            .is_zero()
    );
    // Requests go out only while the player is in the world.
    testing::connect(&mut online(&mut app), true);
    let player = world(&app).player().cloned().unwrap();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let sender = Outbox::new(Some(sender));
    let mut chat = ChatState::default();
    let request = |gem, target_id| requests::Request {
        gem,
        target_id,
        forgetting: false,
        mana_cost: None,
        linked_timer: None,
    };
    let messages = messages::Messages::default();
    requests::spell(
        &mut chat,
        world(&app),
        &player,
        &sender,
        &request(1, 12),
        (&messages, &crate::keys::KeyMap::default()),
        Vec::new,
    );
    assert!(chat.newest().contains("Gem 2 is empty"));
    let said = chat.history.lines(eq_client_core::chat::ChatTab::All).len();
    assert!(receiver.try_recv().is_err());
    requests::spell(
        &mut chat,
        world(&app),
        &player,
        &sender,
        &request(0, 99),
        (&messages, &crate::keys::KeyMap::default()),
        Vec::new,
    );
    // A request sent says nothing, and never claims a cast the server has
    // not answered.
    assert_eq!(
        chat.history.lines(eq_client_core::chat::ChatTab::All).len(),
        said
    );
    assert!(world(&app).casting().pending.is_none());
    assert!(world(&app).casting().cast.is_none());
    requests::spell(
        &mut chat,
        world(&app),
        &player,
        &sender,
        &request(0, 99),
        (&messages, &crate::keys::KeyMap::default()),
        Vec::new,
    );
    // The outbox refuses a full queue and says why itself.
    assert_eq!(sender.take_refused(), [crate::outbox::Refusal::Busy]);
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ClientCommand::CastSpell { target_id: 99, .. }
    ));
    assert!(receiver.try_recv().is_err());
    drop(receiver);
    requests::spell(
        &mut chat,
        world(&app),
        &player,
        &sender,
        &request(0, 99),
        (&messages, &crate::keys::KeyMap::default()),
        Vec::new,
    );
    assert_eq!(sender.take_refused(), [crate::outbox::Refusal::Ended]);
}

#[test]
fn short_server_mana_refuses_casts_locally_but_never_blocks_forgetting() {
    let player = caster();
    let messages = messages::Messages::parse(
        "EQST0002
0
199 Synthetic short mana
",
    );
    let mut request = requests::Request {
        gem: 0,
        target_id: 12,
        forgetting: false,
        mana_cost: Some(10),
        linked_timer: None,
    };
    // The world as the server last reported the player's mana, if it has.
    let with_mana = |mana: Option<u32>| {
        let mut world = ClientWorld::default();
        if let Some(mana) = mana {
            world.apply(
                &WorldUpdate::Game(WorldEvent::Mana(mana)),
                std::time::Instant::now(),
                &NoSpells,
            );
        }
        world
    };
    let check = |world: &ClientWorld, request: &requests::Request| {
        requests::check(
            world,
            &player,
            request,
            (&messages, &crate::keys::KeyMap::default()),
            std::time::Instant::now(),
        )
    };
    assert_eq!(
        check(&with_mana(Some(9)), &request),
        Err(crate::chat::Said::official("Synthetic short mana"))
    );
    request.forgetting = true;
    assert_eq!(check(&with_mana(Some(9)), &request), Ok(73));
    request.forgetting = false;
    for (mana, cost) in [(Some(10), Some(10)), (None, Some(10)), (Some(0), None)] {
        request.mana_cost = cost;
        assert_eq!(
            check(&with_mana(mana), &request),
            Ok(73),
            "{mana:?} {cost:?}"
        );
    }
}

#[test]
fn titaniums_sit_toggle_stands_a_lying_or_looting_player_and_the_mac_clients_leaves_them() {
    use eq_client_core::{GameDialect, Posture, PostureState};
    for (posture, titanium, eqmac) in [
        (None, Some(Posture::Sitting), Some(Posture::Sitting)),
        (
            Some(PostureState::Standing),
            Some(Posture::Sitting),
            Some(Posture::Sitting),
        ),
        (
            Some(PostureState::Ducking),
            Some(Posture::Sitting),
            Some(Posture::Sitting),
        ),
        (
            Some(PostureState::Sitting),
            Some(Posture::Standing),
            Some(Posture::Standing),
        ),
        (Some(PostureState::Lying), Some(Posture::Standing), None),
        (Some(PostureState::Looting), Some(Posture::Standing), None),
        (Some(PostureState::Frozen), None, None),
    ] {
        assert_eq!(
            toggled_seat(posture, GameDialect::Titanium),
            titanium,
            "{posture:?}"
        );
        assert_eq!(
            toggled_seat(posture, GameDialect::EqMac),
            eqmac,
            "{posture:?}"
        );
    }
}

#[test]
fn titaniums_sit_toggle_refuses_under_a_mesmerize() {
    use eq_client_core::{Buff, BuffUpdate, Posture, PostureState};
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let mut app = bar_app(tx);
    app.insert_resource(crate::keys::KeyMap::for_client(
        eq_client_assets::ui::InstalledClient::Titanium,
    ));
    app.insert_resource(messages::Messages::parse(
        "EQST0002\n0 0\n12368 You can't do that while mesmerized.\n",
    ));
    let mut fields = vec!["0"; 183];
    fields[0] = "42";
    fields[1] = "Synthetic mesmerize";
    fields[86..98].fill("254");
    fields[86] = "31";
    app.insert_resource(crate::spellbook::SpellNames::parse(&fields.join("^")));
    let chord = |app: &mut App| {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyS);
        app.update();
    };
    testing::news(
        &mut online(&mut app),
        [WorldEvent::Posture {
            spawn_id: 12,
            posture: PostureState::Lying,
        }],
    );
    chord(&mut app);
    assert!(
        matches!(rx.try_recv(), Ok(ClientCommand::SetPosture { posture, .. }) if posture == Posture::Standing)
    );
    testing::news(
        &mut online(&mut app),
        [
            WorldEvent::BuffSnapshot(Vec::new()),
            WorldEvent::Buff(BuffUpdate {
                applies: eq_client_core::buffs::Applies::Received,
                record_type: 2,
                entity_id: 12,
                slot: 0,
                spell_id: 42,
                buff: Some(Buff {
                    spell_id: 42,
                    caster_level: 20,
                    bard_modifier: 10,
                    effect_type: 2,
                    duration_ticks: 10,
                    counters: 0,
                    caster_id: 30,
                }),
            }),
        ],
    );
    chord(&mut app);
    assert!(rx.try_recv().is_err(), "refused");
    let chat = app.world().resource::<ChatState>();
    assert!(chat.newest().contains("mesmerized"), "{}", chat.newest());
}
