use super::*;
use crate::online::testing;
use eq_client_core::{ClientCommand, SpawnKind};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

fn player() -> eq_client_core::PlayerState {
    let mut player = testing::player(7);
    player.memorized_spells = [Some(14); 8];
    player.spell_refresh_ms = Some([0; 8]);
    player
}

fn app(capacity: usize) -> (App, mpsc::Receiver<ClientCommand>) {
    let mut app = App::new();
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 9, player());
    let (tx, rx) = mpsc::sync_channel(capacity);
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(tx)))
        .init_resource::<ChatState>()
        .init_resource::<crate::trade::TradeState>()
        .init_resource::<crate::spellbook::SpellNames>()
        .init_resource::<crate::hud::messages::Messages>()
        .init_resource::<crate::ViewerSettings>()
        .init_resource::<crate::keys::KeyMap>()
        .init_resource::<crate::options::OptionsState>()
        .add_systems(Update, run);
    (app, rx)
}

fn line(app: &mut App, text: &str, button: bool) -> Result<(), String> {
    let world = app.world_mut();
    let mut online = world.remove_resource::<OnlineState>().unwrap();
    let mut chat = world.remove_resource::<ChatState>().unwrap();
    let outbox = world.remove_resource::<Outbox>().unwrap();
    let qol = eq_client_core::qol::Settings::default();
    let result = if button {
        crate::chat::run_game_command(text, &mut online, &outbox, &mut chat, &qol);
        Ok(())
    } else {
        crate::chat::submit_typed_line(text, &mut chat, &mut online, &outbox, &qol)
    };
    world.insert_resource(online);
    world.insert_resource(chat);
    world.insert_resource(outbox);
    result
}

fn target(app: &mut App, kind: SpawnKind) {
    let mut online = app.world_mut().resource_mut::<OnlineState>();
    let mut spawn = testing::pet(42, 7);
    spawn.kind = kind;
    spawn.class = Some(41);
    testing::spawn_entry(&mut online, 42, spawn);
    online.select_target(Some(42));
}

#[test]
fn native_numeric_prefixes_support_eight_gems_and_report_the_ninth_gap() {
    let (app, _) = app(4);
    let world = app.world().resource::<OnlineState>().world();
    for (input, gem) in [
        ("/CaSt 1", 0),
        ("/cast +008 tail", 7),
        ("/cast 3suffix", 2),
        ("/cast 4294967297", 0),
    ] {
        assert_eq!(parse(input, world), Some(Ok(Action::Cast(gem))));
    }
    for input in ["/cast", "/cast no", "/cast -1", "/cast 0", "/cast 10"] {
        assert!(parse(input, world).unwrap().is_err());
    }
    assert!(
        parse("/cast 9", world)
            .unwrap()
            .unwrap_err()
            .contains("not supported")
    );
    assert_eq!(parse("/casting 1", world), None);
}

#[test]
fn typed_and_hotbutton_casts_use_all_gems_and_keep_original_stamp_once() {
    for button in [false, true] {
        for gem in 0..8 {
            let (mut app, rx) = app(4);
            target(&mut app, SpawnKind::Npc);
            let made = Instant::now();
            app.world_mut().resource_mut::<Outbox>().set_test_time(made);
            line(&mut app, &format!("/cast {}", gem + 1), button).unwrap();
            app.world_mut()
                .resource_mut::<Outbox>()
                .set_test_time(made + Duration::from_secs(1));
            app.update();
            app.update();
            assert!(
                matches!(rx.try_recv(),Ok(ClientCommand::CastSpell{session_id:9,gem:actual,spell_id:14,target_id:42,created}) if actual==gem && created==made)
            );
            assert!(rx.try_recv().is_err());
            assert!(
                app.world()
                    .resource::<OnlineState>()
                    .world()
                    .casting()
                    .cast
                    .is_none()
            );
        }
    }
}

#[test]
fn slash_cast_keeps_normal_busy_and_queue_refusals_and_never_replays() {
    let (mut app, rx) = app(4);
    testing::pending_cast(&mut app.world_mut().resource_mut::<OnlineState>(), Some(14));
    line(&mut app, "/cast 1", false).unwrap();
    app.update();
    assert!(rx.try_recv().is_err());
    testing::pending_cast(&mut app.world_mut().resource_mut::<OnlineState>(), None);
    app.update();
    assert!(rx.try_recv().is_err());
    let (mut full, full_rx) = self::app(0);
    line(&mut full, "/cast 1", false).unwrap();
    full.update();
    full.update();
    assert!(full_rx.try_recv().is_err());
    assert!(
        full.world()
            .resource::<OnlineState>()
            .world()
            .casting()
            .pending
            .is_none()
    );
    // A burst cannot bypass the ordinary gem's provisional one-send boundary.
    line(&mut app, "/cast 1", false).unwrap();
    line(&mut app, "/cast 2", false).unwrap();
    app.update();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::CastSpell { gem: 0, .. })
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn loot_uses_only_selected_corpses_ignores_words_and_does_not_toggle_or_bulk_loot() {
    for kind in [SpawnKind::NpcCorpse, SpawnKind::PlayerCorpse] {
        let (mut app, rx) = app(4);
        target(&mut app, kind);
        line(&mut app, "/loot all please", false).unwrap();
        app.update();
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::Loot {
                session_id: 9,
                corpse_id: 42,
                ..
            })
        ));
        line(&mut app, "/loot", true).unwrap();
        app.update();
        app.update();
        assert!(rx.try_recv().is_err());
        let world = app.world().resource::<OnlineState>().world();
        assert_eq!(world.target().selected, Some(42));
        assert!(!world.loot().unwrap().listed);
    }
}

#[test]
fn living_targets_never_trade_and_no_target_uses_the_native_line() {
    let (mut app, rx) = app(4);
    line(&mut app, "/loot", false).unwrap();
    app.update();
    assert!(rx.try_recv().is_err());
    assert_eq!(
        app.world().resource::<ChatState>().newest(),
        "You must first target a corpse to loot!"
    );
    for kind in [SpawnKind::Npc, SpawnKind::Player] {
        target(&mut app, kind);
        line(&mut app, "/loot", false).unwrap();
        app.update();
        assert!(rx.try_recv().is_err());
        assert!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .merchant()
                .is_none()
        );
    }
}

#[test]
fn deferred_cast_and_loot_are_consumed_on_reset_reselection_and_replaced_admission() {
    for command in ["/cast 1", "/loot"] {
        for forget in [false, true] {
            let (mut app, rx) = app(4);
            target(&mut app, SpawnKind::NpcCorpse);
            line(&mut app, command, false).unwrap();
            if forget {
                app.world_mut()
                    .resource_mut::<ChatState>()
                    .forget_admission();
            } else {
                testing::admit(
                    &mut app.world_mut().resource_mut::<OnlineState>(),
                    10,
                    player(),
                );
            }
            app.update();
            app.update();
            assert!(rx.try_recv().is_err());
        }
    }
    let (mut app, rx) = app(4);
    target(&mut app, SpawnKind::NpcCorpse);
    line(&mut app, "/loot", false).unwrap();
    target(&mut app, SpawnKind::NpcCorpse);
    app.update();
    assert!(rx.try_recv().is_err());
}
