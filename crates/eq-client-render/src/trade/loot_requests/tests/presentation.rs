//! Production receive, reconciliation and both presentation branches, frame by frame.
mod range;
use super::*;
use crate::{outbox::Outbox, skinned::Skinned, trade::Panel, windows::WindowId};
use eq_client_core::{Coins, GameDialect, inventory::InventoryUpdate, loot::LootResponse};

fn host(skinned: bool, dialect: GameDialect) -> Host {
    let mut host = Host::new();
    host.app
        .world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .installed_client = match dialect {
        GameDialect::Titanium => eq_client_assets::ui::InstalledClient::Titanium,
        GameDialect::EqMac => eq_client_assets::ui::InstalledClient::EqMac,
        GameDialect::Rof2 => eq_client_assets::ui::InstalledClient::Rof2,
    };
    crate::online::testing::generation(
        &mut host.app.world_mut().resource_mut::<OnlineState>(),
        dialect,
    );
    host.app
        .insert_resource(Skinned::of(if skinned { &[WindowId::Loot] } else { &[] }))
        .add_systems(Update, crate::trade::present.after(crate::trade::input));
    host
}

fn variants() -> impl Iterator<Item = (bool, GameDialect)> {
    [false, true].into_iter().flat_map(|skinned| {
        [GameDialect::Titanium, GameDialect::EqMac, GameDialect::Rof2]
            .into_iter()
            .map(move |dialect| (skinned, dialect))
    })
}

fn frames(host: &mut Host) -> Vec<Entity> {
    let world = host.app.world_mut();
    let mut query = world.query::<(Entity, &Panel, &WindowId)>();
    query
        .iter(world)
        .filter(|(_, panel, id)| panel.0 == crate::trade::Rows::Loot && **id == WindowId::Loot)
        .map(|(entity, ..)| entity)
        .collect()
}

fn grant(host: &mut Host) {
    host.hear(WorldEvent::Loot(LootUpdate::Opened {
        response: LootResponse::Normal,
        coins: Coins::default(),
    }));
}

#[test]
fn loot_grant_pending_escape_leaves_an_unrelated_visible_popup_first() {
    for (skinned, dialect) in variants() {
        let mut host = host(skinned, dialect);
        host.app
            .init_resource::<crate::windows::Stack>()
            .init_resource::<crate::windows::menu::Menu>()
            .init_resource::<crate::login::FrontEnd>()
            .add_systems(Update, crate::escape::route.before(crate::trade::input));
        host.open();
        assert_eq!(host.world().generation(), dialect);
        let modal = host
            .app
            .world_mut()
            .spawn((
                WindowId::Item,
                Node::default(),
                crate::windows::Frame::default(),
            ))
            .id();
        host.app
            .world_mut()
            .resource_mut::<crate::windows::Stack>()
            .raise(WindowId::Item);
        host.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        host.app.update();
        host.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        assert_eq!(
            *host.app.world().resource::<crate::escape::Escape>(),
            crate::escape::Escape::Close(WindowId::Item)
        );
        assert!(host.commands.try_recv().is_err());
        assert!(!host.world().loot().unwrap().granted);
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        // The unrelated modal's owner closes it; the next Escape may cancel loot.
        host.app.world_mut().despawn(modal);
        host.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        host.app.update();
        host.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        assert!(matches!(
            host.commands.try_recv().unwrap(),
            ClientCommand::EndLoot { corpse_id: 9, .. }
        ));
        assert!(host.world().loot().is_none());
    }
}

#[test]
fn loot_grant_pending_escape_never_answers_or_closes_an_unrelated_server_dialog() {
    for (skinned, dialect) in variants() {
        let mut host = host(skinned, dialect);
        host.app
            .init_resource::<crate::windows::Stack>()
            .init_resource::<crate::windows::menu::Menu>()
            .init_resource::<crate::login::FrontEnd>()
            .add_systems(Update, crate::escape::route.before(crate::trade::input));
        host.open();
        assert_eq!(host.world().generation(), dialect);
        assert!(!WindowId::ServerDialog.describe().closes_on_escape);
        let dialog = host
            .app
            .world_mut()
            .spawn((
                WindowId::ServerDialog,
                Node::default(),
                crate::windows::Frame::default(),
            ))
            .id();
        host.app
            .world_mut()
            .resource_mut::<crate::windows::Stack>()
            .raise(WindowId::ServerDialog);
        host.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        host.app.update();
        host.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        assert_eq!(
            *host.app.world().resource::<crate::escape::Escape>(),
            crate::escape::Escape::Close(WindowId::Loot)
        );
        assert!(matches!(
            host.commands.try_recv().unwrap(),
            ClientCommand::EndLoot { corpse_id: 9, .. }
        ));
        assert!(host.commands.try_recv().is_err());
        assert!(host.app.world().get_entity(dialog).is_ok());
        assert!(host.world().loot().is_none());
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
    }
}

fn inventory_redraw(host: &mut Host) {
    host.hear(WorldEvent::Inventory(InventoryUpdate::Snapshot(Vec::new())));
}

#[test]
fn loot_grant_pending_and_denied_never_create_a_frame_even_after_redraw() {
    for (skinned, dialect) in variants() {
        let mut host = host(skinned, dialect);
        let opening = host.open();
        assert_eq!(host.world().generation(), dialect);
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        host.result(&opening, LootRequestStatus::Sent);
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        inventory_redraw(&mut host);
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        // Neither early item records nor a completion echo invent permission.
        host.list();
        assert!(!host.world().loot().unwrap().granted);
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        host.hear(WorldEvent::Loot(LootUpdate::Opened {
            response: LootResponse::NotAtThisTime,
            coins: Coins::default(),
        }));
        assert_eq!(host.notices(), 1);
        assert!(host.world().loot().is_none());
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        inventory_redraw(&mut host);
        host.app.update();
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        assert!(host.commands.try_recv().is_err());
    }
}

#[test]
fn loot_grant_empty_and_delayed_items_open_without_inventory_or_listing_changes() {
    for (skinned, dialect) in variants() {
        let mut host = host(skinned, dialect);
        host.open();
        assert_eq!(host.world().generation(), dialect);
        let revision = host.world().inventory().revision();
        grant(&mut host);
        let shown = frames(&mut host);
        assert_eq!(shown.len(), 1);
        assert_eq!(host.world().inventory().revision(), revision);
        assert!(!host.world().loot().unwrap().listed);
        assert!(host.world().loot().unwrap().items.is_empty());
        host.app.update();
        assert_eq!(frames(&mut host), shown);
        host.hear(WorldEvent::Loot(LootUpdate::Listed { corpse_id: 9 }));
        assert_eq!(frames(&mut host), shown);
        inventory_redraw(&mut host);
        assert_eq!(frames(&mut host), shown);
        host.hear(WorldEvent::Loot(LootUpdate::Closed));
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        host.open();
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        grant(&mut host);
        let delayed = frames(&mut host);
        assert_eq!(delayed.len(), 1);
        host.hear(WorldEvent::Loot(LootUpdate::Item {
            place: 0,
            item: Box::new(crate::preview::items().remove(0)),
        }));
        assert_eq!(frames(&mut host), delayed);
        assert_eq!(host.world().loot().unwrap().items.len(), 1);
        host.hear(WorldEvent::Loot(LootUpdate::Listed { corpse_id: 9 }));
        assert_eq!(frames(&mut host), delayed);
    }
}

#[test]
fn loot_grant_reopening_another_corpse_waits_for_its_own_answer() {
    for (skinned, dialect) in variants() {
        let mut host = host(skinned, dialect);
        host.open();
        grant(&mut host);
        host.list();
        assert_eq!(frames(&mut host).len(), 1);
        assert!(matches!(
            host.click(Action::EndLoot),
            ClientCommand::EndLoot { corpse_id: 9, .. }
        ));
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        let mut corpse = crate::online::testing::pet(10, 7);
        corpse.name = "SecondCorpse".into();
        corpse.pet_owner = None;
        corpse.kind = eq_client_core::SpawnKind::NpcCorpse;
        {
            let mut online = host.app.world_mut().resource_mut::<OnlineState>();
            crate::online::testing::spawns(&mut online, vec![corpse]);
            online.select_target(Some(10));
        }
        host.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyL);
        host.app.update();
        host.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        assert!(matches!(
            host.commands.try_recv().unwrap(),
            ClientCommand::Loot { corpse_id: 10, .. }
        ));
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        host.hear(WorldEvent::Loot(LootUpdate::Listed { corpse_id: 9 }));
        assert!(!host.world().loot().unwrap().listed);
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        grant(&mut host);
        assert_eq!(frames(&mut host).len(), 1);
        assert_eq!(
            host.app.world().resource::<TradeState>().corpse(),
            Some("SecondCorpse")
        );
    }
}

#[test]
fn loot_grant_pending_escape_cancels_without_a_frame_or_a_late_reopen() {
    for (skinned, dialect) in variants() {
        let mut host = host(skinned, dialect);
        host.app
            .init_resource::<crate::windows::Stack>()
            .init_resource::<crate::windows::menu::Menu>()
            .init_resource::<crate::login::FrontEnd>()
            .add_systems(Update, crate::escape::route.before(crate::trade::input));
        host.open();
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        host.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        host.app.update();
        host.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        assert_eq!(
            *host.app.world().resource::<crate::escape::Escape>(),
            crate::escape::Escape::Close(WindowId::Loot)
        );
        assert!(matches!(
            host.commands.try_recv().unwrap(),
            ClientCommand::EndLoot { corpse_id: 9, .. }
        ));
        assert!(host.world().loot().is_none());
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        grant(&mut host);
        inventory_redraw(&mut host);
        assert!(host.world().loot().is_none());
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        assert!(host.commands.try_recv().is_err());
    }
}

#[test]
fn loot_grant_empty_automatic_loot_waits_for_listing_before_closing() {
    for skinned in [false, true] {
        let mut host = host(skinned, GameDialect::Titanium);
        host.open();
        grant(&mut host);
        assert_eq!(frames(&mut host).len(), 1);
        let button = host
            .app
            .world_mut()
            .spawn((Button, Action::TakeAll, Interaction::None))
            .id();
        crate::clicks::testing::click(&mut host.app, button);
        assert!(host.commands.try_recv().is_err());
        assert_eq!(frames(&mut host).len(), 1);
        host.hear(WorldEvent::Loot(LootUpdate::Listed { corpse_id: 9 }));
        assert!(matches!(
            host.commands.try_recv().unwrap(),
            ClientCommand::EndLoot { corpse_id: 9, .. }
        ));
        assert!(host.world().loot().is_none());
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
    }
}

#[test]
fn loot_grant_failed_close_preserves_the_frame_and_a_new_close_can_end_it() {
    for (skinned, dialect) in variants() {
        let mut host = host(skinned, dialect);
        host.open();
        grant(&mut host);
        let shown = frames(&mut host);
        let (sender, _receiver) = mpsc::sync_channel(0);
        host.app.insert_resource(Outbox::new(Some(sender)));
        let button = host
            .app
            .world_mut()
            .spawn((Button, Action::EndLoot, Interaction::None))
            .id();
        crate::clicks::testing::click(&mut host.app, button);
        assert!(host.world().loot().unwrap().granted);
        assert_eq!(frames(&mut host), shown);
        let (sender, receiver) = mpsc::sync_channel(2);
        host.app.insert_resource(Outbox::new(Some(sender)));
        crate::clicks::testing::click(&mut host.app, button);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ClientCommand::EndLoot { corpse_id: 9, .. }
        ));
        assert!(host.world().loot().is_none());
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
    }
}
