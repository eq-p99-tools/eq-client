//! F uses what is nearest within reach: a door, or an item on the ground.
use bevy::prelude::*;

/// What F would use now.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Use {
    /// Open or close a door, with its model.
    Door(u8, String),
    /// Pick up an item on the ground.
    Item(u32),
}

/// The door or item nearest the player within reach, if any.
pub(super) fn nearest(state: &super::online::OnlineState) -> Option<Use> {
    let door = super::doors::nearest(state);
    let item = state
        .world()
        .player()
        .filter(|_| state.in_world())
        .and_then(|player| {
            eq_client_core::ground::nearest_item(state.world().objects(), player.position)
        });
    match (door, item) {
        (Some((door_distance, door)), item)
            if item.is_none_or(|(_, distance)| door_distance <= distance) =>
        {
            Some(Use::Door(door.id, door.model.clone()))
        }
        (_, Some((drop_id, _))) => Some(Use::Item(drop_id)),
        _ => None,
    }
}

/// One explicit key press queues one request; no state is predicted.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn input(
    keys: super::keys::Keys,
    mut chat: ResMut<super::chat::ChatState>,
    outbox: Res<crate::outbox::Outbox>,
    mut state: ResMut<super::online::OnlineState>,
) {
    if !keys.pressed(super::keys::Act::Use) {
        return;
    }
    match nearest(&state) {
        Some(Use::Door(door_id, _)) => super::doors::open(door_id, &mut state, &outbox),
        Some(Use::Item(drop_id)) => {
            if let Some(line) = super::ground::pick_up(drop_id, &state, &outbox) {
                chat.history.push(super::chat::system_line(line));
            }
        }
        None => (),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::ground::{GroundObject, ObjectUpdate};

    fn state() -> super::super::online::OnlineState {
        let mut state = super::super::online::OnlineState::new(true);
        crate::online::testing::admit(
            &mut state,
            11,
            eq_client_core::PlayerState {
                name: "Example".into(),
                base_attributes: None,
                spawn_id: 1,
                race: 1,
                class: None,
                deity: None,
                skills: None,
                gender: 0,
                level: 1,
                position: default(),
                mana: 0,
                endurance: None,
                spell_refresh_ms: None,
                memorized_spells: [None; 8],
                size: 6.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                hp_percent: None,
                appearance: eq_client_core::outfit::Appearance::default(),
            },
        );
        state
    }

    fn door_at(x: f32) -> eq_client_core::doors::DoorUpdate {
        let mut bytes = [0u8; 80];
        bytes[..4].copy_from_slice(b"DOOR");
        bytes[36..40].copy_from_slice(&x.to_le_bytes());
        bytes[60] = 3;
        eq_client_core::doors::decode(0x4c24, &bytes)
            .unwrap()
            .unwrap()
    }

    fn item_at(x: f32) -> ObjectUpdate {
        ObjectUpdate::Spawn(GroundObject {
            drop_id: 71,
            model: "IT63_ACTORDEF".into(),
            position: eq_client_core::WorldPosition { x, ..default() },
            object_type: 0,
        })
    }

    #[test]
    fn f_uses_whichever_of_a_door_and_an_item_is_nearer() {
        let mut state = state();
        assert_eq!(nearest(&state), None);
        crate::online::testing::doors(&mut state, &door_at(6.0), std::time::Instant::now());
        crate::online::testing::objects(&mut state, &item_at(3.0));
        assert_eq!(nearest(&state), Some(Use::Item(71)));
        crate::online::testing::objects(&mut state, &item_at(9.0));
        assert_eq!(nearest(&state), Some(Use::Door(3, "DOOR".into())));
        crate::online::testing::doors(
            &mut state,
            &eq_client_core::doors::DoorUpdate::RemoveAll,
            std::time::Instant::now(),
        );
        assert_eq!(nearest(&state), Some(Use::Item(71)));
        crate::online::testing::connect(&mut state, false);
        assert_eq!(nearest(&state), None);
    }

    #[test]
    fn pressing_f_near_an_item_queues_a_pickup() {
        let mut state = state();
        crate::online::testing::objects(&mut state, &item_at(3.0));
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<super::super::chat::ChatState>()
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .insert_resource(state)
            .add_systems(Update, input);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyF);
        app.update();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            eq_client_core::ClientCommand::PickUp {
                session_id: 11,
                drop_id: 71,
                ..
            }
        ));
    }
}
