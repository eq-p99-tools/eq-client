//! The player's friends and ignore lists, and the tells they can reply to,
//! as the Titanium client keeps them (program reads R6 and R8).
//!
//! - `/friends Name` and `/ignore Name` each toggle a name: one on the list,
//!   in any case, comes off, any other goes on as typed, up to
//!   [`LIST_LIMIT`] names (0x46fefe, 0x472739). A friend's name must be
//!   under [`FRIEND_NAME_LIMIT`] bytes; the ignore list checks no length,
//!   and keeps what fits its 64-byte entries. Neither filters characters or
//!   changes case; both drop item-link markers first (0x46d46d). The client
//!   saves both lists after every change.
//! - `/reply` answers the most recent teller, from a list of
//!   [`REPLY_LIMIT`] names where each new one moves to the front (0x467ad1).
//!   The program stores the name a channel 7 or channel 14 message carries
//!   as its sender, both alike (0x45a149); tells the player sends add
//!   nothing themselves, and neither does a message from an ignored sender,
//!   which never shows. The chat feeds it channel 7 only for now: what a
//!   server sends on 14 is not captured (recording checklist 6.412). The list lasts the program's run, through zoning,
//!   and is not saved.
//! - The ignore list silences an ignored sender's channel messages on
//!   every channel but broadcasts and channel 14 (0x459b94). A game
//!   master's message shows, and takes the sender off the list.
use std::collections::VecDeque;

/// The most names a friends or ignore list holds.
pub const LIST_LIMIT: usize = 100;
/// The most tell senders the reply list keeps.
pub const REPLY_LIMIT: usize = 50;
/// A friend's name must be shorter than this, in bytes.
pub const FRIEND_NAME_LIMIT: usize = 16;
/// Any listed name must be shorter than this, in bytes: the lists' entries
/// are 64 bytes with their ending.
pub const ENTRY_LIMIT: usize = 64;
/// The marker that opens and closes an item link in chat text.
const LINK_MARKER: char = '\u{12}';

/// The official client's lines for its lists, by `eqstr_us.txt` number.
pub mod strings {
    /// `/reply` with no tell to reply to.
    pub const NO_REPLY: u32 = 0x304c;
    /// `/rtarget` with no tell to target the sender of.
    pub const NO_REPLY_TARGET: u32 = 0x33e6;
    /// The friends list's header.
    pub const FRIENDS_HEADER: u32 = 0x32b3;
    /// The friends list's count.
    pub const FRIENDS_COUNT: u32 = 0x32b4;
    /// An empty friends list, in two lines.
    pub const FRIENDS_EMPTY: [u32; 2] = [0x32b1, 0x32b2];
    /// A friend added.
    pub const FRIEND_ADDED: u32 = 0x32b8;
    /// A friend removed.
    pub const FRIEND_REMOVED: u32 = 0x32b6;
    /// The friends list is full.
    pub const FRIENDS_FULL: u32 = 0x32b7;
    /// The ignore list's header.
    pub const IGNORE_HEADER: u32 = 0x33b8;
    /// A name ignored.
    pub const IGNORE_ADDED: u32 = 0x33bb;
    /// A name no longer ignored.
    pub const IGNORE_REMOVED: u32 = 0x33b9;
    /// The ignore list is full.
    pub const IGNORE_FULL: u32 = 0x33bc;
    /// A game master cannot be ignored.
    pub const IGNORE_REFUSED: u32 = 0x33ba;
}

/// What toggling a name did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Toggled {
    /// The name went on the list, as spelled here.
    Added(String),
    /// The name came off the list, as the list spelled it.
    Removed(String),
    /// The list is full.
    Full,
    /// The name is not one the list keeps: empty, or too long.
    Invalid,
}

/// A name as typed, as the lists keep it: without item-link markers or
/// the spaces about it, and shorter than `limit` bytes; None for one they
/// cannot keep. The official client drops a link's header with its markers;
/// typed text here carries links apart from its words, so only stray
/// markers are left to drop.
#[must_use]
pub fn kept(name: &str, limit: usize) -> Option<String> {
    let name: String = name.chars().filter(|&c| c != LINK_MARKER).collect();
    let name = name.trim();
    (!name.is_empty() && name.len() < limit).then(|| name.to_owned())
}

/// A friends or an ignore list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NameList {
    names: Vec<String>,
}

impl NameList {
    /// The names, in the order they went on.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Whether the list holds this name, in any case.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.names
            .iter()
            .any(|kept| kept.eq_ignore_ascii_case(name))
    }

    /// Takes the name off the list if it is there, else puts it on, when
    /// it is shorter than `limit` bytes.
    pub fn toggle(&mut self, name: &str, limit: usize) -> Toggled {
        let Some(name) = kept(name, limit.min(ENTRY_LIMIT)) else {
            return Toggled::Invalid;
        };
        if let Some(index) = self
            .names
            .iter()
            .position(|kept| kept.eq_ignore_ascii_case(&name))
        {
            return Toggled::Removed(self.names.remove(index));
        }
        if self.names.len() >= LIST_LIMIT {
            return Toggled::Full;
        }
        self.names.push(name.clone());
        Toggled::Added(name)
    }

    /// Takes the name off the list; true when it was there.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.names.len();
        self.names.retain(|kept| !kept.eq_ignore_ascii_case(name));
        self.names.len() != before
    }

    /// The list as this client saves it: one name a line.
    #[must_use]
    pub fn text(&self) -> String {
        self.names
            .iter()
            .flat_map(|name| [name.as_str(), "\n"])
            .collect()
    }

    /// A list saved by [`NameList::text`], or given as names a line,
    /// keeping the names a list can and dropping repeats and any past the
    /// limit.
    #[must_use]
    pub fn read(text: &str) -> Self {
        let mut list = Self::default();
        for name in text.lines().filter_map(|name| kept(name, ENTRY_LIMIT)) {
            if !list.contains(&name) && list.names.len() < LIST_LIMIT {
                list.names.push(name);
            }
        }
        list
    }
}

/// The senders of the tells the player received, latest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replies {
    senders: VecDeque<String>,
}

impl Replies {
    /// Notes a tell's sender, moving them to the front.
    pub fn heard(&mut self, sender: &str) {
        let sender = sender.trim();
        if sender.is_empty() {
            return;
        }
        self.senders
            .retain(|kept| !kept.eq_ignore_ascii_case(sender));
        self.senders.push_front(sender.to_owned());
        self.senders.truncate(REPLY_LIMIT);
    }

    /// Who `/reply` answers: the latest sender.
    #[must_use]
    pub fn latest(&self) -> Option<&str> {
        self.senders.front().map(String::as_str)
    }
}

/// Both lists and the reply list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contacts {
    /// The friends list.
    pub friends: NameList,
    /// The ignore list.
    pub ignored: NameList,
    /// The tells' senders.
    pub replies: Replies,
}

/// Which list a section of the saved file holds.
const FRIENDS_SECTION: &str = "[friends]";
const IGNORE_SECTION: &str = "[ignore]";

impl Contacts {
    /// Both lists as this client saves them.
    #[must_use]
    pub fn text(&self) -> String {
        format!(
            "{FRIENDS_SECTION}\n{}{IGNORE_SECTION}\n{}",
            self.friends.text(),
            self.ignored.text()
        )
    }

    /// The lists saved by [`Contacts::text`]; the reply list starts empty.
    #[must_use]
    pub fn read(text: &str) -> Self {
        let (mut friends, mut ignored) = (String::new(), String::new());
        let mut into: Option<&mut String> = None;
        for line in text.lines() {
            match line.trim() {
                FRIENDS_SECTION => into = Some(&mut friends),
                IGNORE_SECTION => into = Some(&mut ignored),
                name => {
                    if let Some(list) = into.as_deref_mut() {
                        list.push_str(name);
                        list.push('\n');
                    }
                }
            }
        }
        Self {
            friends: NameList::read(&friends),
            ignored: NameList::read(&ignored),
            replies: Replies::default(),
        }
    }

    /// Whether a channel message is silenced: an ignored sender's, on any
    /// channel but broadcasts and channel 14. A game master's always shows
    /// and takes them off the list, which `gm` says. An emote names channel
    /// 22 as a channel message but comes by its own message too, which the
    /// program's other ignore tests (not classified in R6) may cover; it
    /// shows.
    pub fn silences(&mut self, channel: crate::chat::ChannelName, sender: &str, gm: bool) -> bool {
        use crate::chat::ChannelName;
        let channel_message = matches!(
            channel,
            ChannelName::Guild
                | ChannelName::Group
                | ChannelName::Shout
                | ChannelName::Auction
                | ChannelName::Ooc
                | ChannelName::Tell
                | ChannelName::Say
                | ChannelName::GmSay
                | ChannelName::Raid
                // Any other channel a channel message names.
                | ChannelName::Unknown
        );
        if !channel_message || !self.ignored.contains(sender) {
            return false;
        }
        if gm {
            self.ignored.remove(sender);
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChannelName;

    #[test]
    fn names_toggle_on_and_off_as_typed_up_to_the_limit() {
        let mut list = NameList::default();
        assert_eq!(
            list.toggle(" alpha ", FRIEND_NAME_LIMIT),
            Toggled::Added("alpha".into())
        );
        assert!(list.contains("ALPHA"));
        assert_eq!(
            list.toggle("ALPHA", FRIEND_NAME_LIMIT),
            Toggled::Removed("alpha".into())
        );
        assert!(!list.contains("alpha"));
        assert_eq!(list.toggle("", FRIEND_NAME_LIMIT), Toggled::Invalid);
        assert_eq!(
            list.toggle(&"a".repeat(16), FRIEND_NAME_LIMIT),
            Toggled::Invalid
        );
        assert_eq!(
            list.toggle(&"a".repeat(15), FRIEND_NAME_LIMIT),
            Toggled::Added("a".repeat(15))
        );
        assert_eq!(
            list.toggle("Two Words\u{12}", FRIEND_NAME_LIMIT),
            Toggled::Added("Two Words".into())
        );
        // The ignore list checks no length of its own, only what its entries hold.
        let mut ignored = NameList::default();
        assert_eq!(
            ignored.toggle(&"b".repeat(40), ENTRY_LIMIT),
            Toggled::Added("b".repeat(40))
        );
        assert_eq!(
            ignored.toggle(&"b".repeat(64), ENTRY_LIMIT),
            Toggled::Invalid
        );
        let mut full = NameList::default();
        for index in 0..LIST_LIMIT {
            let name: String = format!("N{}", index_name(index));
            assert!(
                matches!(full.toggle(&name, FRIEND_NAME_LIMIT), Toggled::Added(_)),
                "{name}"
            );
        }
        assert_eq!(full.toggle("Another", FRIEND_NAME_LIMIT), Toggled::Full);
        assert_eq!(NameList::read(&full.text()), full);
    }

    /// A name of letters for a number.
    fn index_name(mut index: usize) -> String {
        let mut name = String::new();
        loop {
            name.push(char::from(b'a' + u8::try_from(index % 26).unwrap()));
            index /= 26;
            if index == 0 {
                return name;
            }
        }
    }

    #[test]
    fn replies_answer_the_latest_sender_once_each() {
        let mut replies = Replies::default();
        assert_eq!(replies.latest(), None);
        replies.heard("Alpha");
        replies.heard("Beta");
        replies.heard("alpha");
        assert_eq!(replies.latest(), Some("alpha"));
        assert_eq!(replies.senders.len(), 2);
        for index in 0..60 {
            replies.heard(&format!("S{}", index_name(index)));
        }
        assert_eq!(replies.senders.len(), REPLY_LIMIT);
    }

    #[test]
    fn ignore_silences_channel_messages_but_broadcasts_echoes_and_game_masters() {
        let mut contacts = Contacts::default();
        contacts.ignored.toggle("Pest", ENTRY_LIMIT);
        assert!(contacts.silences(ChannelName::Tell, "pest", false));
        assert!(contacts.silences(ChannelName::Ooc, "Pest", false));
        assert!(!contacts.silences(ChannelName::Broadcast, "Pest", false));
        assert!(!contacts.silences(ChannelName::TellEcho, "Pest", false));
        assert!(!contacts.silences(ChannelName::Emote, "Pest", false));
        assert!(contacts.silences(ChannelName::Unknown, "Pest", false));
        assert!(!contacts.silences(ChannelName::Say, "Friend", false));
        assert!(!contacts.silences(ChannelName::Say, "Pest", true));
        assert!(
            !contacts.ignored.contains("Pest"),
            "a game master comes off the list"
        );
        let mut saved = Contacts::default();
        saved.friends.toggle("Friend", FRIEND_NAME_LIMIT);
        saved.ignored.toggle("Pest", ENTRY_LIMIT);
        assert_eq!(Contacts::read(&saved.text()), saved);
    }
}
