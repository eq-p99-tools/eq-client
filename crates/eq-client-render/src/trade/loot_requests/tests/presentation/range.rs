use super::*;
use crate::trade::loot_range::Model;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Metadata(PathBuf);
impl Metadata {
    fn new(text: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "eq-loot-range-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(path.join("Resources")).unwrap();
        std::fs::write(path.join("Resources/moddat.ini"), text).unwrap();
        Self(path)
    }
}
impl Drop for Metadata {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0.join("Resources/moddat.ini"));
        let _ = std::fs::remove_dir(self.0.join("Resources"));
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn prepared(skinned: bool, classic: u32, reference: Option<f32>) -> (Host, Metadata) {
    let model = eq_client_core::races::model(1, 0).unwrap();
    let text = reference.map_or_else(String::new, |offset| {
        format!("[{model}]\nROffset={offset}\n")
    });
    let metadata = Metadata::new(&text);
    let mut host = host(skinned, GameDialect::Titanium);
    host.app
        .world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .eq_directory = Some(metadata.0.clone());
    let mut corpse = crate::online::testing::pet(9, 0);
    corpse.pet_owner = None;
    corpse.kind = eq_client_core::SpawnKind::NpcCorpse;
    corpse.race = 1;
    corpse.gender = 0;
    corpse.size = 6.0;
    corpse.position = eq_client_core::WorldPosition::default();
    crate::online::testing::spawns(
        &mut host.app.world_mut().resource_mut::<OnlineState>(),
        vec![corpse],
    );
    host.hear(WorldEvent::ZoneDescription(
        eq_network_game::zones::ZoneDescription {
            long_name: "Synthetic".into(),
            zone_type: 0,
            gravity: 0.4,
            safe_point: [0.0; 3],
            max_z: 0.0,
            underworld: -1000.0,
            min_clip: 50.0,
            max_clip: 2000.0,
            experience_multiplier: 1.0,
            weather: default(),
            music: None,
            sky_lock: None,
            graveyard_minutes: None,
            classic_model_scale_flag: Some(classic),
        },
    ));
    host.open();
    grant(&mut host);
    host.list();
    let corpse = host.world().spawn(9).unwrap();
    let scale = eq_client_assets::scale::TITANIUM.drawn_scale(
        1,
        0,
        6.0,
        eq_client_assets::scale::ZoneScale {
            classic_reduced: classic != 0,
        },
    );
    let model = Model::new(9, corpse.revision, Some(classic), model, scale);
    host.app.world_mut().spawn(model);
    (host, metadata)
}

fn position(host: &mut Host, x: f32, y: f32, z: f32) {
    host.hear(WorldEvent::Position {
        spawn_id: 7,
        position: eq_client_core::WorldPosition {
            x,
            y,
            z,
            heading: 0.0,
        },
        velocity: [0.0; 3],
        forward: 0.0,
    });
}

fn check(host: &mut Host, now: Instant) {
    host.app
        .world_mut()
        .resource_scope(|world, mut trade: Mut<TradeState>| {
            let model = world.query::<&Model>().iter(world).next().cloned();
            trade.range.check(
                world.resource::<OnlineState>().world(),
                &world.resource::<crate::ViewerSettings>().0,
                world.resource::<Outbox>(),
                (true, model.as_ref()),
                now,
            );
        });
}

#[test]
fn range_close_uses_model_reference_and_classic_scale_at_the_500ms_boundary() {
    for skinned in [false, true] {
        for classic in [0, 1] {
            for reference in [None, Some(1.0), Some(5.0)] {
                let (mut host, _metadata) = prepared(skinned, classic, reference);
                let scale = 1.2 / if classic == 0 { 1.0 } else { 1.3 };
                let limit = reference.unwrap_or(3.125) * scale * 1.5 + 20.0;
                let now = Instant::now();
                position(&mut host, limit, 0.0, 10000.0);
                check(&mut host, now);
                assert!(
                    host.commands.try_recv().is_err(),
                    "exact boundary and height alone keep open"
                );
                position(&mut host, limit + 0.001, 0.0, -10000.0);
                check(&mut host, now + Duration::from_millis(499));
                assert!(host.commands.try_recv().is_err());
                check(&mut host, now + Duration::from_millis(500));
                assert!(matches!(
                    host.commands.try_recv().unwrap(),
                    ClientCommand::EndLoot {
                        session_id: 5,
                        corpse_id: 9
                    }
                ));
                assert!(host.world().loot().unwrap().granted);
                assert_eq!(frames(&mut host).len(), 1);
                assert_eq!(host.notices(), 0, "the native range path prints nothing");
            }
        }
    }
}

#[test]
fn queued_range_close_preserves_frames_and_custody_until_server_close_and_never_retries() {
    for skinned in [false, true] {
        let (mut host, _metadata) = prepared(skinned, 0, None);
        let shown = frames(&mut host);
        let inventory = host.world().inventory().items().clone();
        let rows = host.world().loot().unwrap().items.clone();
        let now = Instant::now();
        position(&mut host, 30.0, 0.0, 0.0);
        check(&mut host, now);
        let close = host.commands.try_recv().unwrap();
        assert!(matches!(close, ClientCommand::EndLoot { .. }));
        for seconds in 1..5 {
            check(&mut host, now + Duration::from_secs(seconds));
        }
        position(&mut host, 0.0, 0.0, 0.0);
        host.refuse(&close, "No transport success inferred");
        check(&mut host, now + Duration::from_secs(5));
        for action in [Action::Take(0), Action::TakeAll, Action::EndLoot] {
            let button = host
                .app
                .world_mut()
                .spawn((Button, action, Interaction::None))
                .id();
            crate::clicks::testing::click(&mut host.app, button);
        }
        assert!(host.commands.try_recv().is_err());
        assert_eq!(frames(&mut host), shown);
        assert_eq!(host.world().loot().unwrap().items, rows);
        assert_eq!(host.world().inventory().items(), &inventory);
        assert_eq!(host.world().target().selected, Some(9));
        host.hear(WorldEvent::Loot(LootUpdate::Closed));
        assert_eq!(frames(&mut host), Vec::<Entity>::new());
        assert!(host.world().loot().is_none());
        assert_eq!(host.world().target().selected, None);
        assert_eq!(host.world().inventory().items(), &inventory);
        let mut arrived = crate::preview::items().remove(0);
        arrived.slot = eq_client_core::inventory::InventorySlot(23);
        host.hear(WorldEvent::Inventory(InventoryUpdate::Set(vec![
            arrived.clone(),
        ])));
        assert_eq!(host.world().inventory().items()[&arrived.slot], arrived);
        host.app
            .world_mut()
            .resource_mut::<OnlineState>()
            .select_target(Some(9));
        host.open();
        grant(&mut host);
        position(&mut host, 30.0, 0.0, 0.0);
        check(&mut host, now + Duration::from_secs(6));
        assert!(matches!(
            host.commands.try_recv().unwrap(),
            ClientCommand::EndLoot { corpse_id: 9, .. }
        ));
    }
}

#[test]
fn range_close_blocks_a_preexisting_no_drop_yes_and_the_actual_input_uses_the_same_rule() {
    for skinned in [false, true] {
        let (mut host, _metadata) = prepared(skinned, 0, None);
        host.app
            .init_resource::<crate::confirm::Asked>()
            .add_systems(
                Update,
                crate::trade::no_drop::confirm.after(crate::trade::input),
            );
        let mut item = crate::preview::items().remove(0);
        item.details.flags.push("NO DROP".into());
        host.hear(WorldEvent::Loot(LootUpdate::Item {
            place: 0,
            item: Box::new(item),
        }));
        let button = host
            .app
            .world_mut()
            .spawn((Button, Action::Take(0), Interaction::None))
            .id();
        crate::clicks::testing::click(&mut host.app, button);
        assert!(
            host.app
                .world()
                .resource::<crate::confirm::Asked>()
                .question()
                .is_some()
        );
        assert!(host.commands.try_recv().is_err());
        let frame = frames(&mut host)[0];
        host.app
            .world_mut()
            .entity_mut(frame)
            .insert(InheritedVisibility::VISIBLE);
        host.app.add_systems(
            Update,
            crate::trade::loot_range::input.before(crate::trade::input),
        );
        position(&mut host, 30.0, 0.0, 0.0);
        assert!(matches!(
            host.commands.try_recv().unwrap(),
            ClientCommand::EndLoot { corpse_id: 9, .. }
        ));
        host.app
            .world_mut()
            .resource_mut::<crate::confirm::Asked>()
            .give(true);
        host.app.update();
        assert!(host.commands.try_recv().is_err());
        assert!(host.world().loot().unwrap().items.contains_key(&0));
        assert_eq!(frames(&mut host), [frame]);
    }
}

#[test]
fn unknown_basis_hidden_frame_and_unread_generations_send_no_distance_close() {
    for client in [
        eq_client_assets::ui::InstalledClient::Titanium,
        eq_client_assets::ui::InstalledClient::EqMac,
        eq_client_assets::ui::InstalledClient::Rof2,
    ] {
        let (mut host, _metadata) = prepared(false, 0, None);
        host.app
            .world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = client;
        position(&mut host, 100.0, 0.0, 0.0);
        let now = Instant::now();
        host.app
            .world_mut()
            .resource_scope(|world, mut trade: Mut<TradeState>| {
                trade.range.check(
                    world.resource::<OnlineState>().world(),
                    &world.resource::<crate::ViewerSettings>().0,
                    world.resource::<Outbox>(),
                    (false, None),
                    now,
                );
            });
        assert!(host.commands.try_recv().is_err());
        host.app
            .world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = None;
        host.app.world_mut().resource_mut::<TradeState>().range = default();
        check(&mut host, now + Duration::from_secs(1));
        assert!(host.commands.try_recv().is_err());
        assert!(host.world().loot().unwrap().granted);
    }
}

#[test]
fn range_uses_the_prepared_actor_model_and_refuses_a_missing_or_stale_actor() {
    let (mut host, metadata) = prepared(false, 0, None);
    let world = host.app.world_mut();
    let old = world
        .query_filtered::<Entity, With<Model>>()
        .iter(world)
        .next()
        .unwrap();
    world.entity_mut(old).despawn();
    position(&mut host, 30.0, 0.0, 0.0);
    let now = Instant::now();
    check(&mut host, now);
    assert!(
        host.commands.try_recv().is_err(),
        "a marker is not an actor offset"
    );
    let revision = host.world().spawn(9).unwrap().revision;
    let stale = host
        .app
        .world_mut()
        .spawn(Model::new(9, revision + 1, Some(0), "OtherModel", 1.2))
        .id();
    check(&mut host, now + Duration::from_millis(500));
    assert!(host.commands.try_recv().is_err());
    host.app.world_mut().entity_mut(stale).despawn();
    // A stand-in's prepared model can differ from the spawn race's model.
    // Its own authored reference, not a race-name lookup, supplies native S.
    std::fs::write(
        metadata.0.join("Resources/moddat.ini"),
        "[OtherModel]\nROffset=1\n",
    )
    .unwrap();
    host.app
        .world_mut()
        .spawn(Model::new(9, revision, Some(0), "OtherModel", 1.2));
    position(&mut host, 22.0, 0.0, 0.0);
    check(&mut host, now + Duration::from_secs(1));
    assert!(matches!(
        host.commands.try_recv().unwrap(),
        ClientCommand::EndLoot { corpse_id: 9, .. }
    ));
}

#[test]
fn checked_spawn_offset_survives_render_culling_but_not_a_replacement() {
    for replaced in [false, true] {
        let (mut host, _metadata) = prepared(false, 0, None);
        let now = Instant::now();
        check(&mut host, now); // Qualify the current actor while within reach.
        let world = host.app.world_mut();
        let actor = world
            .query_filtered::<Entity, With<Model>>()
            .iter(world)
            .next()
            .unwrap();
        world.entity_mut(actor).despawn();
        if replaced {
            let mut replacement = host.world().spawn(9).unwrap().state.clone();
            replacement.name = "Replacement".into();
            crate::online::testing::spawns(
                &mut host.app.world_mut().resource_mut::<OnlineState>(),
                vec![replacement],
            );
        }
        position(&mut host, 30.0, 0.0, 0.0);
        check(&mut host, now + Duration::from_millis(500));
        assert_eq!(host.commands.try_recv().is_ok(), !replaced);
    }
}
