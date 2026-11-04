//! Actual outbox commands, typed updates, core replies and rendered notices.
use super::*;
use crate::trade::{Action, TradeState};
use bevy::{mesh::skinning::SkinnedMeshInverseBindposes, prelude::*};
use eq_client_core::{ClientCommand, WorldEvent, WorldUpdate, chat::ChatTab, loot::LootUpdate};
use std::sync::{Mutex, mpsc};

struct Host {
    app: App,
    updates: mpsc::Sender<WorldUpdate>,
    commands: mpsc::Receiver<ClientCommand>,
}

impl Host {
    fn new() -> Self {
        let mut online = OnlineState::new(false);
        crate::online::testing::admit(&mut online, 5, crate::online::testing::player(7));
        let mut corpse = crate::online::testing::pet(9, 7);
        corpse.name = "Corpse".into();
        corpse.pet_owner = None;
        corpse.kind = eq_client_core::SpawnKind::NpcCorpse;
        crate::online::testing::spawns(&mut online, vec![corpse]);
        online.select_target(Some(9));
        let (commands, receiver) = mpsc::sync_channel(8);
        let (updates, news) = mpsc::channel();
        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(crate::online::Updates(Mutex::new(Some(news))))
            .insert_resource(crate::outbox::Outbox::new(Some(commands)))
            .init_resource::<crate::ViewerSettings>()
            .init_resource::<crate::hud::HudState>()
            .init_resource::<crate::hud::messages::Messages>()
            .init_resource::<crate::motion::Controls>()
            .init_resource::<crate::chat::ChatState>()
            .init_resource::<crate::notices::Lines>()
            .init_resource::<crate::combat::CombatState>()
            .init_resource::<TradeState>()
            .init_resource::<crate::items::ItemState>()
            .init_resource::<crate::inventory::InventoryState>()
            .init_resource::<crate::sounds::Sounds>()
            .init_resource::<crate::windows::Shown>()
            .init_resource::<crate::escape::Escape>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(
                Update,
                (
                    crate::online::receive,
                    crate::online::carry_as,
                    crate::trade::input,
                )
                    .chain(),
            );
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = eq_client_assets::ui::InstalledClient::EqMac;
        crate::keys::testing::install(&mut app);
        // The direct loot command has no native default; choose one here.
        crate::keys::testing::bind(
            &mut app,
            crate::keys::Act::Loot,
            crate::keys::Chord::key(KeyCode::KeyL),
        );
        crate::clicks::testing::install(&mut app);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        Self {
            app,
            updates,
            commands: receiver,
        }
    }

    fn world(&self) -> &eq_client_core::world::ClientWorld {
        self.app.world().resource::<OnlineState>().world()
    }

    fn pending(&self) -> Option<PendingLoot> {
        self.app
            .world()
            .resource::<TradeState>()
            .loot
            .as_ref()?
            .pending
    }

    fn notices(&self) -> usize {
        self.app
            .world()
            .resource::<crate::chat::ChatState>()
            .history
            .lines(ChatTab::System)
            .len()
    }

    fn hear(&mut self, event: WorldEvent) {
        self.updates.send(WorldUpdate::Game(event)).unwrap();
        self.app.update();
    }

    fn open(&mut self) -> ClientCommand {
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyL);
        self.app.update();
        let mut keys = self.app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::KeyL);
        keys.clear();
        let command = self.commands.try_recv().unwrap();
        assert!(matches!(command, ClientCommand::Loot { corpse_id: 9, .. }));
        command
    }

    fn list(&mut self) {
        self.hear(WorldEvent::Loot(LootUpdate::Item {
            place: 0,
            item: Box::new(crate::preview::items().remove(0)),
        }));
        self.hear(WorldEvent::Loot(LootUpdate::Listed { corpse_id: 9 }));
    }

    fn click(&mut self, action: Action) -> ClientCommand {
        let button = self
            .app
            .world_mut()
            .spawn((Button, action, Interaction::None))
            .id();
        crate::clicks::testing::click(&mut self.app, button);
        self.commands.try_recv().unwrap()
    }

    fn take(&mut self) -> ClientCommand {
        self.click(Action::Take(0))
    }

    fn result(&mut self, command: &ClientCommand, status: LootRequestStatus) {
        self.hear(result(command, status));
    }

    fn refuse(&mut self, command: &ClientCommand, reason: &str) {
        self.result(command, LootRequestStatus::Refused(reason.into()));
    }
}

fn result(command: &ClientCommand, status: LootRequestStatus) -> WorldEvent {
    let request = match *command {
        ClientCommand::Loot { corpse_id, .. } => LootRequest::Open { corpse_id },
        ClientCommand::LootItem {
            corpse_id,
            own_id,
            place,
            auto,
            ..
        } => LootRequest::Item {
            corpse_id,
            own_id,
            place,
            auto,
        },
        ClientCommand::EndLoot { corpse_id, .. } => LootRequest::Close { corpse_id },
        _ => panic!("expected a loot action"),
    };
    WorldEvent::LootRequest {
        session_id: command.session_id().unwrap(),
        created: command.created(),
        request,
        status,
    }
}

#[test]
fn a_matching_open_refusal_closes_only_its_local_opening_and_says_it_once() {
    let mut host = Host::new();
    let old = host.open();
    host.refuse(&old, "Opening refused");
    assert!(host.world().loot().is_none());
    assert!(host.app.world().resource::<TradeState>().loot.is_none());
    assert_eq!(host.notices(), 1);
    host.refuse(&old, "Duplicate must not be presented");
    assert_eq!(host.notices(), 1);
    let new = host.open();
    assert_ne!(old.created(), new.created());
    host.refuse(&old, "Older opening must not be presented");
    assert!(host.world().loot().is_some());
    assert_eq!(host.notices(), 1);
    host.refuse(&new, "New opening refused");
    assert!(host.world().loot().is_none());
    assert_eq!(host.notices(), 2);
}

#[test]
fn every_original_item_field_and_creation_time_must_match_before_releasing_pending() {
    let mut host = Host::new();
    host.open();
    host.list();
    let original = host.take();
    let pending = host.pending();
    for case in 0..7 {
        let mut other = original.clone();
        let ClientCommand::LootItem {
            session_id,
            corpse_id,
            own_id,
            place,
            auto,
            created,
        } = &mut other
        else {
            panic!("expected a queued item action");
        };
        match case {
            0 => *session_id = 4,
            1 => *corpse_id = 10,
            2 => *own_id = 8,
            3 => *place = 1,
            4 => *auto = false,
            5 => {
                *created = created
                    .checked_sub(std::time::Duration::from_secs(1))
                    .unwrap();
            }
            6 => *created += std::time::Duration::from_secs(1),
            _ => unreachable!(),
        }
        host.refuse(&other, "Unrelated refusal");
        assert_eq!(host.pending(), pending, "case {case}");
        assert_eq!(host.notices(), 0, "case {case}");
    }
    host.hear(WorldEvent::LootRequest {
        session_id: 5,
        created: None,
        request: pending.unwrap().request,
        status: LootRequestStatus::Refused("Missing correlation".into()),
    });
    assert_eq!(host.pending(), pending);
    assert_eq!(host.notices(), 0);
}

#[test]
fn local_item_refusal_preserves_rows_and_holdings_and_allows_a_deliberate_retry() {
    let mut host = Host::new();
    host.open();
    host.list();
    let request = host.take();
    let rows = host.world().loot().unwrap().items.clone();
    let inventory = host.world().inventory().items().clone();
    let coins = host.world().coins().copied();
    host.app
        .world_mut()
        .resource_mut::<TradeState>()
        .loot
        .as_mut()
        .unwrap()
        .loot_all = true;
    host.refuse(&request, "Item refused locally");
    assert!(host.pending().is_none());
    assert!(
        !host
            .app
            .world()
            .resource::<TradeState>()
            .loot
            .as_ref()
            .unwrap()
            .loot_all
    );
    assert_eq!(host.world().loot().unwrap().items, rows);
    assert_eq!(host.world().inventory().items(), &inventory);
    assert_eq!(host.world().coins().copied(), coins);
    assert_eq!(host.notices(), 1);
    assert!(host.commands.try_recv().is_err());
    let retry = host.take();
    assert_ne!(request.created(), retry.created());
    let pending = host.pending();
    host.refuse(&request, "Duplicate older refusal");
    host.result(&request, LootRequestStatus::Sent);
    assert_eq!(host.pending(), pending);
    assert_eq!(host.notices(), 1);
    host.refuse(&retry, "Retry refused locally");
    assert!(host.pending().is_none());
    assert_eq!(host.notices(), 2);
    assert_eq!(host.world().loot().unwrap().items, rows);
}

#[test]
fn local_submission_keeps_exclusion_and_receipt_is_still_not_delivery() {
    let mut host = Host::new();
    let open = host.open();
    host.result(&open, LootRequestStatus::Sent);
    host.refuse(&open, "Impossible duplicate disposition");
    assert!(host.world().loot().is_some());
    host.list();
    let take = host.take();
    let inventory = host.world().inventory().items().clone();
    let coins = host.world().coins().copied();
    host.result(&take, LootRequestStatus::Sent);
    host.result(&take, LootRequestStatus::Sent);
    host.refuse(&take, "Impossible duplicate item disposition");
    assert!(host.pending().unwrap().submitted);
    assert!(host.world().loot().unwrap().items.contains_key(&0));
    assert_eq!(host.notices(), 0);
    assert!(host.commands.try_recv().is_err());
    host.hear(WorldEvent::Loot(LootUpdate::Acknowledged { place: 0 }));
    assert!(host.pending().is_none());
    assert!(host.world().loot().unwrap().items.is_empty());
    assert_eq!(host.world().inventory().items(), &inventory);
    assert_eq!(host.world().coins().copied(), coins);
    assert!(host.commands.try_recv().is_err());
    host.hear(WorldEvent::Loot(LootUpdate::Closed));
    assert!(host.world().loot().is_none());
    assert_eq!(host.world().inventory().items(), &inventory);
}

#[test]
fn close_refusal_only_says_the_reason_and_never_changes_a_newer_window_or_request() {
    let mut host = Host::new();
    host.open();
    let close = host.click(Action::EndLoot);
    assert!(close.created().is_none());
    host.result(&close, LootRequestStatus::Sent);
    host.refuse(&close, "Close refused locally");
    assert!(
        host.world().loot().is_none(),
        "a refusal cannot reopen a closed view"
    );
    host.open();
    host.list();
    host.take();
    let pending = host.pending();
    let rows = host.world().loot().unwrap().items.clone();
    host.refuse(&close, "Close refused locally");
    assert_eq!(
        host.notices(),
        1,
        "normal duplicate-notice handling still applies"
    );
    assert_eq!(host.pending(), pending);
    assert_eq!(host.world().loot().unwrap().items, rows);
    assert!(host.app.world().resource::<TradeState>().loot.is_some());
}
