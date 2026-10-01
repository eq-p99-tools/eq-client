//! Who an Escape press belongs to. The official client peels one layer per
//! press: an inventory action under way is cancelled first, then an open loot
//! or merchant window closes, and only with nothing else open does Escape drop
//! the target. Each of those reads this decision instead of the key, so one
//! press never does two things.
use bevy::{prelude::*, window::PrimaryWindow};

/// What this frame's Escape press does, if anything.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub(super) enum Escape {
    /// No Escape this frame, or nothing it applies to.
    #[default]
    Unused,
    /// Cancels the inventory action under way, such as a split.
    Inventory,
    /// Closes the loot window.
    Loot,
    /// Closes the merchant window.
    Shop,
    /// Drops the target.
    Target,
}

/// Decides what this frame's Escape press does. An Escape that cancelled
/// typing belongs to the chat.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn route(
    keys: Res<ButtonInput<KeyCode>>,
    chat: Res<super::chat::ChatState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    inventory: Res<super::inventory::InventoryState>,
    trade: Res<super::trade::TradeState>,
    target: Res<super::target::TargetState>,
    mut escape: ResMut<Escape>,
) {
    let pressed = keys.just_pressed(KeyCode::Escape)
        && !chat.composing
        && !chat.escape_consumed
        && windows.single().is_ok_and(|window| window.focused);
    *escape = if !pressed {
        Escape::Unused
    } else if inventory.action_under_way() {
        Escape::Inventory
    } else if trade.looting() {
        Escape::Loot
    } else if trade.shopping() {
        Escape::Shop
    } else if target.selected.is_some() {
        Escape::Target
    } else {
        Escape::Unused
    };
}
