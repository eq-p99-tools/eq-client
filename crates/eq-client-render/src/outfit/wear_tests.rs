use super::*;
use bevy::ecs::system::RunSystemOnce;
use eq_client_assets::characters::Attachment;
use eq_client_core::{
    WorldEvent,
    outfit::{TextureSlot, WearChange},
};

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
