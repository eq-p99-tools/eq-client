//! Explicit ground drop from the ordinary inventory panel.
use super::InventoryState;
use crate::{
    confirm::{Asked, Question},
    online::OnlineState,
    outbox::{DropPublication, DropReceipt, Outbox},
};
use bevy::prelude::*;
use eq_client_core::options::NoSafeDrop;
use eq_client_core::{Capability, inventory::InventoryDrop};
use std::time::{Duration, Instant};

const QUESTION_LIFETIME: Duration = Duration::from_secs(20);

struct Selection {
    request: InventoryDrop,
    question: Question,
    deadline: Instant,
    from_world: bool,
    coins: Option<eq_client_core::Coins>,
    cursor_coins: Option<eq_client_core::Coins>,
    weight: u64,
}

struct Queued {
    identity: (u64, u64),
    _receipt: DropReceipt,
}

#[derive(Component)]
pub(crate) struct DropButton;

#[derive(Default)]
pub(super) struct State {
    selected: Option<Selection>,
    queued: Option<Queued>,
    requested: Option<InventoryDrop>,
    next_id: u64,
}

impl State {
    pub(super) const fn selecting(&self) -> bool {
        self.selected.is_some()
    }

    pub(super) const fn waiting(&self) -> bool {
        self.queued.is_some()
    }
}

fn question(request: &InventoryDrop, request_id: u64) -> Question {
    Question::DropItem {
        request_id,
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
    /// Queue an owned world release for the same controller as the fallback button.
    pub(crate) fn request_world_drop(&mut self, request: InventoryDrop) {
        if self.ground_drop.selected.is_none()
            && self.ground_drop.queued.is_none()
            && self.ground_drop.requested.is_none()
            && !self.destruction.waiting()
        {
            self.ground_drop.requested = Some(request);
        }
    }

    /// Hide staged presentation until the worker supplies its local projection
    /// or a definite pre-send refusal. Projected inventory owns absence after it.
    pub(crate) fn cursor_drop_hidden(&self, _world: &eq_client_core::world::ClientWorld) -> bool {
        self.ground_drop.waiting()
    }

    pub(super) fn ground_drop_result(
        &mut self,
        session: u64,
        revision: u64,
        error: Option<&str>,
    ) -> bool {
        if self
            .ground_drop
            .queued
            .as_ref()
            .map(|queued| queued.identity)
            != Some((session, revision))
        {
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
        request: InventoryDrop,
        fast: bool,
        chat: &mut crate::chat::ChatState,
        messages: Option<&crate::hud::messages::Messages>,
    ) -> anyhow::Result<()> {
        let (publication, line) =
            self.stage_ground_drop(online, outbox, request, fast, chat, messages)?;
        if let Err(error) = publication.publish() {
            self.ground_drop.queued = None;
            if let Some(line) = line {
                let _ = chat.history.withdraw(line);
            }
            self.actions.message.clear();
            self.revision = self.revision.wrapping_add(1);
            outbox.refused(error);
            return Err(error.into());
        }
        Ok(())
    }

    fn stage_ground_drop(
        &mut self,
        online: &OnlineState,
        outbox: &Outbox,
        mut request: InventoryDrop,
        fast: bool,
        chat: &mut crate::chat::ChatState,
        messages: Option<&crate::hud::messages::Messages>,
    ) -> anyhow::Result<(DropPublication, Option<u64>)> {
        online.world().check_cursor_drop(&request)?;
        request.created = std::time::Instant::now();
        let identity = (request.session_id, request.revision);
        let name = request.item.details.name.clone();
        let (publication, receipt) = outbox.prepare_cursor_drop(online.world(), request)?;
        let before = chat.history.revision();
        if fast {
            drop_line(
                chat,
                messages,
                0x2362,
                Some(0x0f),
                &name,
                "Item drop submitted.",
            );
        }
        let line = (chat.history.revision() != before).then_some(chat.history.revision());
        self.ground_drop.queued = Some(Queued {
            identity,
            _receipt: receipt,
        });
        self.actions.auto_store = false;
        self.actions.split = None;
        self.actions.message = "Drop queued once. Waiting for the submission result.".into();
        self.revision = self.revision.wrapping_add(1);
        Ok((publication, line))
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::type_complexity
)] // One controller owns selection, answers and submission.
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
    (options, messages, mut chat, controls): (
        Option<Res<crate::options::OptionsState>>,
        Option<Res<crate::hud::messages::Messages>>,
        ResMut<crate::chat::ChatState>,
        Option<Res<crate::motion::Controls>>,
    ),
) {
    let now = Instant::now();
    let on_carrier = controls
        .as_ref()
        .is_some_and(|controls| controls.carrier_support().is_some());
    let visible = online.enabled
        && online.in_world()
        && online.world().can(Capability::GroundDropping)
        && state.ground_drop.queued.is_none()
        && !state.destruction.waiting()
        && !on_carrier;
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
    if let Some(selected) = state.ground_drop.selected.as_ref() {
        let prompt = selected.question;
        if !keys.window_focused()
            || !online.enabled
            || on_carrier
            || !selected.from_world && !open
            || now >= selected.deadline
            || online.world().check_cursor_drop(&selected.request).is_err()
            || online.world().coins().copied() != selected.coins
            || online
                .world()
                .coins_in(eq_client_core::money::CoinPlace::Cursor)
                != selected.cursor_coins
            || online.world().movement_weight().ok() != Some(selected.weight)
            || keys.input.just_pressed(KeyCode::Escape)
        {
            state.ground_drop.selected = None;
            if asked.question() == Some(prompt) {
                asked.withdraw();
            }
            let _ = asked.answer(prompt);
        } else if let Some(yes) = asked.answer(prompt) {
            let selected = state.ground_drop.selected.take().expect("selected drop");
            if yes
                && let Err(error) = state.submit_ground_drop(
                    &online,
                    &outbox,
                    selected.request,
                    false,
                    &mut chat,
                    messages.as_deref(),
                )
            {
                state.refuse(crate::outbox::window_line(&error));
            }
        }
        return;
    }
    if matches!(asked.question(), Some(Question::DropItem { .. })) {
        asked.withdraw();
    }
    let requested = state.ground_drop.requested.take();
    let from_world = requested.is_some();
    if !visible || asked.question().is_some() || asked.has_pending_answer() {
        return;
    }
    let request = match requested {
        Some(request) => online.world().check_cursor_drop(&request).map(|()| request),
        None if open && clicks.iter().next().is_some() => online.world().prepare_cursor_drop(),
        None => return,
    };
    // Failing the ordinary item gate silently consumes the attempted drop.
    let Ok(request) = request else {
        return;
    };
    let Ok(weight) = online.world().movement_weight() else {
        return;
    };
    let coins = online.world().coins().copied();
    let cursor_coins = online
        .world()
        .coins_in(eq_client_core::money::CoinPlace::Cursor);
    let mode = options
        .as_ref()
        .map_or(NoSafeDrop::Ask, |options| options.options.no_safe_drop);
    match mode {
        NoSafeDrop::Never => drop_line(
            &mut chat,
            messages.as_deref(),
            0x3020,
            None,
            &request.item.details.name,
            "Dropping items is disabled.",
        ),
        NoSafeDrop::Fast => {
            if let Err(error) = state.submit_ground_drop(
                &online,
                &outbox,
                request,
                true,
                &mut chat,
                messages.as_deref(),
            ) {
                state.refuse(crate::outbox::window_line(&error));
            }
        }
        NoSafeDrop::Ask => {
            let Some(request_id) = state.ground_drop.next_id.checked_add(1) else {
                return;
            };
            let prompt = question(&request, request_id);
            if asked.ask(prompt) {
                state.ground_drop.next_id = request_id;
                state.actions.auto_store = false;
                state.actions.split = None;
                state.ground_drop.selected = Some(Selection {
                    request,
                    question: prompt,
                    deadline: now + QUESTION_LIFETIME,
                    from_world,
                    coins,
                    cursor_coins,
                    weight,
                });
            }
        }
    }
}

fn drop_line(
    chat: &mut crate::chat::ChatState,
    messages: Option<&crate::hud::messages::Messages>,
    id: u32,
    colour: Option<u32>,
    name: &str,
    fallback: &str,
) {
    let said = messages.map_or_else(
        || crate::chat::Said::own(fallback),
        |messages| messages.said_or(id, &[name.to_owned()], fallback),
    );
    let said = if let Some(colour) = colour {
        said.typed(colour)
    } else {
        said
    };
    chat.history.push(crate::chat::system_line(said));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{
        ClientCommand, WorldEvent,
        inventory::{InventoryItem, InventorySlot, InventoryUpdate},
    };

    #[test]
    #[allow(clippy::too_many_lines)] // One reservation covers publication and the worker projection handoff.
    fn staged_drop_owns_cursor_until_worker_projection_then_releases_submission_lock() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(9));
        testing::empty_carried_coins(&mut online);
        let original = cursor(17);
        testing::inventory(
            &mut online,
            InventoryUpdate::Snapshot(vec![original.clone()]),
        );
        let request = online.world().prepare_cursor_drop().unwrap();
        let revision = request.revision;
        let (sender, received) = std::sync::mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(sender));
        let mut state = InventoryState::default();
        let mut chat = crate::chat::ChatState::default();
        let (publication, line) = state
            .stage_ground_drop(&online, &outbox, request, true, &mut chat, None)
            .unwrap();
        assert!(line.is_some());
        assert!(chat.history.line(line.unwrap()).is_some());
        assert!(state.cursor_drop_hidden(online.world()));
        assert_eq!(
            online
                .world()
                .inventory()
                .items()
                .get(&InventorySlot::CURSOR),
            Some(&original)
        );
        assert!(!online.world().worker_inventory().ground_drop_projected());
        assert!(received.try_recv().is_err(), "staging must not publish");
        let offer = || ClientCommand::OfferTrade {
            session_id: 7,
            with_id: 42,
            created: Instant::now(),
        };
        assert_eq!(
            outbox.for_script(Some(7)).send(online.world(), offer()),
            Err(crate::outbox::Refusal::CursorDropPending)
        );
        for command in [
            ClientCommand::Consume {
                session_id: 7,
                slot: InventorySlot::CURSOR,
                created: Instant::now(),
            },
            ClientCommand::ApplyPoison {
                session_id: 7,
                slot: InventorySlot::CURSOR,
                created: Instant::now(),
            },
            ClientCommand::Sell {
                session_id: 7,
                merchant_id: 42,
                own_id: 9,
                slot: InventorySlot::CURSOR.0,
                quantity: 1,
                created: Instant::now(),
            },
        ] {
            assert_eq!(
                outbox.for_script(Some(7)).send(online.world(), command),
                Err(crate::outbox::Refusal::CursorDropPending)
            );
        }
        assert!(!state.click_slot(InventorySlot(22), false, &mut online, &outbox));
        state.actions.auto_store = true;
        state.store_cursor(&mut online, &outbox);
        assert!(!online.world().inventory_moves_pending());
        assert!(received.try_recv().is_err());

        // The consumer is released only after observing the staged line,
        // mask, unchanged custody and cursor lock. No sleeps choose order.
        let (release, gate) = std::sync::mpsc::sync_channel(1);
        let consumer = std::thread::spawn(move || {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            let command = received.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(received.try_recv().is_err());
            (command, received)
        });
        release.send(()).unwrap();
        publication.publish().unwrap();
        let (command, received) = consumer.join().unwrap();
        assert!(matches!(command, ClientCommand::DropItem(_)));

        testing::news(
            &mut online,
            [WorldEvent::InventoryAction {
                session_id: 7,
                revision,
                sequence: None,
                update: Some(InventoryUpdate::GroundDropProjected(
                    eq_client_core::inventory::InventoryDropProjection {
                        revision,
                        item: Box::new(original.clone()),
                    },
                )),
                error: None,
            }],
        );
        assert!(state.ground_drop_result(7, revision, None));
        assert!(!state.cursor_drop_hidden(online.world()));
        assert!(online.world().worker_inventory().ground_drop_projected());
        assert!(
            !online
                .world()
                .inventory()
                .items()
                .contains_key(&InventorySlot::CURSOR)
        );
        assert_eq!(outbox.send(online.world(), offer()), Ok(()));
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::OfferTrade { .. })
        ));
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![original]));
        assert!(!state.cursor_drop_hidden(online.world()));
    }

    #[test]
    fn full_and_closed_queues_roll_back_only_local_drop_presentation() {
        for closed in [false, true] {
            let mut online = OnlineState::new(true);
            testing::admit(&mut online, 7, testing::player(9));
            testing::empty_carried_coins(&mut online);
            let original = cursor(17);
            testing::inventory(
                &mut online,
                InventoryUpdate::Snapshot(vec![original.clone()]),
            );
            let request = online.world().prepare_cursor_drop().unwrap();
            let (sender, receiver) = std::sync::mpsc::sync_channel(0);
            let receiver = (!closed).then_some(receiver);
            let outbox = Outbox::new(Some(sender));
            let mut state = InventoryState::default();
            let mut chat = crate::chat::ChatState::default();
            chat.history
                .push(crate::chat::system_line(crate::chat::Said::own(
                    "Unrelated retained line",
                )));
            let before = chat.history.revision();
            assert!(
                state
                    .submit_ground_drop(&online, &outbox, request, true, &mut chat, None)
                    .is_err()
            );
            assert!(!state.cursor_drop_hidden(online.world()));
            assert_eq!(
                online
                    .world()
                    .inventory()
                    .items()
                    .get(&InventorySlot::CURSOR),
                Some(&original)
            );
            let lines = chat.history.lines(eq_client_core::chat::ChatTab::System);
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].0, before);
            assert!(!online.world().worker_inventory().ground_drop_projected());
            let outcome = outbox.send(
                online.world(),
                ClientCommand::OfferTrade {
                    session_id: 7,
                    with_id: 42,
                    created: Instant::now(),
                },
            );
            assert_eq!(
                outcome,
                Err(if closed {
                    crate::outbox::Refusal::Ended
                } else {
                    crate::outbox::Refusal::Busy
                })
            );
            if let Some(receiver) = receiver {
                assert!(receiver.try_recv().is_err());
            }
        }
    }
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
    #[allow(clippy::too_many_lines)] // Exercise one cursor replacement and confirmed retry.
    fn confirmation_refuses_replaced_cursor_and_submits_a_fresh_selection_once() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(9));
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![cursor(17)]));
        let coin_error = "A cursor drop needs current carried coin information";
        assert_eq!(
            online
                .world()
                .prepare_cursor_drop()
                .unwrap_err()
                .to_string(),
            coin_error
        );
        testing::news(
            &mut online,
            [WorldEvent::Coins(eq_client_core::Coins::default())],
        );
        assert_eq!(
            online
                .world()
                .prepare_cursor_drop()
                .unwrap_err()
                .to_string(),
            coin_error
        );
        testing::empty_carried_coins(&mut online);
        let selected = online.world().prepare_cursor_drop().unwrap();
        testing::news(&mut online, [WorldEvent::PurseUncertain { session_id: 7 }]);
        assert_eq!(
            online
                .world()
                .check_cursor_drop(&selected)
                .unwrap_err()
                .to_string(),
            coin_error
        );
        testing::news(
            &mut online,
            [WorldEvent::Coins(eq_client_core::Coins::default())],
        );

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
    fn projected_submission_allows_a_different_received_cursor_drop_without_readmission() {
        let original = cursor(17);
        let replacement = cursor(18);
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(9));
        testing::empty_carried_coins(&mut online);
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![original]));
        let request = online.world().prepare_cursor_drop().unwrap();
        let revision = request.revision;
        testing::news(
            &mut online,
            [WorldEvent::InventoryAction {
                session_id: 7,
                revision,
                sequence: None,
                update: Some(InventoryUpdate::GroundDropProjected((&request).into())),
                error: None,
            }],
        );
        assert!(online.world().inventory().ground_drop_projected());
        assert!(
            !online
                .world()
                .inventory()
                .items()
                .contains_key(&InventorySlot::CURSOR)
        );
        testing::inventory(&mut online, InventoryUpdate::Cursor(vec![replacement]));
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
            Display::Flex
        );
        assert!(matches!(
            app.world().resource::<Asked>().question(),
            Some(Question::DropItem { .. })
        ));
        assert!(rx.try_recv().is_err());
        app.world_mut().resource_mut::<Asked>().give(true);
        app.update();
        assert!(matches!(rx.try_recv(), Ok(ClientCommand::DropItem(_))));
        assert!(rx.try_recv().is_err());
    }

    /// With `/fastdrop` on, a click sends the drop once with no question,
    /// and the same checks still refuse a cursor whose coins are unknown
    /// (6.536).
    #[test]
    fn fast_drop_sends_once_without_asking_through_the_same_checks() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(9));
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![cursor(17)]));
        let (tx, rx) = mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(online)
            .insert_resource(Outbox::new(Some(tx)))
            .init_resource::<InventoryState>()
            .insert_resource(crate::options::OptionsState::new(
                &eq_client_core::options::Options {
                    no_safe_drop: NoSafeDrop::Fast,
                    ..Default::default()
                },
            ))
            .init_resource::<Asked>()
            .add_systems(Update, input);
        app.world_mut()
            .resource_mut::<crate::windows::Shown>()
            .open(crate::windows::WindowId::Inventory);
        let drop = app
            .world_mut()
            .spawn((Button, Interaction::None, Node::default(), DropButton))
            .id();
        // The coins on hand are unknown: refused, asked nothing, sent nothing.
        crate::clicks::testing::click(&mut app, drop);
        assert!(app.world().resource::<Asked>().question().is_none());
        assert!(rx.try_recv().is_err());
        testing::empty_carried_coins(&mut app.world_mut().resource_mut::<OnlineState>());
        crate::clicks::testing::click(&mut app, drop);
        assert!(app.world().resource::<Asked>().question().is_none());
        assert!(matches!(rx.try_recv(), Ok(ClientCommand::DropItem(_))));
        // One at a time: a second click waits for the first's result.
        crate::clicks::testing::click(&mut app, drop);
        app.update();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn world_question_expires_and_a_late_yes_cannot_answer_a_fresh_attempt() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(9));
        testing::empty_carried_coins(&mut online);
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![cursor(17)]));
        let request = online.world().prepare_cursor_drop().unwrap();
        let (tx, rx) = mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(online)
            .insert_resource(Outbox::new(Some(tx)))
            .add_systems(Update, input);
        app.world_mut()
            .resource_mut::<InventoryState>()
            .request_world_drop(request.clone());
        app.update();
        let first = app.world().resource::<Asked>().question().unwrap();
        assert!(
            !app.world()
                .resource::<crate::windows::Shown>()
                .is_open(crate::windows::WindowId::Inventory)
        );
        app.world_mut().resource_mut::<Asked>().give(true);
        app.world_mut()
            .resource_mut::<InventoryState>()
            .ground_drop
            .selected
            .as_mut()
            .unwrap()
            .deadline = Instant::now();
        app.update();
        assert!(rx.try_recv().is_err());
        app.world_mut()
            .resource_mut::<InventoryState>()
            .request_world_drop(request);
        app.update();
        let second = app.world().resource::<Asked>().question().unwrap();
        assert_ne!(first, second);
        assert_eq!(app.world_mut().resource_mut::<Asked>().answer(first), None);
        app.world_mut().resource_mut::<Asked>().give(false);
        app.update();
        assert!(rx.try_recv().is_err());
        assert!(
            !app.world()
                .resource::<InventoryState>()
                .cursor_drop_hidden(app.world().resource::<OnlineState>().world())
        );
    }

    #[test]
    fn world_question_rechecks_escape_custody_coins_and_admission_before_yes() {
        for case in 0..5 {
            let mut online = OnlineState::new(true);
            testing::admit(&mut online, 7, testing::player(9));
            testing::empty_carried_coins(&mut online);
            testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![cursor(17)]));
            let request = online.world().prepare_cursor_drop().unwrap();
            let (tx, rx) = mpsc::sync_channel(4);
            let mut app = crate::testing::app();
            app.insert_resource(online)
                .insert_resource(Outbox::new(Some(tx)))
                .add_systems(Update, input);
            app.world_mut()
                .resource_mut::<InventoryState>()
                .request_world_drop(request);
            app.update();
            assert!(app.world().resource::<Asked>().question().is_some());
            match case {
                0 => {
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .press(KeyCode::Escape);
                }
                1 => {
                    let mut online = app.world_mut().resource_mut::<OnlineState>();
                    testing::inventory(&mut online, InventoryUpdate::Set(vec![cursor(18)]));
                    testing::inventory(&mut online, InventoryUpdate::Set(vec![cursor(17)]));
                }
                2 => {
                    testing::news(
                        &mut app.world_mut().resource_mut::<OnlineState>(),
                        [WorldEvent::Coins(eq_client_core::Coins {
                            copper: 1,
                            ..Default::default()
                        })],
                    );
                }
                3 => {
                    testing::admit(
                        &mut app.world_mut().resource_mut::<OnlineState>(),
                        8,
                        testing::player(9),
                    );
                }
                _ => {
                    let window = app
                        .world_mut()
                        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
                        .single(app.world())
                        .unwrap();
                    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
                }
            }
            app.world_mut().resource_mut::<Asked>().give(true);
            app.update();
            assert!(rx.try_recv().is_err(), "case {case}");
            assert!(app.world().resource::<Asked>().question().is_none());
        }
    }
}
