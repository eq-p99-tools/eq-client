//! The skin's spellbook window (`SpellBookWnd`): two pages of the book's
//! places as the server keeps them, eight to a page, used through the
//! cursor and the gems as the installed client's own strings describe it
//! (inferred until a recording shows it): a left click puts a spell on the
//! cursor, and a click on a gem then memorizes it there; with a scroll on
//! the cursor, a left click on an empty place scribes it there. A right
//! click chooses a spell, a right click on another place then swaps the
//! two, and the Delete key deletes the chosen spell, each where the session
//! offers it.
use super::{BookView, SpellNames, action_pending, note, prepare_scribe};
use crate::chat::{ChatState, Said, system_line};
use crate::hud::action_bar::{ActionRequests, BookChange};
use crate::windows::WindowId;
use bevy::prelude::*;
use eq_client_core::{BookActionStatus, Capability, ClientCommand, inventory::InventorySlot};

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
    /// The book's reply count when a deletion went, until it is answered.
    deleting: Option<u64>,
}

impl Entry {
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
/// the chosen spell, only while no box takes the keyboard.
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
    answer(&mut hand, world, &messages, &mut chat);
    if !(skinned.has(WindowId::Spellbook) && shown.is_open(WindowId::Spellbook)) {
        hand.held = None;
        hand.chosen = None;
        return;
    }
    // A spell the book no longer keeps in its place leaves the cursor, and
    // is no longer chosen.
    if hand.held.is_some_and(|held| !held.kept(book)) {
        hand.held = None;
    }
    if hand.chosen.is_some_and(|chosen| !chosen.kept(book)) {
        hand.chosen = None;
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
        delete(&mut hand, world, &outbox, requests.as_deref_mut());
    }
    // A right click lands on the place under the pointer.
    if mouse.just_pressed(MouseButton::Right)
        && let Some((_, place)) = places
            .iter()
            .find(|(interaction, _)| **interaction != Interaction::None)
    {
        let entry = (view.slot(place.0), view.spell(book, place.0));
        let said = choose(entry, &mut hand, (world, &outbox), &messages);
        for said in said {
            if said.1 {
                chat.refuse(said.0);
            } else {
                chat.history.push(system_line(said.0));
            }
        }
        if hand.chosen.is_none()
            && let Some(requests) = requests.as_deref_mut()
        {
            // A move leaves no memorization or scribe for the gauges.
            requests.book = None;
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

/// A right click on a place: with no spell chosen, chooses the place's
/// spell, as the official client says how to swap and delete it; on the
/// chosen spell, unchooses it; on another place, swaps the chosen spell with
/// whatever is there. What the chat says, each line marked when it refuses.
fn choose(
    (slot, in_place): (usize, Option<u32>),
    hand: &mut BookHand,
    (world, outbox): (&eq_client_core::world::ClientWorld, &crate::outbox::Outbox),
    messages: &crate::hud::messages::Messages,
) -> Vec<(Said, bool)> {
    let offered = |capability| crate::outbox::offered(world, capability);
    match hand.chosen.take() {
        Some(chosen) if chosen.slot == slot => Vec::new(),
        Some(chosen) => {
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
            sent.map_or_else(
                |_| Vec::new(),
                |()| vec![(messages.said_text(1388, "Moving the spell."), false)],
            )
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

/// The Delete key: deletes the spell a right click chose. Its answer is
/// said once the server gives it (`answer`).
fn delete(
    hand: &mut BookHand,
    world: &eq_client_core::world::ClientWorld,
    outbox: &crate::outbox::Outbox,
    requests: Option<&mut ActionRequests>,
) {
    let Some(chosen) = hand.chosen.take() else {
        return;
    };
    let Ok(slot) = u16::try_from(chosen.slot) else {
        return;
    };
    // The outbox says why a deletion it holds back did not go.
    let sent = outbox.post(world, |stamp| ClientCommand::DeleteSpell {
        session_id: stamp.session_id,
        slot,
        spell_id: chosen.spell,
        created: stamp.created,
    });
    if sent.is_ok() {
        hand.deleting = Some(world.book_action_revision());
        if let Some(requests) = requests {
            // A deletion leaves no memorization or scribe for the gauges.
            requests.book = None;
        }
    }
}

/// Says how the server answered a deletion, once it has: the spell is
/// deleted, or it stays.
fn answer(
    hand: &mut BookHand,
    world: &eq_client_core::world::ClientWorld,
    messages: &crate::hud::messages::Messages,
    chat: &mut ChatState,
) {
    let Some(sent) = hand.deleting else {
        return;
    };
    if world.book_action_revision() == sent {
        return;
    }
    let said = match world.book_action() {
        Some(
            BookActionStatus::Preparing
            | BookActionStatus::Submitted
            | BookActionStatus::AwaitingReply,
        ) => return,
        // The session clears a change the server confirmed.
        None => messages.said_text(4028, "The spell is gone from the book."),
        Some(_) => messages.said_text(4029, "The spell stays in the book."),
    };
    hand.deleting = None;
    chat.history.push(system_line(said));
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
    fn a_right_click_chooses_a_spell_to_swap_or_delete_and_says_how() {
        let mut online = online();
        let (queue, sent) = std::sync::mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(queue));
        let messages = crate::hud::messages::Messages::default();
        let mut hand = BookHand::default();
        // An empty place chooses nothing.
        let said = choose((0, None), &mut hand, (online.world(), &outbox), &messages);
        assert_eq!(said, Vec::new());
        assert_eq!(hand.chosen, None);
        // A spell is chosen, with how to swap it and how to delete it.
        let said = choose(
            (1, Some(42)),
            &mut hand,
            (online.world(), &outbox),
            &messages,
        );
        assert_eq!(said.len(), 2);
        assert!(said.iter().all(|(_, refused)| !refused));
        assert_eq!(hand.chosen, Some(Entry { slot: 1, spell: 42 }));
        // A right click on it again unchooses it.
        let said = choose(
            (1, Some(42)),
            &mut hand,
            (online.world(), &outbox),
            &messages,
        );
        assert_eq!(said, Vec::new());
        assert!(hand.chosen.is_none());
        // A right click on another place swaps the two.
        choose(
            (1, Some(42)),
            &mut hand,
            (online.world(), &outbox),
            &messages,
        );
        let said = choose((9, None), &mut hand, (online.world(), &outbox), &messages);
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
            &messages,
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

    #[test]
    fn the_delete_key_deletes_only_while_no_box_has_the_keyboard() {
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
            .add_systems(Update, clicks);
        // A Delete typed in the chat box deletes nothing.
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Delete);
        app.update();
        assert!(sent.try_recv().is_err());
        assert!(app.world().resource::<BookHand>().chosen.is_some());
        // With the game holding the keyboard, it deletes the chosen spell.
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = false;
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
            &messages,
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
