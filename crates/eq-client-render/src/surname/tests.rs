use super::*;
use crate::{
    clicks::testing as clicks,
    confirm::{self, AnswerButton, QuestionText},
    online::testing,
};
use eq_client_core::chat::ChatTab;
use std::sync::mpsc::Receiver;

/// Installed lines in a made-up wording, by the ids the program reads.
const STRINGS: &str = "EQST0002
0 2
1374 No surname for you.
1375 %1 is now %1 %2.
2239 Far too long.
9050 Become %1?
9051 Asked.
12940 Not yet twenty.
12941 Name one.
";

/// A level 20 player with no surname in admission 5, the dialog's
/// buttons, its text and the surname command running in the program's
/// order.
fn app(level: u8) -> (App, Receiver<ClientCommand>, [Entity; 3]) {
    app_queued(level, 8)
}

/// [`app`] with a queue to the session of `room` commands; a full queue
/// refuses a send, which the outbox says in chat.
fn app_queued(level: u8, room: usize) -> (App, Receiver<ClientCommand>, [Entity; 3]) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(room);
    let mut online = OnlineState::new(false);
    let mut player = testing::player(7);
    player.level = level;
    testing::admit(&mut online, 5, player);
    let mut app = crate::testing::app();
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(sender)))
        .init_resource::<ChatState>()
        .init_resource::<Asked>()
        .init_resource::<Pending>()
        .insert_resource(Messages::parse(STRINGS))
        .add_systems(
            Update,
            (
                confirm::buttons,
                command,
                crate::outbox::show,
                confirm::window,
                confirm::show,
            )
                .chain(),
        );
    let yes = app
        .world_mut()
        .spawn((Interaction::None, AnswerButton(true)))
        .id();
    let no = app
        .world_mut()
        .spawn((Interaction::None, AnswerButton(false)))
        .id();
    let text = app.world_mut().spawn((QuestionText, Text::default())).id();
    (app, receiver, [yes, no, text])
}

fn ask(app: &mut App, typed: &str) {
    let mut chat = app.world_mut().resource_mut::<ChatState>();
    assert_eq!(crate::chat::client_request(typed, &mut chat), Some(Ok(())));
    app.update();
}

fn lines(app: &App) -> Vec<(String, Option<u32>)> {
    app.world()
        .resource::<ChatState>()
        .history
        .lines(ChatTab::All)
        .into_iter()
        .map(|(_, line)| (line.message.text.clone(), line.message_type))
        .collect()
}

fn question(app: &App) -> Option<Question> {
    app.world().resource::<Asked>().question()
}

#[test]
fn only_surname_is_the_command() {
    assert_eq!(typed("/surname Exemplum"), Some("Exemplum".into()));
    assert_eq!(typed("/SURNAME   Two words "), Some("Two words ".into()));
    assert_eq!(typed("/surname"), Some(String::new()));
    assert_eq!(typed("/surnames Exemplum"), None);
    assert_eq!(typed("surname Exemplum"), None);
}

#[test]
fn the_refusals_come_in_the_programs_order() {
    // Under 20 comes first, whatever the words.
    assert!(matches!(
        outcome(&"x".repeat(40), 19, true),
        Outcome::Line(UNDER_LEVEL, _)
    ));
    // No word: usage without a surname, our own line with one.
    assert!(matches!(outcome("", 20, false), Outcome::Line(USAGE, _)));
    assert!(matches!(outcome("   ", 20, true), Outcome::Own(_)));
    // 31 bytes of words pass, 32 don't: the whole argument is measured
    // (inferred), so a short first word with a long rest is refused.
    let short_first = format!("Exemplum {}", "x".repeat(22));
    assert_eq!(short_first.len(), 31);
    assert_eq!(
        outcome(&short_first, 20, false),
        Outcome::Ask("Exemplum".into())
    );
    let longer = format!("{short_first}x");
    assert_eq!(longer.len(), 32);
    assert!(matches!(
        outcome(&longer, 20, false),
        Outcome::Line(LONG, _)
    ));
    // Spaces typed after the words don't count.
    assert_eq!(
        outcome(&format!("{short_first}    "), 20, false),
        Outcome::Ask("Exemplum".into())
    );
}

#[test]
fn the_word_is_read_as_the_program_reads_it_and_cut_to_19() {
    assert_eq!(
        outcome("ExEmPlUm", 20, false),
        Outcome::Ask("ExEmPlUm".into())
    );
    assert_eq!(
        outcome("'Ex Emplum' rest", 20, false),
        Outcome::Ask("Ex Emplum".into())
    );
    assert_eq!(
        outcome("\"Exemplum", 20, false),
        Outcome::Ask("Exemplum".into())
    );
    let long = "Abcdefghijklmnopqrstuvwxy";
    assert_eq!(long.len(), 25);
    assert_eq!(
        outcome(long, 20, false),
        Outcome::Ask("Abcdefghijklmnopqrs".into())
    );
    // The cut never splits a character.
    assert_eq!(cut(&format!("{}é", "a".repeat(18))), "a".repeat(18));
}

#[test]
fn refused_lines_send_nothing_and_come_in_colour_15() {
    let (mut app, receiver, _) = app(19);
    ask(&mut app, "/surname Exemplum");
    assert_eq!(lines(&app), [("Not yet twenty.".into(), Some(15))]);
    let (mut app, receiver_two, _) = self::app(20);
    for typed in ["/surname", &format!("/surname {}", "x".repeat(32))] {
        ask(&mut app, typed);
    }
    assert_eq!(
        lines(&app),
        [
            ("Name one.".into(), Some(15)),
            ("Far too long.".into(), Some(15))
        ]
    );
    assert_eq!(question(&app), None);
    assert!(receiver.try_recv().is_err() && receiver_two.try_recv().is_err());
}

#[test]
fn a_bare_surname_with_one_set_waits_for_its_read() {
    let (mut app, receiver, _) = app(20);
    let mut online = app.world_mut().resource_mut::<OnlineState>();
    testing::news(
        &mut online,
        [eq_client_core::WorldEvent::LastName {
            name: "Example".into(),
            last_name: "Exemplum".into(),
        }],
    );
    ask(&mut app, "/surname");
    let said = lines(&app);
    assert_eq!(said.len(), 1);
    assert!(said[0].0.contains("waits for its program read"), "{said:?}");
    assert_eq!(question(&app), None);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn yes_sends_the_cut_word_the_question_named_once() {
    let (mut app, receiver, [yes, _, text]) = app(20);
    ask(&mut app, "/surname Abcdefghijklmnopqrstuvwxy");
    assert_eq!(question(&app), Some(Question::Surname));
    app.update();
    assert_eq!(
        app.world().get::<Text>(text).unwrap().0,
        "Become Abcdefghijklmnopqrs?"
    );
    assert!(receiver.try_recv().is_err());
    clicks::click(&mut app, yes);
    let Ok(ClientCommand::Surname {
        session_id,
        surname,
    }) = receiver.try_recv()
    else {
        panic!("expected one surname request");
    };
    assert_eq!((session_id, surname.as_str()), (5, "Abcdefghijklmnopqrs"));
    assert_eq!(lines(&app), [("Asked.".into(), Some(15))]);
    app.update();
    assert!(receiver.try_recv().is_err());
    assert!(app.world().resource::<Pending>().asking.is_none());
}

#[test]
fn a_yes_the_outbox_refuses_says_why_and_not_that_it_was_sent() {
    let (mut app, receiver, [yes, _, _]) = app_queued(20, 0);
    ask(&mut app, "/surname Exemplum");
    app.update();
    clicks::click(&mut app, yes);
    assert!(receiver.try_recv().is_err());
    assert_eq!(
        lines(&app)
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>(),
        [crate::outbox::Refusal::Busy.text()]
    );
    assert!(app.world().resource::<Pending>().asking.is_none());
}

#[test]
fn no_the_time_running_out_and_the_session_ending_send_nothing() {
    // No.
    let (mut app, receiver, [_, no, _]) = app(20);
    ask(&mut app, "/surname Exemplum");
    clicks::click(&mut app, no);
    app.update();
    assert_eq!(question(&app), None);
    assert!(app.world().resource::<Pending>().asking.is_none());
    assert!(receiver.try_recv().is_err());
    assert_eq!(lines(&app).len(), 0);

    // Ten seconds.
    let (mut app, receiver, [yes, ..]) = self::app(20);
    ask(&mut app, "/surname Exemplum");
    app.update();
    assert_eq!(question(&app), Some(Question::Surname));
    let mut pending = app.world_mut().resource_mut::<Pending>();
    let asked = &mut pending.asking.as_mut().unwrap().2;
    *asked -= QUESTION_WAIT;
    app.update();
    assert_eq!(question(&app), None);
    clicks::click(&mut app, yes);
    assert!(receiver.try_recv().is_err());

    // The session ends: another admission takes its place.
    let (mut app, receiver, [yes, ..]) = self::app(20);
    ask(&mut app, "/surname Exemplum");
    let mut online = app.world_mut().resource_mut::<OnlineState>();
    let mut player = testing::player(7);
    player.level = 20;
    testing::admit(&mut online, 6, player);
    app.update();
    assert_eq!(question(&app), None);
    clicks::click(&mut app, yes);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn one_question_at_a_time() {
    let (mut app, receiver, _) = app(20);
    ask(&mut app, "/surname Exemplum");
    ask(&mut app, "/surname Other");
    assert_eq!(
        app.world().resource::<Pending>().asking.as_ref().unwrap().0,
        "Exemplum"
    );
    assert_eq!(lines(&app).len(), 1);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn every_answer_is_shown_as_the_program_words_it() {
    let messages = Messages::parse(STRINGS);
    let taken = answer(true, "Example", "Exemplum", Some(&messages));
    assert_eq!(taken.words.text, "Example is now Example Exemplum.");
    assert_eq!(taken.message_type, Some(15));
    let refused = answer(false, "Example", "Exemplum", Some(&messages));
    assert_eq!(refused.words.text, "No surname for you.");
    // Without the installed lines, our own words.
    assert_eq!(
        answer(false, "Example", "Exemplum", None).source,
        eq_client_core::chat::Source::Client
    );
}
