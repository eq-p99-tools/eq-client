//! One explicit confirmation for one exact cursor instance. Scripts click these
//! same controls and answer the ordinary confirmation dialog.
//!
//! With coins on the cursor and no item, the same control destroys the coins
//! (checklist 6.512): one confirmation, then one command per kind, the most
//! valuable kind first. How the Titanium program starts a coin destroy, and
//! whether it asks first, is unread (R49-11); the confirmation is ours, a
//! safeguard kept as item destroy keeps its own.
use super::InventoryState;
use crate::{
    confirm::{Asked, Question},
    online::OnlineState,
    outbox::Outbox,
};
use bevy::prelude::*;
use eq_client_core::{
    Capability, ClientCommand, Coins,
    inventory::{InventoryDestroy, InventorySlot},
    money::{Coin, CoinDestroy, CoinPlace},
    world::ClientWorld,
};

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Select,
    Confirm,
    Cancel,
}

#[derive(Default)]
pub(super) struct Destruction {
    selected: Option<InventoryDestroy>,
    /// The coins on the cursor the player was asked about, in this admission.
    coins: Option<CursorCoins>,
    pending: Option<(u64, u64)>,
}

/// Coins on the cursor, frozen when the player is asked to destroy them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CursorCoins {
    session_id: u64,
    coins: Coins,
}

impl CursorCoins {
    fn question(self) -> Question {
        Question::DestroyCoins {
            session_id: self.session_id,
            coins: self.coins,
        }
    }

    /// The coins still on the cursor, with no item there, in the same
    /// admission: what the player agreed to destroy.
    fn current(self, world: &ClientWorld) -> bool {
        world.session_id() == Some(self.session_id)
            && world.coins_in(CoinPlace::Cursor) == Some(self.coins)
            && !world
                .inventory()
                .items()
                .contains_key(&InventorySlot::CURSOR)
    }
}

/// The coins on the cursor when no item is there: what the Destroy control
/// destroys instead of an item. None with an item on the cursor (it goes as
/// it always has) or no coins.
fn cursor_coins(world: &ClientWorld) -> Option<CursorCoins> {
    let coins = world.coins_in(CoinPlace::Cursor)?;
    let item = world
        .inventory()
        .items()
        .contains_key(&InventorySlot::CURSOR);
    (!item && !coins.is_empty()).then_some(CursorCoins {
        session_id: world.session_id()?,
        coins,
    })
}

/// The coins in words, the most valuable kind first: "3 platinum and 2 gold".
pub(crate) fn coins_text(coins: Coins) -> String {
    let parts: Vec<String> = Coin::ALL
        .into_iter()
        .filter(|coin| coins.of(*coin) > 0)
        .map(|coin| format!("{} {}", coins.of(coin), coin_name(coin)))
        .collect();
    match parts.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

const fn coin_name(coin: Coin) -> &'static str {
    match coin {
        Coin::Platinum => "platinum",
        Coin::Gold => "gold",
        Coin::Silver => "silver",
        Coin::Copper => "copper",
    }
}

/// What a server that doesn't take destroying coins answers the control.
const COINS_UNOFFERED: &str = "This server does not take destroying coins.";

impl Destruction {
    pub(super) const fn selecting(&self) -> bool {
        self.selected.is_some() || self.coins.is_some()
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
        if self
            .destruction
            .coins
            .take()
            .is_some_and(|coins| asked.question() == Some(coins.question()))
        {
            asked.withdraw();
        }
    }

    /// Asks about the coins on the cursor, where the server takes it.
    fn ask_coin_destroy(
        &mut self,
        coins: CursorCoins,
        online: &OnlineState,
        asked: &mut Asked,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            online.world().can(Capability::DestroyingCoins),
            COINS_UNOFFERED
        );
        anyhow::ensure!(
            asked.ask(coins.question()),
            "Answer the current confirmation first"
        );
        self.actions.auto_store = false;
        self.actions.split = None;
        self.actions.message = format!(
            "Destroy {}? This cannot be undone.",
            coins_text(coins.coins)
        );
        self.destruction.coins = Some(coins);
        Ok(())
    }

    /// Destroys the coins the player agreed to, one command per kind, the
    /// most valuable first. The session takes each from the cursor and says
    /// where the coins are; a refusal is said by the outbox or the session.
    fn confirm_coins(
        &mut self,
        coins: CursorCoins,
        online: &OnlineState,
        outbox: &Outbox,
        asked: &mut Asked,
    ) -> anyhow::Result<()> {
        if asked.question() == Some(coins.question()) {
            asked.withdraw();
        }
        anyhow::ensure!(
            coins.current(online.world()),
            "The coins on the cursor changed; select them again"
        );
        // The session holds no destroy back (eq-network
        // `two_kinds_destroyed_back_to_back_are_both_sent_with_nothing_held`),
        // so every kind goes in this frame. Should one be refused here, the
        // kinds already sent are gone: the line says which coins stay.
        let mut sent = Coins::default();
        for coin in Coin::ALL {
            let amount = coins.coins.of(coin);
            if amount == 0 {
                continue;
            }
            if let Err(refusal) = outbox.post(online.world(), |stamp| ClientCommand::DestroyCoins {
                session_id: stamp.session_id,
                destroy: CoinDestroy { coin, amount },
                created: stamp.created,
            }) {
                let mut kept = coins.coins;
                for coin in Coin::ALL {
                    *kept.of_mut(coin) -= sent.of(coin);
                }
                anyhow::bail!(if sent.is_empty() {
                    format!("{}; nothing was destroyed", refusal.text())
                } else {
                    format!(
                        "{}; destroy sent for {}, but {} stays on the cursor",
                        refusal.text(),
                        coins_text(sent),
                        coins_text(kept)
                    )
                });
            }
            *sent.of_mut(coin) = amount;
        }
        self.actions.message = format!("Destroy sent for {}", coins_text(coins.coins));
        Ok(())
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
                if let Some(coins) = cursor_coins(online.world()) {
                    return self.ask_coin_destroy(coins, online, asked);
                }
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
                if let Some(coins) = self.destruction.coins.take() {
                    return self.confirm_coins(coins, online, outbox, asked);
                }
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
        || state.destruction.coins.is_none()
            && matches!(asked.question(), Some(Question::DestroyCoins { .. }))
    {
        asked.withdraw();
    }
    // Any revision change cancels consent, even a same-definition replacement.
    let invalid = state
        .destruction
        .selected
        .as_ref()
        .is_some_and(|request| online.world().check_cursor_destruction(request).is_err())
        || state
            .destruction
            .coins
            .is_some_and(|coins| !coins.current(online.world()))
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
    if let Some(coins) = state.destruction.coins
        && let Some(yes) = asked.answer(coins.question())
    {
        action = Some(if yes { Action::Confirm } else { Action::Cancel });
    }
    if !invalid && let Some(action) = action {
        if let Err(error) = state.destroy_action(action, &online, &outbox, &mut asked) {
            state.refuse(crate::outbox::window_line(&error));
        }
        state.revision = state.revision.wrapping_add(1);
    }
    let selecting = state.destruction.selecting();
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

    /// A player with these coins on the cursor and nothing else there, on a
    /// server that does or doesn't offer destroying coins.
    fn with_cursor_coins(cursor: eq_client_core::Coins, offered: bool) -> OnlineState {
        let mut online = OnlineState::new(true);
        let mut capabilities = vec![Capability::Inventory];
        if offered {
            capabilities.push(Capability::DestroyingCoins);
        }
        testing::news(
            &mut online,
            [
                WorldEvent::Entered {
                    capabilities,
                    choices: Vec::new(),
                    session_id: 7,
                    zone: "qeytoqrg".into(),
                    player: Box::new(testing::player(9)),
                    far_clip: None,
                    spell_landing_flag: None,
                    zone_id: 0,
                },
                WorldEvent::Coins(eq_client_core::Coins::default()),
                WorldEvent::CoinsElsewhere {
                    cursor,
                    bank: eq_client_core::Coins::default(),
                    given: eq_client_core::Coins::default(),
                    offered: eq_client_core::Coins::default(),
                },
            ],
        );
        testing::connect(&mut online, true);
        online.plan_inventory_as(Some(MoveRules::EqEmu));
        testing::inventory(&mut online, InventoryUpdate::Snapshot(Vec::new()));
        online
    }

    fn platinum_and_gold() -> eq_client_core::Coins {
        eq_client_core::Coins {
            platinum: 3,
            gold: 2,
            ..eq_client_core::Coins::default()
        }
    }

    /// The coin destroys sent, in order.
    fn destroyed(rx: &std::sync::mpsc::Receiver<ClientCommand>) -> Vec<(Coin, u32)> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .map(|command| match command {
                ClientCommand::DestroyCoins {
                    session_id: 7,
                    destroy,
                    ..
                } => (destroy.coin, destroy.amount),
                other => panic!("a coin destroy expected, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn coins_on_the_cursor_take_the_destroy_button_and_dialog_scripts_click() {
        // A script's `click("destroy")` and dialog answer press these same
        // entities (the Select-tagged button and the dialog's Yes and No), so
        // a script destroys coins exactly as it destroys an item today.
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let mut app = crate::testing::app();
        app.insert_resource(with_cursor_coins(platinum_and_gold(), true))
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
        let answer = |app: &mut App, yes| {
            app.world_mut()
                .spawn((Button, Interaction::None, crate::confirm::AnswerButton(yes)))
                .id()
        };
        let (no, yes) = (answer(&mut app, false), answer(&mut app, true));
        crate::clicks::testing::click(&mut app, select);
        assert_eq!(
            app.world().resource::<Asked>().question(),
            Some(Question::DestroyCoins {
                session_id: 7,
                coins: platinum_and_gold()
            })
        );
        assert_eq!(
            app.world().resource::<InventoryState>().actions.message,
            "Destroy 3 platinum and 2 gold? This cannot be undone."
        );
        // No keeps the coins and sends nothing.
        crate::clicks::testing::click(&mut app, no);
        app.update();
        assert!(
            !app.world()
                .resource::<InventoryState>()
                .destruction
                .selecting()
        );
        assert!(app.world().resource::<Asked>().question().is_none());
        assert_eq!(destroyed(&rx), []);
        // Yes sends one destroy per kind, the most valuable first, once.
        crate::clicks::testing::click(&mut app, select);
        crate::clicks::testing::click(&mut app, yes);
        app.update();
        assert_eq!(destroyed(&rx), [(Coin::Platinum, 3), (Coin::Gold, 2)]);
        app.update();
        assert_eq!(destroyed(&rx), []);
        assert!(
            !app.world()
                .resource::<InventoryState>()
                .destruction
                .selecting()
        );
    }

    #[test]
    fn a_kind_refused_after_another_was_sent_says_which_coins_stay() {
        // A queue with room for one command: platinum goes, gold is refused.
        let online = with_cursor_coins(platinum_and_gold(), true);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let outbox = Outbox::new(Some(tx));
        let (mut state, mut asked) = (InventoryState::default(), Asked::default());
        state
            .destroy_action(Action::Select, &online, &outbox, &mut asked)
            .unwrap();
        let refusal = state
            .destroy_action(Action::Confirm, &online, &outbox, &mut asked)
            .unwrap_err();
        assert_eq!(
            refusal.to_string(),
            "Too many requests at once; try again; destroy sent for 3 platinum, but 2 gold stays on the cursor"
        );
        assert_eq!(destroyed(&rx), [(Coin::Platinum, 3)]);
        assert!(!state.destruction.selecting());
    }

    #[test]
    fn a_server_that_does_not_take_destroying_coins_is_never_asked() {
        let online = with_cursor_coins(platinum_and_gold(), false);
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(tx));
        let (mut state, mut asked) = (InventoryState::default(), Asked::default());
        let refusal = state
            .destroy_action(Action::Select, &online, &outbox, &mut asked)
            .unwrap_err();
        assert_eq!(refusal.to_string(), COINS_UNOFFERED);
        assert!(asked.question().is_none());
        assert!(!state.destruction.selecting());
        assert_eq!(destroyed(&rx), []);
    }

    #[test]
    fn coins_that_change_after_the_question_are_not_destroyed() {
        let mut online = with_cursor_coins(platinum_and_gold(), true);
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(tx));
        let (mut state, mut asked) = (InventoryState::default(), Asked::default());
        state
            .destroy_action(Action::Select, &online, &outbox, &mut asked)
            .unwrap();
        testing::news(
            &mut online,
            [WorldEvent::CoinsElsewhere {
                cursor: eq_client_core::Coins {
                    gold: 2,
                    ..eq_client_core::Coins::default()
                },
                bank: eq_client_core::Coins::default(),
                given: eq_client_core::Coins::default(),
                offered: eq_client_core::Coins::default(),
            }],
        );
        assert!(
            state
                .destroy_action(Action::Confirm, &online, &outbox, &mut asked)
                .is_err()
        );
        assert!(asked.question().is_none());
        assert!(!state.destruction.selecting());
        assert_eq!(destroyed(&rx), []);
    }

    #[test]
    fn an_item_on_the_cursor_is_destroyed_as_before_whatever_coins_are_there() {
        let mut online = online();
        testing::news(
            &mut online,
            [
                WorldEvent::Coins(eq_client_core::Coins::default()),
                WorldEvent::CoinsElsewhere {
                    cursor: platinum_and_gold(),
                    bank: eq_client_core::Coins::default(),
                    given: eq_client_core::Coins::default(),
                    offered: eq_client_core::Coins::default(),
                },
            ],
        );
        let (tx, _rx) = std::sync::mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(tx));
        let (mut state, mut asked) = (InventoryState::default(), Asked::default());
        state
            .destroy_action(Action::Select, &online, &outbox, &mut asked)
            .unwrap();
        assert!(matches!(
            asked.question(),
            Some(Question::DestroyItem { session_id: 7, .. })
        ));
        assert!(state.destruction.coins.is_none());
    }

    #[test]
    fn coins_read_most_valuable_first() {
        let coins = |platinum, gold, silver, copper| eq_client_core::Coins {
            platinum,
            gold,
            silver,
            copper,
        };
        assert_eq!(coins_text(coins(0, 0, 0, 5)), "5 copper");
        assert_eq!(coins_text(coins(1, 0, 0, 5)), "1 platinum and 5 copper");
        assert_eq!(
            coins_text(coins(1, 2, 3, 4)),
            "1 platinum, 2 gold, 3 silver and 4 copper"
        );
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
