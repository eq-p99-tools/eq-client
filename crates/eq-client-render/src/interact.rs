//! Use selects what is nearest within reach: a door, an item on the ground, or a
//! world container such as a forge.
use bevy::prelude::*;

/// What the Use action would use now.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Use {
    /// Open or close a door, with its model.
    Door(u8, String),
    /// Pick up an item on the ground, or open a world container.
    Object(u32),
}

/// The door or object nearest the player within reach, if any.
pub(super) fn nearest(state: &super::online::OnlineState) -> Option<Use> {
    let door = super::doors::nearest(state);
    let item = state
        .world()
        .player()
        .filter(|_| state.in_world())
        .and_then(|player| {
            eq_client_core::ground::nearest_usable_for(
                state.world().objects(),
                player.position,
                crate::outbox::offered(state.world(), eq_client_core::Capability::WorldContainers),
            )
        });
    match (door, item) {
        (Some((door_distance, door)), item)
            if item.is_none_or(|(_, distance)| door_distance <= distance) =>
        {
            Some(Use::Door(door.id, door.model.clone()))
        }
        (_, Some((drop_id, _))) => Some(Use::Object(drop_id)),
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
        Some(Use::Door(door_id, _)) => super::doors::open(door_id, &state, &outbox),
        Some(Use::Object(drop_id)) => {
            if let Some(refusal) = super::ground::use_object(drop_id, &mut state, &outbox) {
                chat.refuse(refusal);
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
                practice_points: None,
                languages: None,
                disciplines: None,
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
                listing: eq_client_core::listing::Listing::default(),
                name_parts: eq_client_core::names::NameParts::default(),
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
    fn use_selects_whichever_of_a_door_and_an_item_is_nearer() {
        let mut state = state();
        assert_eq!(nearest(&state), None);
        crate::online::testing::doors(&mut state, &door_at(6.0), std::time::Instant::now());
        crate::online::testing::objects(&mut state, &item_at(3.0));
        assert_eq!(nearest(&state), Some(Use::Object(71)));
        crate::online::testing::objects(&mut state, &item_at(9.0));
        assert_eq!(nearest(&state), Some(Use::Door(3, "DOOR".into())));
        crate::online::testing::doors(
            &mut state,
            &eq_client_core::doors::DoorUpdate::RemoveAll,
            std::time::Instant::now(),
        );
        assert_eq!(nearest(&state), Some(Use::Object(71)));
        crate::online::testing::connect(&mut state, false);
        assert_eq!(nearest(&state), None);
    }

    #[test]
    fn carried_combines_do_not_offer_a_station_ahead_of_a_ground_item() {
        let mut state = state();
        let capabilities = eq_client_core::Capability::ALL
            .into_iter()
            .filter(|capability| *capability != eq_client_core::Capability::WorldContainers)
            .collect();
        let player = state.world().player().unwrap().clone();
        crate::online::testing::news(
            &mut state,
            [eq_client_core::WorldEvent::Entered {
                capabilities,
                choices: Vec::new(),
                session_id: 12,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip: None,
                spell_landing_flag: None,
            }],
        );
        crate::online::testing::objects(
            &mut state,
            &ObjectUpdate::Spawn(GroundObject {
                drop_id: 72,
                model: "FORGE".into(),
                object_type: 17,
                position: eq_client_core::WorldPosition {
                    x: 2.0,
                    ..default()
                },
            }),
        );
        crate::online::testing::objects(&mut state, &item_at(3.0));
        assert_eq!(nearest(&state), Some(Use::Object(71)));
    }

    /// A policy that offers no door use exposes no door: neither
    /// the hint nor F sees one, and the item beyond it is what F uses.
    #[test]
    fn a_policy_without_door_use_offers_no_door() {
        let mut state = state();
        let capabilities = eq_client_core::Capability::ALL
            .iter()
            .copied()
            .filter(|capability| *capability != eq_client_core::Capability::Doors)
            .collect();
        let player = state.world().player().unwrap().clone();
        crate::online::testing::news(
            &mut state,
            [eq_client_core::WorldEvent::Entered {
                capabilities,
                choices: Vec::new(),
                session_id: 12,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip: None,
                spell_landing_flag: None,
            }],
        );
        crate::online::testing::doors(&mut state, &door_at(3.0), std::time::Instant::now());
        assert_eq!(nearest(&state), None);
        crate::online::testing::objects(&mut state, &item_at(9.0));
        assert_eq!(nearest(&state), Some(Use::Object(71)));
    }

    fn eqmac_door() -> eq_client_core::doors::Door {
        eq_client_core::doors::Door {
            id: 7,
            model: "DOOR".into(),
            position: eq_client_core::WorldPosition {
                x: 3.0,
                z: 40.0,
                ..default()
            },
            incline: 0,
            size: 100,
            open_type: 0,
            state_at_spawn: 0,
            invert_state: 0,
            parameter: 0,
            action: None,
        }
    }

    fn cursor_item() -> eq_client_core::inventory::InventoryItem {
        eq_client_core::inventory::InventoryItem {
            activation: default(),
            scroll_spell: None,
            book: None,
            rules: default(),
            slot: eq_client_core::inventory::InventorySlot::CURSOR,
            stack_count: None,
            charges: 0,
            bag_slots: 0,
            details: eq_client_core::ItemDetails {
                equipment: None,
                bonuses: None,
                id: 42,
                name: "Synthetic key".into(),
                lore: String::new(),
                weight_tenths: 0,
                slots: 0,
                classes: 0,
                races: 0,
                flags: Vec::new(),
                stats: Vec::new(),
                price: None,
                icon: None,
            },
        }
    }

    #[test]
    fn eqmac_f_queues_one_ordinary_click_without_predicting_an_open_door() {
        let mut state = state();
        crate::online::testing::generation(&mut state, eq_client_core::GameDialect::EqMac);
        crate::online::testing::inventory(
            &mut state,
            eq_client_core::inventory::InventoryUpdate::Snapshot(Vec::new()),
        );
        crate::online::testing::doors(
            &mut state,
            &eq_client_core::doors::DoorUpdate::Spawn(vec![eqmac_door()]),
            std::time::Instant::now(),
        );
        assert_eq!(nearest(&state), Some(Use::Door(7, "DOOR".into())));
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
            .press(KeyCode::KeyU);
        app.update();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            eq_client_core::ClientCommand::ClickDoor {
                session_id: 11,
                door_id: 7,
                ..
            }
        ));
        assert!(receiver.try_recv().is_err());
        let world = app
            .world()
            .resource::<super::super::online::OnlineState>()
            .world();
        assert_eq!(world.doors().entries()[&7].action, None);
        assert_eq!(world.doors().hinge_fraction(7), Some(0.0));
    }

    #[test]
    fn eqmac_hint_and_f_ignore_special_unknown_static_moving_and_held_item_cases() {
        let mut state = state();
        crate::online::testing::generation(&mut state, eq_client_core::GameDialect::EqMac);
        crate::online::testing::inventory(
            &mut state,
            eq_client_core::inventory::InventoryUpdate::Snapshot(Vec::new()),
        );
        for (id, open_type, parameter, action) in [
            (255, 0, 0, None),
            (7, 54, 0, None),
            (7, 55, 0, None),
            (7, 56, 0, None),
            (7, 57, 0, None),
            (7, 0, 0xfffe, None),
            (7, 0, 0, Some(251)),
        ] {
            let mut definition = eqmac_door();
            definition.id = id;
            definition.open_type = open_type;
            definition.parameter = parameter;
            definition.action = action;
            crate::online::testing::doors(
                &mut state,
                &eq_client_core::doors::DoorUpdate::RemoveAll,
                std::time::Instant::now(),
            );
            crate::online::testing::doors(
                &mut state,
                &eq_client_core::doors::DoorUpdate::Spawn(vec![definition]),
                std::time::Instant::now(),
            );
            assert_eq!(nearest(&state), None);
        }
        let now = std::time::Instant::now();
        crate::online::testing::doors(
            &mut state,
            &eq_client_core::doors::DoorUpdate::Spawn(vec![eqmac_door()]),
            now,
        );
        crate::online::testing::doors(
            &mut state,
            &eq_client_core::doors::DoorUpdate::Move { id: 7, action: 2 },
            now,
        );
        assert_eq!(nearest(&state), None);
        crate::online::testing::doors(
            &mut state,
            &eq_client_core::doors::DoorUpdate::Spawn(vec![eqmac_door()]),
            now,
        );
        crate::online::testing::inventory(
            &mut state,
            eq_client_core::inventory::InventoryUpdate::Snapshot(vec![cursor_item()]),
        );
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
            .press(KeyCode::KeyU);
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
