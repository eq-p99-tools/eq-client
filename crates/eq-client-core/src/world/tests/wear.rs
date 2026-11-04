use super::*;
use crate::outfit::{TextureSlot, WearChange, held_model};

#[test]
fn received_wear_updates_drive_own_and_remote_models_without_creating_inventory_items() {
    let mut world = admitted();
    let initial = world.player().unwrap().appearance;
    let change = |spawn_id, slot, material| {
        WorldEvent::WearChange(WearChange {
            spawn_id,
            slot,
            material,
            tint: None,
        })
    };
    game(&mut world, change(5, TextureSlot::Secondary, 205));
    assert_eq!(world.player().unwrap().appearance, initial);
    assert_eq!(
        held_model(
            &world.spawn(5).unwrap().state.appearance,
            TextureSlot::Secondary
        ),
        Some("IT205".into())
    );
    game(&mut world, change(9, TextureSlot::Primary, 42));
    let worn = world.player().unwrap().appearance;
    assert_eq!(held_model(&worn, TextureSlot::Primary), Some("IT42".into()));
    assert_eq!(
        worn.material(TextureSlot::Chest),
        initial.material(TextureSlot::Chest)
    );
    assert_eq!(
        worn.material(TextureSlot::Secondary),
        initial.material(TextureSlot::Secondary)
    );
    game(&mut world, change(77, TextureSlot::Primary, 99));
    assert!(world.spawn(77).is_none());
    assert_eq!(world.player().unwrap().appearance, worn);
    game(&mut world, change(9, TextureSlot::Primary, 0));
    assert_eq!(
        held_model(&world.player().unwrap().appearance, TextureSlot::Primary),
        None
    );
    game(&mut world, change(5, TextureSlot::Secondary, 0));
    assert_eq!(
        held_model(
            &world.spawn(5).unwrap().state.appearance,
            TextureSlot::Secondary
        ),
        None
    );
    assert!(world.inventory().items().is_empty());
}
