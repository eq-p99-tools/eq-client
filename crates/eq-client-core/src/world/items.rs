//! Item definitions the server sent this admission, for inspection.
use crate::ItemDetails;
use std::collections::{BTreeMap, VecDeque};

/// Item definitions the server sent. Once full, the one kept longest is
/// forgotten first, never the newest.
#[derive(Clone, Debug, Default)]
pub struct ItemCache {
    items: BTreeMap<u32, ItemDetails>,
    /// Item IDs, the one kept longest first.
    order: VecDeque<u32>,
}

impl ItemCache {
    /// How many definitions are kept.
    pub const CAPACITY: usize = 128;

    /// The definition of an item, if the server sent it.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&ItemDetails> {
        self.items.get(&id)
    }

    /// How many definitions are kept now.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no definition is kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Keeps a definition; when full, forgets the one kept longest.
    pub(super) fn insert(&mut self, item: ItemDetails) {
        let id = item.id;
        self.order.retain(|kept| *kept != id);
        self.order.push_back(id);
        self.items.insert(id, item);
        while self.items.len() > Self::CAPACITY {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.items.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u32) -> ItemDetails {
        ItemDetails {
            equipment: None,
            bonuses: None,
            id,
            name: format!("Item {id}"),
            lore: String::new(),
            weight_tenths: 0,
            slots: 0,
            classes: 0,
            races: 0,
            flags: Vec::new(),
            stats: Vec::new(),
            price: None,
            icon: None,
        }
    }

    #[test]
    fn the_oldest_definition_goes_first_and_a_resent_one_is_new_again() {
        let mut cache = ItemCache::default();
        cache.insert(item(1));
        for id in 1000..1000 + 200 {
            cache.insert(item(id));
            if id == 1100 {
                // Sent again, the first definition counts as new.
                cache.insert(item(1));
            }
        }
        assert_eq!(cache.len(), ItemCache::CAPACITY);
        assert!(cache.get(1).is_some());
        assert!(cache.get(1000).is_none());
        assert!(cache.get(1199).is_some());
    }
}
