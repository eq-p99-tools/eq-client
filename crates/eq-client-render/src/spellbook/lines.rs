//! The official client's lines for a memorization or a scribe: when it
//! begins, when it ends and when it stops, said in the chat in the installed
//! client's words, so the official-format log keeps them too. Which line
//! names what, and whether the beginning is said at the click or once the
//! player has sat, waits for a recording; until then the beginning goes with
//! the session's sitting (`Preparing`) and the end with the server's answer.
use super::SpellNames;
use crate::chat::{ChatState, Said, system_line};
use crate::hud::action_bar::{ActionRequests, BookChange, BookRequest};
use crate::hud::messages::Messages;
use bevy::prelude::*;
use eq_client_core::BookActionStatus;

/// Says a memorization's or a scribe's lines as the session reports it: its
/// beginning once the player sits for it, its end once the server answers,
/// and its stop when it is cancelled.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn say(
    online: Res<crate::online::OnlineState>,
    mut requests: ResMut<ActionRequests>,
    (names, messages): (Res<SpellNames>, Res<Messages>),
    mut chat: ResMut<ChatState>,
) {
    let world = online.world();
    let Some(request) = requests.book.as_mut() else {
        return;
    };
    if world.ended() || !request.receipt.belongs_to(world.session_id()) {
        requests.book = None;
        return;
    }
    let Some(status) = request.receipt.status() else {
        return;
    };
    if let Some(said) = line(request, &status, (&names, &messages)) {
        chat.history.push(system_line(said));
    }
    if matches!(
        status,
        BookActionStatus::Confirmed
            | BookActionStatus::Cancelled(_)
            | BookActionStatus::Rejected(_)
    ) {
        requests.book = None;
    }
}

/// Formats the line for this request's own progress or outcome.
fn line(
    request: &mut BookRequest,
    status: &BookActionStatus,
    (names, messages): (&SpellNames, &Messages),
) -> Option<Said> {
    // The official lines by eqstr id: the beginning and the end, each with
    // the spell's name, and the stop; and this client's own words for each
    // where the installation lacks them.
    let ((begins, ends, stops), (began, ended, stopped)) = match request.change {
        BookChange::Memorize => (
            (12053, 12007, 12045),
            ("Memorizing", "memorized", "Memorizing"),
        ),
        BookChange::Scribe => ((12049, 12006, 12044), ("Scribing", "scribed", "Scribing")),
    };
    match status {
        BookActionStatus::Preparing if !request.began => {
            request.began = true;
            let name = names.label(request.spell);
            let own = format!("{began} {name}.");
            Some(messages.said_or(begins, &[name], &own))
        }
        BookActionStatus::Confirmed if request.began => {
            let name = names.label(request.spell);
            let own = format!("{name} is {ended}.");
            Some(messages.said_or(ends, &[name], &own))
        }
        BookActionStatus::Cancelled(_) if request.began => {
            Some(messages.said_text(stops, &format!("{stopped} stopped.")))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, Updates, receive, testing};
    use crate::spellbook::{BookEntry, GemChoice, ScribeCursor};
    use crate::windows::{Shown, WindowId};
    use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
    use eq_client_core::{
        ClientCommand, SpellBook, SpellUpdate, WorldEvent, WorldUpdate,
        chat::ChatTab,
        inventory::{InventorySlot, InventoryUpdate},
        world::Link,
    };
    use std::sync::{Mutex, mpsc};

    struct Fixture {
        app: App,
        updates: mpsc::Sender<WorldUpdate>,
        sent: mpsc::Receiver<ClientCommand>,
    }

    impl Fixture {
        fn new() -> Self {
            let mut online = OnlineState::new(true);
            testing::admit(&mut online, 1, testing::player(7));
            let mut book = SpellBook::default();
            book.apply(&SpellUpdate::Slot {
                slot: 1,
                spell_id: 42,
                mode: 0,
            });
            testing::book(&mut online, book);
            let mut scroll = crate::preview::items().remove(0);
            scroll.slot = InventorySlot::CURSOR;
            scroll.scroll_spell = Some(73);
            testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![scroll]));
            let mut app = crate::testing::app();
            let (updates, incoming) = mpsc::channel();
            let (queue, sent) = mpsc::sync_channel(4);
            app.insert_resource(online)
                .insert_resource(crate::outbox::Outbox::new(Some(queue)))
                .insert_resource(SpellNames::parse("42^Synthetic spell\n73^Synthetic scroll"))
                .insert_resource(Updates(Mutex::new(Some(incoming))))
                .init_resource::<Assets<Mesh>>()
                .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
                .init_resource::<Assets<StandardMaterial>>()
                .add_systems(Startup, |mut commands: Commands| {
                    crate::spellbook::spawn(&mut commands)
                })
                .add_systems(Update, (receive, crate::spellbook::update, say).chain());
            app.world_mut()
                .resource_mut::<Shown>()
                .open(WindowId::Spellbook);
            app.update();
            Self { app, updates, sent }
        }

        fn click<T: Component>(&mut self, matches: impl Fn(&T) -> bool) {
            let world = self.app.world_mut();
            let control = world
                .query::<(Entity, &T)>()
                .iter(world)
                .find(|(_, value)| matches(value))
                .unwrap()
                .0;
            crate::clicks::testing::click(&mut self.app, control);
        }

        fn memorize(&mut self) {
            self.click(|row: &BookEntry| row.0 == 0);
            self.click(|gem: &GemChoice| gem.0 == 0);
            assert!(matches!(
                self.sent.try_recv().unwrap(),
                ClientCommand::MemorizeSpell {
                    session_id: 1,
                    gem: 0,
                    spell_id: 42,
                    ..
                }
            ));
            self.frame([status(BookActionStatus::Preparing)]);
            assert_eq!(self.line_count("Memorizing Synthetic spell."), 1);
        }

        fn frame(&mut self, updates: impl IntoIterator<Item = WorldUpdate>) {
            for update in updates {
                self.updates.send(update).unwrap();
            }
            self.app.update();
        }

        fn line_count(&self, text: &str) -> usize {
            self.app
                .world()
                .resource::<ChatState>()
                .history
                .lines(ChatTab::All)
                .iter()
                .filter(|(_, line)| line.message.text == text)
                .count()
        }

        fn world(&self) -> &eq_client_core::world::ClientWorld {
            self.app.world().resource::<OnlineState>().world()
        }
    }

    fn status(status: BookActionStatus) -> WorldUpdate {
        WorldUpdate::Game(WorldEvent::BookAction(status))
    }

    fn spell(update: SpellUpdate) -> WorldUpdate {
        WorldUpdate::Game(WorldEvent::Spell(update))
    }

    #[test]
    fn a_rejected_memorize_does_not_consume_a_later_fallback_delete_confirmation() {
        use crate::book_delete::Action;

        let mut fixture = Fixture::new();
        fixture.memorize();
        fixture.frame([
            status(BookActionStatus::Rejected("Memorize refused".into())),
            WorldUpdate::Connection(Link::Zoning),
        ]);
        assert!(fixture.world().book_action_status().is_none());
        assert!(
            fixture
                .app
                .world()
                .resource::<ActionRequests>()
                .book
                .is_none()
        );
        fixture.frame([WorldUpdate::Connection(Link::Connected)]);

        fixture.click(|action: &Action| *action == Action::Select);
        fixture.click(|action: &Action| *action == Action::Confirm);
        assert!(matches!(
            fixture.sent.try_recv().unwrap(),
            ClientCommand::DeleteSpell {
                session_id: 1,
                slot: 1,
                spell_id: 42,
                ..
            }
        ));
        fixture.frame([
            status(BookActionStatus::Confirmed),
            spell(SpellUpdate::BookDeletion {
                slot: 1,
                success: true,
            }),
        ]);

        assert_eq!(fixture.world().gem(0), None);
        assert_eq!(fixture.world().spell_book().unwrap().slots()[1], None);
        assert_eq!(fixture.line_count("Synthetic spell is memorized."), 0);
    }

    #[test]
    fn a_resumed_memorize_announces_only_its_exact_late_confirmation_once() {
        let mut fixture = Fixture::new();
        fixture.memorize();
        fixture.frame([
            status(BookActionStatus::AwaitingReply),
            WorldUpdate::Connection(Link::Zoning),
        ]);
        fixture.frame([
            WorldUpdate::Connection(Link::Connected),
            status(BookActionStatus::Unconfirmed),
            status(BookActionStatus::Confirmed),
            spell(SpellUpdate::Slot {
                slot: 1,
                spell_id: 42,
                mode: 1,
            }),
        ]);
        assert_eq!(fixture.line_count("Synthetic spell is memorized."), 0);
        fixture.frame([spell(SpellUpdate::Slot {
            slot: 0,
            spell_id: 42,
            mode: 1,
        })]);
        fixture.app.update();
        assert_eq!(fixture.world().gem(0), Some(42));
        assert_eq!(fixture.line_count("Memorizing Synthetic spell."), 1);
        assert_eq!(fixture.line_count("Synthetic spell is memorized."), 1);
    }

    #[test]
    fn scribe_chat_preserves_the_own_cancellation_through_a_link_reset() {
        let mut fixture = Fixture::new();
        fixture.click(|_: &ScribeCursor| true);
        assert!(matches!(
            fixture.sent.try_recv().unwrap(),
            ClientCommand::ScribeSpell { spell_id: 73, .. }
        ));
        fixture.frame([status(BookActionStatus::Preparing)]);
        fixture.frame([
            status(BookActionStatus::Cancelled("The character moved".into())),
            WorldUpdate::Connection(Link::Zoning),
        ]);
        assert_eq!(fixture.line_count("Scribing Synthetic scroll."), 1);
        assert_eq!(fixture.line_count("Scribing stopped."), 1);
        assert_eq!(fixture.line_count("Synthetic scroll is scribed."), 0);
    }
}
