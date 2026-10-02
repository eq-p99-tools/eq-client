//! Tradeskill combines, as the official client offers them: a tradeskill
//! container's window has a Combine button, which asks the server to combine
//! what the container holds. The button shows only on a tradeskill container
//! carried in a pack slot, and greys while a combine waits for the server;
//! what was made arrives on the cursor, and the verdict in chat.
use crate::{online::OnlineState, outbox::Outbox, skinned::Greyed};
use bevy::prelude::*;
use eq_client_core::{inventory::InventorySlot, world::ClientWorld};

/// A tradeskill container window's Combine button, for the container in
/// this pack slot.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CombineButton(pub(crate) InventorySlot);

/// Whether the container in this slot is one the player can combine in.
fn combines(world: &ClientWorld, container: InventorySlot) -> bool {
    world
        .inventory()
        .items()
        .get(&container)
        .is_some_and(eq_client_core::tradeskills::can_combine_in)
}

/// Asks the server to combine when Combine is pressed, unless a combine
/// already waits for its answer.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    buttons: Query<(&Interaction, &CombineButton), Changed<Interaction>>,
) {
    let world = online.world();
    for (interaction, CombineButton(container)) in &buttons {
        if *interaction == Interaction::Pressed
            && world.combining().is_none()
            && combines(world, *container)
        {
            let _ = outbox.post(world, |stamp| eq_client_core::ClientCommand::Combine {
                session_id: stamp.session_id,
                container: *container,
                created: stamp.created,
            });
        }
    }
}

/// Shows Combine only on a tradeskill container, greyed while a combine
/// waits for the server.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    mut commands: Commands,
    online: Res<OnlineState>,
    mut buttons: Query<(Entity, &CombineButton, &mut Visibility, Has<Greyed>)>,
) {
    let world = online.world();
    for (entity, CombineButton(container), mut visibility, greyed) in &mut buttons {
        visibility.set_if_neq(if combines(world, *container) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        let waiting = world.combining().is_some();
        if waiting && !greyed {
            commands.entity(entity).insert(Greyed);
        } else if !waiting && greyed {
            commands.entity(entity).remove::<Greyed>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{
        ItemDetails, WorldEvent,
        inventory::{InventoryItem, InventoryUpdate, ItemActivation, ItemPlacement},
        tradeskills::CombineUpdate,
    };

    /// A bag of this type in pack slot 23.
    fn bag(bag_type: u8) -> InventoryItem {
        InventoryItem {
            activation: ItemActivation::default(),
            scroll_spell: None,
            book: None,
            rules: ItemPlacement {
                bag_type,
                ..ItemPlacement::default()
            },
            slot: InventorySlot(23),
            details: ItemDetails {
                equipment: None,
                bonuses: None,
                id: 17_009,
                name: "Example Kit".into(),
                lore: String::new(),
                weight_tenths: 10,
                slots: 0,
                classes: 0,
                races: 0,
                flags: Vec::new(),
                stats: Vec::new(),
            },
            icon: 0,
            stack_count: None,
            charges: 0,
            bag_slots: 10,
        }
    }

    fn app(bag: InventoryItem) -> (App, Entity) {
        let mut app = crate::testing::app();
        app.add_systems(Update, show);
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![bag]));
        app.insert_resource(online);
        let button = app
            .world_mut()
            .spawn((CombineButton(InventorySlot(23)), Visibility::Inherited))
            .id();
        (app, button)
    }

    #[test]
    fn combine_shows_on_a_tradeskill_container_and_greys_while_it_waits() {
        // A sewing kit.
        let (mut app, button) = app(bag(16));
        app.update();
        let visible =
            |app: &App| app.world().get::<Visibility>(button) == Some(&Visibility::Inherited);
        assert!(visible(&app));
        assert!(app.world().get::<Greyed>(button).is_none());
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Combine(CombineUpdate::Started(InventorySlot(
                23,
            )))],
        );
        app.update();
        assert!(app.world().get::<Greyed>(button).is_some());
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Combine(CombineUpdate::Answered)],
        );
        app.update();
        assert!(app.world().get::<Greyed>(button).is_none());
        assert!(visible(&app));
    }

    #[test]
    fn a_plain_bag_has_no_combine() {
        // A backpack.
        let (mut app, button) = app(bag(5));
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(button),
            Some(&Visibility::Hidden)
        );
    }
}
