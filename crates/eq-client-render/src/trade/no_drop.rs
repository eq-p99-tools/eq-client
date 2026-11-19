//! The NO DROP loot question and `/lootnodrop`, as Titanium's loot click
//! (0x52b9d5) and command (0x47024f) do them (read R45b): a click takes the
//! item at once from the player's own corpse, an item that isn't NO DROP,
//! or as the option allows; otherwise the dialog asks (string 0x818) for 20
//! seconds, and a question not answered by then takes nothing.
use super::{PendingLoot, TradeState, no_drop as is_no_drop};
use crate::{
    chat::{ChatState, system_line},
    confirm::{Asked, Question},
    hud::messages::Messages,
    online::OnlineState,
    options::OptionsState,
    outbox::Outbox,
};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, SpawnKind, inventory::InventoryItem, no_drop::LootNoDrop, world::ClientWorld,
};
use std::time::{Duration, Instant};

/// The question's string.
pub(crate) const QUESTION: u32 = 0x818;
/// How long the question waits for an answer.
const WAIT: Duration = Duration::from_secs(20);

/// A NO DROP loot asked about, or waiting to be.
#[derive(Default)]
pub(super) struct Confirm {
    /// A click that asks, until the dialog takes it.
    wanted: Option<Question>,
    /// The question the dialog asks, and since when.
    asking: Option<(Question, Instant)>,
}

impl Confirm {
    pub(super) const fn want(&mut self, question: Question) {
        self.wanted = Some(question);
    }
}

/// Whether the corpse is the player's own. How the program tells is
/// unread; a player corpse named for the player stands in (checklist 6.487).
fn own_corpse(world: &ClientWorld, corpse_id: u16) -> bool {
    let shown = |raw: &str| eq_client_core::entities::display_name(&raw.replace('`', "'"));
    world
        .player()
        .zip(world.spawn(corpse_id))
        .is_some_and(|(player, corpse)| {
            corpse.state.kind == SpawnKind::PlayerCorpse
                && shown(&corpse.state.name) == format!("{}'s corpse", shown(&player.name))
        })
}

/// Whether the player can use the item (0x4192aa); an item whose masks
/// name no class or race the client knows counts as unusable, which asks.
fn usable(world: &ClientWorld, item: &InventoryItem) -> bool {
    world
        .player()
        .and_then(|player| player.class.zip(Some(player.race)))
        .and_then(|(class, race)| eq_client_core::resources::usable_by(&item.details, class, race))
        .unwrap_or(false)
}

/// Whether a click on this item of this corpse asks first under this
/// choice. The program's own item test is read only as a virtual call; the
/// NO DROP flag stands in for it (checklist 6.487).
pub(super) fn asks(
    world: &ClientWorld,
    corpse_id: u16,
    item: &InventoryItem,
    choice: LootNoDrop,
) -> bool {
    is_no_drop(item) && !own_corpse(world, corpse_id) && choice.asks(usable(world, item))
}

/// The question a click on the place asks, if it asks.
pub(super) fn question(
    world: &ClientWorld,
    session_id: u64,
    place: u16,
    choice: LootNoDrop,
) -> Option<Question> {
    let loot = world.loot()?;
    let item = loot.items.get(&place)?;
    asks(world, loot.corpse_id, item, choice).then_some(Question::LootNoDrop {
        session_id,
        corpse_id: loot.corpse_id,
        place,
        item_id: item.details.id,
    })
}

/// The item a question names, while the corpse still holds it there.
fn asked_item(world: &ClientWorld, question: Question) -> Option<&InventoryItem> {
    let Question::LootNoDrop {
        session_id,
        corpse_id,
        place,
        item_id,
    } = question
    else {
        return None;
    };
    let loot = world.loot()?;
    (world.session_id() == Some(session_id) && loot.corpse_id == corpse_id)
        .then(|| loot.items.get(&place))
        .flatten()
        .filter(|item| item.details.id == item_id)
}

/// What the dialog says: the installed client's words, or this client's.
pub(crate) fn text(world: &ClientWorld, question: Question, messages: Option<&Messages>) -> String {
    let Some(item) = asked_item(world, question) else {
        return String::new();
    };
    let name = &item.details.name;
    let ours = format!("{name} is NO DROP: once looted, it can't be traded. Loot it?");
    match messages {
        Some(messages) => messages.official(QUESTION, std::slice::from_ref(name), &ours),
        None => ours,
    }
}

/// Asks what a click wanted asked, and loots on Yes. No, the 20 seconds
/// running out, or the item or window going takes nothing.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn confirm(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut trade: ResMut<TradeState>,
    mut asked: ResMut<Asked>,
) {
    let world = online.world();
    if let Some((question, since)) = trade.no_drop.asking {
        let answer = asked.answer(question);
        let still = asked_item(world, question).is_some() && trade.loot.is_some();
        if answer == Some(true) && still {
            trade.no_drop.asking = None;
            take(&mut trade, world, &outbox, question);
        } else if answer.is_some() {
            trade.no_drop.asking = None;
        } else if !still || since.elapsed() >= WAIT || asked.question() != Some(question) {
            if asked.question() == Some(question) {
                asked.withdraw();
            }
            trade.no_drop.asking = None;
        }
    }
    if let Some(question) = trade.no_drop.wanted.take()
        && trade.no_drop.asking.is_none()
        && asked.ask(question)
    {
        trade.no_drop.asking = Some((question, Instant::now()));
    }
}

/// Loots the item the question named, as the click would have.
fn take(trade: &mut TradeState, world: &ClientWorld, outbox: &Outbox, question: Question) {
    let Question::LootNoDrop {
        corpse_id, place, ..
    } = question
    else {
        return;
    };
    let (Some(stamp), Some(own_id)) = (
        outbox.peek(world),
        world.player().map(|player| player.spawn_id),
    ) else {
        return;
    };
    let Some(window) = trade
        .loot
        .as_mut()
        .filter(|window| window.pending.is_none())
    else {
        return;
    };
    let command = ClientCommand::LootItem {
        session_id: stamp.session_id,
        corpse_id,
        own_id,
        place,
        auto: true,
        created: stamp.created,
    };
    if outbox.send(world, command).is_ok() {
        window.pending = Some(PendingLoot::new(
            stamp,
            eq_client_core::loot::LootRequest::Item {
                corpse_id,
                own_id,
                place,
                auto: true,
            },
        ));
    }
}

/// Answers a `/lootnodrop`: a word sets the choice and says it; no word,
/// or one that names no choice, lists the three (checklist 6.487 for the
/// latter). A client whose loot click never asks keeps today's answer.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn command(
    mut chat: ResMut<ChatState>,
    mut options: ResMut<OptionsState>,
    messages: Option<Res<Messages>>,
    settings: Option<Res<crate::ViewerSettings>>,
) {
    let Some(words) = chat.loot_no_drop.take() else {
        return;
    };
    let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
    if !client.asks_before_no_drop_loot() {
        chat.refuse(crate::chat::Said::own(
            "This client does not take /lootnodrop that way yet.",
        ));
        return;
    }
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let line = |choice: LootNoDrop| {
        let (id, ours) = choice.line();
        messages.said_text(id, ours)
    };
    let word = words.split_whitespace().next().unwrap_or_default();
    match LootNoDrop::typed(word, |choice| line(choice).words.text) {
        Some(choice) => {
            options.options.loot_no_drop = choice;
            chat.history.push(system_line(line(choice)));
        }
        None => {
            for choice in LootNoDrop::ALL {
                chat.history.push(system_line(line(choice)));
            }
        }
    }
}

#[cfg(test)]
mod tests;
