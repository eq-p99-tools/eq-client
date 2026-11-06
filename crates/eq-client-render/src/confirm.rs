//! The skin's confirmation dialog, asking one Yes-or-No question at a time:
//! a resurrection the server offers first, then a translocate where the
//! installed client asks about one (`translocate`), since the server waits
//! for their answers, then a question of the client's own before something
//! that cannot be undone, as Ask Before Deleting Spells asks before a spell
//! leaves the book. Whoever asked acts on the answer.
use crate::{
    online::OnlineState,
    outbox::Outbox,
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::confirmations::{ConsentId, ConsentKind};

/// The confirmation dialog's Yes (true) or No (false).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AnswerButton(pub(crate) bool);

/// The confirmation dialog's text, which says what is asked.
#[derive(Component)]
pub(crate) struct QuestionText;

/// A question the client asks of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Question {
    /// Exact list and request token; the deletion owner retains the frozen name.
    DeleteCharacter {
        scope: eq_client_core::deletion::Scope,
        request_id: u64,
    },
    /// A frozen whole cursor item; the inventory controller owns its metadata.
    DestroyItem {
        session_id: u64,
        revision: u64,
        item_id: u32,
    },
    /// Whether to delete this spell from this place in the book for good
    /// ([`eq_client_core::qol::Fix::AskBeforeDeletingSpells`]).
    DeleteSpell {
        /// The spell's place in the book.
        slot: usize,
        /// The spell.
        spell: u32,
    },
}

impl Question {
    /// What the dialog says, in the client's own words.
    fn text(self, names: &crate::spellbook::SpellNames, online: &OnlineState) -> String {
        match self {
            Self::DeleteCharacter { .. } => online.deletion.prompt().filter(|request| crate::character_delete::question(request) == self).map_or_else(String::new, |request| format!("Delete character {}? This may permanently remove the character. Choose Yes to confirm this exact name, or No to cancel.", request.name())),
            Self::DestroyItem { session_id, revision, item_id } => {
                let inventory = online.world().inventory();
                inventory.items().get(&eq_client_core::inventory::InventorySlot::CURSOR)
                    .filter(|item| online.world().session_id() == Some(session_id)
                        && inventory.revision() == revision && item.details.id == item_id)
                    .map_or_else(String::new, |item| format!("Destroy all {} of {}? This cannot be undone.",
                        item.stack_count.unwrap_or(1), item.details.name))
            }
            Self::DeleteSpell { spell, .. } => {
                format!(
                    "Delete {} from your spell book for good?",
                    names.label(spell)
                )
            }
        }
    }
}

/// The client's own question waiting for Yes or No, and the answer given,
/// until whoever asked takes it.
#[derive(Resource, Default)]
pub(crate) struct Asked {
    question: Option<Question>,
    answer: Option<(Question, bool)>,
    presented: Option<ConsentId>,
}

impl Asked {
    /// Asks the question, unless another waits: false then.
    #[must_use = "another question may be waiting"]
    pub(crate) fn ask(&mut self, question: Question) -> bool {
        if self.question.is_some() {
            return false;
        }
        self.question = Some(question);
        self.answer = None;
        true
    }

    /// The question waiting for an answer, if any.
    pub(crate) const fn question(&self) -> Option<Question> {
        self.question
    }

    /// The server offer whose question the confirmation window last drew.
    pub(crate) const fn presented(&self) -> Option<ConsentId> {
        self.presented
    }

    /// Takes back the waiting question, unanswered.
    pub(crate) fn withdraw(&mut self) {
        self.question = None;
    }

    /// Takes the answer to the question, once given: Yes (true) or No.
    pub(crate) fn answer(&mut self, question: Question) -> Option<bool> {
        self.answer
            .take_if(|(answered, _)| *answered == question)
            .map(|(_, yes)| yes)
    }
}

/// The server question that takes precedence in the confirmation window.
fn current_consent(online: &OnlineState) -> Option<ConsentId> {
    online
        .world()
        .resurrection()
        .map(|question| question.id)
        .or_else(|| online.world().translocate().map(|question| question.id))
}

/// Keeps the confirmation dialog open while a question waits for an answer.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(online: Res<OnlineState>, asked: Res<Asked>, mut shown: ResMut<Shown>) {
    let asking = current_consent(&online).is_some() || asked.question.is_some();
    if asking != shown.is_open(WindowId::Confirmation) {
        if asking {
            shown.open(WindowId::Confirmation);
        } else {
            shown.close(WindowId::Confirmation);
        }
    }
}

/// Answers the question when Yes or No is pressed: a resurrection or a
/// translocate to the server, the client's own to whoever asked it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    (mut online, outbox): (ResMut<OnlineState>, Res<Outbox>),
    (settings, messages, mut chat): (
        Res<crate::ViewerSettings>,
        Option<Res<crate::hud::messages::Messages>>,
        ResMut<crate::chat::ChatState>,
    ),
    mut asked: ResMut<Asked>,
    buttons: crate::clicks::Clicks<AnswerButton>,
) {
    let Some(yes) = buttons.iter().next().map(|answer| answer.0) else {
        return;
    };
    let current = current_consent(&online);
    // A frame may have received a replacement before it draws the question.
    if current != asked.presented {
        return;
    }
    match current {
        Some(id) if id.kind == ConsentKind::Resurrection => {
            super::resurrection::answer(&mut online, &outbox, id, yes);
        }
        Some(id) => {
            let rules = settings.0.installed_client.translocate_offer();
            let declined = rules.declined.zip(messages.as_deref());
            if let Some(line) = super::translocate::answer(&mut online, &outbox, id, yes, declined)
            {
                chat.history.push(line);
            }
        }
        None => {
            if let Some(question) = asked.question.take() {
                asked.answer = Some((question, yes));
            }
        }
    }
}

/// Shows the dialog's question.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    (online, mut asked, settings): (Res<OnlineState>, ResMut<Asked>, Res<crate::ViewerSettings>),
    (messages, names): (
        Option<Res<crate::hud::messages::Messages>>,
        Res<crate::spellbook::SpellNames>,
    ),
    mut texts: Query<&mut Text, With<QuestionText>>,
    controls: Query<Entity, With<AnswerButton>>,
    mouse: Option<ResMut<crate::clicks::LeftButton>>,
) {
    let current = if texts.is_empty() {
        None
    } else {
        current_consent(&online)
    };
    if asked.presented != current {
        // A press that began on the old question must not answer a new one.
        if let Some(mut mouse) = mouse {
            for control in &controls {
                mouse.claim(control);
            }
        }
        asked.presented = current;
    }
    if texts.is_empty() {
        return;
    }
    let wanted = match current {
        Some(id) if id.kind == ConsentKind::Resurrection => {
            super::resurrection::question(online.world(), messages.as_deref())
        }
        Some(_) => super::translocate::question(
            online.world(),
            settings.0.installed_client.translocate_offer(),
            messages.as_deref(),
        ),
        None => asked
            .question
            .map(|question| question.text(&names, &online))
            .unwrap_or_default(),
    };
    for mut text in &mut texts {
        if text.0 != wanted {
            text.0.clone_from(&wanted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_started_on_one_offer_cannot_answer_its_replacement() {
        use crate::{clicks::testing as clicks, online::testing};
        use eq_client_core::{ClientCommand, WorldEvent, confirmations::ConsentOffer};
        use std::time::{Duration, Instant};

        let mut app = crate::testing::app();
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        app.insert_resource(Outbox::new(Some(tx)))
            .init_resource::<Asked>()
            .init_resource::<crate::chat::ChatState>()
            .add_systems(Update, (buttons, window, show).chain());
        let yes = app
            .world_mut()
            .spawn((Interaction::None, AnswerButton(true)))
            .id();
        let text = app.world_mut().spawn((QuestionText, Text::default())).id();
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let offer = |sequence, caster: &[u8]| {
            let mut body = vec![0; 92];
            body[..4].copy_from_slice(&4u32.to_le_bytes());
            body[12..12 + caster.len()].copy_from_slice(caster);
            WorldEvent::TranslocateOffer(ConsentOffer {
                id: ConsentId {
                    session_id: 1,
                    kind: ConsentKind::Translocate,
                    sequence,
                },
                offer: eq_client_core::confirmations::titanium_translocate(&body).unwrap(),
                expires_at: Instant::now() + Duration::from_secs(300),
            })
        };
        testing::news(&mut online, [offer(1, b"First")]);
        app.insert_resource(online);
        app.update();
        assert!(app.world().get::<Text>(text).unwrap().0.contains("First"));
        clicks::press(&mut app, Some(yes));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [offer(2, b"Second")],
        );
        app.update();
        assert!(app.world().get::<Text>(text).unwrap().0.contains("Second"));
        clicks::release(&mut app, Some(yes));
        assert!(rx.try_recv().is_err());
        clicks::click(&mut app, yes);
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::AnswerTranslocate {
                session_id: 1,
                offer_id: ConsentId {
                    session_id: 1,
                    kind: ConsentKind::Translocate,
                    sequence: 2
                },
                accept: true,
            })
        ));
    }

    #[test]
    fn a_question_of_the_clients_own_waits_for_its_answer() {
        let mut app = crate::testing::app();
        app.init_resource::<Asked>()
            .init_resource::<crate::chat::ChatState>()
            .add_systems(Update, (buttons, window, show).chain());
        let text = app.world_mut().spawn((QuestionText, Text::default())).id();
        let question = Question::DeleteSpell { slot: 3, spell: 0 };
        assert!(app.world_mut().resource_mut::<Asked>().ask(question));
        // One question at a time.
        let other = Question::DeleteSpell { slot: 4, spell: 0 };
        assert!(!app.world_mut().resource_mut::<Asked>().ask(other));
        app.update();
        let open = |app: &App| {
            app.world()
                .resource::<Shown>()
                .is_open(WindowId::Confirmation)
        };
        assert!(open(&app));
        assert!(
            app.world()
                .get::<Text>(text)
                .unwrap()
                .0
                .ends_with("from your spell book for good?")
        );
        // No answers it, and the dialog closes; the answer waits for the
        // asker, and only for its own question.
        let no = app
            .world_mut()
            .spawn((Interaction::None, AnswerButton(false)))
            .id();
        crate::clicks::testing::click(&mut app, no);
        assert!(!open(&app));
        let mut asked = app.world_mut().resource_mut::<Asked>();
        assert_eq!(asked.question(), None);
        assert_eq!(asked.answer(other), None);
        assert_eq!(asked.answer(question), Some(false));
        assert_eq!(asked.answer(question), None);
        // A question taken back goes unanswered.
        assert!(asked.ask(question));
        asked.withdraw();
        assert_eq!(asked.question(), None);
    }
}
