//! `/fastdrop`, which lets the inventory's Drop button put an item on the
//! ground without asking first (checklist 6.536). The program gives it a
//! handler of its own (0x4685ba), which is unread: that it turns the
//! drop's question off and on, its lines, whether it takes words and
//! whether it is saved are all inferred (pending R49-6n). Here it is off at
//! start, never saved, and off again at a camp or a lost link, so it never
//! carries to another character. Only the item drop's question is
//! skipped: a destroy and every other question still ask. Only the
//! player's own typing turns it over; a script or a social line cannot,
//! as it lifts a guard on something that cannot be undone.
use super::chat::{ChatState, Said};
use super::inventory::InventoryState;
use bevy::prelude::*;

/// A `/fastdrop` typed, waiting for [`answers`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Asked;

/// Whether a typed line, spelled out, is `/fastdrop`. Words after it are
/// ignored (pending R49-6n).
pub(crate) fn typed(input: &str) -> bool {
    input
        .strip_prefix('/')
        .and_then(|command| command.split_whitespace().next())
        .is_some_and(|name| name.eq_ignore_ascii_case("fastdrop"))
}

/// The line for the state the toggle lands in, in this client's words
/// (pending R49-6n).
fn line(fast: bool) -> Said {
    Said::own(if fast {
        "Items you drop on the ground will no longer ask first."
    } else {
        "Items you drop on the ground will ask first again."
    })
}

/// Turns the quick drop over for a `/fastdrop` the chat noted, and says the
/// state it lands in.
pub(super) fn answers(mut chat: ResMut<ChatState>, mut inventory: ResMut<InventoryState>) {
    if chat.fastdrop_asked.take().is_none() {
        return;
    }
    inventory.fast_drop = !inventory.fast_drop;
    chat.history
        .push(super::chat::system_line(line(inventory.fast_drop)));
}

#[cfg(test)]
mod tests;
