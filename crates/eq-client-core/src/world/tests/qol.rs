use super::*;

#[test]
fn movement_policy_feedback_is_local_scoped_and_cannot_grant_speed() {
    let mut world = admitted();
    let options = crate::movement::MovementOptions {
        fly: true,
        ..Default::default()
    };
    assert!(
        game(
            &mut world,
            WorldEvent::MovementOptionsApplied {
                session_id: 99,
                request_id: 2,
                options
            }
        )
        .ignored
    );
    assert!(world.movement_options_result().is_none());
    game(
        &mut world,
        WorldEvent::MovementOptionsApplied {
            session_id: 1,
            request_id: 2,
            options,
        },
    );
    assert_eq!(world.movement_options_result(), Some(&(2, Ok(options))));
    assert!(world.motion().is_none());
    game(
        &mut world,
        WorldEvent::MovementOptionsRefused {
            session_id: 1,
            request_id: 3,
            reason: "Expired".into(),
        },
    );
    assert_eq!(
        world.movement_options_result(),
        Some(&(3, Err("Expired".into())))
    );
    game(&mut world, entered(2));
    assert!(world.movement_options_result().is_none());
}

#[test]
fn only_new_known_npc_deaths_naming_the_player_are_kill_candidates() {
    let mut world = admitted();
    let death = crate::Death {
        spawn_id: 5,
        killer_id: 9,
        corpse_id: 5,
        bind_zone_id: 0,
        corpse_name: None,
    };
    assert_eq!(
        game(&mut world, WorldEvent::Death(death.clone())).kills,
        [5]
    );
    assert_eq!(world.spawn(5).unwrap().state.kind, SpawnKind::NpcCorpse);
    assert!(
        game(&mut world, WorldEvent::Death(death.clone()))
            .kills
            .is_empty()
    );
    game(&mut world, WorldEvent::Spawns(vec![spawn(5)]));
    let mut another = death.clone();
    another.killer_id = 8;
    assert!(
        game(&mut world, WorldEvent::Death(another))
            .kills
            .is_empty()
    );
    let mut unseen = death;
    unseen.spawn_id = 77;
    assert!(game(&mut world, WorldEvent::Death(unseen)).kills.is_empty());
}

#[test]
fn received_damage_is_available_without_inventing_health_or_chat() {
    let mut world = admitted();
    let damage = crate::combat::Damage {
        source_id: 5,
        target_id: 77,
        kind: 1,
        spell_id: None,
        outcome: crate::combat::DamageOutcome::Hit(12),
        push: None,
    };
    let changes = game(&mut world, WorldEvent::Damage(damage));
    assert_eq!(changes.damage, [damage]);
    assert!(changes.notices.is_empty());
    assert_eq!(world.health(5), None);
    assert_eq!(world.health(77), None);
}
