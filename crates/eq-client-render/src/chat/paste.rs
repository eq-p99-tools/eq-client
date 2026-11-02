//! Clipboard replies belong to the focused draft and session that requested them.
use bevy::clipboard::ClipboardRead;

#[derive(Default)]
pub(crate) struct Paste(Option<Pending>);

struct Pending {
    read: ClipboardRead,
    session: Option<u64>,
    draft: String,
}

impl Paste {
    pub(super) fn request(&mut self, read: ClipboardRead, session: Option<u64>, draft: &str) {
        self.0 = Some(Pending {
            read,
            session,
            draft: draft.into(),
        });
    }

    pub(super) fn clear(&mut self) {
        self.0 = None;
    }

    /// Append at most one line, never submit it, and discard replies to an old draft.
    pub(super) fn apply(
        &mut self,
        draft: &mut String,
        composing: bool,
        session: Option<u64>,
    ) -> Result<(), ()> {
        let Some(pending) = &mut self.0 else {
            return Ok(());
        };
        if !composing || pending.session != session || pending.draft != *draft {
            self.clear();
            return Ok(());
        }
        let Some(result) = pending.read.poll_result() else {
            return Ok(());
        };
        self.clear();
        let text = result.map_err(|_| ())?;
        for character in text.chars().filter(|character| !character.is_control()) {
            if draft.len() + character.len_utf8() > 512 {
                break;
            }
            draft.push(character);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_is_bounded_utf8_and_cannot_inject_an_enter() {
        let mut paste = Paste::default();
        let mut draft = "a".repeat(508);
        paste.request(ClipboardRead::Ready(Ok("\r\n🗡x".into())), Some(1), &draft);
        paste.apply(&mut draft, true, Some(1)).unwrap();
        assert_eq!(draft.len(), 512);
        assert!(draft.ends_with('🗡'));
        assert!(!draft.chars().any(char::is_control));
        assert!(paste.0.is_none());
    }

    #[test]
    fn a_reply_cannot_change_an_unfocused_replaced_or_new_session_draft() {
        for (focused, session, current) in [
            (false, Some(1), "original"),
            (true, Some(2), "original"),
            (true, None, "original"),
            (true, Some(1), "edited"),
        ] {
            let mut paste = Paste::default();
            paste.request(ClipboardRead::Ready(Ok("late".into())), Some(1), "original");
            let mut draft = current.to_owned();
            paste.apply(&mut draft, focused, session).unwrap();
            assert_eq!(draft, current);
            assert!(paste.0.is_none());
        }
    }
}
