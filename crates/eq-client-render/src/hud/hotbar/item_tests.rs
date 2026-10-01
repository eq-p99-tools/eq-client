use super::*;
use eq_client_core::{
    ClientCommand,
    inventory::{ClickEffect, ClickKind, InventorySlot, InventoryUpdate, ItemActivation},
};

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the ordered integration scenario and its assertions together"
)]
fn item_binding_uses_current_inventory_and_never_activates_replacement_items() {
    let mut item = crate::inventory::demo_items().remove(0);
    item.slot = InventorySlot(13);
    item.charges = 3;
    item.activation = ItemActivation {
        effect: Some(ClickEffect {
            spell_id: 73,
            kind: ClickKind::Click,
            required_level: 1,
            effect_level: 1,
            cast_time_ms: 1000,
            recast_delay_seconds: 0,
            recast_type: 0,
        }),
        ..default()
    };
    let id = item.details.id;
    let mut online = crate::online::OnlineState::new(true);
    crate::online::testing::admit(
        &mut online,
        9,
        eq_client_core::PlayerState {
            name: "Example".into(),
            base_attributes: None,
            deity: None,
            class: Some(1),
            spawn_id: 1,
            race: 1,
            gender: 0,
            level: 60,
            position: eq_client_core::WorldPosition::default(),
            mana: 0,
            endurance: None,
            skills: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 6.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
        },
    );
    crate::online::testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item.clone()]));
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let mut app = App::new();
    crate::keys::testing::install(&mut app);
    app.init_resource::<Bindings>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<crate::chat::ChatState>()
        .init_resource::<crate::target::TargetState>()
        .init_resource::<crate::hud::HudState>()
        .init_resource::<crate::inventory::InventoryState>()
        .insert_resource(online)
        .insert_resource(crate::outbox::Outbox::new(Some(tx)))
        .add_systems(Update, (update, item_actions).chain());
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ))
        .id();
    app.world_mut().spawn((
        crate::inventory::SlotButton(InventorySlot(13)),
        Interaction::Hovered,
    ));
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::Digit1);
    }
    app.update();
    assert_eq!(
        app.world().resource::<Bindings>().0[0],
        Some(Action::Item {
            slot: InventorySlot(13),
            id
        })
    );
    assert!(rx.try_recv().is_err()); // Binding never activates.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ControlLeft);
    app.world_mut()
        .resource_mut::<crate::keys::Typing>()
        .composing = true;
    app.update();
    assert!(rx.try_recv().is_err());
    app.world_mut()
        .resource_mut::<crate::keys::Typing>()
        .composing = false;
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    assert!(rx.try_recv().is_err());
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    let ClientCommand::UseItem(request) = rx.try_recv().unwrap() else {
        panic!("expected item request");
    };
    assert_eq!(
        (request.slot, request.session_id, request.target_id),
        (InventorySlot(13), 9, 1)
    );
    assert_eq!(
        app.world()
            .resource::<crate::online::OnlineState>()
            .world
            .inventory()
            .items()[&InventorySlot(13)]
            .charges,
        3
    );
    app.world_mut()
        .resource_mut::<crate::inventory::InventoryState>()
        .cancel_actions();
    item.details.id += 1;
    crate::online::testing::inventory(
        &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
        InventoryUpdate::Set(vec![item]),
    );
    app.update();
    assert!(rx.try_recv().is_err());
    assert!(
        app.world()
            .resource::<crate::hud::HudState>()
            .action_feedback
            .as_ref()
            .unwrap()
            .1
            .contains("unavailable")
    );
}
