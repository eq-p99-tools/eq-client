//! Clipboard replies belong to the focused draft and session that requested them.
use super::draft::Draft;
use bevy::clipboard::ClipboardRead;

#[derive(Default)]
pub(crate) struct Paste(Option<Pending>);

struct Pending {
    read: ClipboardRead,
    session: Option<u64>,
    draft_revision: u64,
}

impl Paste {
    pub(super) fn request(
        &mut self,
        read: ClipboardRead,
        session: Option<u64>,
        draft_revision: u64,
    ) {
        self.0 = Some(Pending {
            read,
            session,
            draft_revision,
        });
    }

    pub(super) fn clear(&mut self) {
        self.0 = None;
    }

    /// Paste one bounded text span at the caret. A late reply cannot edit a
    /// changed draft, including a caret move or insertion of an item link.
    pub(super) fn apply(
        &mut self,
        draft: &mut Draft,
        composing: bool,
        session: Option<u64>,
    ) -> Result<(), ()> {
        let Some(pending) = &mut self.0 else {
            return Ok(());
        };
        if !composing || pending.session != session || pending.draft_revision != draft.revision() {
            self.clear();
            return Ok(());
        }
        let Some(result) = pending.read.poll_result() else {
            return Ok(());
        };
        self.clear();
        draft.insert_text(&result.map_err(|_| ())?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_is_bounded_utf8_and_cannot_inject_an_enter() {
        let mut paste = Paste::default();
        let mut draft = Draft::default();
        draft.insert_text(&"a".repeat(508));
        paste.request(
            ClipboardRead::Ready(Ok("\r\n🗡x".into())),
            Some(1),
            draft.revision(),
        );
        paste.apply(&mut draft, true, Some(1)).unwrap();
        assert_eq!(draft.text().len(), 512);
        assert!(draft.text().ends_with('🗡'));
        assert!(!draft.text().chars().any(char::is_control));
        assert!(paste.0.is_none());
    }

    #[test]
    fn a_reply_cannot_change_an_unfocused_replaced_or_new_session_draft() {
        for (focused, session, current, moved) in [
            (false, Some(1), "original", false),
            (true, Some(2), "original", false),
            (true, None, "original", false),
            (true, Some(1), "edited", false),
            (true, Some(1), "original", true),
        ] {
            let mut paste = Paste::default();
            let mut draft = Draft::default();
            draft.insert_text("original");
            paste.request(
                ClipboardRead::Ready(Ok("late".into())),
                Some(1),
                draft.revision(),
            );
            if current != draft.text() {
                draft.replace(current);
            }
            if moved {
                draft.home();
            }
            paste.apply(&mut draft, focused, session).unwrap();
            assert_eq!(draft.text(), current);
            assert!(paste.0.is_none());
        }
    }
}
