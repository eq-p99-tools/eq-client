//! The skin's spellbook window (`SpellBookWnd`): two pages of the book's
//! places as the server keeps them, eight to a page, used through the
//! cursor and the gems as the installed client's own strings describe it
//! (inferred until a recording shows it): a left click puts a spell on the
//! cursor, and a click on a gem then memorizes it there; with a scroll on
//! the cursor, a left click on an empty place scribes it there. A right
//! click chooses a spell, a right click on another place then swaps the
//! two, and the Delete key deletes the chosen spell, each where the session
//! offers it; the client asks first unless the player turned that off
//! ([`Fix::AskBeforeDeletingSpells`]).
use super::{BookView, SpellNames, action_pending, note, prepare_scribe};
use crate::chat::{ChatState, Said, system_line};
use crate::confirm::{Asked, Question};
use crate::hud::action_bar::{ActionRequests, BookChange};
use crate::windows::WindowId;
use bevy::prelude::*;
use eq_client_core::{
    BookActionStatus, Capability, ClientCommand, inventory::InventorySlot, qol::Fix,
};

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

/// The mark on a place whose spell a right click chose: the client's own,
/// as how the official client shows the choice is not known.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChosenMark(pub(crate) u8);

/// A spell in the skin's book: its place in the book and the spell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) slot: usize,
    pub(crate) spell: u32,
}

/// What the player does with the skin's book.
#[derive(Resource, Default)]
pub(crate) struct BookHand {
    /// The spell picked up from the book, which rides the cursor until a
    /// gem takes it, a click on an empty place drops it or the book closes.
    pub(crate) held: Option<Entry>,
    /// The spell a right click chose, which a right click on another place
    /// swaps with that place and the Delete key deletes.
    pub(crate) chosen: Option<Entry>,
    /// A deletion or a move the server has not answered yet, with the
    /// book's reply count when it went.
    editing: Option<(Edit, u64)>,
}

/// A change to the book's entries in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edit {
    Deleting,
    Moving,
}

impl Entry {
    /// The question whether to delete it.
    const fn deleting(self) -> Question {
        Question::DeleteSpell {
            slot: self.slot,
            spell: self.spell,
        }
    }

    /// Whether the book still keeps this spell in this place.
    fn kept(self, book: Option<&eq_client_core::SpellBook>) -> bool {
        book.and_then(|book| book.slots().get(self.slot))
            .copied()
            .flatten()
            == Some(self.spell)
    }
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
/// empty place scribes the scroll there. A right click chooses a spell or
/// swaps the chosen one with the place clicked, and the Delete key deletes
/// the chosen spell, asking first where the player wants that, only while
/// no box takes the keyboard.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn clicks(
    (keys, mouse): (crate::keys::Keys, Res<ButtonInput<MouseButton>>),
    (shown, skinned): (Res<crate::windows::Shown>, Res<crate::skinned::Skinned>),
    (online, outbox): (Res<crate::online::OnlineState>, Res<crate::outbox::Outbox>),
    (names, messages): (Res<SpellNames>, Res<crate::hud::messages::Messages>),
    (mut view, mut hand, mut chat): (ResMut<BookView>, ResMut<BookHand>, ResMut<ChatState>),
    (mut requests, mut asked, options): (
        Option<ResMut<ActionRequests>>,
        ResMut<Asked>,
        Res<crate::options::OptionsState>,
    ),
    (places, arrows): (Places, Arrows),
) {
    let world = online.world();
    let book = world.spell_book();
    answer(&mut hand, world, &messages, &mut chat);
    let open = skinned.has(WindowId::Spellbook) && shown.is_open(WindowId::Spellbook);
    // A spell the book no longer keeps in its place leaves the cursor, and
    // is no longer chosen, as both do when the book closes.
    if !open || hand.held.is_some_and(|held| !held.kept(book)) {
        hand.held = None;
    }
    if !open || hand.chosen.is_some_and(|chosen| !chosen.kept(book)) {
        hand.chosen = None;
    }
    follow_deleting(
        (&mut hand, &mut asked),
        (world, &outbox),
        (requests.as_deref_mut(), &mut chat),
    );
    if !open {
        return;
    }
    // An item or coins taken onto the cursor take the place of a spell from
    // the book, so a gem can no longer memorize it; what the official client
    // does is not checked yet.
    if world
        .inventory()
        .items()
        .contains_key(&InventorySlot::CURSOR)
        || crate::coins::on_cursor(world).is_some()
    {
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
    if keys.input.just_pressed(KeyCode::Delete) {
        delete_key(
            (&mut hand, &mut asked),
            (world, &outbox),
            (requests.as_deref_mut(), &mut chat),
            &options.options.qol,
        );
    }
    // A right click lands on the place under the pointer.
    if mouse.just_pressed(MouseButton::Right)
        && let Some((_, place)) = places
            .iter()
            .find(|(interaction, _)| **interaction != Interaction::None)
    {
        let entry = (view.slot(place.0), view.spell(book, place.0));
        let said = choose(
            entry,
            &mut hand,
            (world, &outbox),
            (&messages, requests.as_deref_mut()),
        );
        for said in said {
            if said.1 {
                chat.refuse(said.0);
            } else {
                chat.history.push(system_line(said.0));
            }
        }
        return;
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
        (None, None, Some(spell)) => hand.held = Some(Entry { slot, spell }),
        (None, None, None) => hand.held = None,
    }
}

/// Names the spell in each place of the skin's book under the pointer, with
/// its base mana, cast time and range, as the client's own list does, and
/// marks the place of the spell a right click chose.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn present(
    online: Res<crate::online::OnlineState>,
    (view, names, hand): (Res<BookView>, Res<SpellNames>, Res<BookHand>),
    mut places: Query<(&BookPlace, &mut crate::tooltip::Tooltip)>,
    mut marks: Query<(&ChosenMark, &mut Node)>,
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
    for (ChosenMark(place), mut node) in &mut marks {
        let chosen = hand
            .chosen
            .is_some_and(|chosen| chosen.slot == view.slot(*place));
        let display = if chosen { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// What the chat says while a memorization, a scribe or another change to
/// the book is under way, which a move or a deletion waits for, as the
/// session refuses one meanwhile.
const BUSY: &str = "Wait for the current spell action";

/// A right click on a place: with no spell chosen, chooses the place's
/// spell, as the official client says how to swap and delete it; on the
/// chosen spell, unchooses it; on another place, swaps the chosen spell with
/// whatever is there, unless another change to the book is under way. What
/// the chat says, each line marked when it refuses.
fn choose(
    (slot, in_place): (usize, Option<u32>),
    hand: &mut BookHand,
    (world, outbox): (&eq_client_core::world::ClientWorld, &crate::outbox::Outbox),
    (messages, requests): (&crate::hud::messages::Messages, Option<&mut ActionRequests>),
) -> Vec<(Said, bool)> {
    let offered = |capability| crate::outbox::offered(world, capability);
    match hand.chosen.take() {
        Some(chosen) if chosen.slot == slot => Vec::new(),
        Some(chosen) => {
            if action_pending(world) {
                hand.chosen = Some(chosen);
                return vec![(Said::own(BUSY), true)];
            }
            let (Ok(from), Ok(to)) = (u16::try_from(chosen.slot), u16::try_from(slot)) else {
                return Vec::new();
            };
            // The outbox says why a move it holds back did not go.
            let sent = outbox.post(world, |stamp| ClientCommand::SwapSpell {
                session_id: stamp.session_id,
                from,
                to,
                from_spell: chosen.spell,
                to_spell: in_place,
                created: stamp.created,
            });
            if sent.is_err() {
                return Vec::new();
            }
            hand.editing = Some((Edit::Moving, world.book_action_revision()));
            if let Some(requests) = requests {
                // The move is the book's change now, not a memorization or a
                // scribe for the gauges and the chat's lines to follow.
                requests.book = None;
            }
            vec![(messages.said_text(1388, "Moving the spell."), false)]
        }
        None => {
            let Some(spell) = in_place else {
                return Vec::new();
            };
            let (moving, deleting) = (
                offered(Capability::MovingSpells),
                offered(Capability::DeletingSpells),
            );
            if !moving && !deleting {
                return vec![(Said::own(crate::outbox::UNAVAILABLE), true)];
            }
            hand.chosen = Some(Entry { slot, spell });
            let mut said = Vec::new();
            if moving {
                let fallback = "Right-click another place to swap the two.";
                said.push((messages.said_text(12046, fallback), false));
            }
            if deleting {
                let fallback = "The Delete key deletes the chosen spell for good.";
                said.push((messages.said_text(4027, fallback), false));
            }
            said
        }
    }
}

/// The Delete key: asks whether to delete the chosen spell where the player
/// wants to be asked and the session deletes spells at all, or else deletes
/// it. The chat says why either waits, and the outbox why a deletion it
/// holds back did not go.
fn delete_key(
    (hand, asked): (&mut BookHand, &mut Asked),
    (world, outbox): (&eq_client_core::world::ClientWorld, &crate::outbox::Outbox),
    (requests, chat): (Option<&mut ActionRequests>, &mut ChatState),
    qol: &eq_client_core::qol::Settings,
) {
    let refusal = if qol.on(Fix::AskBeforeDeletingSpells)
        && crate::outbox::offered(world, Capability::DeletingSpells)
    {
        ask_deleting(hand, world, asked)
    } else {
        delete(hand, world, outbox, requests)
    };
    if let Some(refusal) = refusal {
        chat.refuse(refusal);
    }
}

/// The Delete key where the player is asked first: asks whether to delete
/// the spell a right click chose, unless another change to the book is
/// under way; what the chat says if it waits.
fn ask_deleting(
    hand: &BookHand,
    world: &eq_client_core::world::ClientWorld,
    asked: &mut Asked,
) -> Option<Said> {
    let chosen = hand.chosen?;
    if action_pending(world) {
        return Some(Said::own(BUSY));
    }
    asked.ask(chosen.deleting());
    None
}

/// Follows the question whether to delete the chosen spell: takes it back
/// once that spell is no longer the chosen one, as when the book closes,
/// and once it is answered, Yes deletes the spell, unless another change to
/// the book is under way, which the chat then says, and No keeps it, no
/// longer chosen.
fn follow_deleting(
    (hand, asked): (&mut BookHand, &mut Asked),
    (world, outbox): (&eq_client_core::world::ClientWorld, &crate::outbox::Outbox),
    (requests, chat): (Option<&mut ActionRequests>, &mut ChatState),
) {
    if let Some(question @ Question::DeleteSpell { .. }) = asked.question()
        && hand.chosen.map(Entry::deleting) != Some(question)
    {
        asked.withdraw();
    }
    match hand
        .chosen
        .and_then(|chosen| asked.answer(chosen.deleting()))
    {
        Some(true) => {
            if let Some(refusal) = delete(hand, world, outbox, requests) {
                chat.refuse(refusal);
            }
        }
        Some(false) => hand.chosen = None,
        None => (),
    }
}

/// The Delete key: deletes the spell a right click chose, unless another
/// change to the book is under way; what the chat says if it waits. Its
/// answer is said once the server gives it (`answer`).
fn delete(
    hand: &mut BookHand,
    world: &eq_client_core::world::ClientWorld,
    outbox: &crate::outbox::Outbox,
    requests: Option<&mut ActionRequests>,
) -> Option<Said> {
    let chosen = hand.chosen?;
    if action_pending(world) {
        return Some(Said::own(BUSY));
    }
    hand.chosen = None;
    let slot = u16::try_from(chosen.slot).ok()?;
    // The outbox says why a deletion it holds back did not go.
    let sent = outbox.post(world, |stamp| ClientCommand::DeleteSpell {
        session_id: stamp.session_id,
        slot,
        spell_id: chosen.spell,
        created: stamp.created,
    });
    if sent.is_ok() {
        hand.editing = Some((Edit::Deleting, world.book_action_revision()));
        if let Some(requests) = requests {
            // The deletion is the book's change now, not a memorization or a
            // scribe for the gauges and the chat's lines to follow.
            requests.book = None;
        }
    }
    None
}

/// Says how the server answered a deletion or a move, once it has: a
/// deletion is done or the spell stays, in the official words; a move the
/// session refused says why, in its words, as the official client's line
/// for a move was said as it went.
fn answer(
    hand: &mut BookHand,
    world: &eq_client_core::world::ClientWorld,
    messages: &crate::hud::messages::Messages,
    chat: &mut ChatState,
) {
    let Some((edit, sent)) = hand.editing else {
        return;
    };
    if world.book_action_revision() == sent {
        return;
    }
    let status = world.book_action();
    if matches!(
        status,
        Some(
            BookActionStatus::Preparing
                | BookActionStatus::Submitted
                | BookActionStatus::AwaitingReply
        )
    ) {
        return;
    }
    hand.editing = None;
    match (edit, status) {
        // The session clears a change the server confirmed.
        (Edit::Deleting, None) => chat.history.push(system_line(
            messages.said_text(4028, "The spell is gone from the book."),
        )),
        (Edit::Deleting, Some(_)) => chat.history.push(system_line(
            messages.said_text(4029, "The spell stays in the book."),
        )),
        (
            Edit::Moving,
            Some(BookActionStatus::Rejected(reason) | BookActionStatus::Cancelled(reason)),
        ) => chat.refuse(Said::own(reason.clone())),
        (Edit::Moving, _) => (),
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
        (BookChange::Scribe, scroll),
        format!("Scribing {}", names.label(scroll)),
    );
    None
}

/// Memorizes the spell picked up from the book into the gem the player
/// clicked; what the chat says if it does not. A spell memorized already
/// stays where it is, as the official client refuses it.
pub(crate) fn memorize(
    held: Entry,
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
        (BookChange::Memorize, spell),
        format!("Memorizing {} into gem {}", names.label(spell), gem + 1),
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
                price: None,
                icon: None,
            },
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
    fn a_right_click_chooses_a_spell_to_swap_or_delete_and_says_how() {
        let mut online = online();
        let (queue, sent) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        let messages = crate::hud::messages::Messages::default();
        let mut hand = BookHand::default();
        // An empty place chooses nothing.
        let said = choose(
            (0, None),
            &mut hand,
            (online.world(), &outbox),
            (&messages, None),
        );
        assert_eq!(said, Vec::new());
        assert_eq!(hand.chosen, None);
        // A spell is chosen, with how to swap it and how to delete it.
        let said = choose(
            (1, Some(42)),
            &mut hand,
            (online.world(), &outbox),
            (&messages, None),
        );
        assert_eq!(said.len(), 2);
        assert!(said.iter().all(|(_, refused)| !refused));
        assert_eq!(hand.chosen, Some(Entry { slot: 1, spell: 42 }));
        // A right click on it again unchooses it.
        let said = choose(
            (1, Some(42)),
            &mut hand,
            (online.world(), &outbox),
            (&messages, None),
        );
        assert_eq!(said, Vec::new());
        assert!(hand.chosen.is_none());
        // A right click on another place swaps the two.
        choose(
            (1, Some(42)),
            &mut hand,
            (online.world(), &outbox),
            (&messages, None),
        );
        let said = choose(
            (9, None),
            &mut hand,
            (online.world(), &outbox),
            (&messages, None),
        );
        assert_eq!(said.len(), 1);
        assert!(matches!(
            sent.try_recv().unwrap(),
            ClientCommand::SwapSpell {
                from: 1,
                to: 9,
                from_spell: 42,
                to_spell: None,
                ..
            }
        ));
        assert!(hand.chosen.is_none());
        // The Delete key deletes the chosen spell; the answer is said once
        // the server gives it.
        choose(
            (1, Some(42)),
            &mut hand,
            (online.world(), &outbox),
            (&messages, None),
        );
        delete(&mut hand, online.world(), &outbox, None);
        assert!(matches!(
            sent.try_recv().unwrap(),
            ClientCommand::DeleteSpell {
                slot: 1,
                spell_id: 42,
                ..
            }
        ));
        let mut chat = ChatState::default();
        answer(&mut hand, online.world(), &messages, &mut chat);
        assert_eq!(chat.newest(), "");
        crate::online::testing::book_action(&mut online, BookActionStatus::AwaitingReply);
        answer(&mut hand, online.world(), &messages, &mut chat);
        assert_eq!(chat.newest(), "");
        crate::online::testing::book_action(&mut online, BookActionStatus::Confirmed);
        answer(&mut hand, online.world(), &messages, &mut chat);
        assert_ne!(chat.newest(), "");
        // With nothing chosen, the key does nothing.
        delete(&mut hand, online.world(), &outbox, None);
        assert!(sent.try_recv().is_err());
    }

    /// An app with the skin's book open, a spell chosen in place 1, and the
    /// confirmation dialog's buttons answering before the book's clicks.
    fn book_app() -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
        let mut app = crate::testing::app();
        let (queue, sent) = std::sync::mpsc::sync_channel(4);
        let hand = BookHand {
            chosen: Some(Entry { slot: 1, spell: 42 }),
            ..BookHand::default()
        };
        let mut shown = crate::windows::Shown::default();
        shown.open(WindowId::Spellbook);
        app.insert_resource(online())
            .insert_resource(crate::outbox::Outbox::new(Some(queue)))
            .insert_resource(crate::skinned::Skinned::of(&[WindowId::Spellbook]))
            .insert_resource(shown)
            .insert_resource(hand)
            .add_systems(Update, (crate::confirm::buttons, clicks).chain());
        (app, sent)
    }

    /// Presses the Delete key afresh.
    fn press_delete(app: &mut App) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset(KeyCode::Delete);
        keys.press(KeyCode::Delete);
    }

    /// Answers the confirmation dialog, with the key let go.
    fn answer_dialog(app: &mut App, yes: bool) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset(KeyCode::Delete);
        app.world_mut()
            .spawn((Interaction::Pressed, crate::confirm::AnswerButton(yes)));
    }

    #[test]
    fn the_delete_key_asks_first_and_only_while_no_box_has_the_keyboard() {
        let (mut app, sent) = book_app();
        let question = Question::DeleteSpell { slot: 1, spell: 42 };
        // A Delete typed in the chat box asks nothing.
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        press_delete(&mut app);
        app.update();
        assert_eq!(app.world().resource::<Asked>().question(), None);
        // With the game holding the keyboard, it asks first.
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = false;
        app.update();
        assert!(sent.try_recv().is_err());
        assert_eq!(app.world().resource::<Asked>().question(), Some(question));
        // No keeps the spell, no longer chosen.
        answer_dialog(&mut app, false);
        app.update();
        assert!(sent.try_recv().is_err());
        assert!(app.world().resource::<BookHand>().chosen.is_none());
        // Chosen again, Yes deletes it.
        app.world_mut().resource_mut::<BookHand>().chosen = Some(Entry { slot: 1, spell: 42 });
        press_delete(&mut app);
        app.update();
        answer_dialog(&mut app, true);
        app.update();
        assert!(matches!(
            sent.try_recv().unwrap(),
            ClientCommand::DeleteSpell {
                slot: 1,
                spell_id: 42,
                ..
            }
        ));
        assert!(app.world().resource::<BookHand>().chosen.is_none());
        // Closing the book takes a question back.
        app.world_mut().resource_mut::<BookHand>().chosen = Some(Entry { slot: 1, spell: 42 });
        press_delete(&mut app);
        app.update();
        assert_eq!(app.world().resource::<Asked>().question(), Some(question));
        app.world_mut()
            .resource_mut::<crate::windows::Shown>()
            .close(WindowId::Spellbook);
        app.update();
        assert_eq!(app.world().resource::<Asked>().question(), None);
    }

    #[test]
    fn with_asking_turned_off_the_delete_key_deletes_at_once() {
        let (mut app, sent) = book_app();
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .set(
                eq_client_core::options::Toggle::Qol(Fix::AskBeforeDeletingSpells),
                false,
            );
        press_delete(&mut app);
        app.update();
        assert!(matches!(
            sent.try_recv().unwrap(),
            ClientCommand::DeleteSpell {
                slot: 1,
                spell_id: 42,
                ..
            }
        ));
        assert_eq!(app.world().resource::<Asked>().question(), None);
    }

    #[test]
    fn a_session_offering_neither_swaps_nor_deletes_chooses_nothing() {
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::news(
            &mut online,
            [eq_client_core::WorldEvent::Entered {
                capabilities: vec![Capability::Spellbook],
                session_id: 7,
                zone: "qeytoqrg".into(),
                player: Box::new(crate::online::testing::player(1)),
                far_clip: None,
            }],
        );
        let (queue, _sent) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        let messages = crate::hud::messages::Messages::default();
        let mut hand = BookHand::default();
        let said = choose(
            (1, Some(42)),
            &mut hand,
            (online.world(), &outbox),
            (&messages, None),
        );
        assert_eq!(said.len(), 1);
        assert!(said[0].1);
        assert!(hand.chosen.is_none());
    }

    #[test]
    fn a_held_spell_goes_into_the_gem_clicked_unless_a_gem_has_it() {
        let online = online();
        let (queue, sent) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        let names = SpellNames::parse("42^Synthetic spell");
        let messages = crate::hud::messages::Messages::default();
        let mut requests = ActionRequests::default();
        let held = Entry { slot: 1, spell: 42 };
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
        assert_eq!(request.label, "Memorizing Synthetic spell into gem 3");
        let mut player = crate::online::testing::player(1);
        player.memorized_spells[4] = Some(42);
        let mut memorized = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut memorized, 7, player);
        assert!(memorize(held, 2, (&memorized, &outbox), (&names, &messages), None).is_some());
        assert!(sent.try_recv().is_err());
    }
}
