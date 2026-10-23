//! Display text that preserves item-link identity through string formatting.
use super::Message;
use eq_network_game::chat::ItemLink;

/// Text with inspectable item links. Unlike a wire `Message`, this can be
/// assembled from several formatted arguments and has no single raw packet.
#[derive(Clone, Debug, Default)]
pub struct RichText {
    /// Readable UTF-8 text.
    pub text: String,
    /// Original link bodies, with label ranges relative to this text.
    pub item_links: Vec<ItemLink>,
}

impl RichText {
    /// Copies an argument's link annotations at its inserted byte offset.
    /// Invalid or mismatched spans are ignored instead of creating bad clicks.
    pub fn insert_links(&mut self, argument: &Self, offset: usize) {
        self.item_links
            .extend(argument.item_links.iter().filter_map(|link| {
                let start = offset.checked_add(link.text_start)?;
                let end = offset.checked_add(link.text_end)?;
                if argument.text.get(link.text_start..link.text_end)? != link.text
                    || self.text.get(start..end)? != link.text
                {
                    return None;
                }
                let mut link = link.clone();
                link.text_start = start;
                link.text_end = end;
                Some(link)
            }));
    }
}

impl From<Message> for RichText {
    fn from(message: Message) -> Self {
        Self {
            text: message.text,
            item_links: message.item_links,
        }
    }
}

impl From<String> for RichText {
    fn from(text: String) -> Self {
        Self {
            text,
            item_links: Vec::new(),
        }
    }
}

impl From<&str> for RichText {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

// The wire type deliberately has no equality implementation. Display values
// compare text and every retained link field, independent of a raw packet.
impl PartialEq for RichText {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && self.item_links.len() == other.item_links.len()
            && self.item_links.iter().zip(&other.item_links).all(|(a, b)| {
                (
                    &a.body,
                    &a.text,
                    a.start,
                    a.end,
                    a.text_start,
                    a.text_end,
                    a.item_id,
                ) == (
                    &b.body,
                    &b.text,
                    b.start,
                    b.end,
                    b.text_start,
                    b.text_end,
                    b.item_id,
                )
            })
    }
}

impl Eq for RichText {}
