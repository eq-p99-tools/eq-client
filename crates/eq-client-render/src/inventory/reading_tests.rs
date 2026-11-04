//! Synthetic right-click requests and the shared visible refusal path.
use super::*;
use crate::{
    online::{OnlineState, testing},
    outbox::{Outbox, Refusal},
    windows::{Shown, WindowId},
};
use eq_client_core::{Capability, ClientCommand, WorldEvent, books::Book};

fn online(reading: bool) -> OnlineState {
    let mut online = OnlineState::new(true);
    testing::news(
        &mut online,
        [WorldEvent::Entered {
            capabilities: if reading {
                vec![Capability::Reading]
            } else {
                Vec::new()
            },
            choices: Vec::new(),
            session_id: 7,
            zone: "qeytoqrg".into(),
            player: Box::new(testing::player(9)),
            far_clip: None,
            spell_landing_flag: None,
        }],
    );
    testing::connect(&mut online, true);
    let mut note = crate::preview::items().into_iter().next().unwrap();
    note.slot = InventorySlot(23);
    note.bag_slots = 0;
    note.book = Some(Book {
        file: "SyntheticText".into(),
        kind: 0,
    });
    testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![note]));
    online
}

fn click(online: &OnlineState, sender: &Outbox, shown: &mut Shown) {
    right_click(
        InventorySlot(23),
        online,
        sender,
        shown,
        &crate::skinned::Skinned::default(),
        &mut crate::items::ItemState::default(),
    );
}

#[test]
fn reading_click_posts_the_current_slot_without_inventing_text_or_opening_a_window() {
    let online = online(true);
    let before = online.world().inventory().clone();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let sender = Outbox::new(Some(tx));
    let mut shown = Shown::default();
    click(&online, &sender, &mut shown);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientCommand::ReadItem {
            session_id: 7,
            slot: InventorySlot(23)
        }
    ));
    assert!(rx.try_recv().is_err());
    assert_eq!(sender.take_refused(), []);
    assert_eq!(online.world().inventory(), &before);
    assert!(online.world().reading().is_none());
    assert!(!shown.is_open(WindowId::Note));
    assert!(!shown.is_open(WindowId::Book));
}

#[test]
fn reading_post_refusals_reach_chat_once_without_a_second_window_notice() {
    for (case, expected) in [
        (0, Refusal::Offline),
        (1, Refusal::Unavailable(Capability::Reading)),
        (2, Refusal::Busy),
        (3, Refusal::Ended),
        (4, Refusal::Away),
    ] {
        let mut online = online(case != 1);
        if case == 4 {
            testing::link(&mut online, eq_client_core::world::Link::Zoning);
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(usize::from(case != 2));
        let mut receiver = Some(rx);
        let sender = Outbox::new((case != 0).then_some(tx));
        if case == 3 {
            drop(receiver.take());
        }
        let mut shown = Shown::default();
        click(&online, &sender, &mut shown);
        if let Some(receiver) = &receiver {
            assert!(receiver.try_recv().is_err());
        }
        assert!(!shown.is_open(WindowId::Note));
        assert!(!shown.is_open(WindowId::Book));
        let mut app = App::new();
        app.insert_resource(sender)
            .init_resource::<crate::chat::ChatState>()
            .add_systems(Update, crate::outbox::show);
        app.update();
        let history = &app.world().resource::<crate::chat::ChatState>().history;
        let lines = history.lines(eq_client_core::chat::ChatTab::System);
        assert_eq!(lines.len(), 1, "case {case}");
        assert_eq!(lines[0].1.message.text, expected.text(), "case {case}");
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::chat::ChatState>()
                .history
                .lines(eq_client_core::chat::ChatTab::System)
                .len(),
            1,
            "case {case}"
        );
    }
}
