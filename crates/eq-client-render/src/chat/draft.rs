//! Text editing with atomic, admission-scoped item spans.
use eq_client_core::{GameDialect, ItemLink, chat::chat_link};
use std::ops::Range;

const MAX_TEXT: usize = 512;

type Scope = (u64, GameDialect);

#[derive(Default)]
pub(super) struct Draft {
    text: String,
    caret: usize,
    links: Vec<Link>,
    revision: u64,
}

struct Link {
    range: Range<usize>,
    scope: Scope,
    wire: String,
}

impl Draft {
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) const fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn has_links(&self) -> bool {
        !self.links.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.text.clear();
        self.links.clear();
        self.caret = 0;
        self.changed();
    }

    pub(super) fn replace(&mut self, text: &str) {
        self.clear();
        self.insert_text(text);
    }

    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// The caret sits at a UTF-8 boundary and never inside an item link.
    pub(super) fn marked(&self) -> String {
        let mut text = self.text.clone();
        text.insert(self.caret, '|');
        text
    }

    pub(super) fn insert_text(&mut self, text: &str) {
        let mut allowed = String::new();
        let room = MAX_TEXT - self.text.len();
        for character in text.chars().filter(|c| !c.is_control()) {
            if allowed.len() + character.len_utf8() > room {
                break;
            }
            allowed.push(character);
        }
        if !allowed.is_empty() {
            self.insert(&allowed);
        }
    }

    /// Insert a complete label and its verified wire representation together.
    pub(super) fn insert_link(
        &mut self,
        link: &ItemLink,
        scope: Scope,
    ) -> Result<(), &'static str> {
        if self.text.len() + link.text.len() > MAX_TEXT {
            return Err("This item link does not fit in the chat draft");
        }
        let wire = chat_link(scope.1, link).map_err(|_| "This item link cannot be sent")?;
        let start = self.caret;
        self.insert(&link.text);
        let index = self
            .links
            .partition_point(|other| other.range.start < start);
        self.links.insert(
            index,
            Link {
                range: start..self.caret,
                scope,
                wire,
            },
        );
        Ok(())
    }

    fn insert(&mut self, text: &str) {
        self.text.insert_str(self.caret, text);
        for link in &mut self.links {
            if link.range.start >= self.caret {
                link.range.start += text.len();
                link.range.end += text.len();
            }
        }
        self.caret += text.len();
        self.changed();
    }

    /// Build the message input only at submission, keeping wire markup out of
    /// the editor and checking that every token belongs to the current session.
    pub(super) fn encoded(
        &self,
        admission: Option<u64>,
        dialect: GameDialect,
    ) -> Result<String, String> {
        let mut wire = String::new();
        let mut end = 0;
        for link in &self.links {
            if Some(link.scope.0) != admission || link.scope.1 != dialect {
                return Err("This draft has an item link from a different admission".into());
            }
            wire.push_str(&self.text[end..link.range.start]);
            wire.push_str(&link.wire);
            end = link.range.end;
        }
        wire.push_str(&self.text[end..]);
        Ok(wire)
    }

    pub(super) fn home(&mut self) {
        self.move_to(0);
    }

    pub(super) fn end(&mut self) {
        self.move_to(self.text.len());
    }

    pub(super) fn left(&mut self) {
        self.move_to(self.previous());
    }

    pub(super) fn right(&mut self) {
        self.move_to(self.next());
    }

    pub(super) fn backspace(&mut self) {
        self.remove(self.previous()..self.caret);
    }

    pub(super) fn delete(&mut self) {
        self.remove(self.caret..self.next());
    }

    fn move_to(&mut self, caret: usize) {
        if self.caret != caret {
            self.caret = caret;
            self.changed();
        }
    }

    fn previous(&self) -> usize {
        self.links
            .iter()
            .find(|link| link.range.end == self.caret)
            .map_or_else(
                || self.text.floor_char_boundary(self.caret.saturating_sub(1)),
                |link| link.range.start,
            )
    }

    fn next(&self) -> usize {
        self.links
            .iter()
            .find(|link| link.range.start == self.caret)
            .map_or_else(
                || {
                    self.text
                        .ceil_char_boundary((self.caret + 1).min(self.text.len()))
                },
                |link| link.range.end,
            )
    }

    fn remove(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let width = range.len();
        self.text.replace_range(range.clone(), "");
        self.links.retain_mut(|link| {
            if link.range.end <= range.start {
                true
            } else if link.range.start >= range.end {
                link.range.start -= width;
                link.range.end -= width;
                true
            } else {
                false
            }
        });
        self.caret = range.start;
        self.changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u32, text: &str) -> ItemLink {
        ItemLink {
            body: format!("0{id:05X}{:031X}abcd1234", 0),
            text: text.into(),
            item_id: id,
            start: 0,
            end: 0,
            text_start: 0,
            text_end: text.len(),
        }
    }

    #[test]
    fn unicode_text_edits_and_atomic_deletion_keep_other_links_intact() {
        let scope = (10, GameDialect::Titanium);
        let mut draft = Draft::default();
        draft.insert_text("É");
        draft.insert_link(&item(1, "First link"), scope).unwrap();
        draft.insert_text(" ");
        draft.insert_link(&item(2, "Other link"), scope).unwrap();
        draft.left();
        assert_eq!(draft.marked(), "ÉFirst link |Other link");
        draft.left();
        draft.backspace();
        draft.insert_text("sword");
        draft.home();
        draft.delete();
        draft.insert_text("/auc ");
        assert_eq!(draft.text(), "/auc sword Other link");
        let wire = draft.encoded(Some(10), scope.1).unwrap();
        assert!(wire.starts_with("/auc sword \x12000002"));
        assert!(wire.ends_with("abcd1234Other link\x12"));
        assert_eq!(wire.matches('\x12').count(), 2);
        assert!(draft.encoded(Some(11), scope.1).is_err());
        assert!(draft.encoded(Some(10), GameDialect::EqMac).is_err());
    }

    #[test]
    fn rejected_links_and_plain_controls_never_make_partial_markup() {
        let scope = (10, GameDialect::Titanium);
        let mut draft = Draft::default();
        draft.insert_text(&"x".repeat(MAX_TEXT - 1));
        assert!(draft.insert_link(&item(1, "Too long"), scope).is_err());
        draft.insert_text("\x12🙂");
        assert_eq!(draft.text().len(), MAX_TEXT - 1);
        assert!(!draft.has_links());
        draft.replace("new");
        assert_eq!(draft.marked(), "new|");
    }
}
