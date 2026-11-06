use super::*;
use bevy::ecs::system::RunSystemOnce;
use eq_client_assets::characters::Attachment;
use eq_client_core::{
    WorldEvent,
    outfit::{TextureSlot, WearChange},
};

#[test]
fn submitted_hand_changes_redress_the_avatar_and_paperdoll_in_the_same_zone() {
    use crate::item_models::{ItemLibrary, Shape};
    use crate::online::{OnlineState, testing};
    use eq_client_core::inventory::{InventorySlot, InventoryUpdate};
    use std::num::NonZeroU16;

    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.appearance.materials[TextureSlot::Primary.index()] = 42;
    testing::admit(&mut online, 5, player);
    let mut original = crate::preview::items().remove(0);
    original.details.base_held_model = NonZeroU16::new(42);
    let mut replacement = original.clone();
    replacement.slot = InventorySlot::CURSOR;
    replacement.details.id += 1;
    replacement.details.base_held_model = NonZeroU16::new(43);
    testing::inventory(
        &mut online,
        InventoryUpdate::Snapshot(vec![original.clone(), replacement.clone()]),
    );

    let mut world = World::new();
    let figures = [character(&mut world), character(&mut world)];
    let mut library = ItemLibrary::default();
    for name in ["IT42", "IT43"] {
        library.insert(
            name,
            Shape {
                primitives: Vec::new(),
                bounds: None,
                flat: false,
            },
        );
    }
    world.insert_resource(library);
    world.insert_resource(crate::ViewerSettings::default());
    world.insert_resource(crate::options::OptionsState::new(default()));
    world.init_resource::<Wardrobe>();
    world.insert_resource(online);
    let held = |world: &World, figure| {
        let character = world
            .get::<crate::character::AnimatedCharacter>(figure)
            .unwrap();
        let held = character.held[0].as_ref().unwrap();
        (held.entity, held.number)
    };
    world.run_system_once(dress).unwrap();
    let before = figures.map(|figure| held(&world, figure));
    assert_eq!(before.map(|(_, model)| model), [42; 2]);

    {
        let mut online = world.resource_mut::<OnlineState>();
        let revision = online.world().worker_inventory().revision();
        std::mem::swap(&mut original.slot, &mut replacement.slot);
        testing::news(
            &mut online,
            [
                WorldEvent::InventoryAction {
                    session_id: 5,
                    revision,
                    sequence: None,
                    update: Some(InventoryUpdate::Prediction(vec![original, replacement])),
                    error: None,
                },
                WorldEvent::Inventory(InventoryUpdate::Settled),
            ],
        );
        let player = online.world().player().unwrap();
        assert_eq!(player.appearance.material(TextureSlot::Primary), 42);
    }
    world.run_system_once(dress).unwrap();
    assert_eq!(figures.map(|figure| held(&world, figure).1), [43; 2]);
    assert!(
        before
            .into_iter()
            .all(|(entity, _)| world.get_entity(entity).is_err())
    );
}

fn character(world: &mut World) -> Entity {
    crate::character::testing::stores(world);
    let prepared = crate::character::testing::prepared(world);
    let root = crate::character::testing::drawn(world, &prepared);
    world
        .get_mut::<crate::character::AnimatedCharacter>(root)
        .unwrap()
        .attachments = [
        Some(Mat4::from_translation(Vec3::X)),
        Some(Mat4::from_translation(Vec3::Y)),
        Some(Mat4::from_translation(Vec3::Z)),
    ];
    root
}

fn received(online: &mut crate::online::OnlineState, changes: &[(TextureSlot, u32)]) -> Appearance {
    crate::online::testing::news(
        online,
        changes.iter().map(|&(slot, material)| {
            WorldEvent::WearChange(WearChange {
                spawn_id: 7,
                slot,
                material,
                tint: None,
            })
        }),
    );
    worn(None, None, online.world(), true).0.1
}

fn draw_held(world: &mut World, root: Entity, look: Appearance) {
    let ready = world
        .run_system_once(
            move |mut commands: Commands,
                  mut characters: Query<&mut crate::character::AnimatedCharacter>| {
                hold(
                    &mut commands,
                    &mut characters.get_mut(root).unwrap(),
                    &look,
                    |name| {
                        assert!(["IT42", "IT43", "IT205"].contains(&name));
                        Asset::Ready(Arc::new(crate::item_models::Shape {
                            primitives: Vec::new(),
                            bounds: None,
                            flat: false,
                        }))
                    },
                )
            },
        )
        .unwrap();
    assert!(ready);
}

#[test]
fn received_wear_models_attach_to_the_right_hand_and_shield_and_clear_on_removal() {
    let mut online = crate::online::OnlineState::new(true);
    crate::online::testing::admit(&mut online, 5, crate::online::testing::player(7));
    let mut world = World::new();
    let root = character(&mut world);
    let look = received(
        &mut online,
        &[
            (TextureSlot::Primary, 42),
            (TextureSlot::Secondary, 205),
            (TextureSlot::Chest, 3),
        ],
    );
    assert_eq!(look.material(TextureSlot::Chest), 3);
    draw_held(&mut world, root, look);
    let drawn = world
        .get::<crate::character::AnimatedCharacter>(root)
        .unwrap();
    let primary = drawn.held[0].as_ref().unwrap();
    let secondary = drawn.held[1].as_ref().unwrap();
    assert_eq!((primary.number, primary.point), (42, Attachment::RightHand));
    assert_eq!(
        (secondary.number, secondary.point),
        (205, Attachment::Shield)
    );
    let (primary, secondary) = (primary.entity, secondary.entity);
    assert_eq!(
        world.get::<Transform>(primary).unwrap().translation,
        Vec3::X
    );
    assert_eq!(
        world.get::<Transform>(secondary).unwrap().translation,
        Vec3::Z
    );
    assert_eq!(
        *world.get::<Visibility>(primary).unwrap(),
        Visibility::Inherited
    );
    assert_eq!(
        *world.get::<Visibility>(secondary).unwrap(),
        Visibility::Inherited
    );
    let look = received(
        &mut online,
        &[(TextureSlot::Primary, 0), (TextureSlot::Secondary, 0)],
    );
    assert_eq!(look.material(TextureSlot::Chest), 3);
    draw_held(&mut world, root, look);
    assert!(world.get_entity(primary).is_err());
    assert!(world.get_entity(secondary).is_err());
    assert!(
        world
            .get::<crate::character::AnimatedCharacter>(root)
            .unwrap()
            .held
            .iter()
            .all(Option::is_none)
    );
}

#[test]
fn received_secondary_replacement_moves_to_the_left_hand_and_preserves_the_primary() {
    let mut online = crate::online::OnlineState::new(true);
    crate::online::testing::admit(&mut online, 5, crate::online::testing::player(7));
    let mut world = World::new();
    let root = character(&mut world);
    let look = received(
        &mut online,
        &[(TextureSlot::Primary, 42), (TextureSlot::Secondary, 205)],
    );
    draw_held(&mut world, root, look);
    let drawn = world
        .get::<crate::character::AnimatedCharacter>(root)
        .unwrap();
    let primary = drawn.held[0].as_ref().unwrap().entity;
    let shield = drawn.held[1].as_ref().unwrap().entity;
    let look = received(&mut online, &[(TextureSlot::Secondary, 43)]);
    draw_held(&mut world, root, look);
    assert!(world.get_entity(shield).is_err());
    let drawn = world
        .get::<crate::character::AnimatedCharacter>(root)
        .unwrap();
    assert_eq!(drawn.held[0].as_ref().unwrap().entity, primary);
    let secondary = drawn.held[1].as_ref().unwrap();
    assert_eq!(
        (secondary.number, secondary.point),
        (43, Attachment::LeftHand)
    );
    assert_eq!(
        world
            .get::<Transform>(secondary.entity)
            .unwrap()
            .translation,
        Vec3::Y
    );
}
