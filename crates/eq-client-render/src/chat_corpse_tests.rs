use super::{corpse_for_pull, targeted_corpse};
use crate::online::{OnlineState, testing};
use eq_client_core::SpawnKind;

#[test]
fn one_own_corpse_needs_no_target_and_both_corpse_kinds_can_be_targeted() {
    for kind in [SpawnKind::PlayerCorpse, SpawnKind::NpcCorpse] {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 5, testing::player(7));
        let mut corpse = testing::pet(9, 0);
        corpse.kind = kind;
        corpse.name = "eXaMpLe's_corpse679".into();
        testing::spawns(&mut online, vec![corpse.clone()]);
        assert_eq!(corpse_for_pull(&online), Ok(9));
        testing::connect(&mut online, true);
        let (tx, _rx) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(tx));
        let commands = |line| {
            super::game_commands(line, &online, &outbox, &super::Settings::default())
                .unwrap()
                .unwrap()
        };
        assert!(matches!(
            commands("/corpse").as_slice(),
            [eq_client_core::ClientCommand::SummonCorpse {
                session_id: 5,
                spawn_id: 9
            }]
        ));
        assert!(matches!(
            commands("/corpsedrag").as_slice(),
            [eq_client_core::ClientCommand::DragCorpse {
                session_id: 5,
                spawn_id: 9
            }]
        ));
        assert!(matches!(
            commands("/corpsedrop").as_slice(),
            [eq_client_core::ClientCommand::DropCorpse {
                session_id: 5,
                spawn_id: None
            }]
        ));
        assert!(
            targeted_corpse(&online).is_err(),
            "untargeted drop still means all"
        );
        online.select_target(Some(9));
        assert_eq!(corpse_for_pull(&online), Ok(9));
        corpse.spawn_id = 10;
        corpse.name = "Other's corpse10".into();
        testing::spawns(&mut online, vec![corpse]);
        online.select_target(Some(10));
        assert_eq!(corpse_for_pull(&online), Ok(10));
    }
}

#[test]
fn ambiguous_own_corpses_require_a_target_and_living_names_are_not_owners() {
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 5, testing::player(7));
    let mut corpse = testing::pet(9, 0);
    corpse.kind = SpawnKind::NpcCorpse;
    corpse.name = "Example's corpse9".into();
    let mut second = corpse.clone();
    second.spawn_id = 10;
    testing::spawns(&mut online, vec![corpse.clone(), second]);
    assert!(corpse_for_pull(&online).is_err());
    online.select_target(Some(9));
    assert_eq!(corpse_for_pull(&online), Ok(9));
    corpse.kind = SpawnKind::Npc;
    testing::spawns(&mut online, vec![corpse]);
    assert!(targeted_corpse(&online).is_err());
}
