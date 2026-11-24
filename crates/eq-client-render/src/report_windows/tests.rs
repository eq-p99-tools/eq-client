use super::*;
use crate::online::testing;
use eq_client_core::{SpawnKind, WorldEvent, WorldPosition};

/// An admitted Titanium world: the player (7) stands at a place with
/// fractions, and an NPC (20) is in the zone. The player targets `target`.
fn world(target: Option<u16>) -> (OnlineState, crate::outbox::Stamp) {
    let mut player = testing::player(7);
    player.position = WorldPosition {
        x: 10.7,
        y: -3.9,
        z: 2.2,
        heading: 100.6,
    };
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 5, player);
    let mut guard = testing::pet(20, 0);
    guard.pet_owner = None;
    guard.kind = SpawnKind::Npc;
    guard.name = "a_guard00".into();
    testing::spawns(&mut online, vec![guard]);
    testing::news(
        &mut online,
        [WorldEvent::TargetChanged {
            session_id: 5,
            request_id: 0,
            state: eq_client_core::targets::TargetState {
                revision: 1,
                spawn_id: target,
                available: true,
            },
        }],
    );
    let (sender, _receiver) = std::sync::mpsc::sync_channel(4);
    let stamp = crate::outbox::Outbox::new(Some(sender))
        .stamp(online.world())
        .unwrap();
    (online, stamp)
}

#[test]
fn the_skins_pieces_name_the_windows_controls_in_their_own_window_only() {
    assert_eq!(
        ReportButton::of("DupButton", WindowId::BugReport),
        Some(ReportButton::Repeatable)
    );
    assert_eq!(
        ReportButton::of("SendButton", WindowId::BugReport),
        Some(ReportButton::Send(Report::Bug))
    );
    assert_eq!(
        ReportButton::of("FBKW_CancelButton", WindowId::Feedback),
        Some(ReportButton::Cancel(Report::Feedback))
    );
    assert_eq!(ReportButton::of("SendButton", WindowId::Feedback), None);
    assert_eq!(ReportButton::of("CancelButton", WindowId::Note), None);
    assert_eq!(
        ReportLabel::of("BUGW_LocLabelY", WindowId::BugReport),
        Some(ReportLabel::Place(1))
    );
    assert_eq!(
        ReportLabel::of("FBKW_UserName", WindowId::Feedback),
        Some(ReportLabel::UserName)
    );
    // The labels beside the check boxes keep the skin's words.
    assert_eq!(ReportLabel::of("BUGW_DupLabel", WindowId::BugReport), None);
    assert_eq!(
        ReportBox::of(Some("Edit"), WindowId::BugReport),
        Some(ReportBox(Report::Bug))
    );
    assert_eq!(ReportBox::of(Some("Prompt"), WindowId::BugReport), None);
    assert_eq!(ReportBox::of(Some("Edit"), WindowId::Feedback), None);
}

#[test]
fn opening_the_bug_report_fills_the_place_in_whole_numbers_and_clears_the_boxes() {
    let (online, _) = world(Some(20));
    let mut shown = Shown::default();
    let mut windows = ReportWindows::default();
    windows.bug.repeatable = true;
    windows.bug.crash = true;
    windows.bug.character_flags = true;
    windows.bug.target = target(online.world());
    windows.bug.text = "kept".into();
    windows.open(Report::Bug, online.world(), &mut shown);
    assert!(shown.displayed(WindowId::BugReport));
    // Truncated toward zero, as the program's ftol.
    assert_eq!(windows.bug.place, Some([10, -3, 2]));
    assert_eq!(windows.bug.heading, 100);
    for button in [
        ReportButton::Repeatable,
        ReportButton::Crash,
        ReportButton::Flags,
        ReportButton::Target,
    ] {
        assert!(!windows.checked(button), "{button:?}");
    }
    assert_eq!(windows.bug.text, "kept");
    let world = online.world();
    let label = |label| windows.label(label, world);
    assert_eq!(
        [0, 1, 2].map(|axis| label(ReportLabel::Place(axis))),
        ["10", "-3", "2"]
    );
    assert_eq!(label(ReportLabel::TargetName), "");
    assert_eq!(label(ReportLabel::UserName), "Example");
}

#[test]
fn the_target_box_names_the_target_and_stays_clear_without_one() {
    let (online, stamp) = world(None);
    let mut shown = Shown::default();
    let mut windows = ReportWindows::default();
    windows.open(Report::Bug, online.world(), &mut shown);
    let press = |windows: &mut ReportWindows, online: &OnlineState, shown: &mut Shown| {
        windows.press(ReportButton::Target, online.world(), Some(stamp), shown)
    };
    assert_eq!(press(&mut windows, &online, &mut shown), Pressed::Done);
    assert!(!windows.checked(ReportButton::Target));

    let (online, _) = world(Some(20));
    press(&mut windows, &online, &mut shown);
    assert!(windows.checked(ReportButton::Target));
    assert_eq!(
        windows.bug.target,
        Some(BugTarget {
            spawn_id: 20,
            name: "a_guard00".into()
        })
    );
    assert_eq!(
        windows.label(ReportLabel::TargetName, online.world()),
        "a guard"
    );
    // Unchecked, the name goes.
    press(&mut windows, &online, &mut shown);
    assert!(!windows.checked(ReportButton::Target));
    assert_eq!(windows.label(ReportLabel::TargetName, online.world()), "");
}

#[test]
fn sending_the_bug_report_carries_its_boxes_and_clears_its_words() {
    let (online, stamp) = world(Some(20));
    let world = online.world();
    let mut shown = Shown::default();
    let mut windows = ReportWindows::default();
    windows.open(Report::Bug, world, &mut shown);
    for button in [
        ReportButton::Repeatable,
        ReportButton::Flags,
        ReportButton::Target,
    ] {
        windows.press(button, world, Some(stamp), &mut shown);
    }
    windows.bug.category = BugCategory::ALL
        .iter()
        .position(|category| *category == BugCategory::Pathing)
        .unwrap();
    windows.bug.text = "  the guard walks into the wall ".into();
    windows.editing = Some(Report::Bug);
    // Send asks first; the window stays open with its words.
    assert_eq!(
        windows.press(
            ReportButton::Send(Report::Bug),
            world,
            Some(stamp),
            &mut shown,
        ),
        Pressed::Ask
    );
    assert!(shown.displayed(WindowId::BugReport));
    assert_eq!(windows.bug.text, "  the guard walks into the wall ");
    // No sends the same report Yes does, and says nothing of the machine.
    let (pressed, note) = windows.answered(false, world, Some(stamp), &mut shown);
    assert!(!note);
    let Pressed::Send(command, echo) = pressed else {
        panic!("{pressed:?}");
    };
    let ClientCommand::ReportBug { report, .. } = *command else {
        panic!("{command:?}");
    };
    assert_eq!(
        report,
        BugReport {
            category: BugCategory::Pathing,
            text: "the guard walks into the wall".into(),
            place: Some(BugPlace {
                x: 10.0,
                y: -3.0,
                z: 2.0,
                heading: 100.0,
            }),
            target: Some(BugTarget {
                spawn_id: 20,
                name: "a_guard00".into()
            }),
            repeatable: true,
            crash: false,
            character_flags: true,
        }
    );
    assert_eq!(echo, Some(crate::reports::bug_sent()));
    assert_eq!(windows.bug.text, "");
    assert_eq!(windows.editing, None);
    assert!(!shown.displayed(WindowId::BugReport));
}

#[test]
fn an_empty_bug_report_says_so_and_a_cancelled_one_keeps_its_words() {
    let (online, stamp) = world(None);
    let world = online.world();
    let mut shown = Shown::default();
    let mut windows = ReportWindows::default();
    windows.open(Report::Bug, world, &mut shown);
    windows.bug.text = "   ".into();
    // The program asks before it looks at the words (0x4e9f7a).
    assert_eq!(
        windows.press(
            ReportButton::Send(Report::Bug),
            world,
            Some(stamp),
            &mut shown
        ),
        Pressed::Ask
    );
    for yes in [true, false] {
        windows.open(Report::Bug, world, &mut shown);
        assert_eq!(
            windows.answered(yes, world, Some(stamp), &mut shown),
            (Pressed::Say(crate::reports::bug_empty()), false)
        );
        // The window hides either way.
        assert!(!shown.displayed(WindowId::BugReport));
    }

    windows.open(Report::Bug, world, &mut shown);
    windows.bug.text = "half written".into();
    assert_eq!(
        windows.press(
            ReportButton::Cancel(Report::Bug),
            world,
            Some(stamp),
            &mut shown,
        ),
        Pressed::Say(ReportLine::new(
            BUG_CANCELLED,
            crate::reports::PROGRAM_COLOUR,
            "Your bug report was cancelled.",
        ))
    );
    assert_eq!(windows.bug.text, "half written");
    assert!(!shown.displayed(WindowId::BugReport));
}

#[test]
fn the_feedback_window_sends_quietly_and_its_cancel_clears_it() {
    let (online, stamp) = world(None);
    let world = online.world();
    let mut shown = Shown::default();
    let mut windows = ReportWindows::default();
    let mut press = |windows: &mut ReportWindows, button| {
        windows.open(Report::Feedback, world, &mut shown);
        let pressed = windows.press(button, world, Some(stamp), &mut shown);
        assert!(!shown.displayed(WindowId::Feedback));
        pressed
    };
    // Empty, Send only closes.
    assert_eq!(
        press(&mut windows, ReportButton::Send(Report::Feedback)),
        Pressed::Done
    );
    windows.feedback = "a note".into();
    assert_eq!(
        press(&mut windows, ReportButton::Cancel(Report::Feedback)),
        Pressed::Done
    );
    assert_eq!(windows.feedback, "");
    windows.feedback = " a note ".into();
    let pressed = press(&mut windows, ReportButton::Send(Report::Feedback));
    let Pressed::Send(command, None) = pressed else {
        panic!("{pressed:?}");
    };
    assert!(matches!(
        *command,
        ClientCommand::SendFeedback { ref text, place: Some(_), .. } if text == "a note"
    ));
    assert_eq!(windows.feedback, "");
}

#[test]
fn the_category_drop_down_lists_every_category_in_order() {
    let names = category_names();
    assert_eq!(names.len(), BugCategory::ALL.len());
    assert_eq!(names[0], BugCategory::ALL[0].name());
    let mut form = BugForm {
        category: names.len(),
        ..BugForm::default()
    };
    // Out of range, it is Other.
    assert_eq!(form.category(), BugCategory::Other);
    form.category = 0;
    assert_eq!(form.category(), BugCategory::ALL[0]);
}

#[test]
fn both_windows_get_a_frame_while_open_and_close_on_their_close_box() {
    // Without a frame of their own the skin never draws them (live look,
    // 11-24): they are framed while open, as the note and book are.
    for id in [WindowId::BugReport, WindowId::Feedback] {
        assert!(crate::skinned::closes_here(id), "{id:?}");
    }
}

#[test]
fn without_an_installation_the_command_says_there_is_no_window() {
    let mut app = crate::testing::app();
    app.init_resource::<ChatState>()
        .init_resource::<crate::skinned::Screens>()
        .add_systems(Update, open);
    app.world_mut().resource_mut::<ChatState>().report_window = Some(Report::Bug);
    app.update();
    assert!(
        !app.world()
            .resource::<Shown>()
            .displayed(WindowId::BugReport)
    );
    let chat = app.world().resource::<ChatState>();
    assert_eq!(chat.report_window, None);
    assert!(
        format!(
            "{:?}",
            chat.history.lines(eq_client_core::chat::ChatTab::All)
        )
        .contains("Turn on Typed Reports")
    );
}

#[test]
fn a_scripts_report_reads_what_the_windows_show() {
    let (online, stamp) = world(Some(20));
    let world = online.world();
    let mut shown = Shown::default();
    let mut windows = ReportWindows::default();
    windows.open(Report::Bug, world, &mut shown);
    for button in [ReportButton::Crash, ReportButton::Target] {
        windows.press(button, world, Some(stamp), &mut shown);
    }
    windows.bug.text = "the guard".into();
    windows.feedback = "a note".into();
    windows.editing = Some(Report::Bug);
    assert_eq!(
        windows.observe(&shown, world),
        Observation {
            bug_shown: true,
            category: BugCategory::ALL[0].name(),
            checked: [false, true, false, true],
            place: Some([10, -3, 2]),
            target_name: "a guard".into(),
            bug_text: "the guard".into(),
            feedback_shown: false,
            feedback_text: "a note".into(),
            editing: Some(Report::Bug),
            question_up: false,
        }
    );
}

#[test]
fn the_question_answers_as_no_at_its_deadline() {
    use std::time::Duration;
    let deadline = Duration::from_secs(60);
    let at = |seconds| Duration::from_secs(seconds);
    assert_eq!(outcome(deadline, at(59), None, true, true), Outcome::Wait);
    assert_eq!(
        outcome(deadline, at(60), None, true, true),
        Outcome::Answer(false)
    );
    assert_eq!(
        outcome(deadline, at(1), Some(true), false, true),
        Outcome::Answer(true)
    );
    // Lost by the dialog, or its session ended: nothing goes.
    assert_eq!(outcome(deadline, at(1), None, false, true), Outcome::Drop);
    assert_eq!(
        outcome(deadline, at(1), Some(true), false, false),
        Outcome::Drop
    );
    assert_eq!(outcome(deadline, at(60), None, true, false), Outcome::Drop);
}

#[test]
fn with_send_system_info_set_the_report_goes_at_once_and_a_second_send_waits() {
    let (online, stamp) = world(None);
    let world = online.world();
    let mut shown = Shown::default();
    let mut windows = ReportWindows::default();
    windows.open(Report::Bug, world, &mut shown);
    windows.bug.text = "words".into();
    windows.asking = Some(std::time::Duration::from_secs(60));
    let send = ReportButton::Send(Report::Bug);
    assert_eq!(
        windows.press(send, world, Some(stamp), &mut shown),
        Pressed::Done
    );
    assert!(shown.displayed(WindowId::BugReport));
    windows.asking = None;
    windows.sends_system_info = true;
    assert!(matches!(
        windows.press(send, world, Some(stamp), &mut shown),
        Pressed::Send(..)
    ));
    assert!(!shown.displayed(WindowId::BugReport));
}

/// An app asking the bug report's question, with what it sends.
fn asking() -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
    let (online, _) = world(None);
    let (sender, receiver) = std::sync::mpsc::sync_channel(4);
    let mut app = crate::testing::app();
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(sender)))
        .init_resource::<ChatState>()
        .add_systems(Update, question);
    let world = app.world_mut();
    let mut windows = ReportWindows::default();
    let online = world.resource::<OnlineState>();
    let mut shown = Shown::default();
    windows.open(Report::Bug, online.world(), &mut shown);
    windows.bug.text = "the words".into();
    windows.asking = Some(std::time::Duration::from_secs(60));
    windows.asked_in = online.world().session_id();
    world.insert_resource(windows);
    world.insert_resource(shown);
    assert!(world.resource_mut::<Asked>().ask(Question::BugReport));
    (app, receiver)
}

#[test]
fn after_cancel_the_question_stays_and_its_answer_sends_the_kept_words() {
    let (mut app, receiver) = asking();
    let world = app.world_mut();
    let online = world.remove_resource::<OnlineState>().unwrap();
    world.resource_scope(|world, mut windows: Mut<ReportWindows>| {
        let mut shown = world.resource_mut::<Shown>();
        windows.press(
            ReportButton::Cancel(Report::Bug),
            online.world(),
            None,
            &mut shown,
        );
    });
    world.insert_resource(online);
    app.update();
    assert!(receiver.try_recv().is_err());
    assert!(app.world().resource::<ReportWindows>().asking.is_some());
    assert_eq!(
        app.world().resource::<Asked>().question(),
        Some(Question::BugReport)
    );
    app.world_mut().resource_mut::<Asked>().give(false);
    app.update();
    let Ok(ClientCommand::ReportBug { report, .. }) = receiver.try_recv() else {
        panic!("nothing sent");
    };
    assert_eq!(report.text, "the words");
    let windows = app.world().resource::<ReportWindows>();
    assert_eq!(windows.asking, None);
    assert_eq!(windows.bug.text, "");
}

#[test]
fn yes_sends_the_report_with_this_clients_line_and_the_deadline_sends_it_quietly() {
    let (mut app, receiver) = asking();
    app.world_mut().resource_mut::<Asked>().give(true);
    app.update();
    assert!(receiver.try_recv().is_ok());
    let history = |app: &App| {
        format!(
            "{:?}",
            app.world()
                .resource::<ChatState>()
                .history
                .lines(eq_client_core::chat::ChatTab::All)
        )
    };
    assert!(history(&app).contains(NO_SYSTEM_INFO));
    assert!(
        !app.world()
            .resource::<Shown>()
            .displayed(WindowId::BugReport)
    );

    let (mut app, receiver) = asking();
    app.world_mut().resource_mut::<ReportWindows>().asking = Some(std::time::Duration::ZERO);
    app.update();
    assert!(receiver.try_recv().is_ok());
    assert!(!history(&app).contains(NO_SYSTEM_INFO));
    assert_eq!(app.world().resource::<Asked>().question(), None);
}

#[test]
fn nothing_goes_once_the_session_it_was_asked_in_is_gone() {
    let (mut app, receiver) = asking();
    app.insert_resource(OnlineState::new(true));
    app.world_mut().resource_mut::<Asked>().give(true);
    app.update();
    assert!(receiver.try_recv().is_err());
    assert_eq!(app.world().resource::<ReportWindows>().asking, None);
}

#[test]
fn only_a_send_that_would_ask_reads_send_system_info() {
    let mut windows = ReportWindows::default();
    assert!(windows.reads_system_info(ReportButton::Send(Report::Bug)));
    for button in [
        ReportButton::Send(Report::Feedback),
        ReportButton::Cancel(Report::Bug),
        ReportButton::Location,
        ReportButton::Target,
    ] {
        assert!(!windows.reads_system_info(button), "{button:?}");
    }
    windows.asking = Some(std::time::Duration::ZERO);
    assert!(!windows.reads_system_info(ReportButton::Send(Report::Bug)));
}

#[test]
fn a_frame_with_no_click_leaves_send_system_info_unread() {
    let install = std::env::temp_dir().join(format!("eq-bug-sysinfo-{}", std::process::id()));
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(
        install.join("eqclient.ini"),
        "[Defaults]\nSendSystemInfo=1\n",
    )
    .unwrap();
    let mut app = crate::testing::app();
    app.init_resource::<ChatState>()
        .add_systems(Update, buttons);
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .eq_directory = Some(install.clone());
    app.update();
    app.update();
    assert!(!app.world().resource::<ReportWindows>().sends_system_info);
    // The same installation, read as a Send would read it.
    assert!(sends_system_info(
        &app.world().resource::<crate::ViewerSettings>().0
    ));
    let _ = std::fs::remove_dir_all(&install);
}
