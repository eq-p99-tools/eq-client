//! `/autoinventory`, as Titanium's handler runs it (read R46 section 1):
//! the cursor item goes to the first place the program's order finds, worn
//! slots first, else stays with the installed line 0xd2f.
use super::InventoryState;
use crate::chat::{ChatState, Said};
use crate::hud::messages::Messages;
use crate::online::OnlineState;
use crate::outbox::Outbox;
use bevy::prelude::*;

/// An `/autoinventory` asked; its words are ignored, as the handler reads
/// none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Asked;

/// The line when the item fits nowhere and stays on the cursor.
const NO_ROOM: u32 = 0xd2f;

/// Answers an `/autoinventory` the chat noted.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn run(
    mut chat: ResMut<ChatState>,
    mut state: ResMut<InventoryState>,
    mut online: ResMut<OnlineState>,
    sender: Res<Outbox>,
    (messages, settings): (Option<Res<Messages>>, Option<Res<crate::ViewerSettings>>),
) {
    if chat.autoinventory.take().is_none() {
        return;
    }
    let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
    if !client.runs_r46_forms() {
        chat.refuse(Said::own(
            "This client does not take /autoinventory that way yet.",
        ));
        return;
    }
    match state.auto_inventory(&mut online, &sender) {
        Ok(true) => (),
        Ok(false) => {
            let empty = Messages::default();
            let messages = messages.as_deref().unwrap_or(&empty);
            chat.refuse(messages.said_text(NO_ROOM, "There is no room for that item."));
        }
        Err(error) => chat.refuse(crate::outbox::window_line(&error)),
    }
}
