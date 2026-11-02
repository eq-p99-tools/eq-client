//! Enforce the source byte limit without breaking egui's character-indexed API.
use crate::script::MAX_SOURCE_BYTES;
use bevy_egui::egui::{TextBuffer, text::CharIndex};

pub(super) struct Bounded<'a>(pub &'a mut String);

impl TextBuffer for Bounded<'_> {
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<Bounded<'static>>()
    }
    fn is_mutable(&self) -> bool {
        true
    }
    fn as_str(&self) -> &str {
        self.0
    }
    fn insert_text(&mut self, text: &str, index: CharIndex) -> usize {
        let text = text.replace("\r\n", "\n");
        let available = MAX_SOURCE_BYTES.saturating_sub(self.0.len());
        let end = text.floor_char_boundary(available.min(text.len()));
        self.0.insert_text(&text[..end], index)
    }
    fn delete_char_range(&mut self, range: std::ops::Range<CharIndex>) {
        self.0.delete_char_range(range);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_insertions_respect_bytes_and_character_positions() {
        let mut text = "é!".to_owned();
        assert_eq!(Bounded(&mut text).insert_text("中\r\n", CharIndex(1)), 2);
        assert_eq!(text, "é中\n!");
        Bounded(&mut text).delete_char_range(CharIndex(1)..CharIndex(2));
        assert_eq!(text, "é\n!");
        let mut text = "x".repeat(MAX_SOURCE_BYTES - 1);
        assert_eq!(Bounded(&mut text).insert_text("é", CharIndex(0)), 0);
        assert_eq!(text.len(), MAX_SOURCE_BYTES - 1);
    }
}
