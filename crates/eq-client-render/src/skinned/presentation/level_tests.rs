use super::*;
use crate::online::{OnlineState, testing};
use eq_client_core::{GameDialect, WorldEvent, listing::ListingChange, world::ClientWorld};

#[test]
fn eqmac_level_labels_use_the_shown_spawn_level_while_gates_keep_character_level() {
    let mut online = OnlineState::new(true);
    let mut world = ClientWorld::default();
    world.carry_as(GameDialect::EqMac);
    testing::set_world(&mut online, world);
    let mut player = testing::player(7);
    player.level = 55;
    testing::admit(&mut online, 1, player);
    testing::news(
        &mut online,
        [WorldEvent::Listing {
            spawn_id: 7,
            change: ListingChange::Level(1),
        }],
    );
    assert_eq!(online.world().player().unwrap().level, 55);
    assert_eq!(label_text(online.world(), None, None, 2), "1");
    testing::news(
        &mut online,
        [WorldEvent::Level {
            current: 56,
            previous: 55,
            experience: 0,
        }],
    );
    assert_eq!(label_text(online.world(), None, None, 2), "56");
}
