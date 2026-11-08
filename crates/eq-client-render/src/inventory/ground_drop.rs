//! Explicit ground drop from the ordinary inventory panel.
use super::InventoryState;
use crate::{
    confirm::{Asked, Question},
    online::OnlineState,
    outbox::Outbox,
};
use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand, inventory::InventoryDrop};

#[derive(Component)]
pub(crate) struct DropButton;

#[derive(Default)]
pub(super) struct State {
    selected: Option<InventoryDrop>,
    queued: Option<(u64, u64)>,
}

impl State {
    pub(super) const fn selecting(&self) -> bool {
        self.selected.is_some()
    }

    pub(super) const fn waiting(&self) -> bool {
        self.queued.is_some()
    }
}

fn question(request: &InventoryDrop) -> Question {
    Question::DropItem {
        session_id: request.session_id,
        revision: request.revision,
        item_id: request.item.details.id,
    }
}

pub(super) fn spawn(panel: &mut ChildSpawnerCommands) {
    crate::theme::button_with(
        panel,
        DropButton,
        "Drop cursor item",
        crate::theme::Size::Body,
    );
}

impl InventoryState {
    pub(super) fn ground_drop_result(
        &mut self,
        session: u64,
        revision: u64,
        error: Option<&str>,
    ) -> bool {
        if self.ground_drop.queued != Some((session, revision)) {
            return false;
        }
        self.ground_drop.queued = None;
        self.actions.message.clear();
        if let Some(error) = error {
            self.refuse(format!("Drop refused: {error}"));
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    fn submit_ground_drop(
        &mut self,
        online: &OnlineState,
        outbox: &Outbox,
        mut request: InventoryDrop,
    ) -> anyhow::Result<()> {
        online.world().check_cursor_drop(&request)?;
        request.created = std::time::Instant::now();
        let identity = (request.session_id, request.revision);
        outbox.send(online.world(), ClientCommand::DropItem(Box::new(request)))?;
        self.ground_drop.queued = Some(identity);
        self.actions.message = "Drop queued once. Waiting for the submission result.".into();
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)] // Bevy system parameters.
pub(crate) fn input(
    mut state: ResMut<InventoryState>,
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut asked: ResMut<Asked>,
    shown: Res<crate::windows::Shown>,
    escape: Res<crate::escape::Escape>,
    keys: crate::keys::Keys,
    clicks: crate::clicks::Clicks<DropButton>,
    mut buttons: Query<&mut Node, With<DropButton>>,
) {
    let visible = online.enabled
        && online.in_world()
        && online.world().can(Capability::GroundDropping)
        && state.ground_drop.queued.is_none()
        && !state.destruction.waiting()
        && !online.world().inventory().ground_drop_pending();
    for mut node in &mut buttons {
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    let open = shown.is_open(crate::windows::WindowId::Inventory)
        && keys.window_focused()
        && *escape != crate::escape::Escape::Inventory;
    if let Some(request) = state.ground_drop.selected.as_ref() {
        let prompt = question(request);
        if !open || online.world().check_cursor_drop(request).is_err() {
            state.ground_drop.selected = None;
            if asked.question() == Some(prompt) {
                asked.withdraw();
            }
            let _ = asked.answer(prompt);
        } else if let Some(yes) = asked.answer(prompt) {
            let request = state.ground_drop.selected.take().expect("selected drop");
            if yes && let Err(error) = state.submit_ground_drop(&online, &outbox, request) {
                state.refuse(crate::outbox::window_line(&error));
            }
        }
        return;
    }
    if matches!(asked.question(), Some(Question::DropItem { .. })) {
        asked.withdraw();
    }
    if visible && open && clicks.iter().next().is_some() {
        match online.world().prepare_cursor_drop() {
            Ok(request) if asked.ask(question(&request)) => {
                state.actions.auto_store = false;
                state.actions.split = None;
                state.ground_drop.selected = Some(request);
            }
            Ok(_) => state.refuse("Answer the current confirmation first".into()),
            Err(error) => state.refuse(crate::outbox::window_line(&error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{
        WorldEvent,
        inventory::{InventoryItem, InventorySlot, InventoryUpdate},
    };
    use std::{num::NonZeroU32, sync::mpsc};

    fn cursor(serial: u32) -> InventoryItem {
        let mut item = crate::preview::items().into_iter().next().unwrap();
        item.slot = InventorySlot::CURSOR;
        item.bag_slots = 0;
        item.stack_count = None;
        item.rules.ground_drop_serial = NonZeroU32::new(serial);
        item
    }

    #[test]
    fn confirmation_refuses_replaced_cursor_and_submits_a_fresh_selection_once() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(9));
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![cursor(17)]));
        assert!(online.world().prepare_cursor_drop().is_err());
        testing::empty_carried_coins(&mut online);

        let (tx, rx) = mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(online)
            .insert_resource(Outbox::new(Some(tx)))
            .init_resource::<InventoryState>()
            .init_resource::<Asked>()
            .add_systems(Update, (input, crate::confirm::buttons).chain());
        app.world_mut()
            .resource_mut::<crate::windows::Shown>()
            .open(crate::windows::WindowId::Inventory);
        let drop = app
            .world_mut()
            .spawn((Button, Interaction::None, Node::default(), DropButton))
            .id();
        let yes = app
            .world_mut()
            .spawn((
                Button,
                Interaction::None,
                crate::confirm::AnswerButton(true),
            ))
            .id();

        crate::clicks::testing::click(&mut app, drop);
        assert!(matches!(
            app.world().resource::<Asked>().question(),
            Some(Question::DropItem { .. })
        ));
        assert!(rx.try_recv().is_err());

        assert!(app.world().resource::<InventoryState>().action_under_way());
        *app.world_mut().resource_mut::<crate::escape::Escape>() = crate::escape::Escape::Inventory;
        app.update();
        assert!(app.world().resource::<Asked>().question().is_none());
        assert!(!app.world().resource::<InventoryState>().action_under_way());
        assert!(rx.try_recv().is_err());
        *app.world_mut().resource_mut::<crate::escape::Escape>() = crate::escape::Escape::Unused;
        crate::clicks::testing::click(&mut app, drop);

        let item = cursor(18);
        testing::inventory(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            InventoryUpdate::Set(vec![item.clone()]),
        );
        crate::clicks::testing::click(&mut app, yes);
        app.update();
        assert!(rx.try_recv().is_err());

        crate::clicks::testing::click(&mut app, drop);
        crate::clicks::testing::click(&mut app, yes);
        app.update();
        let ClientCommand::DropItem(request) = rx.try_recv().unwrap() else {
            panic!("expected the typed drop");
        };
        assert_eq!((request.session_id, request.own_id), (7, 9));
        assert_eq!(request.item, item);
        app.update();
        assert!(rx.try_recv().is_err());
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .inventory()
                .items()
                .get(&InventorySlot::CURSOR),
            Some(&item)
        );
    }

    #[test]
    fn submitted_drop_keeps_confirmation_unavailable_until_snapshot() {
        let original = cursor(17);
        let replacement = cursor(18);
        for updates in [
            vec![InventoryUpdate::Cursor(vec![replacement.clone()])],
            vec![
                InventoryUpdate::Set(vec![replacement.clone()]),
                InventoryUpdate::Set(vec![original.clone()]),
            ],
        ] {
            let mut online = OnlineState::new(true);
            testing::admit(&mut online, 7, testing::player(9));
            testing::empty_carried_coins(&mut online);
            testing::inventory(
                &mut online,
                InventoryUpdate::Snapshot(vec![original.clone()]),
            );
            let revision = online.world().inventory().revision();
            testing::news(
                &mut online,
                [WorldEvent::InventoryAction {
                    session_id: 7,
                    revision,
                    sequence: None,
                    update: Some(InventoryUpdate::GroundDropPending(
                        original.rules.ground_drop_serial.unwrap(),
                    )),
                    error: None,
                }],
            );
            for update in updates {
                testing::inventory(&mut online, update);
            }
            testing::inventory(&mut online, InventoryUpdate::Remove(InventorySlot::CURSOR));
            assert!(online.world().inventory().ground_drop_pending());
            assert!(online.world().inventory().stale());
            assert_eq!(
                online
                    .world()
                    .inventory()
                    .items()
                    .get(&InventorySlot::CURSOR),
                Some(&original)
            );
            assert!(online.world().prepare_cursor_drop().is_err());

            let (tx, rx) = mpsc::sync_channel(4);
            let mut app = crate::testing::app();
            app.insert_resource(online)
                .insert_resource(Outbox::new(Some(tx)))
                .init_resource::<InventoryState>()
                .init_resource::<Asked>()
                .add_systems(Update, input);
            app.world_mut()
                .resource_mut::<crate::windows::Shown>()
                .open(crate::windows::WindowId::Inventory);
            let button = app
                .world_mut()
                .spawn((Button, Interaction::None, Node::default(), DropButton))
                .id();
            crate::clicks::testing::click(&mut app, button);
            assert_eq!(
                app.world().get::<Node>(button).unwrap().display,
                Display::None
            );
            assert!(app.world().resource::<Asked>().question().is_none());
            assert!(rx.try_recv().is_err());

            testing::inventory(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                InventoryUpdate::Snapshot(vec![replacement.clone()]),
            );
            crate::clicks::testing::click(&mut app, button);
            assert_eq!(
                app.world().get::<Node>(button).unwrap().display,
                Display::Flex
            );
            assert!(matches!(
                app.world().resource::<Asked>().question(),
                Some(Question::DropItem { .. })
            ));
            assert!(rx.try_recv().is_err());
        }
    }
}
