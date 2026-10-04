//! Typed chat presentation and bounded per-tab history.
use chrono::NaiveDateTime;
pub use eq_network_game::chat::{ChannelName, Message, SpeakMode};
use std::collections::{BTreeMap, VecDeque};

/// Whose words a chat line holds, which decides whether the official
/// client's log takes it: the server's and the official client's go in, this
/// client's own never do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The server's words.
    Server,
    /// The official client's words: its string by id, or a line it writes
    /// itself, such as the rule under a `/who` list.
    Official,
    /// This client's own words, such as one of its refusals.
    Client,
}

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
    /// Whose words these are.
    pub source: Source,
    /// The message type the server gave the line (`EQEmu`'s numbers, 256
    /// and up for the kinds the official client's Colors page sets), by
    /// which the official client colours it; None for a channel's line,
    /// whose channel says it, and for this client's own lines.
    pub message_type: Option<u32>,
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

/// The installed client's string for an NPC's say, naming the speaker,
/// then the text: the one a server's own formatted NPC say uses.
pub const NPC_SAY: u32 = 1032;
/// Its string for an NPC's shout, in the same form.
pub const NPC_SHOUT: u32 = 1034;
/// Its string for an NPC's emote, the speaker, then the text.
pub const NPC_EMOTE: u32 = 1036;

/// How a server's special message reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spoken {
    /// As the installed client's string with this id, naming the speaker,
    /// then the text, as the server's own formatted NPC line of that kind
    /// reads.
    String(u32),
    /// As a line of this channel is worded.
    Channel(ChannelName),
}

/// How a server's special message reads, by how its speaker speaks: an
/// NPC's say, shout or emote in the common tongue as the installed client's
/// string for that kind of NPC line ([`NPC_SAY`], [`NPC_SHOUT`],
/// [`NPC_EMOTE`]), and a word to the group as a group line. None, so it
/// stays a System line that reads as its text, for a plain server line, an
/// emote shown as its text alone, a mode not known, a line with no speaker,
/// and a line in a tongue other than the common one, whose form waits for a
/// recording. That the official client shows each mode so is inferred from
/// `EQEmu`'s notes on its speak modes, not checked.
#[must_use]
pub fn spoken(
    mode: Option<SpeakMode>,
    language: Option<u8>,
    speaker: Option<&str>,
) -> Option<Spoken> {
    if speaker.is_none_or(str::is_empty) || language.is_some_and(|language| language != 0) {
        return None;
    }
    Some(match mode? {
        SpeakMode::Say => Spoken::String(NPC_SAY),
        SpeakMode::Shout => Spoken::String(NPC_SHOUT),
        SpeakMode::Emote => Spoken::String(NPC_EMOTE),
        SpeakMode::Group => Spoken::Channel(ChannelName::Group),
        SpeakMode::Raw | SpeakMode::EmoteAlt | SpeakMode::Other(_) => return None,
    })
}

/// The number servers give the kind of text a channel's lines are, which
/// the official client keys its Colors page by: 256 for what is said, 257
/// for tells, and on. None for a channel whose lines are of many kinds, as
/// the system lines are. The numbers are `EQEmu`'s; that the official
/// client's colours follow them is inferred.
#[must_use]
pub const fn color_kind(channel: ChannelName) -> Option<u16> {
    Some(match channel {
        ChannelName::Say => 256,
        ChannelName::Tell => 257,
        ChannelName::Group => 258,
        ChannelName::Guild | ChannelName::GuildMotd => 259,
        ChannelName::Ooc => 260,
        ChannelName::Auction => 261,
        ChannelName::Shout => 262,
        ChannelName::Emote => 263,
        ChannelName::Broadcast => 269,
        ChannelName::TellEcho => 308,
        ChannelName::Raid => 327,
        _ => return None,
    })
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
    messages: BTreeMap<ChatTab, VecDeque<(u64, NaiveDateTime, ChatLine)>>,
    revision: u64,
}

/// A line the history keeps, as [`ChatHistory::arrivals`] gives it.
#[derive(Clone, Copy, Debug)]
pub struct Arrival<'a> {
    /// Its place in the order lines arrived, as [`ChatHistory::lines`]
    /// gives it.
    pub id: u64,
    /// When it arrived, by the computer's clock.
    pub at: NaiveDateTime,
    /// The line.
    pub line: &'a ChatLine,
}

impl ChatHistory {
    /// Adds nonempty text without dropping its item links, as arriving now
    /// by the computer's clock.
    pub fn push(&mut self, line: ChatLine) {
        self.push_at(line, chrono::Local::now().naive_local());
    }

    /// Adds nonempty text as [`push`](Self::push) does, as arriving at a
    /// given time by the computer's clock.
    pub fn push_at(&mut self, line: ChatLine, at: NaiveDateTime) {
        if line.message.text.is_empty() {
            return;
        }
        self.revision += 1;
        let entries = self
            .messages
            .entry(ChatTab::for_channel(line.channel))
            .or_default();
        entries.push_back((self.revision, at, line));
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
            .map(|(_, entries)| entries.iter().filter(|(id, ..)| *id > seen).count())
            .sum()
    }

    /// Selected history in original receive order, including mixed-channel All.
    pub fn lines(&self, tab: ChatTab) -> Vec<(u64, &ChatLine)> {
        self.arrivals(tab)
            .into_iter()
            .map(|arrival| (arrival.id, arrival.line))
            .collect()
    }

    /// The lines [`lines`](Self::lines) selects, in the same order, each
    /// with when it arrived.
    pub fn arrivals(&self, tab: ChatTab) -> Vec<Arrival<'_>> {
        let mut lines: Vec<_> = self
            .messages
            .iter()
            .filter(|(key, _)| tab == ChatTab::All || **key == tab)
            .flat_map(|(_, entries)| {
                entries.iter().map(|(id, at, line)| Arrival {
                    id: *id,
                    at: *at,
                    line,
                })
            })
            .collect();
        lines.sort_by_key(|arrival| arrival.id);
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(channel: ChannelName) -> ChatLine {
        ChatLine {
            channel,
            message_type: None,
            sender: None,
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: "Synthetic message".into(),
                item_links: vec![],
            },
            source: Source::Server,
        }
    }
    #[test]
    fn a_special_message_reads_as_an_npc_line_of_its_kind() {
        let named = Some("a guard");
        assert_eq!(
            [
                SpeakMode::Say,
                SpeakMode::Shout,
                SpeakMode::Emote,
                SpeakMode::Group
            ]
            .map(|mode| spoken(Some(mode), Some(0), named)),
            [
                Some(Spoken::String(NPC_SAY)),
                Some(Spoken::String(NPC_SHOUT)),
                Some(Spoken::String(NPC_EMOTE)),
                Some(Spoken::Channel(ChannelName::Group))
            ]
        );
        // The rest stay System lines that read as their text.
        for mode in [SpeakMode::Raw, SpeakMode::EmoteAlt, SpeakMode::Other(9)] {
            assert_eq!(spoken(Some(mode), Some(0), named), None);
        }
        assert_eq!(spoken(Some(SpeakMode::Say), Some(0), None), None);
        assert_eq!(spoken(Some(SpeakMode::Say), Some(0), Some("")), None);
        assert_eq!(spoken(Some(SpeakMode::Say), Some(4), named), None);
        // A message without a speak mode keeps its own channel.
        assert_eq!(spoken(None, None, named), None);
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
    fn a_line_keeps_when_it_arrived() {
        let mut history = ChatHistory::default();
        let at = |second| {
            chrono::NaiveDate::from_ymd_opt(2026, 10, 3)
                .and_then(|day| day.and_hms_opt(17, 42, second))
                .unwrap()
        };
        history.push_at(line(ChannelName::Tell), at(5));
        history.push_at(line(ChannelName::Guild), at(9));
        let arrivals = history.arrivals(ChatTab::All);
        assert_eq!(
            arrivals
                .iter()
                .map(|arrival| (arrival.id, arrival.at))
                .collect::<Vec<_>>(),
            [(1, at(5)), (2, at(9))]
        );
        assert_eq!(arrivals[1].line.channel, ChannelName::Guild);
        // A tab gives only its own lines, as their times.
        let tells = history.arrivals(ChatTab::Tell);
        assert_eq!(tells.len(), 1);
        assert_eq!(tells[0].at, at(5));
        assert_eq!(history.lines(ChatTab::Tell)[0].0, tells[0].id);
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
