//! Synthetic merchant layouts: no installed skin, art or captured stock.
use super::{Context, SlotContent, tree};
use crate::{
    online::{OnlineState, testing},
    trade::{Action, Chosen, MerchantSlot, TradeState},
    windows::WindowId,
};
use bevy::prelude::*;
use eq_client_assets::sidl::{Library, Screen};
use eq_client_core::{
    Capability, ClientCommand, Coins, WorldEvent,
    inventory::{InventorySlot, InventoryUpdate},
    merchant::{MerchantItem, MerchantUpdate},
};
use std::sync::mpsc::{self, Receiver};

// Deliberately reordered pieces, a sparse catalog, arbitrary names/sizes and a
// carried-item cell exercise the control context rather than a skin heuristic.
const XML: &str = r#"<XML>
    <InvSlot item="Far"><ScreenID>UnrelatedFar</ScreenID><EQType>6079</EQType>
        <Location><X>143</X><Y>11</Y></Location><Size><CX>37</CX><CY>31</CY></Size></InvSlot>
    <InvSlot item="Gap"><ScreenID>UnrelatedGap</ScreenID><EQType>6007</EQType>
        <Location><X>61</X><Y>11</Y></Location><Size><CX>37</CX><CY>31</CY></Size></InvSlot>
    <InvSlot item="Start"><ScreenID>UnrelatedStart</ScreenID><EQType>6000</EQType>
        <Location><X>17</X><Y>11</Y></Location><Size><CX>37</CX><CY>31</CY></Size></InvSlot>
    <InvSlot item="Carried"><EQType>22</EQType><Location><X>17</X><Y>55</Y></Location>
        <Size><CX>37</CX><CY>31</CY></Size></InvSlot>
    <Screen item="CatalogPanel"><Location><X>13</X><Y>19</Y></Location>
        <Size><CX>200</CX><CY>100</CY></Size>
        <Pieces>Far</Pieces><Pieces>Gap</Pieces><Pieces>Start</Pieces><Pieces>Carried</Pieces>
    </Screen>
    <Screen item="Sample"><Size><CX>260</CX><CY>180</CY></Size><Pieces>CatalogPanel</Pieces></Screen>
</XML>"#;

#[derive(Resource, Default)]
struct Layouts(Vec<(WindowId, Screen)>);

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn draw_layouts(mut commands: Commands, mut art: crate::sheets::Art, mut layouts: ResMut<Layouts>) {
    for (owner, screen) in std::mem::take(&mut layouts.0) {
        commands.spawn(Node::default()).with_children(|window| {
            tree::draw(
                window,
                &screen,
                &mut art,
                &Context {
                    id: owner,
                    paperdoll: None,
                    depth: 0,
                    tab_frame: None,
                },
            );
        });
    }
}

fn screen() -> Screen {
    Library::parse("<XML/>", "<XML/>")
        .unwrap()
        .screen(XML, "Sample")
        .unwrap()
}

fn ware(slot: u32, name: &str) -> WorldEvent {
    let mut item = crate::preview::items().remove(0);
    item.slot = InventorySlot(22); // Catalog key is independent of the item's inventory field.
    item.details.name = name.into();
    item.details.icon = None;
    WorldEvent::Merchant(MerchantUpdate::Item(Box::new(MerchantItem {
        slot,
        item,
        price: 12,
        quantity: 0,
    })))
}

fn news(app: &mut App, events: impl IntoIterator<Item = WorldEvent>) {
    testing::news(&mut app.world_mut().resource_mut::<OnlineState>(), events);
}

fn fixture(owner: WindowId) -> (App, Receiver<ClientCommand>) {
    let mut app = crate::testing::app();
    let mut online = OnlineState::new(true);
    online.plan_inventory_as(Some(eq_client_core::inventory::MoveRules::EqEmu));
    testing::news(
        &mut online,
        [WorldEvent::Entered {
            capabilities: vec![Capability::Inventory, Capability::Trading],
            choices: Vec::new(),
            session_id: 1,
            zone: "qeytoqrg".into(),
            player: Box::new(testing::player(7)),
            far_clip: None,
            spell_landing_flag: None,
        }],
    );
    testing::connect(&mut online, true);
    testing::inventory(
        &mut online,
        InventoryUpdate::Snapshot(crate::preview::items()),
    );
    testing::news(
        &mut online,
        [
            WorldEvent::Coins(Coins::default()),
            WorldEvent::Merchant(MerchantUpdate::Opened {
                merchant_id: 9,
                accepted: true,
                rate: 1.0,
            }),
            ware(7, "Maple Rod"),
            ware(0, "Amber Gem"),
        ],
    );
    let (sender, receiver) = mpsc::sync_channel(8);
    app.insert_resource(online)
        .insert_resource(crate::outbox::Outbox::new(Some(sender)))
        .insert_resource(Layouts(vec![(owner, screen())]))
        .add_systems(
            Update,
            (
                draw_layouts,
                crate::inventory::input,
                crate::trade::input,
                crate::trade::fill_wares,
            )
                .chain(),
        );
    app.world_mut()
        .resource_mut::<super::Skinned>()
        .0
        .insert(owner);
    app.world_mut()
        .resource_mut::<crate::windows::Shown>()
        .open(owner);
    (app, receiver)
}

fn cells(app: &mut App, slot: u32) -> Vec<Entity> {
    let mut query = app.world_mut().query::<(Entity, &MerchantSlot)>();
    query
        .iter(app.world())
        .filter_map(|(entity, cell)| (cell.slot == slot).then_some(entity))
        .collect()
}

fn named(app: &App, cell: Entity) -> Option<&str> {
    app.world()
        .get::<crate::tooltip::Tooltip>(cell)
        .map(|name| name.0.as_str())
}

fn contents(app: &App, cell: Entity) -> Vec<Entity> {
    app.world()
        .get::<Children>(cell)
        .into_iter()
        .flatten()
        .copied()
        .filter(|child| app.world().get::<SlotContent>(*child).is_some())
        .collect()
}

#[test]
fn merchant_grid_cells_use_context_and_wire_keys_instead_of_geometry_or_piece_order() {
    assert!(MerchantSlot::from_eq_type(5999).is_none());
    assert_eq!(MerchantSlot::from_eq_type(6000).unwrap().slot, 0);
    assert_eq!(MerchantSlot::from_eq_type(6079).unwrap().slot, 79);
    assert!(MerchantSlot::from_eq_type(6080).is_none());
    assert!(MerchantSlot::from_eq_type(u32::MAX).is_none());
    let (mut app, _requests) = fixture(WindowId::Merchant);
    app.update();
    let first = cells(&mut app, 0)[0];
    let seventh = cells(&mut app, 7)[0];
    let last = cells(&mut app, 79)[0];
    assert_eq!(named(&app, first), Some("Amber Gem"));
    assert_eq!(named(&app, seventh), Some("Maple Rod"));
    assert_eq!(named(&app, last), None);
    assert!(matches!(
        app.world().get::<Action>(seventh),
        Some(Action::Choose(7))
    ));
    assert!(app.world().get::<Action>(last).is_none());
    for cell in [first, seventh, last] {
        assert!(
            app.world()
                .get::<crate::inventory::SlotButton>(cell)
                .is_none()
        );
        assert!(
            app.world()
                .get::<crate::hud::hotbar::Pickable>(cell)
                .is_none()
        );
        assert!(app.world().get::<crate::clicks::OnPress>(cell).is_some());
        let node = app.world().get::<Node>(cell).unwrap();
        assert_eq!((node.width, node.height), (px(37.0), px(31.0)));
    }
    let mut carried = app.world_mut().query::<&crate::inventory::SlotButton>();
    assert!(
        carried
            .iter(app.world())
            .any(|slot| slot.0 == InventorySlot(22))
    );

    let (mut other, _requests) = fixture(WindowId::Inventory);
    other.update();
    assert!(
        cells(&mut other, 0).is_empty(),
        "inventory context gained a merchant cell"
    );
}

#[test]
fn merchant_grid_refresh_clears_stale_actions_and_preserves_authored_children() {
    let (mut app, _requests) = fixture(WindowId::Merchant);
    app.update();
    let first = cells(&mut app, 0)[0];
    let seventh = cells(&mut app, 7)[0];
    let decoration = app
        .world_mut()
        .spawn((Name::new("authored background"), ChildOf(seventh)))
        .id();
    let old = contents(&app, seventh);
    assert!(!old.is_empty(), "occupied catalog cell has no content");
    news(
        &mut app,
        [
            WorldEvent::Merchant(MerchantUpdate::Removed { slot: 0 }),
            ware(7, "Oak Staff"),
        ],
    );
    app.update();
    assert_eq!(named(&app, first), None);
    assert!(app.world().get::<Action>(first).is_none());
    assert!(
        contents(&app, first).is_empty(),
        "removed stock left drawn content"
    );
    assert_eq!(named(&app, seventh), Some("Oak Staff"));
    assert!(
        old.into_iter()
            .all(|child| app.world().get_entity(child).is_err())
    );
    assert!(app.world().get_entity(decoration).is_ok());
    app.world_mut().resource_mut::<OnlineState>().close_shop();
    app.update();
    assert_eq!(named(&app, seventh), None);
    assert!(app.world().get::<Action>(seventh).is_none());
    news(
        &mut app,
        [
            WorldEvent::Merchant(MerchantUpdate::Opened {
                merchant_id: 10,
                accepted: true,
                rate: 1.0,
            }),
            ware(0, "New Catalog"),
        ],
    );
    app.update();
    assert_eq!(named(&app, first), Some("New Catalog"));
    assert!(matches!(
        app.world().get::<Action>(first),
        Some(Action::Choose(0))
    ));
    assert_eq!(named(&app, seventh), None);
    assert!(app.world().get::<Action>(seventh).is_none());
}

#[test]
fn merchant_grid_selection_sends_no_purchase_and_never_enters_inventory_input() {
    let (mut app, requests) = fixture(WindowId::Merchant);
    app.update();
    let before = app
        .world()
        .resource::<OnlineState>()
        .world()
        .inventory()
        .items()
        .clone();
    let seventh = cells(&mut app, 7)[0];
    let chat_revision = app
        .world()
        .resource::<crate::chat::ChatState>()
        .history
        .revision();
    crate::clicks::testing::click(&mut app, seventh);
    assert_eq!(
        app.world().resource::<TradeState>().chosen(),
        Some(Chosen::Ware(7))
    );
    let online = app.world().resource::<OnlineState>();
    assert_eq!(online.world().inventory().items(), &before);
    assert_eq!(online.world().coins(), Some(&Coins::default()));
    assert_eq!(
        crate::trade::chosen(app.world().resource::<TradeState>(), online.world())
            .unwrap()
            .0
            .details
            .name,
        "Maple Rod"
    );
    assert!(
        requests.try_recv().is_err(),
        "selecting a catalog cell sent an operation"
    );
    assert_eq!(
        app.world()
            .resource::<crate::chat::ChatState>()
            .history
            .revision(),
        chat_revision
    );
    let empty = cells(&mut app, 79)[0];
    crate::clicks::testing::click(&mut app, empty);
    assert_eq!(
        app.world().resource::<TradeState>().chosen(),
        Some(Chosen::Ware(7))
    );
    assert!(
        requests.try_recv().is_err(),
        "an empty catalog cell sent an operation"
    );
    assert_eq!(
        app.world()
            .resource::<crate::chat::ChatState>()
            .history
            .revision(),
        chat_revision
    );
    // A stock update can precede the pass that removes the old cell action.
    // Run input before projection, as the viewer does, and ignore that action.
    let first = cells(&mut app, 0)[0];
    crate::clicks::testing::click(&mut app, first);
    assert_eq!(
        app.world().resource::<TradeState>().chosen(),
        Some(Chosen::Ware(0))
    );
    news(
        &mut app,
        [WorldEvent::Merchant(MerchantUpdate::Removed { slot: 7 })],
    );
    assert!(matches!(
        app.world().get::<Action>(seventh),
        Some(Action::Choose(7))
    ));
    crate::clicks::testing::click(&mut app, seventh);
    assert_eq!(
        app.world().resource::<TradeState>().chosen(),
        Some(Chosen::Ware(0))
    );
    assert!(app.world().get::<Action>(seventh).is_none());
    assert!(
        requests.try_recv().is_err(),
        "stale catalog selection sent an operation"
    );
}

#[test]
fn merchant_grid_late_controls_fill_without_rebuilding_unchanged_cells() {
    let (mut app, _requests) = fixture(WindowId::Merchant);
    app.update();
    let first = cells(&mut app, 0)[0];
    let drawn = contents(&app, first);
    news(&mut app, [WorldEvent::Coins(Coins::from_copper(1))]);
    app.update();
    assert_eq!(contents(&app, first), drawn);
    app.world_mut()
        .resource_mut::<Layouts>()
        .0
        .push((WindowId::Merchant, screen()));
    app.update();
    let firsts = cells(&mut app, 0);
    assert_eq!(firsts.len(), 2);
    assert!(
        firsts
            .into_iter()
            .all(|cell| named(&app, cell) == Some("Amber Gem"))
    );
    assert_eq!(contents(&app, first), drawn);
}
