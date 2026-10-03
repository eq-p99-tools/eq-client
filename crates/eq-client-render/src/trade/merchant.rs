//! The skin's merchant window (`EQUI_MerchantWnd.xml`): the merchant's
//! wares in its list, and the item chosen to buy or to sell, with its
//! picture, name and price, which its Buy and Sell buttons act on.
use super::{Action, Chosen, TradeState};
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_core::{
    inventory::{InventoryItem, InventorySlot},
    world::ClientWorld,
};

/// How tall each of the list's rows is: as tall as the icon its first
/// column shows. The official client's row height is not checked yet.
const ROW_HEIGHT: f32 = 20.0;

/// The skin's list of the merchant's wares: each column's width, and the
/// rows last drawn.
#[derive(Component, Debug)]
pub(crate) struct MerchantRows {
    columns: Vec<f32>,
    drawn: Option<Vec<Row>>,
}

impl MerchantRows {
    pub(crate) const fn new(columns: Vec<f32>) -> Self {
        Self {
            columns,
            drawn: None,
        }
    }
}

/// A row as drawn: the ware's list slot, its icon and name, how many the
/// merchant has (none for as many as wanted), its price in copper, and
/// whether it is the one chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    slot: u32,
    icon: u32,
    name: String,
    quantity: u32,
    price: u32,
    chosen: bool,
}

/// The merchant's wares, in the order of their list slots.
fn rows(world: &ClientWorld, chosen: Option<Chosen>) -> Vec<Row> {
    world
        .merchant()
        .map(|merchant| {
            merchant
                .stock
                .values()
                .map(|ware| Row {
                    slot: ware.slot,
                    icon: ware.item.icon,
                    name: ware.item.details.name.clone(),
                    quantity: ware.quantity,
                    price: ware.price,
                    chosen: chosen == Some(Chosen::Ware(ware.slot)),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A price's coins, from platinum down to copper.
const fn coins(copper: u32) -> [u32; 4] {
    [
        copper / 1000,
        copper / 100 % 10,
        copper / 10 % 10,
        copper % 10,
    ]
}

/// Fills the skin's list with the merchant's wares, again whenever they or
/// the choice change: each row the ware's icon, its name, how many the
/// merchant has, and its price in the columns for each coin. A click on a
/// row chooses it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn fill(
    mut commands: Commands,
    online: Res<crate::online::OnlineState>,
    trade: Res<TradeState>,
    mut art: crate::sheets::Art,
    mut lists: Query<(Entity, &mut MerchantRows, Option<&Children>)>,
) {
    let wanted = rows(online.world(), trade.chosen());
    for (entity, mut list, children) in &mut lists {
        if list.drawn.as_ref() == Some(&wanted) {
            continue;
        }
        for child in children.into_iter().flatten() {
            commands.entity(*child).despawn();
        }
        for row in &wanted {
            let line = commands
                .spawn((
                    Button,
                    Action::Choose(row.slot),
                    crate::outbox::Needs::Capability(super::needs(Action::Choose(row.slot))),
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(if row.chosen {
                        theme::BUTTON
                    } else {
                        Color::NONE
                    }),
                    ChildOf(entity),
                ))
                .id();
            let mut widths = list.columns.iter();
            if let Some(width) = widths.next() {
                let cell = commands
                    .spawn((
                        Node {
                            width: px(*width),
                            height: px(ROW_HEIGHT),
                            flex_shrink: 0.0,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        ChildOf(line),
                    ))
                    .id();
                if let Some(icon) = art.item(row.icon) {
                    commands.spawn((
                        icon,
                        Node {
                            width: px(ROW_HEIGHT),
                            height: px(ROW_HEIGHT),
                            ..default()
                        },
                        ChildOf(cell),
                    ));
                }
            }
            let [platinum, gold, silver, copper] = coins(row.price);
            let quantity = if row.quantity > 0 {
                row.quantity.to_string()
            } else {
                String::new()
            };
            let cells = [
                row.name.clone(),
                quantity,
                platinum.to_string(),
                gold.to_string(),
                silver.to_string(),
                copper.to_string(),
            ];
            for (words, width) in cells.into_iter().zip(widths) {
                commands.spawn((
                    theme::text(words, Size::Small, theme::INK_BRIGHT),
                    TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    Node {
                        width: px(*width),
                        flex_shrink: 0.0,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    ChildOf(line),
                ));
            }
        }
        list.drawn = Some(wanted.clone());
    }
}

/// The item chosen in the skin's merchant window, and the price the
/// merchant asks for it when it is one of its wares; what the merchant pays
/// for a carried item is not known yet.
pub(crate) fn chosen<'a>(
    trade: &TradeState,
    world: &'a ClientWorld,
) -> Option<(&'a InventoryItem, Option<u32>)> {
    match trade.chosen()? {
        Chosen::Ware(slot) => world
            .merchant()?
            .stock
            .get(&slot)
            .map(|ware| (&ware.item, Some(ware.price))),
        Chosen::Carried(slot) => world
            .inventory()
            .items()
            .get(&InventorySlot(slot))
            .map(|item| (item, None)),
    }
}

/// The skin's box for the chosen item's picture, with where in it the
/// picture goes.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ChosenPicture {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) size: f32,
}

/// The chosen item's picture in its box.
#[derive(Component)]
pub(crate) struct ChosenIcon;

/// What the chosen item's box last showed: its icon and name, if anything.
type Shown<'s> = Local<'s, Option<Option<(u32, String)>>>;

/// Draws the chosen item's picture in the skin's box, named on hover, when
/// the choice or the items change.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn picture(
    mut commands: Commands,
    online: Res<crate::online::OnlineState>,
    trade: Res<TradeState>,
    mut art: crate::sheets::Art,
    (mut last, added): (Shown, Query<(), Added<ChosenPicture>>),
    boxes: Query<(Entity, &ChosenPicture, Option<&Children>)>,
    old: Query<(), With<ChosenIcon>>,
) {
    let item = chosen(&trade, online.world());
    let shown = item.map(|(item, _)| (item.icon, item.details.name.clone()));
    if last.as_ref() == Some(&shown) && added.is_empty() {
        return;
    }
    *last = Some(shown.clone());
    for (entity, place, children) in &boxes {
        for child in children.into_iter().flatten() {
            if old.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
        let Some((icon, name)) = &shown else {
            commands.entity(entity).remove::<crate::tooltip::Tooltip>();
            continue;
        };
        commands
            .entity(entity)
            .insert(crate::tooltip::Tooltip(name.clone()));
        if let Some(image) = art.item(*icon) {
            commands.spawn((
                ChosenIcon,
                image,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(place.x),
                    top: px(place.y),
                    width: px(place.size),
                    height: px(place.size),
                    ..default()
                },
                ChildOf(entity),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_price_splits_into_its_coins() {
        assert_eq!(coins(12_345), [12, 3, 4, 5]);
        assert_eq!(coins(7), [0, 0, 0, 7]);
    }
}
