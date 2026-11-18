use super::*;
use crate::alternate_advancement::{Effect, Rank, Selection, Update};

fn aa(session_id: u64, update: Update) -> WorldEvent {
    WorldEvent::AlternateAdvancement { session_id, update }
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
    assert_eq!(world.environmental_aa().effects(1, 1).unwrap().len(), 1);
    game(&mut world, entered(2));
    assert!(world.environmental_aa().effects(1, 1).is_none());
    assert!(game(&mut world, aa(1, definition.clone())).ignored);
    game(&mut world, aa(2, selections));
    assert!(world.environmental_aa().effects(1, 1).is_none());
    game(&mut world, aa(2, definition.clone()));
    assert_eq!(world.environmental_aa().effects(1, 1).unwrap().len(), 1);
    connection(&mut world, false, true);
    assert!(world.environmental_aa().effects(1, 1).is_none());
    assert!(game(&mut world, aa(2, definition)).ignored);
}
