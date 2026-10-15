//! Bag and world container windows sized to what they hold, where the skin
//! leaves that to the client. A skin may keep a bag's second column of
//! slots to the window's right edge and its buttons to the bottom edge, in a
//! window too narrow for two columns and tall enough for the biggest bag;
//! the window then grows to fit both columns and ends just under the
//! container's last row, its buttons with it. A skin that places every slot
//! and button keeps its own size. How the official client sizes these
//! windows is inferred from how such skins anchor their pieces, unchecked
//! against it (recording-checklist item 6.229).

use super::WindowId;
use eq_client_assets::sidl::{Anchor, Element, Screen};
use eq_client_core::{inventory::InventorySlot, world::ClientWorld};

/// A container window's first place's number (`EQType`); the rest follow.
const FIRST_PLACE: u32 = 30;

/// The places a container window has room for.
const PLACES: u32 = 10;

/// How many places the window's container has: the bag's size, or a world
/// container's ten (inferred: the servers send no count for one).
pub(super) fn places(world: &ClientWorld, id: WindowId) -> Option<u8> {
    match id {
        WindowId::Bag(bag) => world
            .inventory()
            .items()
            .get(&InventorySlot(bag))
            .map(|bag| bag.bag_slots),
        WindowId::WorldContainer => world.container().map(|_| 10),
        _ => None,
    }
}

/// The window fitted to a container of this many places, where that
/// changes its size:
/// - as wide as the slots the skin places plus the farthest a slot it keeps
///   to the right edge reaches in from it, so the right column clears the
///   left one, and never narrower than the skin makes it;
/// - where the skin keeps buttons to the bottom edge, as tall as the bottom
///   of the container's last slot plus the farthest such a button reaches up
///   from the bottom edge.
pub(super) fn fitted(screen: &Screen, places: u8) -> Option<Screen> {
    let slots = screen
        .pieces
        .iter()
        .filter_map(|(_, element)| match element {
            Element::InvSlot(slot) => slot
                .slot
                .and_then(|number| number.checked_sub(FIRST_PLACE))
                .filter(|index| *index < PLACES)
                .map(|index| (index, slot)),
            _ => None,
        });
    let (mut placed_right, mut reach, mut last_bottom) = (0.0_f32, None::<f32>, None::<f32>);
    for (index, slot) in slots {
        let bottom = match slot.anchors {
            None => {
                placed_right = placed_right.max(slot.area.x + slot.area.width);
                Some(slot.area.y + slot.area.height)
            }
            Some(anchors) => {
                if let Anchor::End(left) = anchors.left {
                    reach = Some(reach.map_or(left, |reach| reach.max(left)));
                }
                match anchors.bottom {
                    Anchor::Start(bottom) => Some(bottom),
                    Anchor::End(_) => None,
                }
            }
        };
        if index < u32::from(places) {
            last_bottom = bottom.map(|bottom| last_bottom.map_or(bottom, |last| last.max(bottom)));
        }
    }
    let room = screen
        .pieces
        .iter()
        .filter_map(|(_, element)| match element {
            Element::Button(button) => match button.anchors?.top {
                Anchor::End(top) => Some(top),
                Anchor::Start(_) => None,
            },
            _ => None,
        })
        .reduce(f32::max);
    let mut area = screen.area;
    if let Some(reach) = reach {
        area.width = area.width.max(placed_right + reach);
    }
    if let (Some(room), Some(bottom)) = (room, last_bottom) {
        area.height = bottom + room;
    }
    (area != screen.area).then(|| Screen {
        area,
        ..screen.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_assets::{
        sidl::{Anchors, Button, InvSlot},
        ui::Area,
    };

    fn area(x: f32, y: f32, width: f32, height: f32) -> Area {
        Area {
            x,
            y,
            width,
            height,
        }
    }

    /// A container window shaped as some skins shape it, in made-up
    /// numbers: odd places placed down the left, even places kept 50 to 8
    /// from the right edge, Close kept 30 to 6 above the bottom and Combine
    /// 56 to 32, in a window 90 wide (too narrow for two columns) and 400 tall.
    fn anchored() -> Screen {
        let mut pieces = Vec::new();
        for index in 0..10_u8 {
            let row = f32::from(index / 2);
            let (top, bottom) = (30.0 + row * 42.0, 72.0 + row * 42.0);
            let slot = if index % 2 == 0 {
                InvSlot {
                    id: None,
                    area: area(6.0, top, 42.0, 42.0),
                    anchors: None,
                    slot: Some(30 + u32::from(index)),
                    background: None,
                }
            } else {
                InvSlot {
                    id: None,
                    area: area(0.0, 0.0, 0.0, 0.0),
                    anchors: Some(Anchors {
                        left: Anchor::End(50.0),
                        top: Anchor::Start(top),
                        right: Anchor::End(8.0),
                        bottom: Anchor::Start(bottom),
                    }),
                    slot: Some(30 + u32::from(index)),
                    background: None,
                }
            };
            pieces.push((format!("Slot{index}"), Element::InvSlot(slot)));
        }
        for (name, top, bottom) in [("Close", 30.0, 6.0), ("Combine", 56.0, 32.0)] {
            pieces.push((
                name.to_owned(),
                Element::Button(button(Some(Anchors {
                    left: Anchor::Start(6.0),
                    top: Anchor::End(top),
                    right: Anchor::End(6.0),
                    bottom: Anchor::End(bottom),
                }))),
            ));
        }
        screen(area(10.0, 20.0, 90.0, 400.0), pieces)
    }

    fn button(anchors: Option<Anchors>) -> Button {
        Button {
            id: None,
            area: area(10.0, 300.0, 60.0, 20.0),
            placed: anchors.is_none(),
            anchors,
            look: eq_client_assets::sidl::ButtonLook::default(),
            checkbox: false,
            text: None,
            text_color: None,
            font: None,
            decal: None,
            decal_area: None,
            tooltip: None,
        }
    }

    fn screen(area: Area, pieces: Vec<(String, Element)>) -> Screen {
        Screen {
            name: "Container".to_owned(),
            title: None,
            title_color: None,
            font: None,
            area,
            template: None,
            title_bar: None,
            border: false,
            transparent: true,
            tooltip: None,
            pieces,
            tab_frame: None,
        }
    }

    #[test]
    fn a_window_that_anchors_its_pieces_fits_the_bag() {
        let skin = anchored();
        // Eight places: four rows, the last ending at 30 + 4 * 42; wide
        // enough for the right column to clear the left (48 + 50).
        let eight = fitted(&skin, 8).unwrap();
        assert_eq!(eight.area, area(10.0, 20.0, 98.0, 198.0 + 56.0));
        // The right column then lands beside the left, not over it.
        let Element::InvSlot(second) = &eight.pieces[1].1 else {
            panic!("a slot")
        };
        let second = second.area_in(eight.area.width, eight.area.height);
        assert_eq!((second.x, second.y), (48.0, 30.0));
        // Ten places: five rows; four: two.
        assert_eq!(fitted(&skin, 10).unwrap().area.height, 240.0 + 56.0);
        assert_eq!(fitted(&skin, 4).unwrap().area.height, 114.0 + 56.0);
        // The buttons sit just under the last row.
        let Element::Button(close) = &eight.pieces[10].1 else {
            panic!("a button")
        };
        let close = close
            .anchors
            .unwrap()
            .within(eight.area.width, eight.area.height);
        assert_eq!(close.y, 198.0 + 26.0);
    }

    #[test]
    fn a_window_that_places_its_pieces_keeps_the_skins_size() {
        let mut skin = anchored();
        for (_, element) in &mut skin.pieces {
            match element {
                Element::InvSlot(slot) if slot.anchors.is_some() => {
                    slot.anchors = None;
                    slot.area = area(48.0, 30.0, 42.0, 42.0);
                }
                Element::Button(button) => *button = self::button(None),
                _ => {}
            }
        }
        assert_eq!(fitted(&skin, 8), None);
        assert_eq!(fitted(&skin, 10), None);
    }

    #[test]
    fn an_empty_bag_keeps_the_skins_height() {
        let fitted = fitted(&anchored(), 0).unwrap();
        assert_eq!((fitted.area.width, fitted.area.height), (98.0, 400.0));
    }
}
