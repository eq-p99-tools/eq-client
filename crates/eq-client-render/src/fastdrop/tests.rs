use super::*;
use crate::chat::submit_social_line;
use crate::online::{OnlineState, testing};

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<ChatState>()
        .init_resource::<InventoryState>()
        .add_systems(Update, answers);
    app
}

fn ask(app: &mut App) -> Option<String> {
    let mut chat = app.world_mut().resource_mut::<ChatState>();
    assert_eq!(
        crate::chat::client_request_in_scope("/fastdrop", &mut chat, None),
        Some(Ok(()))
    );
    app.update();
    app.world()
        .resource::<ChatState>()
        .history
        .lines(eq_client_core::chat::ChatTab::System)
        .last()
        .map(|(_, line)| line.message.text.clone())
}

#[test]
fn the_words_are_fastdrop_spelled_out_and_ignore_what_follows() {
    assert!(typed("/fastdrop"));
    assert!(typed("/FASTDROP on"));
    assert!(!typed("/fastdropx"));
    assert!(!typed("fastdrop"));
    assert_eq!(
        crate::commands::spelled("/fa").as_deref(),
        Some("/fastdrop")
    );
}

#[test]
fn each_fastdrop_turns_it_over_and_says_the_state_it_lands_in() {
    let mut app = app();
    assert!(!app.world().resource::<InventoryState>().fast_drop);
    assert_eq!(
        ask(&mut app).as_deref(),
        Some("Items you drop on the ground will no longer ask first.")
    );
    assert!(app.world().resource::<InventoryState>().fast_drop);
    assert_eq!(
        ask(&mut app).as_deref(),
        Some("Items you drop on the ground will ask first again.")
    );
    assert!(!app.world().resource::<InventoryState>().fast_drop);
}

#[test]
fn a_social_or_hotbutton_line_cannot_turn_it_over() {
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 1, testing::player(7));
    let outbox = crate::outbox::Outbox::new(None);
    let mut chat = ChatState::default();
    for line in ["/fastdrop", "/fa", "/FASTDROP now"] {
        assert!(
            submit_social_line(
                line,
                &mut chat,
                &mut online,
                &outbox,
                &eq_client_core::qol::Settings::default()
            )
            .is_err(),
            "{line}"
        );
    }
    assert_eq!(chat.fastdrop_asked, None);
}
