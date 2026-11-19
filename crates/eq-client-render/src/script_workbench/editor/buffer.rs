//! Enforce the source byte limit without breaking egui's character-indexed API.
use crate::script::MAX_SOURCE_BYTES;
use bevy_egui::egui::{TextBuffer, text::CharIndex};

/// The source text, and whether it may change: a run's source is read-only.
pub(super) struct Bounded<'a>(pub &'a mut String, pub bool);

impl TextBuffer for Bounded<'_> {
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<Bounded<'static>>()
    }
    fn is_mutable(&self) -> bool {
        self.1
    }
    fn as_str(&self) -> &str {
        self.0
    }
    // egui's text editor calls these whatever `is_mutable` says, so a
    // read-only buffer refuses the change here.
    fn insert_text(&mut self, text: &str, index: CharIndex) -> usize {
        if !self.1 {
            return 0;
        }
        let text = text.replace("\r\n", "\n");
        let available = MAX_SOURCE_BYTES.saturating_sub(self.0.len());
        let end = text.floor_char_boundary(available.min(text.len()));
        self.0.insert_text(&text[..end], index)
    }
    fn delete_char_range(&mut self, range: std::ops::Range<CharIndex>) {
        if !self.1 {
            return;
        }
        self.0.delete_char_range(range);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_insertions_respect_bytes_and_character_positions() {
        let mut text = "é!".to_owned();
        assert_eq!(
            Bounded(&mut text, true).insert_text("中\r\n", CharIndex(1)),
            2
        );
        assert_eq!(text, "é中\n!");
        Bounded(&mut text, true).delete_char_range(CharIndex(1)..CharIndex(2));
        assert_eq!(text, "é\n!");
        let mut text = "x".repeat(MAX_SOURCE_BYTES - 1);
        assert_eq!(Bounded(&mut text, true).insert_text("é", CharIndex(0)), 0);
        assert_eq!(text.len(), MAX_SOURCE_BYTES - 1);
    }

    #[test]
    fn a_read_only_buffer_refuses_every_change() {
        let mut text = "wait(1)".to_owned();
        let mut buffer = Bounded(&mut text, false);
        assert_eq!(buffer.insert_text("x", CharIndex(0)), 0);
        buffer.delete_char_range(CharIndex(0)..CharIndex(1));
        buffer.replace_with("other");
        assert_eq!(buffer.take(), "wait(1)");
        assert_eq!(text, "wait(1)");
    }
}
