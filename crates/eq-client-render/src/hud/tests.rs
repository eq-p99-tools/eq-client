use super::*;
use crate::{
    chat::ChatState,
    online::OnlineState,
    target::{CommandsToServer, TargetState},
};
use eq_client_core::{ClientCommand, PlayerState, WorldPosition};

#[test]
fn local_instant_classification_never_removes_an_explicit_server_buff() {
    let mut fields = vec!["0"; 183];
    fields[0] = "42";
    let names = crate::spellbook::SpellNames::parse(&fields.join("^"));
    let mut hud = HudState {
        buff_state: eq_client_core::buffs::BuffTracker::empty_snapshot(),
        ..default()
    };
    hud.buff_update(eq_client_core::BuffUpdate {
        entity_id: 7,
        slot: 2,
        spell_id: 42,
        buff: Some(eq_client_core::Buff {
            spell_id: 42,
            caster_level: 1,
            effect_type: 2,
            bard_modifier: 10,
            duration_ticks: 10,
            counters: 0,
            caster_id: 7,
        }),
    });
    hud.spell_effect(
        eq_client_core::SpellEffect {
            target_id: 7,
            caster_id: 7,
            caster_level: 1,
            instrument_modifier: 10,
            spell_id: 42,
            spell_level: 1,
            effect_flag: 4,
        },
        Some(&names),
    );
    assert_eq!(hud.buff_state.slots().unwrap()[&2].duration_ticks, 10);
    assert!(hud.buff_state.effects().is_empty());
}

#[test]
fn estimated_resource_bars_fill_and_clear_with_their_maxima() {
    let mut app = App::new();
    app.insert_resource(HudState {
        mana: Some(10),
        endurance: Some(15),
        resource_estimate: Some((20, 20)),
        ..default()
    })
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
fn only_confirmed_effects_restore_icons_and_fades_clear_them_without_fake_slots() {
    let mut hud = HudState {
        buff_state: eq_client_core::buffs::BuffTracker::empty_snapshot(),
        ..default()
    };
    let mut effect = eq_client_core::SpellEffect {
        target_id: 7,
        caster_id: 7,
        caster_level: 1,
        instrument_modifier: 10,
        spell_id: 42,
        spell_level: 0,
        effect_flag: 0,
    };
    hud.spell_effect(effect.clone(), None);
    assert!(hud.buff_state.effects().is_empty());
    effect.effect_flag = 4;
    hud.spell_effect(effect.clone(), None);
    hud.spell_effect(effect, None);
    assert_eq!(hud.buff_state.effects().len(), 1);
    assert!(hud.buff_state.slots().unwrap().is_empty());
    hud.buff_update(eq_client_core::BuffUpdate {
        entity_id: 7,
        slot: 3,
        spell_id: 42,
        buff: None,
    });
    assert!(hud.buff_state.effects().is_empty());
}

#[test]
fn current_resources_do_not_claim_an_unknown_maximum_or_percentage() {
    let mut app = App::new();
    app.insert_resource(HudState {
        mana: Some(25),
        endurance: Some(20),
        ..default()
    })
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
    app.world_mut().resource_mut::<HudState>().mana = Some(0);
    app.update();
    assert_eq!(app.world().get::<Text>(mana).unwrap().0, "0 / ?");
}

#[test]
fn interruption_label_uses_local_text_then_expires() {
    let mut app = App::new();
    app.insert_resource(HudState {
        interrupted: Some((std::time::Instant::now(), 73)),
        ..default()
    })
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
    app.world_mut().resource_mut::<HudState>().interrupted = Some((std::time::Instant::now(), 99));
    app.update();
    assert!(
        app.world()
            .get::<Text>(label)
            .unwrap()
            .0
            .contains("server reason 99")
    );
    app.world_mut().resource_mut::<HudState>().interrupted = Some((
        std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(4))
            .unwrap(),
        73,
    ));
    app.update();
    assert!(app.world().get::<Text>(label).unwrap().0.is_empty());
}

#[test]
fn pending_cast_is_labelled_as_awaiting_and_cleared_on_reset() {
    let mut app = App::new();
    app.insert_resource(HudState {
        pending_cast: Some(73),
        ..default()
    })
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
    app.world_mut().resource_mut::<HudState>().reset_cooldowns();
    app.update();
    assert!(app.world().get::<Text>(label).unwrap().0.is_empty());
}

#[test]
fn spell_hover_tracks_live_gem_contents_and_clears_when_pointer_leaves() {
    let mut app = App::new();
    let mut state = HudState::default();
    state.spells[0] = Some(42);
    app.insert_resource(state)
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
    app.world_mut().resource_mut::<HudState>().spells[0] = None;
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
fn action_feedback_expires_and_is_discarded_on_admission_reset() {
    let mut app = App::new();
    app.insert_resource(HudState {
        action_feedback: Some((std::time::Instant::now(), "Request queue is full".into())),
        ..default()
    })
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
    app.world_mut().resource_mut::<HudState>().reset_cooldowns();
    assert!(app.world().resource::<HudState>().action_feedback.is_none());
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the ordered integration scenario and its assertions together"
)]
fn interruption_is_scoped_to_own_caster_and_cannot_be_undone_by_mana_updates() {
    use eq_client_core::SpellUpdate;
    let mut hud = HudState::default();
    let now = std::time::Instant::now();
    let begin = SpellUpdate::Began {
        caster_id: 7,
        spell_id: 42,
        duration_ms: 3000,
    };
    hud.cast_update(7, &begin, now);
    assert!(hud.casting.is_some());
    hud.cast_update(
        7,
        &SpellUpdate::Interrupted {
            caster_id: 8,
            message_id: 439,
        },
        now,
    );
    assert!(hud.casting.is_some());
    assert!(hud.interrupted.is_none());
    hud.cast_update(
        7,
        &SpellUpdate::Mana {
            spell_id: 42,
            keep_casting: true,
        },
        now,
    );
    assert!(hud.casting.is_some());
    hud.cast_update(
        7,
        &SpellUpdate::Interrupted {
            caster_id: 7,
            message_id: 439,
        },
        now,
    );
    assert!(hud.casting.is_none());
    assert_eq!(hud.interrupted, Some((now, 439)));
    hud.cast_update(
        7,
        &SpellUpdate::Mana {
            spell_id: 42,
            keep_casting: false,
        },
        now,
    );
    assert_eq!(hud.interrupted, Some((now, 439)));
    hud.cast_update(7, &begin, now);
    assert!(hud.interrupted.is_none());
    hud.cast_update(
        7,
        &SpellUpdate::Mana {
            spell_id: 99,
            keep_casting: false,
        },
        now,
    );
    assert!(hud.casting.is_some());
    hud.cast_update(
        7,
        &SpellUpdate::Mana {
            spell_id: 42,
            keep_casting: false,
        },
        now,
    );
    assert!(hud.casting.is_none());
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the ordered UI interaction scenario together"
)]
fn gem_clicks_cast_or_forget_without_predicting_slots_and_chat_blocks_actions() {
    let mut app = App::new();
    let mut online = OnlineState::new(true);
    online.connected = true;
    online.session_id = Some(7);
    let mut gems = [None; 8];
    gems[0] = Some(73);
    online.player = Some(PlayerState {
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
    });
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(online)
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
    // A submitted request blocks another click even before a server Begin notification.
    app.world_mut().resource_mut::<HudState>().pending_cast = Some(73);
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
    app.update();
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
    app.update();
    assert!(rx.try_recv().is_err());
    app.world_mut().resource_mut::<HudState>().pending_cast = None;
    assert!(
        app.world()
            .resource::<HudState>()
            .action_feedback
            .as_ref()
            .unwrap()
            .1
            .contains("acknowledge")
    );
    // A server-confirmed cast blocks further gem actions, but never predicts a new slot.
    app.world_mut().resource_mut::<HudState>().casting = Some((
        73,
        std::time::Instant::now(),
        std::time::Duration::from_secs(2),
    ));
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
    app.update();
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
    app.update();
    assert!(rx.try_recv().is_err());
    app.world_mut().resource_mut::<HudState>().casting = None;
    assert!(
        app.world()
            .resource::<HudState>()
            .action_feedback
            .as_ref()
            .unwrap()
            .1
            .contains("Already casting")
    );
    let mut fields = vec!["0"; 16];
    fields[0] = "73";
    fields[1] = "Synthetic spell";
    fields[15] = "30000";
    let names = crate::spellbook::SpellNames::parse(&fields.join("^"));
    let now = std::time::Instant::now();
    {
        let mut hud = app.world_mut().resource_mut::<HudState>();
        hud.spells[0] = Some(73);
        // A refresh for a changed slot cannot start a timer for an unrelated spell.
        hud.cast_update(
            12,
            &eq_client_core::SpellUpdate::BarRefresh {
                slot: 0,
                spell_id: 74,
                reduction_ms: 0,
            },
            now,
        );
        hud.cooldowns.resolve(&names, now);
        assert!(hud.cooldowns.remaining(73, now).is_zero());
        hud.cast_update(
            12,
            &eq_client_core::SpellUpdate::BarRefresh {
                slot: 0,
                spell_id: 73,
                reduction_ms: 0,
            },
            now,
        );
        hud.cooldowns.resolve(&names, now);
    }
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
    app.update();
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
    app.update();
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
    assert_eq!(
        app.world()
            .resource::<OnlineState>()
            .player
            .as_ref()
            .unwrap()
            .memorized_spells[0],
        Some(73)
    );
    app.world_mut().resource_mut::<ChatState>().composing = true;
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
    app.update();
    *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
    app.update();
    assert!(rx.try_recv().is_err());
    let mut hud = app.world_mut().resource_mut::<HudState>();
    hud.reset_cooldowns();
    assert!(
        hud.cooldowns
            .remaining(73, std::time::Instant::now())
            .is_zero()
    );
    assert!(hud.action_feedback.is_none());
    let player = app
        .world()
        .resource::<OnlineState>()
        .player
        .as_ref()
        .unwrap()
        .clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let mut hud = HudState::default();
    requests::spell(
        &mut hud,
        &player,
        &sender,
        &requests::Request {
            session_id: 7,
            gem: 1,
            target_id: 12,
            forgetting: false,
            mana_cost: None,
        },
        &messages::Messages::default(),
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
        &player,
        &sender,
        &requests::Request {
            session_id: 7,
            gem: 0,
            target_id: 99,
            forgetting: false,
            mana_cost: None,
        },
        &messages::Messages::default(),
    );
    assert!(hud.action_feedback.as_ref().unwrap().1.contains("queued"));
    assert!(hud.pending_cast.is_none());
    assert!(hud.casting.is_none());
    requests::spell(
        &mut hud,
        &player,
        &sender,
        &requests::Request {
            session_id: 7,
            gem: 0,
            target_id: 99,
            forgetting: false,
            mana_cost: None,
        },
        &messages::Messages::default(),
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
        &player,
        &sender,
        &requests::Request {
            session_id: 7,
            gem: 0,
            target_id: 99,
            forgetting: false,
            mana_cost: None,
        },
        &messages::Messages::default(),
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
    let mut gems = [None; 8];
    gems[0] = Some(73);
    let player = PlayerState {
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
    };
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
    let mut hud = HudState {
        mana: Some(9),
        ..HudState::default()
    };
    requests::spell(&mut hud, &player, &sender, &request, &messages);
    assert_eq!(
        hud.action_feedback.as_ref().unwrap().1,
        "Synthetic short mana"
    );
    assert!(receiver.try_recv().is_err());
    request.forgetting = true;
    requests::spell(&mut hud, &player, &sender, &request, &messages);
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ClientCommand::ForgetSpell { gem: 0, .. }
    ));
    request.forgetting = false;
    for (mana, cost) in [(Some(10), Some(10)), (None, Some(10)), (Some(0), None)] {
        hud.mana = mana;
        request.mana_cost = cost;
        requests::spell(&mut hud, &player, &sender, &request, &messages);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ClientCommand::CastSpell { spell_id: 73, .. }
        ));
    }
}
