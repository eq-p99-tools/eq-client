use super::super::{Action, LootWindow, TradeState, input, no_drop};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, Coins, WorldEvent,
    inventory::{InventorySlot, InventoryUpdate},
    loot::{LootRequest, LootResponse, LootUpdate},
};
use std::sync::mpsc::{self, Receiver};

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn ordinary(mut commands: Commands) {
    super::super::show_panel(
        &mut commands,
        None,
        "Corpse",
        (
            super::super::Rows::Loot,
            &[
                (
                    Action::Take {
                        place: 0,
                        auto: false,
                    },
                    "First".into(),
                ),
                (
                    Action::Take {
                        place: 1,
                        auto: false,
                    },
                    "Second".into(),
                ),
            ],
            0.0,
        ),
        &[],
    );
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn skinned(mut commands: Commands, mut art: crate::sheets::Art) {
    commands.spawn(Node::default()).with_children(|parent| {
        crate::skinned::testing_loot_slot(parent, &mut art, 0);
        crate::skinned::testing_loot_slot(parent, &mut art, 1);
    });
}

fn fixture(skin: bool, no_drop_item: bool) -> (App, Receiver<ClientCommand>, [Entity; 2]) {
    use crate::online::{OnlineState, testing};
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 5, testing::player(7));
    testing::inventory(&mut online, InventoryUpdate::Snapshot(Vec::new()));
    online.open_loot(9);
    testing::news(
        &mut online,
        [WorldEvent::Loot(LootUpdate::Opened {
            response: LootResponse::Normal,
            coins: Coins::default(),
        })],
    );
    for place in [0, 1] {
        let mut item = crate::preview::items().remove(0);
        item.details.flags = if no_drop_item {
            vec!["NO DROP".into()]
        } else {
            Vec::new()
        };
        testing::news(
            &mut online,
            [WorldEvent::Loot(LootUpdate::Item {
                place,
                item: Box::new(item),
            })],
        );
    }
    testing::news(
        &mut online,
        [WorldEvent::Loot(LootUpdate::Listed { corpse_id: 9 })],
    );
    let (tx, rx) = mpsc::sync_channel(8);
    let mut app = crate::testing::app();
    app.insert_resource(online)
        .insert_resource(crate::outbox::Outbox::new(Some(tx)))
        .insert_resource(TradeState {
            loot: Some(LootWindow {
                name: "Corpse".into(),
                opening: None,
                pending: None,
                loot_all: false,
            }),
            ..default()
        })
        .add_systems(
            Update,
            (crate::online::carry_as, input, no_drop::confirm).chain(),
        );
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .installed_client = eq_client_assets::ui::InstalledClient::Titanium;
    crate::keys::testing::install(&mut app);
    if skin {
        app.add_systems(Startup, skinned);
    } else {
        app.add_systems(Startup, ordinary);
    }
    app.update();
    let mut query = app.world_mut().query::<(Entity, &Action)>();
    let cells = [0, 1].map(|place| {
        query
            .iter(app.world())
            .find_map(|(entity, action)| {
                matches!(action, Action::Take { place: current, .. } if *current == place)
                    .then_some(entity)
            })
            .unwrap()
    });
    (app, rx, cells)
}

fn right_press(app: &mut App, cell: Entity, scripted: bool) {
    *app.world_mut().get_mut::<Interaction>(cell).unwrap() = if scripted {
        Interaction::Pressed
    } else {
        Interaction::Hovered
    };
    if scripted {
        app.world_mut()
            .entity_mut(cell)
            .insert(crate::tooltip::ScriptPressed);
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}

fn right_release(app: &mut App, over: Option<Entity>) {
    let world = app.world_mut();
    let mut query = world.query::<(Entity, &mut Interaction)>();
    for (entity, mut interaction) in query.iter_mut(world) {
        *interaction = if Some(entity) == over {
            Interaction::Hovered
        } else {
            Interaction::None
        };
    }
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}

fn request(app: &App, rx: &Receiver<ClientCommand>, auto: bool) {
    let ClientCommand::LootItem {
        session_id,
        corpse_id,
        own_id,
        place,
        auto: sent,
        created,
    } = rx.try_recv().unwrap()
    else {
        panic!("expected the normal loot item command")
    };
    assert_eq!(
        (session_id, corpse_id, own_id, place, sent),
        (5, 9, 7, 0, auto)
    );
    let body = eq_client_core::loot::item_request(corpse_id, own_id, place, sent).unwrap();
    assert_eq!(&body[12..], &u32::from(auto).to_le_bytes());
    assert_eq!(
        app.world()
            .resource::<TradeState>()
            .loot
            .as_ref()
            .unwrap()
            .pending,
        Some(super::super::PendingLoot::new(
            crate::outbox::Stamp {
                session_id,
                created
            },
            LootRequest::Item {
                corpse_id,
                own_id,
                place,
                auto
            },
        ))
    );
    assert!(rx.try_recv().is_err(), "one gesture queues one request");
    let world = app.world().resource::<crate::online::OnlineState>().world();
    assert!(
        world.loot().unwrap().items.contains_key(&0),
        "no optimistic removal"
    );
    assert!(
        !world
            .inventory()
            .items()
            .contains_key(&InventorySlot::CURSOR),
        "no invented cursor delivery"
    );
}

#[test]
fn real_ordinary_and_skinned_cells_keep_both_button_destinations() {
    for skin in [false, true] {
        let (mut app, rx, cells) = fixture(skin, false);
        crate::clicks::testing::press(&mut app, Some(cells[0]));
        request(&app, &rx, false);
        crate::clicks::testing::release(&mut app, Some(cells[0]));
        assert!(
            rx.try_recv().is_err(),
            "left release must not queue an auto request"
        );
        right_press(&mut app, cells[1], false);
        right_release(&mut app, Some(cells[1]));
        assert!(
            rx.try_recv().is_err(),
            "pending pickup excludes another item request"
        );
        for scripted in [false, true] {
            let (mut app, rx, cells) = fixture(skin, false);
            right_press(&mut app, cells[0], scripted);
            assert!(
                rx.try_recv().is_err(),
                "right press waits for same-cell release"
            );
            right_release(&mut app, Some(cells[0]));
            request(&app, &rx, true);
        }
    }
}

#[test]
fn right_release_off_cell_stale_item_and_script_cancellation_queue_nothing() {
    for skin in [false, true] {
        for outcome in [0, 1, 2, 3, 4, 5] {
            let (mut app, rx, cells) = fixture(skin, false);
            right_press(&mut app, cells[0], outcome == 3);
            match outcome {
                0 => right_release(&mut app, Some(cells[1])),
                1 => right_release(&mut app, None),
                2 => {
                    let mut item = crate::preview::items().remove(0);
                    item.details.id += 1;
                    crate::online::testing::news(
                        &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
                        [WorldEvent::Loot(LootUpdate::Item {
                            place: 0,
                            item: Box::new(item),
                        })],
                    );
                    right_release(&mut app, Some(cells[0]));
                }
                3 => {
                    app.world_mut()
                        .resource_mut::<TradeState>()
                        .cancel_loot_press(cells[0]);
                    right_release(&mut app, Some(cells[0]));
                }
                4 => {
                    app.world_mut()
                        .resource_mut::<crate::online::OnlineState>()
                        .open_loot(10);
                    right_release(&mut app, Some(cells[0]));
                }
                _ => {
                    crate::online::testing::admit(
                        &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
                        6,
                        crate::online::testing::player(7),
                    );
                    right_release(&mut app, Some(cells[0]));
                }
            }
            assert!(rx.try_recv().is_err());
        }
    }
}

#[test]
fn both_destinations_require_an_empty_cursor_even_after_confirmation() {
    for skin in [false, true] {
        for after_question in [false, true] {
            for auto in [false, true] {
                let (mut app, rx, cells) = fixture(skin, after_question);
                if after_question {
                    if auto {
                        right_press(&mut app, cells[0], false);
                        right_release(&mut app, Some(cells[0]));
                    } else {
                        crate::clicks::testing::click(&mut app, cells[0]);
                    }
                    assert!(
                        app.world()
                            .resource::<crate::confirm::Asked>()
                            .question()
                            .is_some()
                    );
                }
                let mut held = crate::preview::items().remove(0);
                held.slot = InventorySlot::CURSOR;
                crate::online::testing::inventory(
                    &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
                    InventoryUpdate::Snapshot(vec![held]),
                );
                if after_question {
                    app.world_mut()
                        .resource_mut::<crate::confirm::Asked>()
                        .give(true);
                    app.update();
                } else if auto {
                    right_press(&mut app, cells[0], false);
                    right_release(&mut app, Some(cells[0]));
                } else {
                    crate::clicks::testing::click(&mut app, cells[0]);
                }
                assert!(rx.try_recv().is_err());
                assert!(
                    app.world()
                        .resource::<TradeState>()
                        .loot
                        .as_ref()
                        .unwrap()
                        .pending
                        .is_none()
                );
            }
        }
    }
}

#[test]
fn no_drop_yes_retains_the_original_button_and_no_never_sends() {
    for skin in [false, true] {
        for auto in [false, true] {
            for yes in [false, true] {
                let (mut app, rx, cells) = fixture(skin, true);
                if auto {
                    right_press(&mut app, cells[0], false);
                    right_release(&mut app, Some(cells[0]));
                } else {
                    crate::clicks::testing::click(&mut app, cells[0]);
                }
                assert!(
                    matches!(app.world().resource::<crate::confirm::Asked>().question(),
                    Some(crate::confirm::Question::LootNoDrop { auto: asked, .. }) if asked == auto)
                );
                assert!(rx.try_recv().is_err());
                app.world_mut()
                    .resource_mut::<crate::confirm::Asked>()
                    .give(yes);
                app.update();
                if yes {
                    request(&app, &rx, auto);
                } else {
                    assert!(rx.try_recv().is_err());
                }
            }
        }
    }
}
