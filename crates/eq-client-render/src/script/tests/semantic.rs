use super::{frame_at, pointing, set_focus};
use crate::{
    chat::{ChatState, system_line},
    items::ItemState,
    online::{OnlineState, testing as session},
    outbox::Outbox,
    script::{Phase, Program, RunStatus, Script},
    trade::TradeState,
    windows::{Shown, WindowId},
};
use bevy::{prelude::*, window::PrimaryWindow};
use eq_client_assets::ui::InstalledClient;
use eq_client_core::{ClientCommand, ItemLink, SpawnKind, WorldEvent, chat::Source};
use std::{path::Path, sync::mpsc};

const ADMISSION: u64 = 9;
const TARGET: u16 = 42;

fn app() -> (App, mpsc::Receiver<ClientCommand>) {
    let mut app = pointing(Vec::new());
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    set_focus(&mut app, window, true);
    let mut online = OnlineState::new(true);
    session::admit(&mut online, ADMISSION, session::player(7));
    let (tx, rx) = mpsc::sync_channel(4);
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(tx)))
        .world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .installed_client = InstalledClient::EqMac;
    (app, rx)
}

fn start(app: &mut App, source: &str) {
    let program = Program::parse(source, Path::new(".")).unwrap();
    app.insert_resource(Script::from_program(program));
}

fn select_service(app: &mut App, kind: SpawnKind, class: u8, distance: f32) -> String {
    let mut online = app.world_mut().resource_mut::<OnlineState>();
    let mut spawn = session::pet(TARGET, 7);
    spawn.kind = kind;
    spawn.class = Some(class);
    spawn.pet_owner = None;
    spawn.position.x = distance;
    session::spawn_entry(&mut online, TARGET, spawn);
    online.select_target(Some(TARGET));
    let revision = online.world().spawn(TARGET).unwrap().revision;
    format!("interact {ADMISSION} {TARGET} {revision}")
}

fn refused(app: &App, line: usize, reason: &str) {
    let status = app.world().resource::<RunStatus>();
    assert_eq!(status.phase(), Phase::Failed);
    assert_eq!(status.line(), Some(line));
    assert_eq!(status.events().back().unwrap().message, reason);
}

#[test]
fn scripted_interaction_opens_only_the_exact_selected_service() {
    for (kind, class) in [
        (SpawnKind::Npc, 41),
        (SpawnKind::Npc, 40),
        (SpawnKind::NpcCorpse, 1),
    ] {
        let (mut app, rx) = app();
        let source = select_service(&mut app, kind, class, 10.0);
        start(&mut app, &source);
        frame_at(&mut app, 0);
        frame_at(&mut app, 10);
        let world = app.world();
        let online = world.resource::<OnlineState>();
        assert_eq!(world.resource::<RunStatus>().phase(), Phase::Completed);
        assert_eq!(online.world().target().selected, Some(TARGET));
        match class {
            41 => {
                assert!(matches!(
                    rx.try_recv(),
                    Ok(ClientCommand::Shop {
                        session_id: ADMISSION,
                        merchant_id: TARGET,
                        open: true,
                        ..
                    })
                ));
                assert!(online.world().merchant().is_none());
                assert_eq!(
                    world.resource::<TradeState>().opening_shop(online.world()),
                    Some(TARGET)
                );
            }
            40 => assert!(world.resource::<Shown>().is_open(WindowId::Bank)),
            _ => {
                assert!(matches!(
                    rx.try_recv(),
                    Ok(ClientCommand::Loot {
                        session_id: ADMISSION,
                        corpse_id: TARGET,
                        ..
                    })
                ));
                let loot = online.world().loot().unwrap();
                assert_eq!(loot.corpse_id, TARGET);
                assert!(!loot.listed);
            }
        }
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn scripted_interaction_refuses_reselection_range_and_pending_open() {
    for (case, reason) in [
        (
            "retarget",
            "Interaction target changed or is no longer available",
        ),
        (
            "replaced",
            "Interaction target changed or is no longer available",
        ),
        ("range", "Interaction target is out of reach"),
        (
            "pending",
            "Close or settle the current transaction before opening another",
        ),
    ] {
        let (mut app, rx) = app();
        let distance = if case == "range" { 21.0 } else { 10.0 };
        let source = select_service(&mut app, SpawnKind::Npc, 41, distance);
        match case {
            "retarget" => app
                .world_mut()
                .resource_mut::<OnlineState>()
                .select_target(None),
            "replaced" => {
                let replacement = select_service(&mut app, SpawnKind::Npc, 41, distance);
                assert_ne!(source, replacement);
            }
            _ => (),
        }
        let source = if case == "pending" {
            format!("{source}\n{source}\nreport must not run")
        } else {
            format!("{source}\nreport must not run")
        };
        start(&mut app, &source);
        frame_at(&mut app, 0);
        frame_at(&mut app, 10);
        frame_at(&mut app, 20);
        let line = if case == "pending" {
            assert!(matches!(
                rx.try_recv(),
                Ok(ClientCommand::Shop {
                    merchant_id: TARGET,
                    ..
                })
            ));
            2
        } else {
            1
        };
        refused(&app, line, reason);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn scripted_interaction_refuses_training_pending_container_and_combine() {
    use eq_client_core::{
        inventory::InventorySlot,
        tradeskills::CombineUpdate,
        training::{TrainingOffer, TrainingUpdate},
    };

    let conflicts: [fn(&mut OnlineState); 3] = [
        |online| {
            session::news(
                online,
                [WorldEvent::Training(TrainingUpdate::Offered(
                    TrainingOffer {
                        trainer: 75,
                        caps: vec![0; 100],
                    },
                ))],
            );
            assert_eq!(online.world().training().unwrap().trainer, 75);
        },
        |online| {
            online.ask_container(90);
            assert_eq!(online.world().pending_container_open(), Some(90));
            assert!(online.world().container().is_none());
        },
        |online| {
            let slot = InventorySlot(22);
            session::news(online, [WorldEvent::Combine(CombineUpdate::Started(slot))]);
            assert_eq!(online.world().combining(), Some(slot));
        },
    ];
    for setup in conflicts {
        for (kind, class) in [
            (SpawnKind::Npc, 41),
            (SpawnKind::Npc, 40),
            (SpawnKind::NpcCorpse, 1),
        ] {
            let (mut app, rx) = app();
            let source = select_service(&mut app, kind, class, 10.0);
            setup(&mut app.world_mut().resource_mut::<OnlineState>());
            start(&mut app, &format!("{source}\nreport must not run"));
            frame_at(&mut app, 0);
            frame_at(&mut app, 10);
            refused(
                &app,
                1,
                "Close or settle the current transaction before opening another",
            );
            let world = app.world();
            let online = world.resource::<OnlineState>();
            assert!(!world.resource::<Shown>().is_open(WindowId::Bank));
            assert!(online.world().loot().is_none());
            assert!(
                world
                    .resource::<TradeState>()
                    .opening_shop(online.world())
                    .is_none()
            );
            assert!(rx.try_recv().is_err());
        }
    }
}

fn linked_message(app: &mut App) -> (u64, String) {
    let body = format!("00002A{}1234ABCD", "0".repeat(31));
    let mut line = system_line("Synthetic blade");
    line.source = Source::Server;
    line.message.item_links.push(ItemLink {
        body: body.clone(),
        text: "Synthetic blade".into(),
        item_id: 42,
        start: 0,
        end: 0,
        text_start: 0,
        text_end: 15,
    });
    app.world_mut()
        .resource_scope(|world, mut chat: Mut<ChatState>| {
            chat.receive(line, world.resource::<OnlineState>());
            (chat.history.revision(), body)
        })
}

#[test]
fn scripted_item_inspection_reuses_the_exact_received_link_and_definition() {
    let (mut app, rx) = app();
    let (message, body) = linked_message(&mut app);
    app.add_systems(Update, crate::items::input.after(crate::script::drive));
    let source = format!("inspect_link {ADMISSION} {message} 0");
    start(&mut app, &source);
    frame_at(&mut app, 0);
    assert_eq!(
        rx.try_recv().unwrap(),
        ClientCommand::InspectItem {
            session_id: ADMISSION,
            link_body: body
        }
    );
    assert_eq!(
        app.world().resource::<ItemState>().inspection(),
        Some((42, false))
    );
    let mut definition = crate::preview::items().remove(0).details;
    definition.id = 42;
    session::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::ItemDetails(definition)],
    );
    frame_at(&mut app, 10);
    assert_eq!(
        app.world().resource::<RunStatus>().phase(),
        Phase::Completed
    );
    assert_eq!(
        app.world().resource::<ItemState>().inspection(),
        Some((42, true))
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn scripted_item_inspection_refuses_unknown_and_stale_links() {
    for case in ["ordinal", "admission", "evicted"] {
        let (mut app, rx) = app();
        let (message, _) = linked_message(&mut app);
        let mut admission = ADMISSION;
        let mut ordinal = 0;
        match case {
            "ordinal" => ordinal = 1,
            "admission" => admission += 1,
            _ => {
                for _ in 0..16 {
                    linked_message(&mut app);
                }
                let chat = app.world().resource::<ChatState>();
                assert!(chat.history.line(message).is_some());
            }
        }
        start(
            &mut app,
            &format!("inspect_link {admission} {message} {ordinal}"),
        );
        frame_at(&mut app, 0);
        frame_at(&mut app, 10);
        let reason = if case == "admission" {
            "Item inspection needs the current admission"
        } else {
            "Item link is unknown or no longer available"
        };
        refused(&app, 1, reason);
        assert!(app.world().resource::<ItemState>().inspection().is_none());
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn named_social_script_control_requires_a_local_session() {
    for local in [false, true] {
        let (mut app, rx) = app();
        let program = Program::parse("social play Test", Path::new(".")).unwrap();
        app.insert_resource(Script::from_program(program).local_session(local));
        frame_at(&mut app, 0);
        frame_at(&mut app, 10);
        if local {
            assert!(matches!(
                &app.world().resource::<ChatState>().socials.request,
                Some(crate::socials::Queued { request: crate::socials::Request::Play(name), owner: Some(_) }) if name == "Test"
            ));
            assert_eq!(
                app.world().resource::<RunStatus>().phase(),
                Phase::Completed
            );
        } else {
            refused(
                &app,
                1,
                "social script steps need a local character in the world",
            );
            assert!(
                app.world()
                    .resource::<ChatState>()
                    .socials
                    .request
                    .is_none()
            );
        }
        assert!(rx.try_recv().is_err());
    }
}

fn socials_app() -> (App, mpsc::Receiver<ClientCommand>) {
    let (mut app, rx) = app();
    install_socials(&mut app);
    frame_at(&mut app, 0);
    (app, rx)
}

fn install_socials(app: &mut App) {
    session::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::WorldName {
            short_name: "LocalFixture".into(),
        }],
    );
    app.init_resource::<crate::script::AuthoredInput>()
        .init_resource::<crate::script::RunControl>()
        .add_systems(
            Update,
            (
                crate::profile_files::follow,
                crate::socials::prepare,
                crate::socials::update,
            )
                .chain()
                .after(crate::script::drive),
        );
    queue_social(app, "/social set Test | /say First | /say Later");
}

fn queue_social(app: &mut App, text: &str) {
    let request = crate::socials::Request::parse(text).unwrap().unwrap();
    app.world_mut()
        .resource_mut::<ChatState>()
        .queue_social(request, None)
        .unwrap();
}

fn control_at(app: &mut App, request: crate::script::Request, ms: u64) {
    app.world_mut()
        .resource_mut::<crate::script::RunControl>()
        .request(request);
    frame_at(app, ms);
}

#[test]
fn single_step_applies_social_edits_without_interrupting_a_manual_social() {
    use crate::{
        hud::hotbar::{Action, Bindings},
        script::Request,
        socials::State,
    };

    let (mut app, rx) = socials_app();
    let program = Program::parse(
        "social set Stepped | /say Synthetic\nsocial bind 1 Stepped\nsocial delete Stepped\nwait 1000",
        Path::new("."),
    ).unwrap();
    app.insert_resource(Script::from_program(program).local_session(true));
    queue_social(&mut app, "/social play Test");
    control_at(&mut app, Request::Pause, 10);
    assert_eq!(
        app.world().resource::<Script>().status().phase(),
        Phase::Paused
    );
    assert_eq!(
        rx.try_recv().unwrap(),
        ClientCommand::SendChat(eq_client_core::OutboundChat::Say("First".into()))
    );

    control_at(&mut app, Request::Step, 20);
    assert_eq!(
        app.world().resource::<ChatState>().newest(),
        "Saved social Stepped."
    );
    assert_eq!(
        rx.try_recv().unwrap(),
        ClientCommand::SendChat(eq_client_core::OutboundChat::Say("Later".into()))
    );
    frame_at(&mut app, 30);
    assert_eq!(
        app.world().resource::<Script>().status().phase(),
        Phase::Paused
    );

    control_at(&mut app, Request::Step, 40);
    let Some(Action::Social(id)) = app.world().resource::<Bindings>().0[0] else {
        panic!("the single-stepped bind did not run");
    };
    assert_eq!(app.world().resource::<State>().name(id), Some("Stepped"));
    frame_at(&mut app, 50);
    assert_eq!(
        app.world().resource::<Script>().status().phase(),
        Phase::Paused
    );

    control_at(&mut app, Request::Step, 60);
    assert_eq!(app.world().resource::<State>().name(id), None);
    assert!(rx.try_recv().is_err());
}

#[test]
fn single_step_plays_one_social_line_even_if_the_script_resumes_immediately() {
    use crate::script::Request;

    let (mut app, rx) = socials_app();
    let program = Program::parse("social play Test\nwait 1000", Path::new(".")).unwrap();
    app.insert_resource(Script::from_program(program).local_session(true));
    control_at(&mut app, Request::Pause, 10);
    assert_eq!(
        app.world().resource::<Script>().status().phase(),
        Phase::Paused
    );
    assert!(rx.try_recv().is_err());

    control_at(&mut app, Request::Step, 20);
    assert_eq!(
        rx.try_recv().unwrap(),
        ClientCommand::SendChat(eq_client_core::OutboundChat::Say("First".into()))
    );

    control_at(&mut app, Request::Resume, 30);
    frame_at(&mut app, 40);
    assert!(rx.try_recv().is_err());
}

#[test]
fn stopping_or_pausing_a_script_retires_its_social_before_the_next_line() {
    use crate::script::{Request, RunControl};

    for control in [Request::Stop, Request::Pause] {
        let (mut app, rx) = socials_app();
        let program = Program::parse("social play Test\nwait 1000", Path::new(".")).unwrap();
        app.insert_resource(Script::from_program(program).local_session(true));
        frame_at(&mut app, 10);
        assert_eq!(
            rx.try_recv().unwrap(),
            ClientCommand::SendChat(eq_client_core::OutboundChat::Say("First".into()))
        );
        app.world_mut()
            .resource_mut::<RunControl>()
            .request(control);
        frame_at(&mut app, 20);
        frame_at(&mut app, 30);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn native_takeover_cancels_a_background_scripts_social() {
    use crate::script::InputMode;

    let (mut app, window) = super::background(Vec::new());
    let mut online = OnlineState::new(true);
    session::admit(&mut online, ADMISSION, session::player(7));
    let (tx, rx) = mpsc::sync_channel(4);
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(tx)));
    install_socials(&mut app);
    frame_at(&mut app, 0);
    let program = Program::parse("social play Test\nwait 1000", Path::new(".")).unwrap();
    app.insert_resource(
        Script::from_program(program)
            .local_session(true)
            .input_mode(InputMode::Background),
    );
    frame_at(&mut app, 10);
    assert_eq!(
        rx.try_recv().unwrap(),
        ClientCommand::SendChat(eq_client_core::OutboundChat::Say("First".into()))
    );
    super::native_key(&mut app, window, KeyCode::KeyW, true);
    frame_at(&mut app, 20);
    super::native_key(&mut app, window, KeyCode::KeyW, false);
    frame_at(&mut app, 30);
    assert!(rx.try_recv().is_err());
}

#[test]
fn an_unsafe_social_definition_never_queues_a_target_change_or_followup() {
    let (mut app, rx) = socials_app();
    let _ = select_service(&mut app, SpawnKind::Npc, 41, 10.0);
    let result = crate::chat::client_request(
        "/social set Unsafe | /target Missing | /ranged",
        &mut app.world_mut().resource_mut::<ChatState>(),
    )
    .unwrap();
    assert!(result.is_err());
    frame_at(&mut app, 10);
    frame_at(&mut app, 20);
    assert!(rx.try_recv().is_err());
    assert_eq!(
        app.world()
            .resource::<OnlineState>()
            .world()
            .target()
            .selected,
        Some(TARGET)
    );
}

#[test]
fn a_stopped_script_does_not_cancel_a_manually_owned_social() {
    use crate::script::{Request, RunControl};

    let (mut app, rx) = socials_app();
    let program = Program::parse("wait 1000", Path::new(".")).unwrap();
    app.insert_resource(Script::from_program(program));
    queue_social(&mut app, "/social play Test");
    frame_at(&mut app, 10);
    assert_eq!(
        rx.try_recv().unwrap(),
        ClientCommand::SendChat(eq_client_core::OutboundChat::Say("First".into()))
    );
    app.world_mut()
        .resource_mut::<RunControl>()
        .request(Request::Stop);
    frame_at(&mut app, 20);
    assert_eq!(
        rx.try_recv().unwrap(),
        ClientCommand::SendChat(eq_client_core::OutboundChat::Say("Later".into()))
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_public_script_press_cannot_activate_a_social_but_a_native_press_can() {
    let (mut app, rx) = socials_app();
    queue_social(&mut app, "/social bind 1 Test");
    frame_at(&mut app, 10);
    start(&mut app, "press 1");
    frame_at(&mut app, 20);
    assert!(rx.try_recv().is_err());
    assert!(
        app.world()
            .resource::<ChatState>()
            .newest()
            .contains("named social step")
    );
    frame_at(&mut app, 30);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Digit1);
    frame_at(&mut app, 40);
    assert_eq!(
        rx.try_recv().unwrap(),
        ClientCommand::SendChat(eq_client_core::OutboundChat::Say("First".into()))
    );
}

#[test]
fn scripted_link_composition_edits_the_draft_and_sends_no_request() {
    let (mut app, rx) = app();
    let (message, _) = linked_message(&mut app);
    start(&mut app, &format!("compose_link {ADMISSION} {message} 0"));
    frame_at(&mut app, 0);
    frame_at(&mut app, 10);
    assert_eq!(
        app.world().resource::<RunStatus>().phase(),
        Phase::Completed
    );
    assert_eq!(
        app.world().resource::<ChatState>().draft(),
        "Synthetic blade"
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn scripted_owned_link_uses_the_current_received_item_without_sending() {
    use eq_client_core::{
        GameDialect, ItemDetails,
        inventory::{InventoryItem, InventorySlot, InventoryUpdate, ItemPlacement},
    };

    let (mut app, rx) = app();
    let slot = InventorySlot(23);
    let mut item = InventoryItem {
        slot,
        activation: default(),
        scroll_spell: None,
        book: None,
        rules: ItemPlacement {
            titanium_link_hash: Some(0x1234_abcd),
            ..default()
        },
        details: ItemDetails {
            id: 42,
            name: "Synthetic blade".into(),
            base_held_model: None,
            equipment: None,
            bonuses: None,
            lore: String::new(),
            weight_tenths: 0,
            slots: 0,
            classes: 0,
            races: 0,
            flags: Vec::new(),
            stats: Vec::new(),
            price: None,
            icon: None,
        },
        stack_count: None,
        charges: 0,
        bag_slots: 0,
    };
    {
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        session::generation(&mut online, GameDialect::Titanium);
        session::inventory(&mut online, InventoryUpdate::Snapshot(vec![item.clone()]));
    }

    start(&mut app, "compose_owned_link 8 23");
    frame_at(&mut app, 0);
    frame_at(&mut app, 10);
    refused(&app, 1, "Item link composition needs the current admission");
    assert_eq!(app.world().resource::<ChatState>().draft(), "");

    let source = format!("compose_owned_link {ADMISSION} 23");
    start(&mut app, &source);
    frame_at(&mut app, 20);
    frame_at(&mut app, 30);
    assert_eq!(
        app.world().resource::<RunStatus>().phase(),
        Phase::Completed
    );
    assert_eq!(
        app.world().resource::<ChatState>().draft(),
        "Synthetic blade"
    );

    item.rules.titanium_link_hash = None;
    session::inventory(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        InventoryUpdate::Set(vec![item]),
    );
    start(&mut app, &source);
    frame_at(&mut app, 40);
    frame_at(&mut app, 50);
    refused(
        &app,
        1,
        "This item lacks the metadata needed to make a link",
    );
    assert_eq!(
        app.world().resource::<ChatState>().draft(),
        "Synthetic blade"
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn scripted_item_inspection_does_not_replace_a_pending_request() {
    let (mut app, rx) = app();
    let (message, _) = linked_message(&mut app);
    let action = format!("inspect_link {ADMISSION} {message} 0");
    start(&mut app, &format!("{action}\n{action}"));
    for ms in [0, 10, 20] {
        frame_at(&mut app, ms);
    }
    refused(&app, 2, "Wait for the current item inspection");
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::InspectItem { .. })
    ));
    assert!(rx.try_recv().is_err());
    assert_eq!(
        app.world().resource::<ItemState>().inspection(),
        Some((42, false))
    );
}
