//! A completed, exclusively world-owned Titanium cursor gesture.
//! Left-button identity and equality at 750 ms remain unverified.
use eq_client_core::{
    inventory::{InventoryItem, InventorySlot},
    world::ClientWorld,
};
use std::time::{Duration, Instant};

const CLICK_LIMIT: Duration = Duration::from_millis(750);

/// Titanium 0x4a91a4 falls through to 0x49812e only for these common-item
/// types. Containers/books are not excluded by their type byte (read 178).
pub(super) fn plain_world(item: &InventoryItem) -> bool {
    item.rules.titanium_item_class == Some(0) && matches!(item.rules.item_type, 0x0c | 0x21 | 0x27)
}

/// Capturing a click must not require a droppable item: a spawn wins first.
#[derive(Clone, PartialEq)]
pub(super) struct Cursor {
    session: u64,
    actor: u16,
    revision: u64,
    item: InventoryItem,
}

impl Cursor {
    pub(super) fn current(world: &ClientWorld) -> Option<Self> {
        let inventory = world.inventory();
        (world.in_world()
            && inventory.received()
            && !inventory.stale()
            && !inventory.predicted()
            && !world.inventory_moves_pending())
        .then(|| {
            Some(Self {
                session: world.session_id()?,
                actor: world.player()?.spawn_id,
                revision: inventory.revision(),
                item: inventory.items().get(&InventorySlot::CURSOR)?.clone(),
            })
        })
        .flatten()
    }
}

struct Press {
    cursor: Cursor,
    began: Instant,
    scripted: bool,
}

#[derive(Default)]
pub(super) struct WorldDrop {
    pressed: Option<Press>,
}

impl WorldDrop {
    pub(super) fn cancel(&mut self) {
        self.pressed = None;
    }

    /// Native edges retain arrival order. A release consumes its own press;
    /// a later press in that frame remains armed for its later release.
    pub(super) fn track(
        &mut self,
        available: bool,
        scripted: bool,
        cursor: Option<Cursor>,
        edges: &[bool],
        now: Instant,
    ) -> (bool, bool) {
        if !available {
            self.cancel();
            return (false, false);
        }
        let mut owned = self.pressed.is_some();
        if self.pressed.as_ref().is_some_and(|press| {
            press.scripted != scripted
                || now.saturating_duration_since(press.began) >= CLICK_LIMIT
                || cursor.as_ref() != Some(&press.cursor)
        }) {
            self.cancel();
        }
        let mut released = false;
        for down in edges {
            if *down {
                if self.pressed.is_none() {
                    self.pressed = cursor.clone().map(|cursor| Press {
                        cursor,
                        began: now,
                        scripted,
                    });
                }
                owned |= self.pressed.is_some();
            } else {
                released |= self.pressed.take().is_some();
            }
        }
        (owned || released, released)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn type_exclusions_apply_only_to_known_common_items() {
        let mut item = crate::preview::items().remove(0);
        for class in [None, Some(0), Some(1), Some(2)] {
            item.rules.titanium_item_class = class;
            for kind in [0, 0x0c, 0x21, 0x27] {
                item.rules.item_type = kind;
                assert_eq!(plain_world(&item), class == Some(0) && kind != 0);
            }
        }
    }
    fn cursor() -> Cursor {
        Cursor {
            session: 7,
            actor: 9,
            revision: 1,
            item: crate::preview::items().remove(0),
        }
    }
    #[test]
    fn ordered_edges_keep_two_same_frame_gestures_distinct() {
        let now = Instant::now();
        let mut owner = WorldDrop::default();
        assert_eq!(
            owner.track(true, false, Some(cursor()), &[true, false], now),
            (true, true)
        );
        assert_eq!(
            owner.track(true, false, Some(cursor()), &[false], now),
            (false, false)
        );
        owner.track(true, false, Some(cursor()), &[true], now);
        assert_eq!(
            owner.track(true, false, Some(cursor()), &[false, true], now),
            (true, true)
        );
        assert_eq!(
            owner.track(true, false, Some(cursor()), &[false], now),
            (true, true)
        );
    }
    #[test]
    fn world_press_has_one_release_and_a_conservative_750ms_boundary() {
        let now = Instant::now();
        for (milliseconds, submits) in [(749, true), (750, false), (751, false)] {
            let mut owner = WorldDrop::default();
            owner.track(true, false, Some(cursor()), &[true], now);
            assert_eq!(
                owner
                    .track(
                        true,
                        false,
                        Some(cursor()),
                        &[false],
                        now + Duration::from_millis(milliseconds)
                    )
                    .1,
                submits
            );
            assert!(!owner.track(true, false, Some(cursor()), &[false], now).1);
        }
    }
    #[test]
    fn lost_world_ownership_or_a_changed_item_cannot_resume_an_old_press() {
        let now = Instant::now();
        for reason in 0..4 {
            let mut owner = WorldDrop::default();
            owner.track(true, false, Some(cursor()), &[true], now);
            let mut changed = cursor();
            if reason == 1 {
                changed.revision += 1;
            }
            if reason == 2 {
                changed.session += 1;
            }
            owner.track(reason != 0, reason == 3, Some(changed), &[], now);
            assert!(!owner.track(true, false, Some(cursor()), &[false], now).1);
        }
    }
}
