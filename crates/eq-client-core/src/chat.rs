//! Typed chat presentation and bounded per-tab history.
pub use eq_network_game::chat::{ChannelName, Message};
use std::collections::{BTreeMap, VecDeque};

/// A received message retaining its structured item links and channel metadata.
#[derive(Clone, Debug)]
pub struct ChatLine {
    /// Communication channel.
    pub channel: ChannelName,
    /// Author, when supplied by the server.
    pub sender: Option<String>,
    /// Recipient, when supplied by the server.
    pub target: Option<String>,
    /// Decoded text and original item-link data.
    pub message: Message,
}

/// Channel groups presented by the chat window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub enum ChatTab {
    /// Every channel, in receive order.
    #[default]
    All,
    /// Local speech.
    Say,
    /// Auction channel.
    Auction,
    /// Out-of-character channel.
    Ooc,
    /// Guild chat and guild MOTD.
    Guild,
    /// Group channel.
    Group,
    /// Private tells.
    Tell,
    /// Raid channel.
    Raid,
    /// Zone shouts.
    Shout,
    /// Emotes.
    Emote,
    /// MOTD, system, GM, broadcast, and unrecognized channels.
    System,
}
impl ChatTab {
    /// Stable tab order.
    pub const ALL: [Self; 11] = [
        Self::All,
        Self::Say,
        Self::Auction,
        Self::Ooc,
        Self::Guild,
        Self::Group,
        Self::Tell,
        Self::Raid,
        Self::Shout,
        Self::Emote,
        Self::System,
    ];
    /// Compact visible tab label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Say => "Say",
            Self::Auction => "Auc",
            Self::Ooc => "OOC",
            Self::Guild => "Guild",
            Self::Group => "Group",
            Self::Tell => "Tell",
            Self::Raid => "Raid",
            Self::Shout => "Shout",
            Self::Emote => "Emote",
            Self::System => "System",
        }
    }
    /// Routes every protocol channel, including future values, into a visible tab.
    pub fn for_channel(channel: ChannelName) -> Self {
        match channel {
            ChannelName::Say => Self::Say,
            ChannelName::Auction => Self::Auction,
            ChannelName::Ooc => Self::Ooc,
            ChannelName::Guild | ChannelName::GuildMotd => Self::Guild,
            ChannelName::Group => Self::Group,
            ChannelName::Tell | ChannelName::TellEcho => Self::Tell,
            ChannelName::Raid => Self::Raid,
            ChannelName::Shout => Self::Shout,
            ChannelName::Emote => Self::Emote,
            _ => Self::System,
        }
    }
}

/// Exact default palette from the mobile client's `src/App.css` (not its optional high-contrast override).
pub fn channel_rgb(channel: ChannelName) -> [u8; 3] {
    match channel {
        ChannelName::Auction => [0x00, 0x80, 0x00],
        ChannelName::Ooc => [0xff, 0xcc, 0x66],
        ChannelName::Guild | ChannelName::GuildMotd => [0x28, 0xf0, 0x28],
        ChannelName::Group => [0x00, 0xff, 0xff],
        ChannelName::Tell | ChannelName::TellEcho => [0xbe, 0x28, 0xbe],
        ChannelName::Raid => [0x00, 0xc8, 0xc8],
        ChannelName::Shout => [0xff, 0x00, 0x00],
        ChannelName::Emote => [0x5a, 0x5a, 0xff],
        _ => [0xff, 0xff, 0xff],
    }
}

/// Keeps 200 messages per channel group so busy auctions cannot evict guild/tell history.
#[derive(Default)]
pub struct ChatHistory {
    messages: BTreeMap<ChatTab, VecDeque<(u64, ChatLine)>>,
    revision: u64,
}
impl ChatHistory {
    /// Adds nonempty text without dropping its item links.
    pub fn push(&mut self, line: ChatLine) {
        if line.message.text.is_empty() {
            return;
        }
        self.revision += 1;
        let entries = self
            .messages
            .entry(ChatTab::for_channel(line.channel))
            .or_default();
        entries.push_back((self.revision, line));
        if entries.len() > 200 {
            entries.pop_front();
        }
    }
    /// Monotonic receive sequence for redraw and unread tracking.
    pub const fn revision(&self) -> u64 {
        self.revision
    }
    /// Counts unread retained messages without allocating or sorting.
    pub fn unread(&self, tab: ChatTab, seen: u64) -> usize {
        self.messages
            .iter()
            .filter(|(key, _)| tab == ChatTab::All || **key == tab)
            .map(|(_, entries)| entries.iter().filter(|(id, _)| *id > seen).count())
            .sum()
    }

    /// Selected history in original receive order, including mixed-channel All.
    pub fn lines(&self, tab: ChatTab) -> Vec<(u64, &ChatLine)> {
        let mut lines: Vec<_> = self
            .messages
            .iter()
            .filter(|(key, _)| tab == ChatTab::All || **key == tab)
            .flat_map(|(_, entries)| entries.iter().map(|(id, line)| (*id, line)))
            .collect();
        lines.sort_by_key(|(id, _)| *id);
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(channel: ChannelName) -> ChatLine {
        ChatLine {
            channel,
            sender: None,
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: "Synthetic message".into(),
                item_links: vec![],
            },
        }
    }
    #[test]
    fn busy_channels_do_not_evict_quiet_channels_and_all_preserves_order() {
        let mut history = ChatHistory::default();
        history.push(line(ChannelName::Guild));
        for _ in 0..250 {
            history.push(line(ChannelName::Auction));
        }
        history.push(line(ChannelName::Tell));
        assert_eq!(history.lines(ChatTab::Guild).len(), 1);
        assert_eq!(history.lines(ChatTab::Auction).len(), 200);
        let all = history.lines(ChatTab::All);
        assert_eq!(all.len(), 202);
        assert_eq!(all[0].1.channel, ChannelName::Guild);
        assert_eq!(all.last().unwrap().1.channel, ChannelName::Tell);
        assert_eq!(ChatTab::for_channel(ChannelName::Unknown), ChatTab::System);
        // The echo of a tell the player sent reads with the tells.
        assert_eq!(ChatTab::for_channel(ChannelName::TellEcho), ChatTab::Tell);
        assert_eq!(
            channel_rgb(ChannelName::TellEcho),
            channel_rgb(ChannelName::Tell)
        );
        assert_eq!(ChatTab::for_channel(ChannelName::GuildMotd), ChatTab::Guild);
    }
    #[test]
    fn palette_matches_mobile_default_values() {
        assert_eq!(channel_rgb(ChannelName::Auction), [0, 128, 0]);
        assert_eq!(channel_rgb(ChannelName::Ooc), [255, 204, 102]);
        assert_eq!(channel_rgb(ChannelName::GuildMotd), [40, 240, 40]);
        assert_eq!(channel_rgb(ChannelName::Tell), [190, 40, 190]);
        assert_eq!(channel_rgb(ChannelName::Unknown), [255, 255, 255]);
    }
}
