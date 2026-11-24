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
    let pressed = windows.press(
        ReportButton::Send(Report::Bug),
        world,
        Some(stamp),
        &mut shown,
    );
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
    assert_eq!(
        windows.press(
            ReportButton::Send(Report::Bug),
            world,
            Some(stamp),
            &mut shown
        ),
        Pressed::Say(crate::reports::bug_empty())
    );
    // The window hides either way.
    assert!(!shown.displayed(WindowId::BugReport));

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
        }
    );
}
