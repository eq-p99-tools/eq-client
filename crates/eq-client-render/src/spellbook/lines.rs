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
use std::time::Instant;

/// Says a memorization's or a scribe's lines as the session reports it: its
/// beginning once the player sits for it, its end once the server answers,
/// and its stop when it is cancelled.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn say(
    online: Res<crate::online::OnlineState>,
    requests: Res<ActionRequests>,
    (names, messages): (Res<SpellNames>, Res<Messages>),
    mut chat: ResMut<ChatState>,
    (mut seen, mut following): (Local<u64>, Local<Option<Instant>>),
) {
    let world = online.world();
    let revision = world.book_action_revision();
    if *seen == revision {
        return;
    }
    *seen = revision;
    let Some(request) = requests.book.as_ref() else {
        *following = None;
        return;
    };
    let status = world.book_action();
    if let Some(said) = line(request, status, &mut following, (&names, &messages)) {
        chat.history.push(system_line(said));
    }
}

/// The line the session's news about this request calls for, if any: the
/// request whose lines are being said is the one whose beginning was said.
fn line(
    request: &BookRequest,
    status: Option<&BookActionStatus>,
    following: &mut Option<Instant>,
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
    let name = names.label(request.spell);
    let ours = *following == Some(request.since);
    match status {
        Some(BookActionStatus::Preparing) if !ours => {
            *following = Some(request.since);
            let own = format!("{began} {name}.");
            Some(messages.said_or(begins, &[name], &own))
        }
        // The session clears a change the server confirmed.
        None if ours => {
            *following = None;
            let own = format!("{name} is {ended}.");
            Some(messages.said_or(ends, &[name], &own))
        }
        Some(BookActionStatus::Cancelled(_)) if ours => {
            *following = None;
            Some(messages.said_text(stops, &format!("{stopped} stopped.")))
        }
        Some(BookActionStatus::Rejected(_)) => {
            *following = None;
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(change: BookChange) -> BookRequest {
        BookRequest {
            since: Instant::now(),
            label: String::new(),
            change,
            spell: 42,
        }
    }

    /// The text of the line for this news, if any.
    fn text(
        request: &BookRequest,
        status: Option<&BookActionStatus>,
        following: &mut Option<Instant>,
    ) -> Option<String> {
        let names = SpellNames::parse("42^Synthetic spell");
        // Stand-in words, not the official ones, under their ids.
        let messages = Messages::parse(
            "EQST0002\n0 6\n12053 Begin %1\n12007 End %1\n12045 Stop\n12049 Scribe %1\n12006 Scribed %1\n12044 Scribe stop\n",
        );
        line(request, status, following, (&names, &messages)).map(|said| said.text)
    }

    #[test]
    fn a_memorization_says_when_it_begins_and_ends_once_each() {
        let memorizing = request(BookChange::Memorize);
        let mut following = None;
        assert_eq!(
            text(
                &memorizing,
                Some(&BookActionStatus::Preparing),
                &mut following
            )
            .as_deref(),
            Some("Begin Synthetic spell")
        );
        assert_eq!(
            text(
                &memorizing,
                Some(&BookActionStatus::Preparing),
                &mut following
            ),
            None
        );
        assert_eq!(
            text(
                &memorizing,
                Some(&BookActionStatus::AwaitingReply),
                &mut following
            ),
            None
        );
        assert_eq!(
            text(&memorizing, None, &mut following).as_deref(),
            Some("End Synthetic spell")
        );
        // A later change the server confirms, such as a move, is not this
        // memorization's end.
        assert_eq!(text(&memorizing, None, &mut following), None);
    }

    #[test]
    fn a_scribe_says_when_it_stops_and_a_refusal_says_nothing() {
        let scribing = request(BookChange::Scribe);
        let mut following = None;
        assert_eq!(
            text(
                &scribing,
                Some(&BookActionStatus::Preparing),
                &mut following
            )
            .as_deref(),
            Some("Scribe Synthetic spell")
        );
        let cancelled = BookActionStatus::Cancelled("moved".into());
        assert_eq!(
            text(&scribing, Some(&cancelled), &mut following).as_deref(),
            Some("Scribe stop")
        );
        assert_eq!(text(&scribing, Some(&cancelled), &mut following), None);
        // One the session refuses before the player sits says nothing.
        let refused = request(BookChange::Memorize);
        let rejected = BookActionStatus::Rejected("busy".into());
        assert_eq!(text(&refused, Some(&rejected), &mut following), None);
        assert_eq!(text(&refused, None, &mut following), None);
    }
}
