//! Confirmed deletion and server-authoritative reordering of spellbook entries.
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, SpellBook};

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Select,
    Confirm,
    Cancel,
    Earlier,
    Later,
}

#[derive(Default)]
pub(crate) struct Confirmation {
    selected: Option<(u64, u16, u32)>,
    pub queued: bool,
}

/// Creates separate request, confirmation, and cancellation controls.
pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            column_gap: px(6),
            row_gap: px(4),
            flex_wrap: FlexWrap::Wrap,
            ..default()
        })
        .with_children(|row| {
            for (action, label) in [
                // These move the selected spell one slot; Previous and Next
                // under the gems turn the page.
                (Action::Earlier, "Move earlier"),
                (Action::Later, "Move later"),
                (Action::Select, "Delete spell"),
                (Action::Confirm, "Confirm"),
                (Action::Cancel, "Cancel"),
            ] {
                // Moving and deleting each need the session to offer them.
                let needs = match action {
                    Action::Earlier | Action::Later => eq_client_core::Capability::MovingSpells,
                    Action::Select | Action::Confirm | Action::Cancel => {
                        eq_client_core::Capability::DeletingSpells
                    }
                };
                row.spawn((
                    Button,
                    action,
                    crate::outbox::Needs::Capability(needs),
                    Node {
                        padding: UiRect::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(if matches!(action, Action::Select | Action::Confirm) {
                        theme::BUTTON_DANGER
                    } else {
                        theme::BUTTON
                    }),
                ))
                .with_child((Text::new(label), theme::font(Size::Body)));
            }
        });
}

impl Confirmation {
    /// Shows consent controls only while a specific deletion is awaiting confirmation.
    pub fn visible(&self, action: Action) -> bool {
        match action {
            Action::Confirm | Action::Cancel => self.selected.is_some(),
            Action::Select | Action::Earlier | Action::Later => self.selected.is_none(),
        }
    }

    /// Cancels unsubmitted consent without releasing an outstanding request.
    pub fn cancel(&mut self) {
        self.selected = None;
    }
    /// Invalidates consent whenever the session or selected book entry changes.
    pub fn validate(&mut self, session: Option<u64>, spell: Option<u32>, book: Option<&SpellBook>) {
        if self.selected.is_some_and(|(old, slot, id)| {
            session != Some(old)
                || spell != Some(id)
                || book
                    .and_then(|book| book.slots().get(usize::from(slot)))
                    .copied()
                    .flatten()
                    != Some(id)
        }) {
            self.selected = None;
        }
    }

    /// Only a separate confirmation click can return a fresh deletion
    /// command, unless the player turned asking off
    /// ([`eq_client_core::qol::Fix::AskBeforeDeletingSpells`]): then Delete
    /// spell returns it at once.
    pub fn act(
        &mut self,
        action: Action,
        (session, spell, book): (Option<u64>, Option<u32>, Option<&SpellBook>),
        asks: bool,
    ) -> anyhow::Result<Option<ClientCommand>> {
        use anyhow::{Context, ensure};
        self.validate(session, spell, book);
        ensure!(!self.queued, "Waiting for spellbook reply");
        match action {
            Action::Earlier | Action::Later => {
                self.cancel();
                reorder(session, spell, book, matches!(action, Action::Later)).map(Some)
            }
            Action::Cancel => {
                self.selected = None;
                Ok(None)
            }
            Action::Select => {
                let session = session.context("Connect before deleting a spell")?;
                let id = spell.context("Select a spell first")?;
                let slot = book
                    .context("Spellbook unavailable")?
                    .slots()
                    .iter()
                    .position(|entry| *entry == Some(id))
                    .context("Selected spell is no longer scribed")?;
                let chosen = (session, u16::try_from(slot)?, id);
                if !asks {
                    return Ok(Some(deletion(chosen)));
                }
                self.selected = Some(chosen);
                Ok(None)
            }
            Action::Confirm => {
                let chosen = self.selected.take().context("Click Delete spell first")?;
                Ok(Some(deletion(chosen)))
            }
        }
    }
}

/// The command that deletes a spell from its place, in a session.
fn deletion((session_id, slot, spell_id): (u64, u16, u32)) -> ClientCommand {
    ClientCommand::DeleteSpell {
        session_id,
        slot,
        spell_id,
        created: std::time::Instant::now(),
    }
}

/// Queues adjacent physical slots, including empty ones, without rearranging local state.
fn reorder(
    session: Option<u64>,
    spell: Option<u32>,
    book: Option<&SpellBook>,
    later: bool,
) -> anyhow::Result<ClientCommand> {
    use anyhow::Context;
    let session_id = session.context("Connect before reordering spells")?;
    let from_spell = spell.context("Select a spell first")?;
    let slots = book.context("Spellbook unavailable")?.slots();
    let from = slots
        .iter()
        .position(|entry| *entry == Some(from_spell))
        .context("Selected spell is no longer scribed")?;
    let to = if later {
        from.checked_add(1)
    } else {
        from.checked_sub(1)
    }
    .filter(|slot| *slot < slots.len())
    .context("Already at the end of the book")?;
    Ok(ClientCommand::SwapSpell {
        session_id,
        from: u16::try_from(from)?,
        to: u16::try_from(to)?,
        from_spell,
        to_spell: slots[to],
        created: std::time::Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reordering_preserves_real_indexes_and_waits_for_server_state() {
        let mut book = SpellBook::default();
        for (slot, spell_id) in [(0, 42), (2, 73), (399, 99)] {
            book.apply(&eq_client_core::SpellUpdate::Slot {
                slot,
                spell_id,
                mode: 0,
            });
        }
        assert!(reorder(Some(7), Some(42), Some(&book), false).is_err());
        assert!(reorder(Some(7), Some(99), Some(&book), true).is_err());
        assert!(reorder(None, Some(73), Some(&book), false).is_err());
        assert!(matches!(
            reorder(Some(7), Some(73), Some(&book), false).unwrap(),
            ClientCommand::SwapSpell {
                session_id: 7,
                from: 2,
                to: 1,
                from_spell: 73,
                to_spell: None,
                ..
            }
        ));
        assert_eq!(book.slots()[2], Some(73));
        assert_eq!(book.slots()[1], None);
    }

    #[test]
    fn confirmation_requires_unchanged_selection_and_never_edits_the_book() {
        let mut book = SpellBook::default();
        book.apply(&eq_client_core::SpellUpdate::Slot {
            slot: 3,
            spell_id: 42,
            mode: 0,
        });
        let mut state = Confirmation::default();
        assert!(
            state
                .act(Action::Confirm, (Some(7), Some(42), Some(&book)), true)
                .is_err()
        );
        assert!(
            state
                .act(Action::Select, (Some(7), Some(42), Some(&book)), true)
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .act(Action::Confirm, (Some(8), Some(42), Some(&book)), true)
                .is_err()
        );
        state
            .act(Action::Select, (Some(7), Some(42), Some(&book)), true)
            .unwrap();
        assert!(
            state
                .act(Action::Confirm, (Some(7), Some(73), Some(&book)), true)
                .is_err()
        );
        state
            .act(Action::Select, (Some(7), Some(42), Some(&book)), true)
            .unwrap();
        state
            .act(Action::Cancel, (Some(7), Some(42), Some(&book)), true)
            .unwrap();
        assert!(
            state
                .act(Action::Confirm, (Some(7), Some(42), Some(&book)), true)
                .is_err()
        );
        state
            .act(Action::Select, (Some(7), Some(42), Some(&book)), true)
            .unwrap();
        assert!(matches!(
            state
                .act(Action::Confirm, (Some(7), Some(42), Some(&book)), true)
                .unwrap(),
            Some(ClientCommand::DeleteSpell {
                session_id: 7,
                slot: 3,
                spell_id: 42,
                ..
            })
        ));
        assert_eq!(book.slots()[3], Some(42));
        assert!(
            state
                .act(Action::Confirm, (Some(7), Some(42), Some(&book)), true)
                .is_err()
        );
        state.queued = true;
        assert!(
            state
                .act(Action::Select, (Some(7), Some(42), Some(&book)), true)
                .is_err()
        );
    }

    #[test]
    fn with_asking_turned_off_delete_spell_deletes_at_once() {
        let mut book = SpellBook::default();
        book.apply(&eq_client_core::SpellUpdate::Slot {
            slot: 3,
            spell_id: 42,
            mode: 0,
        });
        let mut state = Confirmation::default();
        assert!(matches!(
            state
                .act(Action::Select, (Some(7), Some(42), Some(&book)), false)
                .unwrap(),
            Some(ClientCommand::DeleteSpell {
                session_id: 7,
                slot: 3,
                spell_id: 42,
                ..
            })
        ));
        // Nothing waits for a Confirm.
        assert!(state.visible(Action::Select));
    }
}
