use super::*;
use crate::alternate_advancement::{Effect, Rank, Selection, Update};

fn aa(session_id: u64, update: Update) -> WorldEvent {
    WorldEvent::AlternateAdvancement {
        session_id,
        updates: vec![update],
    }
}

fn definition(id: u32, value: i32) -> Update {
    Update::Definition {
        id,
        rank: Some(Rank {
            id,
            required_level: 1,
            classes: 1 << 1,
            sequence: id,
            rank: 1,
            hotkey_sid: 0,
            hotkey_sid2: 0,
            title_sid: 0,
            desc_sid: 0,
            cost: 0,
            prereq_skill: 0,
            prereq_minpoints: 0,
            kind: 0,
            spellid: 0,
            spell_type: 0,
            spell_refresh: 0,
            max_level: 0,
            last_id: 0,
            next_id: 0,
            cost2: 0,
            effects: vec![Effect {
                id: 14,
                value,
                limit: 0,
                slot: 1,
            }],
        }),
    }
}

fn selected(ids: &[u32]) -> Update {
    Update::Selections(Some(
        ids.iter().map(|&id| Selection { id, value: 99 }).collect(),
    ))
}

fn effects(world: &ClientWorld) -> Option<Vec<Effect>> {
    world
        .alternate_advancement()
        .effects(1, 1)
        .map(|effects| effects.into_iter().copied().collect())
}

#[test]
fn aa_batches_replay_definitions_selections_and_withdrawals_in_order() {
    let facts = vec![
        definition(42, 1),
        selected(&[42]),
        definition(43, -1),
        selected(&[42, 43]),
        definition(42, 2),
        Update::Definition { id: 43, rank: None },
        definition(43, -2),
        Update::DefinitionsUnknown,
        definition(42, 1),
        definition(43, -1),
        Update::Unknown,
        selected(&[]),
        Update::Selections(None),
        selected(&[0]),
        definition(42, 1),
        selected(&[42]),
        Update::Definition { id: 42, rank: None },
    ];
    for size in [1, 2, 5, facts.len()] {
        let mut single = admitted();
        let mut batch = admitted();
        for chunk in facts.chunks(size) {
            for fact in chunk {
                game(&mut single, aa(1, fact.clone()));
            }
            game(
                &mut batch,
                WorldEvent::AlternateAdvancement {
                    session_id: 1,
                    updates: chunk.to_vec(),
                },
            );
            assert_eq!(effects(&single), effects(&batch));
        }
    }
    let mut world = admitted();
    game(
        &mut world,
        WorldEvent::AlternateAdvancement {
            session_id: 1,
            updates: vec![definition(42, 1), definition(43, -1), selected(&[42, 43])],
        },
    );
    assert_eq!(
        effects(&world)
            .unwrap()
            .iter()
            .map(|effect| effect.value)
            .sum::<i32>(),
        0
    );
    game(
        &mut world,
        WorldEvent::AlternateAdvancement {
            session_id: 1,
            updates: vec![Update::Unknown, selected(&[])],
        },
    );
    assert_eq!(effects(&world), Some(Vec::new()));
    game(&mut world, aa(1, Update::Selections(None)));
    assert!(effects(&world).is_none());
}

#[test]
fn aa_batch_replay_preserves_definition_capacity_and_replacement_behavior() {
    let mut facts = vec![selected(&[1])];
    facts.extend((1..4096).map(|id| definition(id, 1)));
    assert_eq!(facts.len(), 4096);
    let mut single = admitted();
    let mut batch = admitted();
    for fact in &facts {
        game(&mut single, aa(1, fact.clone()));
    }
    game(
        &mut batch,
        WorldEvent::AlternateAdvancement {
            session_id: 1,
            updates: facts,
        },
    );
    let replacement = vec![definition(4096, 1), definition(1, 3), selected(&[4096])];
    for fact in &replacement {
        game(&mut single, aa(1, fact.clone()));
    }
    game(
        &mut batch,
        WorldEvent::AlternateAdvancement {
            session_id: 1,
            updates: replacement,
        },
    );
    assert_eq!(effects(&single), effects(&batch));
    assert!(effects(&batch).is_some());
    game(&mut single, aa(1, selected(&[1])));
    game(&mut batch, aa(1, selected(&[1])));
    assert_eq!(effects(&single), effects(&batch));
    assert_eq!(effects(&batch).unwrap()[0].value, 3);
    // A new ID over capacity clears definitions and drops that triggering rank.
    let overflow = vec![definition(4097, 1), selected(&[4097])];
    for fact in &overflow {
        game(&mut single, aa(1, fact.clone()));
    }
    game(
        &mut batch,
        WorldEvent::AlternateAdvancement {
            session_id: 1,
            updates: overflow,
        },
    );
    assert_eq!(effects(&single), effects(&batch));
    assert!(effects(&batch).is_none());
    let recovery = vec![definition(4097, 1), selected(&[4097])];
    for fact in &recovery {
        game(&mut single, aa(1, fact.clone()));
    }
    game(
        &mut batch,
        WorldEvent::AlternateAdvancement {
            session_id: 1,
            updates: recovery,
        },
    );
    assert_eq!(effects(&single), effects(&batch));
    assert_eq!(effects(&batch).unwrap()[0].value, 1);
}

#[test]
fn aa_observations_belong_to_one_live_admission() {
    let mut world = admitted();
    let selections = Update::Selections(Some(vec![Selection { id: 42, value: 1 }]));
    let definition = Update::Definition {
        id: 42,
        rank: Some(Rank {
            id: 42,
            required_level: 1,
            classes: 1 << 1,
            sequence: 9,
            rank: 1,
            hotkey_sid: 0,
            hotkey_sid2: 0,
            title_sid: 0,
            desc_sid: 0,
            cost: 0,
            prereq_skill: 0,
            prereq_minpoints: 0,
            kind: 0,
            spellid: 0,
            spell_type: 0,
            spell_refresh: 0,
            max_level: 0,
            last_id: 0,
            next_id: 0,
            cost2: 0,
            effects: vec![Effect {
                id: 14,
                value: 1,
                limit: 0,
                slot: 1,
            }],
        }),
    };
    game(&mut world, aa(1, selections.clone()));
    game(&mut world, aa(1, definition.clone()));
    assert_eq!(
        world.alternate_advancement().effects(1, 1).unwrap().len(),
        1
    );
    game(&mut world, entered(2));
    assert!(world.alternate_advancement().effects(1, 1).is_none());
    assert!(
        game(
            &mut world,
            WorldEvent::AlternateAdvancement {
                session_id: 1,
                updates: vec![definition.clone(), selections.clone()],
            }
        )
        .ignored
    );
    game(&mut world, aa(2, selections));
    assert!(world.alternate_advancement().effects(1, 1).is_none());
    game(&mut world, aa(2, definition.clone()));
    assert_eq!(
        world.alternate_advancement().effects(1, 1).unwrap().len(),
        1
    );
    connection(&mut world, false, true);
    assert!(world.alternate_advancement().effects(1, 1).is_none());
    assert!(
        game(
            &mut world,
            WorldEvent::AlternateAdvancement {
                session_id: 2,
                updates: vec![definition, selected(&[42])],
            }
        )
        .ignored
    );
}

#[test]
fn eqmac_aa_facts_are_kept_and_rof2_sends_none_yet() {
    use crate::alternate_advancement::{MacPoints, MacRank};
    let points = Update::MacPoints(Some(MacPoints {
        aapoints: 2,
        exp_aa: 0,
        per_aa: 0,
    }));
    let mut world = admitted();
    world.carry_as(crate::GameDialect::EqMac);
    game(&mut world, aa(1, points.clone()));
    game(
        &mut world,
        aa(
            1,
            Update::MacSelections(Some(vec![MacRank { id: 12, value: 16 }])),
        ),
    );
    let facts = world.alternate_advancement();
    assert_eq!(facts.mac_points().map(|points| points.aapoints), Some(2));
    assert_eq!(facts.mac_selections().map(<[_]>::len), Some(1));
    // A new admission forgets them.
    game(&mut world, entered(2));
    assert_eq!(world.alternate_advancement().mac_points(), None);

    let mut world = admitted();
    world.carry_as(crate::GameDialect::Rof2);
    assert!(game(&mut world, aa(1, points)).ignored);
    assert_eq!(world.alternate_advancement().mac_points(), None);
}
