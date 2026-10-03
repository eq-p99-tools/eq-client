//! The skin's spellbook window (`SpellBookWnd`): two pages of the book's
//! places as the server keeps them, eight to a page, used through the
//! cursor and the gems as the installed client's own strings describe it
//! (inferred until a recording shows it): a left click puts a spell on the
//! cursor, and a click on a gem then memorizes it there; with a scroll on
//! the cursor, a left click on an empty place scribes it there.
use super::{BookView, SpellNames, action_pending, note, prepare_scribe};
use crate::chat::{ChatState, Said};
use crate::hud::action_bar::{ActionRequests, BookChange};
use crate::windows::WindowId;
use bevy::prelude::*;
use eq_client_core::inventory::InventorySlot;

/// The places on the two open pages: eight on each.
pub(crate) const PLACES: usize = 16;

/// A place on the open pages, by its number there (`SBW_Spell0` to
/// `SBW_Spell15`): the left page's eight, then the right's.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BookPlace(pub(crate) u8);

/// One of the skin's arrows that turn the book's pages: forward (true) or
/// back.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TurnsPages(pub(crate) bool);

/// A spell picked up from the skin's book: its place in the book and the
/// spell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Held {
    pub(crate) slot: usize,
    pub(crate) spell: u32,
}

/// The spell the player picked up from the skin's book, which rides the
/// cursor until a gem takes it, a click on an empty place drops it or the
/// book closes.
#[derive(Resource, Default)]
pub(crate) struct BookHand {
    pub(crate) held: Option<Held>,
}

impl BookView {
    /// The book's place a button on the open pages shows.
    pub(crate) const fn slot(&self, place: u8) -> usize {
        self.spread * PLACES + place as usize
    }

    /// The spell in the place a button on the open pages shows.
    pub(crate) fn spell(&self, book: Option<&eq_client_core::SpellBook>, place: u8) -> Option<u32> {
        book?.slots().get(self.slot(place)).copied().flatten()
    }
}

/// The places on the skin's book's open pages, under the pointer or not.
type Places<'w, 's> = Query<'w, 's, (&'static Interaction, &'static BookPlace)>;

/// The skin's book's arrows, pressed or let go.
type Arrows<'w, 's> =
    Query<'w, 's, (&'static Interaction, &'static TurnsPages), Changed<Interaction>>;

/// The skin's book under the player's clicks: its arrows turn its pages, a
/// left click on a spell puts the spell on the cursor and one on an empty
/// place takes it off, and with a scroll on the cursor a left click on an
/// empty place scribes the scroll there.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn clicks(
    (keys, mouse): (crate::keys::Keys, Res<ButtonInput<MouseButton>>),
    (shown, skinned): (Res<crate::windows::Shown>, Res<crate::skinned::Skinned>),
    (online, outbox): (Res<crate::online::OnlineState>, Res<crate::outbox::Outbox>),
    (names, messages): (Res<SpellNames>, Res<crate::hud::messages::Messages>),
    (mut view, mut hand, mut chat): (ResMut<BookView>, ResMut<BookHand>, ResMut<ChatState>),
    mut requests: Option<ResMut<ActionRequests>>,
    (places, arrows): (Places, Arrows),
) {
    let world = online.world();
    let book = world.spell_book();
    if !(skinned.has(WindowId::Spellbook) && shown.is_open(WindowId::Spellbook)) {
        hand.held = None;
        return;
    }
    // A spell the book no longer keeps in its place leaves the cursor.
    if hand.held.is_some_and(|held| {
        book.and_then(|book| book.slots().get(held.slot))
            .copied()
            .flatten()
            != Some(held.spell)
    }) {
        hand.held = None;
    }
    let spreads = book.map_or(1, |book| book.slots().len().div_ceil(PLACES).max(1));
    view.spread = view.spread.min(spreads - 1);
    if !keys.focused() {
        return;
    }
    for (interaction, TurnsPages(forward)) in &arrows {
        if *interaction == Interaction::Pressed {
            view.spread = if *forward {
                (view.spread + 1).min(spreads - 1)
            } else {
                view.spread.saturating_sub(1)
            };
        }
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(place) = places
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, place)| place.0)
    else {
        return;
    };
    let slot = view.slot(place);
    let in_place = view.spell(book, place);
    let cursor = world.inventory().items().get(&InventorySlot::CURSOR);
    match (cursor.and_then(|item| item.scroll_spell), cursor, in_place) {
        (Some(scroll), ..) => {
            let refusal = scribe(
                (&online, &outbox),
                (slot, in_place, scroll),
                (&names, &messages),
                requests.as_deref_mut(),
            );
            if let Some(said) = refusal {
                chat.refuse(said);
            }
        }
        // Another item on the cursor leaves the book as it is.
        (None, Some(_), _) => (),
        (None, None, Some(spell)) => hand.held = Some(Held { slot, spell }),
        (None, None, None) => hand.held = None,
    }
}

/// Names the spell in each place of the skin's book under the pointer, with
/// its base mana, cast time and range, as the client's own list does.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn tooltips(
    online: Res<crate::online::OnlineState>,
    (view, names): (Res<BookView>, Res<SpellNames>),
    mut places: Query<(&BookPlace, &mut crate::tooltip::Tooltip)>,
) {
    let book = online.world().spell_book();
    for (place, mut tooltip) in &mut places {
        let wanted = view.spell(book, place.0).map_or_else(String::new, |spell| {
            format!("{}\n{}", names.label(spell), names.details(spell))
        });
        if tooltip.0 != wanted {
            tooltip.0 = wanted;
        }
    }
}

/// Scribes the scroll on the cursor into this empty place of the book; what
/// the chat says if it does not.
fn scribe(
    (online, outbox): (&crate::online::OnlineState, &crate::outbox::Outbox),
    (slot, in_place, scroll): (usize, Option<u32>, u32),
    (names, messages): (&SpellNames, &crate::hud::messages::Messages),
    requests: Option<&mut ActionRequests>,
) -> Option<Said> {
    let world = online.world();
    let book = world.spell_book();
    if in_place.is_some() {
        return Some(messages.said_text(12051, "That place in the book holds a spell already."));
    }
    if book.is_some_and(|book| book.slots().contains(&Some(scroll))) {
        return Some(messages.said_or(
            12050,
            &[names.label(scroll)],
            "The book has that spell already.",
        ));
    }
    if action_pending(world) {
        return Some(Said::own("Wait for the current spell action"));
    }
    // The outbox says why a scribe it holds back did not go.
    let stamp = outbox.stamp(world).ok()?;
    let command = match prepare_scribe(Some(online), book, Some(stamp), Some(slot)) {
        Ok(command) => command,
        Err(error) => return Some(Said::own(error.to_string())),
    };
    outbox.send(world, command).ok()?;
    note(
        requests,
        format!("Scribing {}", names.label(scroll)),
        BookChange::Scribe,
    );
    None
}

/// Memorizes the spell picked up from the book into the gem the player
/// clicked; what the chat says if it does not. A spell memorized already
/// stays where it is, as the official client refuses it.
pub(crate) fn memorize(
    held: Held,
    gem: u8,
    (online, outbox): (&crate::online::OnlineState, &crate::outbox::Outbox),
    (names, messages): (&SpellNames, &crate::hud::messages::Messages),
    requests: Option<&mut ActionRequests>,
) -> Option<Said> {
    let world = online.world();
    if world.gems().contains(&Some(held.spell)) {
        return Some(messages.said_text(12052, "That spell is in a gem already."));
    }
    if action_pending(world) {
        return Some(Said::own("Wait for the current spell action"));
    }
    // The outbox says why a memorization it holds back did not go.
    let spell = super::request_memorize(
        Some(online),
        world.spell_book(),
        Some(outbox),
        Some(held.spell),
        gem,
    )
    .ok()?;
    note(
        requests,
        format!("Memorizing {} into gem {}", names.label(spell), gem + 1),
        BookChange::Memorize,
    );
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::ClientCommand;

    fn scroll(spell: u32) -> eq_client_core::inventory::InventoryItem {
        eq_client_core::inventory::InventoryItem {
            activation: eq_client_core::inventory::ItemActivation::default(),
            scroll_spell: Some(spell),
            book: None,
            rules: eq_client_core::inventory::ItemPlacement::default(),
            slot: InventorySlot::CURSOR,
            details: eq_client_core::ItemDetails {
                equipment: None,
                bonuses: None,
                id: 15000 + spell,
                name: "Synthetic scroll".into(),
                lore: String::new(),
                weight_tenths: 1,
                slots: 0,
                classes: 2,
                races: 1,
                flags: Vec::new(),
                stats: Vec::new(),
            },
            icon: 0,
            stack_count: None,
            charges: 1,
            bag_slots: 0,
        }
    }

    /// A player whose book holds spell 42 in its second place.
    fn online() -> crate::online::OnlineState {
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 7, crate::online::testing::player(1));
        let mut book = eq_client_core::SpellBook::default();
        book.apply(&eq_client_core::SpellUpdate::Slot {
            slot: 1,
            spell_id: 42,
            mode: 0,
        });
        crate::online::testing::news(&mut online, [eq_client_core::WorldEvent::SpellBook(book)]);
        online
    }

    #[test]
    fn a_place_on_the_open_pages_is_its_place_in_the_book() {
        let view = BookView { page: 0, spread: 2 };
        assert_eq!(view.slot(0), 32);
        assert_eq!(view.slot(15), 47);
        let online = online();
        let first = BookView::default();
        assert_eq!(first.spell(online.world().spell_book(), 1), Some(42));
        assert_eq!(first.spell(online.world().spell_book(), 0), None);
        assert_eq!(view.spell(None, 1), None);
    }

    #[test]
    fn a_scroll_is_scribed_into_the_empty_place_clicked_and_refused_elsewhere() {
        let mut online = online();
        crate::online::testing::inventory(
            &mut online,
            eq_client_core::inventory::InventoryUpdate::Snapshot(vec![scroll(73)]),
        );
        let (queue, sent) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        let names = SpellNames::parse("73^Synthetic spell");
        let messages = crate::hud::messages::Messages::default();
        let mut requests = ActionRequests::default();
        // The place holds a spell.
        let refused = scribe(
            (&online, &outbox),
            (1, Some(42), 73),
            (&names, &messages),
            Some(&mut requests),
        );
        assert!(refused.is_some());
        assert!(sent.try_recv().is_err());
        // An empty place takes it, where it was clicked.
        let scribed = scribe(
            (&online, &outbox),
            (5, None, 73),
            (&names, &messages),
            Some(&mut requests),
        );
        assert_eq!(scribed, None);
        assert!(matches!(
            sent.try_recv().unwrap(),
            ClientCommand::ScribeSpell {
                slot: 5,
                spell_id: 73,
                ..
            }
        ));
        let request = requests.book.unwrap();
        assert_eq!(request.change, BookChange::Scribe);
        assert_eq!(request.label, "Scribing Synthetic spell");
        // A scroll for a spell the book has already is refused.
        let refused = scribe((&online, &outbox), (6, None, 42), (&names, &messages), None);
        assert!(refused.is_some());
        assert!(sent.try_recv().is_err());
    }

    #[test]
    fn a_held_spell_goes_into_the_gem_clicked_unless_a_gem_has_it() {
        let online = online();
        let (queue, sent) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        let names = SpellNames::parse("42^Courage");
        let messages = crate::hud::messages::Messages::default();
        let mut requests = ActionRequests::default();
        let held = Held { slot: 1, spell: 42 };
        assert_eq!(
            memorize(
                held,
                2,
                (&online, &outbox),
                (&names, &messages),
                Some(&mut requests)
            ),
            None
        );
        assert!(matches!(
            sent.try_recv().unwrap(),
            ClientCommand::MemorizeSpell {
                gem: 2,
                spell_id: 42,
                ..
            }
        ));
        let request = requests.book.unwrap();
        assert_eq!(request.change, BookChange::Memorize);
        assert_eq!(request.label, "Memorizing Courage into gem 3");
        let mut player = crate::online::testing::player(1);
        player.memorized_spells[4] = Some(42);
        let mut memorized = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut memorized, 7, player);
        assert!(memorize(held, 2, (&memorized, &outbox), (&names, &messages), None).is_some());
        assert!(sent.try_recv().is_err());
    }
}
