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
        assert_eq!(scoped(&mut app, "press A\n").code, "start_queued");
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
        assert_eq!(start(&mut app, 3, "wait 1000\n").code, "start_queued");
        assert_eq!(
            request(&mut app, 4, json!({"op":"stop","run_id":2})).code,
            "stop_requested"
        );
    }
}

#[test]
fn waits_loops_and_paused_runs_cannot_resume_in_a_new_admission_of_the_same_zone() {
    for source in [
        "wait 1000\npress A\n",
        "repeat 2\nwait 1000\npress A\nend\n",
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
    scoped(&mut app, "hold W 1000\npress A\n");
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
    scoped(&mut app, "wait 1\npress A\n");
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
    let (native, _) = messages.get_message(native.id).unwrap();
    assert!(native.state.is_pressed());
    assert_eq!(native.text.as_deref(), Some("k"));
}

#[test]
fn delayed_social_dispatch_checks_the_current_world_even_before_driver_observation() {
    let mut app = admitted();
    scoped(&mut app, "wait 1000\n");
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
    scoped(&mut app, "gm level 1\nchat say synthetic\nwait 1000\n");
    for ms in 1..10 {
        frame(&mut app, ms);
    }
    for expected in ["#level 1", "synthetic"] {
        assert_eq!(
            received.try_recv().unwrap(),
            ClientCommand::SendChatInAdmission {
                session_id: 7,
                message: OutboundChat::Say(expected.into()),
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
        "press Enter\npress H\npress I\npress Enter\nwait 1000\n",
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
    scoped(&mut app, "press Y\nwait 1000\n");
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
