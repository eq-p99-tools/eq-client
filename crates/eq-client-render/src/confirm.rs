//! The skin's confirmation dialog, asking one Yes-or-No question at a time:
//! a resurrection the server offers first, since the server waits for its
//! answer, then a question of the client's own before something that cannot
//! be undone, as Ask Before Deleting Spells asks before a spell leaves the
//! book. Whoever asked acts on the answer.
use crate::{
    online::OnlineState,
    outbox::Outbox,
    windows::{Shown, WindowId},
};
use bevy::prelude::*;

/// The confirmation dialog's Yes (true) or No (false).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AnswerButton(pub(crate) bool);

/// The confirmation dialog's text, which says what is asked.
#[derive(Component)]
pub(crate) struct QuestionText;

/// A question the client asks of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Question {
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
    fn text(self, names: &crate::spellbook::SpellNames) -> String {
        match self {
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
}

impl Asked {
    /// Asks the question, unless another waits: false then.
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

/// Keeps the confirmation dialog open while a question waits for an answer.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(online: Res<OnlineState>, asked: Res<Asked>, mut shown: ResMut<Shown>) {
    let asking = online.world().resurrection().is_some() || asked.question.is_some();
    if asking != shown.is_open(WindowId::Confirmation) {
        if asking {
            shown.open(WindowId::Confirmation);
        } else {
            shown.close(WindowId::Confirmation);
        }
    }
}

/// Answers the question when Yes or No is pressed: a resurrection to the
/// server, the client's own to whoever asked it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    mut asked: ResMut<Asked>,
    buttons: Query<(&Interaction, &AnswerButton), Changed<Interaction>>,
) {
    let Some(yes) = buttons
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, answer)| answer.0)
    else {
        return;
    };
    if online.world().resurrection().is_some() {
        super::resurrection::answer(&mut online, &outbox, yes);
    } else if let Some(question) = asked.question.take() {
        asked.answer = Some((question, yes));
    }
}

/// Shows the dialog's question.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    (online, asked): (Res<OnlineState>, Res<Asked>),
    (messages, names): (
        Option<Res<crate::hud::messages::Messages>>,
        Res<crate::spellbook::SpellNames>,
    ),
    mut texts: Query<&mut Text, With<QuestionText>>,
) {
    if texts.is_empty() {
        return;
    }
    let wanted = if online.world().resurrection().is_some() {
        super::resurrection::question(online.world(), messages.as_deref())
    } else {
        asked
            .question
            .map(|question| question.text(&names))
            .unwrap_or_default()
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
    fn a_question_of_the_clients_own_waits_for_its_answer() {
        let mut app = crate::testing::app();
        app.init_resource::<Asked>()
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
        app.world_mut()
            .spawn((Interaction::Pressed, AnswerButton(false)));
        app.update();
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
