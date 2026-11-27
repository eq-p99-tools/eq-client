//! Normal receive → core change → quiet attack-off, including a full queue.
use super::*;
use crate::{combat::CombatState, outbox::Outbox};
use eq_client_core::{ClientCommand, Death};

#[test]
fn target_death_and_despawn_send_one_quiet_off_and_do_not_resume_idle_swings() {
    for despawn in [false, true] {
        for full in [false, true] {
            let (updates, news) = std::sync::mpsc::channel();
            let (commands, requests) = std::sync::mpsc::sync_channel(1);
            let mut app = loading_app(news);
            app.insert_resource(Outbox::new(Some(commands.clone())))
                .add_systems(
                    Update,
                    (crate::combat::input, crate::combat::idle_swings)
                        .chain()
                        .after(receive),
                );
            {
                let mut online = app.world_mut().resource_mut::<OnlineState>();
                let mut target = testing::pet(9, 0);
                target.pet_owner = None;
                testing::spawns(&mut online, vec![target]);
                online.select_target(Some(9));
            }
            app.update(); // Establish the ordinary combat admission first.
            app.world_mut().resource_mut::<CombatState>().auto_attack = true;
            if full {
                commands
                    .try_send(ClientCommand::AutoAttack {
                        session_id: 1,
                        enabled: true,
                        created: std::time::Instant::now(),
                    })
                    .unwrap();
            }
            let event = if despawn {
                WorldEvent::Despawn(9)
            } else {
                WorldEvent::Death(Death {
                    spawn_id: 9,
                    killer_id: 7,
                    corpse_id: 9,
                    bind_zone_id: 0,
                    corpse_name: None,
                })
            };
            updates.send(WorldUpdate::Game(event)).unwrap();
            app.update();
            assert!(!app.world().resource::<CombatState>().auto_attack);
            assert_eq!(world(&app).target().selected, None);
            if full {
                assert!(matches!(
                    requests.try_recv().unwrap(),
                    ClientCommand::AutoAttack { enabled: true, .. }
                ));
                assert!(requests.try_recv().is_err());
                app.update(); // Only the unsent off retries after queue space.
            }
            assert!(matches!(
                requests.try_recv().unwrap(),
                ClientCommand::AutoAttack {
                    enabled: false,
                    session_id: 1,
                    ..
                }
            ));
            let lines = app
                .world()
                .resource::<crate::chat::ChatState>()
                .history
                .revision();
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs(10));
            app.update();
            app.update();
            assert!(requests.try_recv().is_err());
            assert_eq!(
                app.world()
                    .resource::<crate::chat::ChatState>()
                    .history
                    .revision(),
                lines
            );
        }
    }
}
