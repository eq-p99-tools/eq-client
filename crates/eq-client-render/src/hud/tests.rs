use super::*;
use crate::{
    chat::ChatState,
    online::{OnlineState, testing},
    target::{CommandsToServer, TargetState},
};
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
        spell_refresh_ms: None,
        memorized_spells: gems,
        size: 6.0,
        walk_speed: 0.0,
        run_speed: 0.0,
        hp_percent: Some(100),
        appearance: eq_client_core::outfit::Appearance::default(),
    }
}

/// The caster, admitted and connected.
fn admitted() -> OnlineState {
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 7, caster());
    online
}

fn world(app: &App) -> &ClientWorld {
    &app.world().resource::<OnlineState>().world
}

fn online(app: &mut App) -> Mut<'_, OnlineState> {
    app.world_mut().resource_mut::<OnlineState>()
}

#[test]
fn estimated_resource_bars_fill_and_clear_with_their_maxima() {
    let mut app = App::new();
    let mut state = OnlineState::new(true);
    testing::resources(&mut state, 10, 15);
    app.insert_resource(HudState {
        resource_estimate: Some((20, 20)),
        ..default()
    })
    .insert_resource(state)
    .init_resource::<crate::spellbook::SpellNames>()
    .init_resource::<messages::Messages>()
    .add_systems(Update, update);
    let mana = app
        .world_mut()
        .spawn((Text::default(), HudLabel::Stat("MANA")))
        .id();
    let fill = app
        .world_mut()
        .spawn((Node::default(), HudFill("MANA")))
        .id();
    let stamina = app
        .world_mut()
        .spawn((Node::default(), HudFill("STAMINA")))
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
        .init_resource::<messages::Messages>()
        .add_systems(Update, update);
    let mana = app
        .world_mut()
        .spawn((Text::default(), HudLabel::Stat("MANA")))
        .id();
    let stamina = app
        .world_mut()
        .spawn((Text::default(), HudLabel::Stat("STAMINA")))
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
    assert!(text.contains("Synthetic spell | Alt+1"));
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
    assert!(
        app.world()
            .get::<Text>(details)
            .unwrap()
            .0
            .contains("Hover a gem")
    );
}

#[test]
fn action_feedback_expires() {
    let mut app = App::new();
    app.insert_resource(HudState {
        action_feedback: Some((std::time::Instant::now(), "Request queue is full".into())),
        ..default()
    })
    .insert_resource(OnlineState::new(true))
    .init_resource::<crate::spellbook::SpellNames>()
    .add_systems(Update, spell_details);
    let label = app.world_mut().spawn((SpellDetails, Text::default())).id();
    app.update();
    assert_eq!(
        app.world().get::<Text>(label).unwrap().0,
        "Request queue is full"
    );
    app.world_mut()
        .resource_mut::<HudState>()
        .action_feedback
        .as_mut()
        .unwrap()
        .0 -= std::time::Duration::from_secs(4);
    app.update();
    assert!(
        app.world()
            .get::<Text>(label)
            .unwrap()
            .0
            .contains("Hover a gem")
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the ordered UI interaction scenario together"
)]
fn gem_clicks_cast_or_forget_without_predicting_slots_and_chat_blocks_actions() {
    let mut app = App::new();
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(admitted())
        .insert_resource(CommandsToServer(Some(tx)))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ChatState>()
        .init_resource::<TargetState>()
        .init_resource::<HudState>()
        .init_resource::<hotbar::Bindings>()
        .init_resource::<crate::spellbook::SpellNames>()
        .init_resource::<messages::Messages>()
        .add_systems(Update, actions);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        bevy::window::PrimaryWindow,
    ));
    let gem = app
        .world_mut()
        .spawn((SpellGem(0), Interaction::Pressed))
        .id();
    app.update();
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
        .spawn((hotbar::Slot(0), Interaction::Pressed))
        .id();
    app.update();
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
    let press = |app: &mut App| {
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
        app.update();
    };
    let feedback = |app: &App| {
        app.world()
            .resource::<HudState>()
            .action_feedback
            .as_ref()
            .unwrap()
            .1
            .clone()
    };
    // A submitted request blocks another click even before a server Begin notification.
    testing::pending_cast(&mut online(&mut app), Some(73));
    press(&mut app);
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
    press(&mut app);
    assert!(rx.try_recv().is_err());
    assert!(feedback(&app).contains("Already casting"));
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
        state.world.tick(now, &names);
        assert!(state.world.casting().cooldowns.remaining(73, now).is_zero());
        testing::news_at(&mut state, [WorldEvent::Spell(refresh(73))], now);
        state.world.tick(now, &names);
    }
    press(&mut app);
    assert!(rx.try_recv().is_err());
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
    app.update();
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
    app.world_mut().resource_mut::<ChatState>().composing = true;
    press(&mut app);
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
    let player = world(&app).player().cloned().unwrap();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let mut hud = HudState::default();
    let request = |gem, target_id| requests::Request {
        session_id: 7,
        gem,
        target_id,
        forgetting: false,
        mana_cost: None,
    };
    let messages = messages::Messages::default();
    requests::spell(
        &mut hud,
        world(&app),
        &player,
        &sender,
        &request(1, 12),
        &messages,
    );
    assert!(
        hud.action_feedback
            .as_ref()
            .unwrap()
            .1
            .contains("Empty spell gem")
    );
    assert!(receiver.try_recv().is_err());
    requests::spell(
        &mut hud,
        world(&app),
        &player,
        &sender,
        &request(0, 99),
        &messages,
    );
    assert!(hud.action_feedback.as_ref().unwrap().1.contains("queued"));
    // A request never claims a cast the server has not answered.
    assert!(world(&app).casting().pending.is_none());
    assert!(world(&app).casting().cast.is_none());
    requests::spell(
        &mut hud,
        world(&app),
        &player,
        &sender,
        &request(0, 99),
        &messages,
    );
    assert!(
        hud.action_feedback
            .as_ref()
            .unwrap()
            .1
            .contains("queue is full")
    );
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ClientCommand::CastSpell { target_id: 99, .. }
    ));
    assert!(receiver.try_recv().is_err());
    drop(receiver);
    requests::spell(
        &mut hud,
        world(&app),
        &player,
        &sender,
        &request(0, 99),
        &messages,
    );
    assert!(
        hud.action_feedback
            .as_ref()
            .unwrap()
            .1
            .contains("was not sent")
    );
}

#[test]
fn short_server_mana_refuses_casts_locally_but_never_blocks_forgetting() {
    let player = caster();
    let (sender, receiver) = std::sync::mpsc::sync_channel(4);
    let messages = messages::Messages::parse(
        "EQST0002
0
199 Synthetic short mana
",
    );
    let mut request = requests::Request {
        session_id: 7,
        gem: 0,
        target_id: 12,
        forgetting: false,
        mana_cost: Some(10),
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
    let mut hud = HudState::default();
    requests::spell(
        &mut hud,
        &with_mana(Some(9)),
        &player,
        &sender,
        &request,
        &messages,
    );
    assert_eq!(
        hud.action_feedback.as_ref().unwrap().1,
        "Synthetic short mana"
    );
    assert!(receiver.try_recv().is_err());
    request.forgetting = true;
    requests::spell(
        &mut hud,
        &with_mana(Some(9)),
        &player,
        &sender,
        &request,
        &messages,
    );
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ClientCommand::ForgetSpell { gem: 0, .. }
    ));
    request.forgetting = false;
    for (mana, cost) in [(Some(10), Some(10)), (None, Some(10)), (Some(0), None)] {
        request.mana_cost = cost;
        requests::spell(
            &mut hud,
            &with_mana(mana),
            &player,
            &sender,
            &request,
            &messages,
        );
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ClientCommand::CastSpell { spell_id: 73, .. }
        ));
    }
}
