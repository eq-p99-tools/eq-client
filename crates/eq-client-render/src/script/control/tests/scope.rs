use super::*;
use crate::online::OnlineState;
use eq_client_core::{WorldEvent, ZoneOffer};

fn admitted() -> App {
    let mut app = app();
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .local_session = true;
    {
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        online.enabled = true;
        enter(&mut online, 7, "samplezone");
    }
    app.add_systems(
        Update,
        (
            crate::chat::guard_script_requests,
            super::super::super::guard_scope,
            observe,
        )
            .chain(),
    );
    app
}

fn enter(online: &mut OnlineState, id: u64, zone: &str) {
    session::admit(online, id, session::player(9));
    session::news(
        online,
        [WorldEvent::Entered {
            capabilities: eq_client_core::Capability::ALL.to_vec(),
            choices: Vec::new(),
            session_id: id,
            zone: zone.into(),
            player: Box::new(session::player(9)),
            far_clip: None,
            spell_landing_flag: None,
            zone_id: 0,
        }],
    );
}

fn scoped(app: &mut App, source: &str) -> Receipt {
    request(
        app,
        1,
        json!({"op":"start","source":source,
        "expected_world":{"admission_id":7,"zone":"samplezone"}}),
    )
}

fn refused(app: &App) {
    assert!(!app.world().contains_resource::<Script>());
    let status = app.world().resource::<RunStatus>();
    assert_eq!(status.phase(), Phase::Failed);
    assert!(status.scope_refused);
    assert!(
        status
            .events()
            .back()
            .unwrap()
            .message
            .contains("World scope refused")
    );
    let keys = app.world().resource::<ButtonInput<KeyCode>>();
    assert!(!keys.pressed(KeyCode::KeyW));
    assert!(!keys.pressed(KeyCode::KeyA));
    assert!(!keys.just_pressed(KeyCode::KeyA));
}

#[test]
fn a_queued_action_requires_both_exact_current_identifiers() {
    for (id, zone) in [(8, "samplezone"), (7, "otherzone"), (7, "SampleZone")] {
        let mut app = admitted();
        enter(&mut app.world_mut().resource_mut::<OnlineState>(), id, zone);
        assert_eq!(scoped(&mut app, "press(\"A\")\n").code, "start_queued");
        refused(&app);
        let value: Value =
            serde_json::from_slice(&app.world().resource::<Bridge>().snapshot()).unwrap();
        assert_eq!(
            value["run"]["expected_world"],
            json!({"admission_id":7,"zone":"samplezone"})
        );
        assert_eq!(value["run"]["scope_refused"], true);
        assert_eq!(value["world"], json!({"admission_id":id,"zone":zone}));
        // Reading status and an explicitly unscoped recovery remain possible.
        assert_eq!(request(&mut app, 2, json!({"op":"status"})).code, "status");
        assert_eq!(start(&mut app, 3, "wait(1000)\n").code, "start_queued");
        assert_eq!(
            request(&mut app, 4, json!({"op":"stop","run_id":2})).code,
            "stop_requested"
        );
    }
}

#[test]
fn waits_loops_and_paused_runs_cannot_resume_in_a_new_admission_of_the_same_zone() {
    for source in [
        "wait(1000)\npress(\"A\")\n",
        "for _ in range(2):\n    wait(1000)\n    press(\"A\")\n",
    ] {
        for paused in [false, true] {
            let mut app = admitted();
            scoped(&mut app, source);
            if paused {
                app.world_mut()
                    .resource_mut::<RunControl>()
                    .request(Request::Pause);
                frame(&mut app, 1000);
                frame(&mut app, 1001);
                assert_eq!(
                    app.world().resource::<Script>().status().phase(),
                    Phase::Paused
                );
            }
            enter(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                8,
                "samplezone",
            );
            frame(&mut app, 2000);
            refused(&app);
        }
    }
}

#[test]
fn a_transfer_withdraws_scoped_held_input_and_pending_clicks() {
    let mut app = admitted();
    scoped(&mut app, "hold(\"W\", 1000)\npress(\"A\")\n");
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    let button = app
        .world_mut()
        .spawn((
            Node::default(),
            Interaction::Pressed,
            InheritedVisibility::VISIBLE,
        ))
        .id();
    {
        let mut script = app.world_mut().resource_mut::<Script>();
        script.clicked = Some(MouseButton::Left);
        script.pressed = Some(button);
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    session::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::ZoneTransfer(ZoneOffer {
            zone_id: 2,
            instance_id: 0,
            position: default(),
            reason: 0,
            to_bind: false,
            solicited: true,
        })],
    );
    frame(&mut app, 10);
    refused(&app);
    assert_eq!(
        app.world().get::<Interaction>(button),
        Some(&Interaction::None)
    );
    assert!(
        !app.world()
            .resource::<crate::clicks::LeftButton>()
            .clicked(button)
    );
}

fn receive_new_admission(app: &mut App) {
    app.add_systems(
        Update,
        (|mut online: ResMut<OnlineState>| enter(&mut online, 8, "samplezone"))
            .before(super::super::super::guard_scope),
    );
}

#[test]
fn late_waiting_scope_refusal_preserves_native_click_and_release() {
    let mut app = admitted();
    scoped(&mut app, "wait(1000)\n");
    let native = app
        .world_mut()
        .spawn((
            Node::default(),
            Interaction::Pressed,
            InheritedVisibility::VISIBLE,
        ))
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    frame(&mut app, 1);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    *app.world_mut().get_mut::<Interaction>(native).unwrap() = Interaction::Hovered;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    // clicks::track sees the native release in PreUpdate, then Receive
    // withdraws the script world before the ordinary Input handlers run.
    receive_new_admission(&mut app);
    frame(&mut app, 2);
    refused(&app);
    assert!(
        app.world()
            .resource::<crate::clicks::LeftButton>()
            .clicked(native)
    );
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .just_released(MouseButton::Left)
    );
    assert_eq!(
        app.world().get::<Interaction>(native),
        Some(&Interaction::Hovered)
    );
}

#[test]
fn late_waiting_scope_refusal_preserves_an_unrelated_native_hotbar_drag() {
    use crate::hud::hotbar::{
        Action,
        carry::{self, Carry, Pickable, Presses, Source},
    };
    use bevy::{ecs::system::RunSystemOnce, ui::RelativeCursorPosition};
    let mut app = admitted();
    scoped(&mut app, "wait(1000)\n");
    app.world_mut().spawn((
        Node::default(),
        Interaction::Pressed,
        InheritedVisibility::VISIBLE,
        Pickable(Source::Fixed(Action::Sit)),
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().run_system_once(carry::route).unwrap();
    assert!(app.world().resource::<Presses>().pending());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut().resource_mut::<Presses>().hold_past();
    app.world_mut().run_system_once(carry::route).unwrap();
    assert_eq!(app.world().resource::<Carry>().hotkey(), Some(Action::Sit));
    let own_right = app
        .world_mut()
        .spawn((
            Node::default(),
            Interaction::Pressed,
            InheritedVisibility::VISIBLE,
        ))
        .id();
    {
        let mut script = app.world_mut().resource_mut::<Script>();
        script.clicked = Some(MouseButton::Right);
        script.pressed = Some(own_right);
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    receive_new_admission(&mut app);
    frame(&mut app, 2);
    refused(&app);
    // Check the script barrier's output before a hotbar handler applies its
    // own admission/profile rules. The script cannot erase native ownership.
    assert!(app.world().resource::<Presses>().pending());
    assert_eq!(app.world().resource::<Carry>().hotkey(), Some(Action::Sit));
    assert_eq!(
        app.world().get::<Interaction>(own_right),
        Some(&Interaction::None)
    );
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
}

#[test]
fn late_scope_refusal_cancels_only_the_owned_pending_or_just_released_click() {
    use crate::{
        hud::hotbar::{
            Action,
            carry::{self, Pickable, Presses, Source},
        },
        script::{ClickTarget, Step},
    };
    use bevy::{ecs::system::RunSystemOnce, ui::RelativeCursorPosition};
    for released_in_preupdate in [false, true] {
        let mut app = admitted();
        super::super::super::install_input(&mut app, super::super::super::InputMode::Attended);
        scoped(&mut app, "wait(1000)\n");
        let own = app
            .world_mut()
            .spawn((
                Node::default(),
                Interaction::Pressed,
                InheritedVisibility::VISIBLE,
                Pickable(Source::Fixed(Action::Sit)),
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::ZERO),
                },
            ))
            .id();
        {
            let mut script = app.world_mut().resource_mut::<Script>();
            script.clicked = Some(MouseButton::Left);
            script.pressed = Some(own);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        frame(&mut app, 1);
        app.world_mut().run_system_once(carry::route).unwrap();
        assert!(app.world().resource::<Presses>().pending());
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        if released_in_preupdate {
            // Exercise the real driver's hold-release branch. It clears
            // Script::pressed before Receive, retaining the authored target.
            app.world_mut().resource_mut::<Script>().current = Some((
                Step::HoldClick(
                    ClickTarget::Button("synthetic", "own"),
                    Duration::from_millis(1),
                ),
                Duration::ZERO,
            ));
        }
        receive_new_admission(&mut app);
        frame(&mut app, 2);
        refused(&app);
        assert!(
            !app.world()
                .resource::<crate::clicks::LeftButton>()
                .clicked(own)
        );
        assert!(!app.world().resource::<Presses>().pending());
        assert!(!app.world().resource::<Presses>().clicked(own));
        assert_eq!(
            app.world().get::<Interaction>(own),
            Some(&Interaction::None)
        );
    }
}

#[test]
fn receiving_a_new_world_after_preupdate_cancels_that_frames_synthetic_edges() {
    #[derive(Resource, Default)]
    struct Change(bool);
    fn change(mut change: ResMut<Change>, mut online: ResMut<OnlineState>) {
        if std::mem::take(&mut change.0) {
            enter(&mut online, 8, "samplezone");
        }
    }
    let mut app = admitted();
    super::super::super::install_input(&mut app, super::super::super::InputMode::Attended);
    app.init_resource::<Change>()
        .add_systems(Update, change.before(super::super::super::guard_scope));
    scoped(&mut app, "wait(1)\npress(\"A\")\n");
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyK);
    let native = app
        .world_mut()
        .write_message(bevy::input::keyboard::KeyboardInput {
            key_code: KeyCode::KeyK,
            logical_key: bevy::input::keyboard::Key::Character("k".into()),
            state: bevy::input::ButtonState::Pressed,
            text: Some("k".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    app.world_mut().resource_mut::<Change>().0 = true;
    frame(&mut app, 1);
    refused(&app);
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyK)
    );
    let messages = app
        .world()
        .resource::<Messages<bevy::input::keyboard::KeyboardInput>>();
    let mut cursor = messages.get_cursor();
    assert!(
        cursor
            .read(messages)
            .filter(|event| event.key_code == KeyCode::KeyA)
            .all(|event| !event.state.is_pressed() && event.text.is_none())
    );
    let (native, _) = messages.get_message(native.unwrap().id).unwrap();
    assert!(native.state.is_pressed());
    assert_eq!(native.text.as_deref(), Some("k"));
}

#[test]
fn delayed_social_dispatch_checks_the_current_world_even_before_driver_observation() {
    let mut app = admitted();
    scoped(&mut app, "wait(1000)\n");
    let owner = app.world().resource::<Script>().status().identity.clone();
    assert!(
        app.world()
            .resource::<Script>()
            .social_dispatch(&owner, app.world().resource::<OnlineState>())
            .is_some()
    );
    enter(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        8,
        "samplezone",
    );
    assert!(
        app.world()
            .resource::<Script>()
            .social_dispatch(&owner, app.world().resource::<OnlineState>())
            .is_none()
    );
}

#[test]
fn controlled_gm_and_chat_queue_the_originating_admission() {
    use eq_client_core::{ClientCommand, OutboundChat};
    let mut app = admitted();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(crate::outbox::Outbox::new(Some(queue)));
    scoped(
        &mut app,
        "gm(\"level 1\")\nchat(\"say synthetic\")\nwait(1000)\n",
    );
    for ms in 1..10 {
        frame(&mut app, ms);
    }
    for expected in ["#level 1", "synthetic"] {
        assert_eq!(
            received.try_recv().unwrap(),
            ClientCommand::SendChatInAdmission {
                session_id: 7,
                message: OutboundChat::Say(expected.into()),
                tongue: eq_client_core::Tongue::COMMON,
            }
        );
    }
    request(&mut app, 2, json!({"op":"stop","run_id":1}));
    app.world()
        .resource::<crate::outbox::Outbox>()
        .send(
            app.world().resource::<OnlineState>().world(),
            ClientCommand::SendChat(OutboundChat::Say("native".into())),
        )
        .unwrap();
    assert!(matches!(
        received.try_recv(),
        Ok(ClientCommand::SendChat(_))
    ));
}

#[test]
fn synthetic_enter_stamps_its_draft_without_changing_native_chat() {
    use eq_client_core::ClientCommand;
    let mut app = admitted();
    super::super::super::install_input(&mut app, super::super::super::InputMode::Attended);
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(crate::outbox::Outbox::new(Some(queue)))
        .add_systems(
            Update,
            crate::chat::input.after(super::super::super::guard_scope),
        );
    scoped(
        &mut app,
        "press(\"Enter\")\npress(\"H\")\npress(\"I\")\npress(\"Enter\")\nwait(1000)\n",
    );
    for ms in 1..20 {
        frame(&mut app, ms);
    }
    assert!(matches!(
        received.try_recv(),
        Ok(ClientCommand::SendChatInAdmission { session_id: 7, .. })
    ));
    assert!(received.try_recv().is_err());
}

#[test]
fn a_scripted_hail_key_queues_a_current_admission_message() {
    use eq_client_core::ClientCommand;
    let mut app = admitted();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(crate::outbox::Outbox::new(Some(queue)))
        .insert_resource(crate::keys::testing::map(&[(
            crate::keys::Act::Hail,
            crate::keys::Chord::key(KeyCode::KeyY),
        )]))
        .add_systems(
            Update,
            crate::combat::input.after(super::super::super::guard_scope),
        );
    scoped(&mut app, "press(\"Y\")\nwait(1000)\n");
    assert!(matches!(
        received.try_recv(),
        Ok(ClientCommand::SendChatInAdmission { session_id: 7, .. })
    ));
}

#[test]
fn deferred_target_world_stamp_is_withdrawn_but_native_replacement_survives() {
    use crate::script::ExpectedWorld;
    for native_replacement in [false, true] {
        let mut app = admitted();
        {
            let mut chat = app.world_mut().resource_mut::<crate::chat::ChatState>();
            crate::chat::client_request_in_scope(
                "/target Scoped",
                &mut chat,
                Some(ExpectedWorld {
                    admission_id: 7,
                    zone: "samplezone".into(),
                }),
            )
            .unwrap()
            .unwrap();
            if native_replacement {
                crate::chat::client_request("/target Native", &mut chat)
                    .unwrap()
                    .unwrap();
            }
        }
        enter(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            8,
            "samplezone",
        );
        frame(&mut app, 1);
        assert_eq!(
            app.world()
                .resource::<crate::chat::ChatState>()
                .requested_target
                .as_deref(),
            native_replacement.then_some("Native")
        );
    }
}

#[test]
fn deferred_guild_world_stamp_is_withdrawn_but_native_replacement_survives() {
    use crate::{guild::Command, script::ExpectedWorld};
    for (admission, zone) in [(8, "samplezone"), (7, "otherzone")] {
        for native_replacement in [false, true] {
            let mut app = admitted();
            {
                let mut chat = app.world_mut().resource_mut::<crate::chat::ChatState>();
                crate::chat::client_request_in_scope(
                    "/guildmotd scoped",
                    &mut chat,
                    Some(ExpectedWorld {
                        admission_id: 7,
                        zone: "samplezone".into(),
                    }),
                )
                .unwrap()
                .unwrap();
                if native_replacement {
                    crate::chat::client_request("/guildmotd native", &mut chat)
                        .unwrap()
                        .unwrap();
                }
            }
            enter(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                admission,
                zone,
            );
            frame(&mut app, 1);
            assert_eq!(
                app.world().resource::<crate::chat::ChatState>().guild_asked,
                native_replacement.then(|| Command::SetMotd("native".into()))
            );
        }
    }
}

#[test]
fn scripted_follow_defers_to_ordinary_follow_without_a_network_command() {
    let mut app = admitted();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(crate::outbox::Outbox::new(Some(queue)));
    assert_eq!(
        scoped(&mut app, "slash(\"follow\")\nwait(1000)\n").code,
        "start_queued"
    );
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Running);
    assert!(
        app.world()
            .resource::<crate::chat::ChatState>()
            .follow_asked
            .is_some()
    );
    assert_eq!(
        app.world()
            .resource::<Script>()
            .status()
            .expected_world
            .as_ref(),
        Some(&crate::script::ExpectedWorld {
            admission_id: 7,
            zone: "samplezone".into(),
        })
    );
    assert!(received.try_recv().is_err());
}

#[test]
fn scripted_follow_world_stamp_is_withdrawn_but_native_replacement_survives() {
    for (admission, zone) in [(8, "samplezone"), (7, "otherzone")] {
        for native_replacement in [false, true] {
            let mut app = admitted();
            scoped(&mut app, "slash(\"follow\")\nwait(1000)\n");
            assert!(
                app.world()
                    .resource::<crate::chat::ChatState>()
                    .follow_asked
                    .is_some()
            );
            if native_replacement {
                crate::chat::client_request(
                    "/follow",
                    &mut app.world_mut().resource_mut::<crate::chat::ChatState>(),
                )
                .unwrap()
                .unwrap();
            }
            enter(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                admission,
                zone,
            );
            frame(&mut app, 1);
            refused(&app);
            assert_eq!(
                app.world()
                    .resource::<crate::chat::ChatState>()
                    .follow_asked
                    .is_some(),
                native_replacement
            );
        }
    }
}

#[test]
fn stopping_before_a_follow_step_does_not_queue_follow() {
    let mut app = admitted();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(crate::outbox::Outbox::new(Some(queue)));
    scoped(&mut app, "wait(1000)\nslash(\"follow\")\n");
    assert_eq!(
        request(&mut app, 2, json!({"op":"stop","run_id":1})).code,
        "stop_requested"
    );
    frame(&mut app, 2000);
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Stopped);
    assert!(
        app.world()
            .resource::<crate::chat::ChatState>()
            .follow_asked
            .is_none()
    );
    assert!(received.try_recv().is_err());
}

#[test]
fn a_script_runs_a_chat_channel_command_as_typing_does() {
    // A script may do whatever the player can (6.584): slash() takes the
    // chat server's commands as the chat line does, and the typed handler
    // refuses what it refuses from the keyboard, such as the chat server's
    // mail words.
    for (command, queued) in [
        ("join general", 1),
        ("1 hello", 1),
        ("leaveall", 1),
        ("chat mailto Someone hi", 0),
    ] {
        let mut app = admitted();
        let (queue, _received) = std::sync::mpsc::sync_channel(4);
        app.insert_resource(crate::outbox::Outbox::new(Some(queue)));
        assert_eq!(
            scoped(&mut app, &format!("slash(\"{command}\")\n")).code,
            "start_queued",
            "{command}"
        );
        for ms in 1..10 {
            frame(&mut app, ms);
        }
        assert_eq!(
            app.world()
                .resource::<crate::chat::ChatState>()
                .channel_asked
                .len(),
            queued,
            "{command}"
        );
    }
}
