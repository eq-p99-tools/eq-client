//! Display-only estimates for the local avatar's two held models.
use super::ClientWorld;
use crate::{
    inventory::{Inventory, InventoryItem, InventorySlot, InventoryUpdate},
    outfit::TextureSlot,
};
use std::num::NonZeroU16;

const HANDS: [InventorySlot; 2] = [InventorySlot(13), InventorySlot(14)];

/// How the inventory placement behind a held-model estimate was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldModelEstimate {
    /// A submitted move still supplies the placement, including after silence settlement.
    Submitted(u16),
    /// A received slot or snapshot corroborated the placement. The model is still the item's base model.
    ReceivedInventory(u16),
}

impl HeldModelEstimate {
    /// The model number to display, or zero for a projected empty hand.
    #[must_use]
    pub const fn material(self) -> u16 {
        match self {
            Self::Submitted(model) | Self::ReceivedInventory(model) => model,
        }
    }
}

#[derive(Default)]
pub(super) struct HeldModels([Option<Hand>; 2]);

/// An expected placement used only to correlate this display estimate.
struct Hand {
    item_id: Option<u32>,
    estimate: HeldModelEstimate,
}

impl HeldModels {
    /// Considers only hand changes in a prediction the worker actually submitted.
    pub(super) fn submitted(&mut self, inventory: &Inventory, update: &InventoryUpdate) {
        let InventoryUpdate::Prediction(items) = update else {
            return;
        };
        for (estimate, slot) in self.0.iter_mut().zip(HANDS) {
            let current = inventory.items().get(&slot);
            let next = items.iter().find(|item| item.slot == slot);
            if item_model(current) != item_model(next) {
                *estimate = material(next).map(|model| Hand {
                    item_id: item_id(next),
                    estimate: HeldModelEstimate::Submitted(model),
                });
            }
        }
    }

    /// An exact inventory receipt can corroborate a submission or retract a
    /// conflicting estimate. It cannot resurrect an estimate displaced by wear.
    pub(super) fn received(&mut self, inventory: &Inventory, update: &InventoryUpdate) {
        if inventory.stale() {
            *self = Self::default();
            return;
        }
        for (hand, slot) in self.0.iter_mut().zip(HANDS) {
            let Some(current) = hand else {
                continue;
            };
            let item = inventory.items().get(&slot);
            let model = current.estimate.material();
            if item_id(item) != current.item_id || material(item) != Some(model) {
                *hand = None;
            } else if covers(update, slot) {
                current.estimate = HeldModelEstimate::ReceivedInventory(model);
            }
        }
    }

    pub(super) fn clear_hand(&mut self, slot: TextureSlot) {
        if let Some(hand) = hand_index(slot) {
            self.0[hand] = None;
        }
    }
}

fn covers(update: &InventoryUpdate, slot: InventorySlot) -> bool {
    match update {
        InventoryUpdate::Snapshot(_) => true,
        InventoryUpdate::Set(items) => items.iter().any(|item| item.slot == slot),
        InventoryUpdate::Remove(removed) => *removed == slot,
        _ => false,
    }
}

fn hand_index(slot: TextureSlot) -> Option<usize> {
    match slot {
        TextureSlot::Primary => Some(0),
        TextureSlot::Secondary => Some(1),
        _ => None,
    }
}

fn item_model(item: Option<&InventoryItem>) -> Option<(u32, Option<NonZeroU16>)> {
    item.map(|item| (item.details.id, item.details.base_held_model))
}

fn item_id(item: Option<&InventoryItem>) -> Option<u32> {
    item.map(|item| item.details.id)
}

fn material(item: Option<&InventoryItem>) -> Option<u16> {
    item.map_or(Some(0), |item| {
        item.details.base_held_model.map(NonZeroU16::get)
    })
}

impl ClientWorld {
    /// The local avatar's temporary base model for one hand, where available.
    /// The admitted player's appearance remains the source for received wear.
    #[must_use]
    pub fn held_model_estimate(&self, slot: TextureSlot) -> Option<HeldModelEstimate> {
        self.held_models.0[hand_index(slot)?]
            .as_ref()
            .map(|hand| hand.estimate)
    }
}
