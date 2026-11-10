//! One explicit confirmation for one exact cursor instance. Scripts click these
//! same controls and answer the ordinary confirmation dialog.
use super::InventoryState;
use crate::{
    confirm::{Asked, Question},
    online::OnlineState,
    outbox::Outbox,
};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, inventory::InventoryDestroy};

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Select,
    Confirm,
    Cancel,
}

#[derive(Default)]
pub(super) struct Destruction {
    selected: Option<InventoryDestroy>,
    pending: Option<(u64, u64)>,
}

impl Destruction {
    pub(super) const fn selecting(&self) -> bool {
        self.selected.is_some()
    }

    pub(super) const fn waiting(&self) -> bool {
        self.pending.is_some()
    }
}

fn question(request: &InventoryDestroy) -> Question {
    Question::DestroyItem {
        session_id: request.session_id,
        revision: request.revision,
        item_id: request.item.details.id,
    }
}

pub(super) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            for (action, label) in [
                (Action::Select, "Destroy cursor item"),
                (Action::Confirm, "Confirm destruction"),
                (Action::Cancel, "Cancel destruction"),
            ] {
                crate::theme::button_with(row, action, label, crate::theme::Size::Body);
            }
        });
}

impl InventoryState {
    fn cancel_destruction(&mut self, asked: &mut Asked) {
        if self
            .destruction
            .selected
            .take()
            .is_some_and(|request| asked.question() == Some(question(&request)))
        {
            asked.withdraw();
        }
    }

    /// A refusal or submission result only drains this exact local intent.
    /// Submission never claims that the server destroyed anything.
    pub(super) fn destruction_result(
        &mut self,
        session: u64,
        revision: u64,
        error: Option<&str>,
    ) -> bool {
        if self.destruction.pending != Some((session, revision)) {
            return false;
        }
        self.destruction.pending = None;
        if let Some(error) = error {
            self.refuse(format!("Destroy refused: {error}"));
        } else {
            self.actions.message = "Destroy sent; removal is a local prediction until received inventory state arrives".into();
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    fn destroy_action(
        &mut self,
        action: Action,
        online: &OnlineState,
        outbox: &Outbox,
        asked: &mut Asked,
    ) -> anyhow::Result<()> {
        use anyhow::{Context, ensure};
        if action == Action::Cancel {
            self.cancel_destruction(asked);
            self.actions.message.clear();
            return Ok(());
        }
        ensure!(
            self.destruction.pending.is_none(),
            "Wait for the destroy submission result"
        );
        ensure!(
            !self.ground_drop.waiting(),
            "Wait for the drop submission result"
        );
        match action {
            Action::Select => {
                self.cancel_destruction(asked);
                let request = online.world().prepare_cursor_destruction()?;
                let prompt = format!(
                    "Destroy all {} of {}? This cannot be undone. Empty containers only.",
                    request.item.stack_count.unwrap_or(1),
                    request.item.details.name
                );
                ensure!(
                    asked.ask(question(&request)),
                    "Answer the current confirmation first"
                );
                self.actions.auto_store = false;
                self.actions.split = None;
                self.actions.message = prompt;
                self.destruction.selected = Some(request);
            }
            Action::Confirm => {
                let mut request = self
                    .destruction
                    .selected
                    .take()
                    .context("Select Destroy cursor item first")?;
                if asked.question() == Some(question(&request)) {
                    asked.withdraw();
                }
                request.created = std::time::Instant::now();
                online.world().check_cursor_destruction(&request)?;
                outbox.send(
                    online.world(),
                    ClientCommand::DestroyItem(Box::new(request.clone())),
                )?;
                self.destruction.pending = Some((request.session_id, request.revision));
                self.actions.message = "Destroy queued once; waiting for submission result".into();
            }
            Action::Cancel => unreachable!(),
        }
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
    clicks: crate::clicks::Clicks<Action>,
    mut buttons: Query<(&Action, &mut Node)>,
) {
    if state.destruction.selected.is_none()
        && matches!(asked.question(), Some(Question::DestroyItem { .. }))
    {
        asked.withdraw();
    }
    // Any revision change cancels consent, even a same-definition replacement.
    let invalid = state
        .destruction
        .selected
        .as_ref()
        .is_some_and(|request| online.world().check_cursor_destruction(request).is_err())
        || !shown.is_open(crate::windows::WindowId::Inventory)
        || !keys.window_focused()
        || *escape == crate::escape::Escape::Inventory;
    if invalid && state.destruction.selecting() {
        state.cancel_destruction(&mut asked);
        state.actions.message =
            "Destroy confirmation canceled; select the current cursor item again".into();
        state.revision = state.revision.wrapping_add(1);
    }
    let mut action = clicks.iter().next().copied();
    if let Some(request) = &state.destruction.selected
        && let Some(yes) = asked.answer(question(request))
    {
        action = Some(if yes { Action::Confirm } else { Action::Cancel });
    }
    if !invalid && let Some(action) = action {
        if let Err(error) = state.destroy_action(action, &online, &outbox, &mut asked) {
            state.refuse(crate::outbox::window_line(&error));
        }
        state.revision = state.revision.wrapping_add(1);
    }
    let selecting = state.destruction.selected.is_some();
    for (action, mut node) in &mut buttons {
        node.display = if matches!(action, Action::Select) == selecting {
            Display::None
        } else {
            Display::Flex
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{
        Capability, WorldEvent,
        inventory::{InventorySlot, InventoryUpdate, MoveRules},
    };

    fn online() -> OnlineState {
        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: vec![Capability::Inventory],
                choices: Vec::new(),
                session_id: 7,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(9)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        testing::connect(&mut online, true);
        online.plan_inventory_as(Some(MoveRules::EqEmu));
        let mut item = crate::preview::items().into_iter().next().unwrap();
        item.slot = InventorySlot::CURSOR;
        item.bag_slots = 0;
        item.stack_count = Some(7);
        item.rules.stack_size = 20;
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item]));
        online
    }

    #[test]
    fn ordinary_destroy_button_and_dialog_yes_no_share_the_confirmation_owner() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(online())
            .insert_resource(Outbox::new(Some(tx)))
            .init_resource::<InventoryState>()
            .init_resource::<Asked>()
            .add_systems(Update, (input, crate::confirm::buttons).chain());
        app.world_mut()
            .resource_mut::<crate::windows::Shown>()
            .open(crate::windows::WindowId::Inventory);
        let select = app
            .world_mut()
            .spawn((Button, Interaction::None, Node::default(), Action::Select))
            .id();
        let no = app
            .world_mut()
            .spawn((
                Button,
                Interaction::None,
                crate::confirm::AnswerButton(false),
            ))
            .id();
        let yes = app
            .world_mut()
            .spawn((
                Button,
                Interaction::None,
                crate::confirm::AnswerButton(true),
            ))
            .id();
        crate::clicks::testing::click(&mut app, select);
        assert!(
            app.world()
                .resource::<InventoryState>()
                .destruction
                .selecting()
        );
        assert!(rx.try_recv().is_err());
        crate::clicks::testing::click(&mut app, no);
        app.update();
        assert!(
            !app.world()
                .resource::<InventoryState>()
                .destruction
                .selecting()
        );
        assert!(rx.try_recv().is_err());
        crate::clicks::testing::click(&mut app, select);
        crate::clicks::testing::click(&mut app, yes);
        app.update();
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::DestroyItem(_)
        ));
        app.update();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn cancel_and_confirm_use_one_frozen_whole_cursor_and_never_remove_locally() {
        let online = online();
        let before = online.world().inventory().clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(tx));
        let (mut state, mut asked) = (InventoryState::default(), Asked::default());
        state
            .destroy_action(Action::Select, &online, &outbox, &mut asked)
            .unwrap();
        assert!(matches!(
            asked.question(),
            Some(Question::DestroyItem { session_id: 7, .. })
        ));
        assert!(rx.try_recv().is_err());
        state
            .destroy_action(Action::Cancel, &online, &outbox, &mut asked)
            .unwrap();
        assert!(asked.question().is_none());
        assert!(
            state
                .destroy_action(Action::Confirm, &online, &outbox, &mut asked)
                .is_err()
        );
        assert!(rx.try_recv().is_err());
        state
            .destroy_action(Action::Select, &online, &outbox, &mut asked)
            .unwrap();
        state
            .destroy_action(Action::Confirm, &online, &outbox, &mut asked)
            .unwrap();
        let ClientCommand::DestroyItem(request) = rx.try_recv().unwrap() else {
            panic!("typed destruction expected");
        };
        assert_eq!(request.revision, before.revision());
        assert_eq!(request.item, before.items()[&InventorySlot::CURSOR]);
        assert_eq!(request.item.stack_count, Some(7));
        assert!(
            state
                .destroy_action(Action::Confirm, &online, &outbox, &mut asked)
                .is_err()
        );
        assert!(
            state
                .destroy_action(Action::Select, &online, &outbox, &mut asked)
                .is_err()
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(online.world().inventory(), &before);
        assert!(!state.destruction_result(8, request.revision, None));
        assert!(state.destruction_result(7, request.revision, Some("Fixture refusal")));
        assert!(!state.destruction_result(7, request.revision, None));
        assert!(state.actions.message.contains("Fixture refusal"));
    }

    #[test]
    fn replacement_quantity_change_and_link_reset_void_consent_without_a_packet() {
        for case in 0..3 {
            let mut online = online();
            let (tx, rx) = std::sync::mpsc::sync_channel(2);
            let outbox = Outbox::new(Some(tx));
            let (mut state, mut asked) = (InventoryState::default(), Asked::default());
            state
                .destroy_action(Action::Select, &online, &outbox, &mut asked)
                .unwrap();
            if case == 2 {
                testing::link(&mut online, eq_client_core::world::Link::Zoning);
            } else {
                let mut replacement =
                    online.world().inventory().items()[&InventorySlot::CURSOR].clone();
                if case == 1 {
                    replacement.stack_count = Some(6);
                }
                testing::inventory(&mut online, InventoryUpdate::Set(vec![replacement]));
            }
            assert!(
                state
                    .destroy_action(Action::Confirm, &online, &outbox, &mut asked)
                    .is_err()
            );
            assert!(state.destruction.selected.is_none());
            assert!(asked.question().is_none());
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn queue_failure_releases_confirmation_and_reset_does_not_replay() {
        let online = online();
        let (tx, rx) = std::sync::mpsc::sync_channel(0);
        let outbox = Outbox::new(Some(tx));
        let (mut state, mut asked) = (InventoryState::default(), Asked::default());
        state
            .destroy_action(Action::Select, &online, &outbox, &mut asked)
            .unwrap();
        assert!(
            state
                .destroy_action(Action::Confirm, &online, &outbox, &mut asked)
                .is_err()
        );
        assert!(state.destruction.pending.is_none());
        assert!(state.destruction.selected.is_none());
        assert!(rx.try_recv().is_err());
        state.forget();
        assert!(
            state
                .destroy_action(Action::Confirm, &online, &outbox, &mut asked)
                .is_err()
        );
    }

    #[test]
    fn unsupported_provider_contents_and_cursor_queue_never_open_confirmation() {
        for case in 0..3 {
            let mut online = online();
            if case == 0 {
                online.plan_inventory_as(Some(MoveRules::Quarm));
            }
            if case == 1 {
                let mut bag = online.world().inventory().items()[&InventorySlot::CURSOR].clone();
                bag.bag_slots = 4;
                bag.stack_count = None;
                let mut content = bag.clone();
                content.slot = InventorySlot(331);
                content.bag_slots = 0;
                testing::inventory(&mut online, InventoryUpdate::Set(vec![bag, content]));
            }
            if case == 2 {
                let item = online.world().inventory().items()[&InventorySlot::CURSOR].clone();
                testing::inventory(&mut online, InventoryUpdate::Cursor(vec![item]));
            }
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let outbox = Outbox::new(Some(tx));
            let (mut state, mut asked) = (InventoryState::default(), Asked::default());
            assert!(
                state
                    .destroy_action(Action::Select, &online, &outbox, &mut asked)
                    .is_err()
            );
            assert!(asked.question().is_none());
            assert!(rx.try_recv().is_err());
        }
    }
}
