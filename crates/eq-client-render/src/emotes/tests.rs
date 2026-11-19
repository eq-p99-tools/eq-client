use super::*;
use crate::online::testing;
use eq_client_core::WorldEvent;

/// An admitted player at the origin, with these spawns about.
fn world(spawns: &[(u16, &str, (f32, f32, f32))]) -> OnlineState {
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 1, testing::player(7));
    let spawns = spawns
        .iter()
        .map(|(id, name, (x, y, z))| {
            let mut spawn = testing::pet(*id, 0);
            spawn.pet_owner = None;
            spawn.name = (*name).into();
            spawn.position.x = *x;
            spawn.position.y = *y;
            spawn.position.z = *z;
            spawn
        })
        .collect();
    testing::news(&mut online, [WorldEvent::Spawns(spawns)]);
    online
}

fn emote(name: &str) -> &'static Emote {
    emotes::named(name).unwrap()
}

fn said(outcome: &Outcome) -> (&str, &str, Option<u8>) {
    match outcome {
        Outcome::Said { spoken, animation } => (&spoken.sent, &spoken.local, *animation),
        other => panic!("{other:?}"),
    }
}

fn decided(online: &OnlineState, name: &str, words: &str, moving: bool) -> Outcome {
    decide(
        online.world(),
        emote(name),
        words,
        moving,
        &Messages::default(),
    )
}

#[test]
fn a_typed_name_picks_the_shortest_spawn_it_begins_whatever_the_case_or_range() {
    let online = world(&[
        (10, "Gnollguard00", (10.0, 0.0, 0.0)),
        (11, "Gnome00", (5000.0, 0.0, 0.0)),
        (12, "a_gnoll_pup01", (10.0, 0.0, 0.0)),
    ]);
    let world = online.world();
    // The far gnome is shorter, so it wins: distance plays no part.
    assert_eq!(named_spawn(world, "gno"), Some(11));
    assert_eq!(named_spawn(world, "GNOLL"), Some(10));
    // Digits are dropped and '_' reads as a space.
    assert_eq!(named_spawn(world, "a_gnoll"), Some(12));
    assert_eq!(named_spawn(world, "a gnoll pup"), Some(12));
    assert_eq!(named_spawn(world, "orc"), None);
    assert_eq!(named_spawn(world, "00"), None);
    // The player's own spawn can match.
    assert_eq!(named_spawn(world, "exam"), Some(7));
}

#[test]
fn a_name_nobody_has_is_refused_and_the_players_own_counts_as_no_target() {
    let online = world(&[(10, "Gnome00", (10.0, 0.0, 0.0))]);
    assert_eq!(
        decided(&online, "grin", "orc", false),
        Outcome::Refused(Refusal::NotFound)
    );
    // Picking yourself says the no-target line.
    assert_eq!(
        said(&decided(&online, "grin", "example", false)),
        (" uses /grin.", "You use /grin.", None)
    );
    assert_eq!(
        said(&decided(&online, "grin", "gnome", false)),
        (" uses /grin on Gnome.", "You use /grin on Gnome.", None)
    );
}

#[test]
fn the_range_is_sixty_units_flat_between_origins() {
    let online = world(&[
        (10, "Near00", (60.0, 0.0, 0.0)),
        (11, "Far00", (60.1, 0.0, 0.0)),
        (12, "High00", (36.0, 47.0, 500.0)),
    ]);
    assert!(matches!(
        decided(&online, "grin", "near", false),
        Outcome::Said { .. }
    ));
    assert_eq!(
        decided(&online, "grin", "far", false),
        Outcome::Refused(Refusal::TooFar)
    );
    // 59.2 across, whatever the height.
    assert!(matches!(
        decided(&online, "grin", "high", false),
        Outcome::Said { .. }
    ));
}

#[test]
fn with_no_word_the_current_target_is_used() {
    let mut online = world(&[(10, "Gnome00", (10.0, 0.0, 0.0))]);
    assert_eq!(said(&decided(&online, "grin", "", false)).0, " uses /grin.");
    online.select_target(Some(10));
    assert_eq!(
        said(&decided(&online, "grin", "", false)).0,
        " uses /grin on Gnome."
    );
    // A target out of range is refused as a named one is.
    let mut far = world(&[(10, "Gnome00", (100.0, 0.0, 0.0))]);
    far.select_target(Some(10));
    assert_eq!(
        decided(&far, "grin", "", false),
        Outcome::Refused(Refusal::TooFar)
    );
}

#[test]
fn the_installed_my_target_word_means_the_current_target() {
    let mut online = world(&[
        (10, "Gnome00", (10.0, 0.0, 0.0)),
        (11, "Target00", (10.0, 0.0, 0.0)),
    ]);
    online.select_target(Some(10));
    let messages = Messages::parse("EQST0002\n1\n12295 TARGET\n");
    let outcome = decide(online.world(), emote("grin"), "target", false, &messages);
    assert_eq!(said(&outcome).0, " uses /grin on Gnome.");
    // Without the installed word, it is a name like any other.
    assert_eq!(
        said(&decided(&online, "grin", "target", false)).0,
        " uses /grin on Target."
    );
}

#[test]
fn a_still_emote_needs_the_player_standing_still_and_others_animate_only_then() {
    let online = world(&[]);
    assert_eq!(
        decided(&online, "wave", "", true),
        Outcome::Refused(Refusal::StandStill)
    );
    assert_eq!(said(&decided(&online, "wave", "", false)).2, Some(0x1d));
    // A moving player's groan is said without its animation.
    assert_eq!(said(&decided(&online, "groan", "", true)).2, None);
    assert_eq!(said(&decided(&online, "groan", "", false)).2, Some(0x1c));
    // So is a sitting player's.
    let mut sitting = world(&[]);
    testing::news(
        &mut sitting,
        [WorldEvent::Posture {
            spawn_id: 7,
            posture: PostureState::Sitting,
        }],
    );
    assert_eq!(said(&decided(&sitting, "groan", "", false)).2, None);
    assert_eq!(
        decided(&sitting, "wave", "", false),
        Outcome::Refused(Refusal::StandStill)
    );
}

#[test]
fn extra_words_after_the_target_replace_the_emotes_own() {
    let online = world(&[(10, "Gnome00", (10.0, 0.0, 0.0))]);
    assert_eq!(
        said(&decided(&online, "grin", "gnome grins widely", false)),
        (" grins widely.", "You grins widely.", None)
    );
    // One character is not enough.
    assert_eq!(
        said(&decided(&online, "grin", "gnome x", false)).0,
        " uses /grin on Gnome."
    );
}

#[test]
fn fidget_with_a_target_says_nothing() {
    let online = world(&[(10, "Gnome00", (10.0, 0.0, 0.0))]);
    assert_eq!(decided(&online, "fidget", "gnome", false), Outcome::Silent);
}

/// An app running the emote system, sending to `tx`.
fn app(
    online: OnlineState,
    client: eq_client_assets::ui::InstalledClient,
) -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mut app = App::new();
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(tx)))
        .insert_resource(crate::ViewerSettings(crate::ViewerConfig {
            installed_client: client,
            ..default()
        }))
        .init_resource::<ChatState>()
        .add_systems(Update, run);
    (app, rx)
}

#[test]
fn said_emotes_send_their_text_then_their_animation_in_order() {
    let online = world(&[(10, "Gnome00", (10.0, 0.0, 0.0))]);
    let (mut app, rx) = app(online, eq_client_assets::ui::InstalledClient::Titanium);
    // Two in one frame, as a social's lines queue them.
    let mut chat = app.world_mut().resource_mut::<ChatState>();
    chat.emotes.push((emote("wave").case, String::new()));
    chat.emotes.push((emote("grin").case, "gnome".into()));
    app.update();
    let sent: Vec<_> = rx.try_iter().collect();
    assert!(
        matches!(&sent[..], [
            ClientCommand::Emote { text: first, .. },
            ClientCommand::EmoteAnimation { animation: 0x1d, .. },
            ClientCommand::Emote { text: second, .. },
        ] if first == "uses /wave." && second == "uses /grin on Gnome."),
        "{sent:?}"
    );
    let chat = app.world().resource::<ChatState>();
    let lines: Vec<_> = chat
        .history
        .lines(eq_client_core::chat::ChatTab::All)
        .into_iter()
        .map(|(_, line)| (line.message.text.as_str(), line.message_type))
        .collect();
    assert_eq!(
        lines,
        [
            ("You use /wave.", Some(EMOTE_COLOUR)),
            ("You use /grin on Gnome.", Some(EMOTE_COLOUR)),
        ]
    );
}

#[test]
fn the_mac_era_client_keeps_its_earlier_answer() {
    let online = world(&[]);
    let (mut app, rx) = app(online, eq_client_assets::ui::InstalledClient::EqMac);
    app.world_mut()
        .resource_mut::<ChatState>()
        .emotes
        .push((emote("wave").case, String::new()));
    app.update();
    assert!(rx.try_recv().is_err());
    let chat = app.world().resource::<ChatState>();
    assert!(
        chat.history
            .lines(eq_client_core::chat::ChatTab::All)
            .iter()
            .any(|(_, line)| line.message.text == "This client does not take /wave that way yet.")
    );
}
