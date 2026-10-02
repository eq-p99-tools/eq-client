//! Logs in the official client's format, which parsers and timers such as
//! GINA, `GamParse` and `EQLogParser` read: one file per character and server
//! in the installation's `Logs` folder, a line per message in the chat's
//! words, stamped with the local time.
use crate::chat::{ChannelName, ChatLine};

/// The official client's line as `/log` turns logging on (`eqstr_us.txt`),
/// which a front end shows where the installation has it.
pub const LOGGING_ON: u32 = 13221;
/// Its line as `/log` turns logging off.
pub const LOGGING_OFF: u32 = 13222;
/// Logging turning on, in this client's words.
pub const LOGGING_ON_TEXT: &str = "Your chat log is on.";
/// Logging turning off, in this client's words.
pub const LOGGING_OFF_TEXT: &str = "Your chat log is off.";

/// The log file's name for a character on a server, by the server's short
/// name: `eqlog_<character>_<server>.txt`.
#[must_use]
pub fn file_name(character: &str, server: &str) -> String {
    format!("eqlog_{character}_{server}.txt")
}

/// A log line: the local time in brackets, as `[Thu Oct 02 02:41:05 2026]`,
/// then the words.
#[must_use]
pub fn line(time: chrono::NaiveDateTime, words: &str) -> String {
    format!("[{}] {words}", time.format("%a %b %d %H:%M:%S %Y"))
}

/// What a chat line says in the official client's words: a player's speech
/// with who spoke and where, as the installed client's string table words
/// it with the common tongue (1410, 1414 to 1416, 1420 to 1422 and 5112),
/// the player's own as the official client words it (1400 and 5109 are in
/// the table; the rest are its own words), and anything else as it reads.
/// The server repeats the player's own speech to them under their name.
#[must_use]
pub fn words(line: &ChatLine, player: &str) -> String {
    let text = &line.message.text;
    let Some(sender) = line.sender.as_deref().filter(|sender| !sender.is_empty()) else {
        return text.clone();
    };
    if sender.eq_ignore_ascii_case(player) {
        return match (line.channel, line.target.as_deref()) {
            (ChannelName::Say, _) => format!("You say, '{text}'"),
            (ChannelName::Tell, Some(target)) => format!("You told {target}, '{text}'"),
            (ChannelName::Group, _) => format!("You tell your party, '{text}'"),
            (ChannelName::Guild, _) => format!("You say to your guild, '{text}'"),
            (ChannelName::Ooc, _) => format!("You say out of character, '{text}'"),
            (ChannelName::Shout, _) => format!("You shout, '{text}'"),
            (ChannelName::Auction, _) => format!("You auction, '{text}'"),
            (ChannelName::Raid, _) => format!("You tell your raid, '{text}'"),
            _ => text.clone(),
        };
    }
    match line.channel {
        ChannelName::Say => format!("{sender} says, '{text}'"),
        ChannelName::Tell => format!("{sender} tells you, '{text}'"),
        ChannelName::Group => format!("{sender} tells the group, '{text}'"),
        ChannelName::Guild => format!("{sender} tells the guild, '{text}'"),
        ChannelName::Ooc => format!("{sender} says out of character, '{text}'"),
        ChannelName::Shout => format!("{sender} shouts, '{text}'"),
        ChannelName::Auction => format!("{sender} auctions, '{text}'"),
        // 5112 keeps a space before the tongue, which the common one leaves
        // empty.
        ChannelName::Raid => format!("{sender} tells the raid,  '{text}'"),
        _ => text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Message;

    fn spoken(channel: ChannelName, sender: Option<&str>, text: &str) -> ChatLine {
        ChatLine {
            channel,
            sender: sender.map(str::to_owned),
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: text.to_owned(),
                item_links: vec![],
            },
        }
    }

    #[test]
    fn lines_read_as_the_official_clients_do() {
        let time = chrono::NaiveDate::from_ymd_opt(2026, 10, 2)
            .unwrap()
            .and_hms_opt(2, 41, 5)
            .unwrap();
        assert_eq!(
            line(time, "You have entered The Qeynos Hills."),
            "[Fri Oct 02 02:41:05 2026] You have entered The Qeynos Hills."
        );
        assert_eq!(
            file_name("Examplar", "ExampleWorld"),
            "eqlog_Examplar_ExampleWorld.txt"
        );
    }

    #[test]
    fn speech_names_who_spoke_and_where() {
        let say = spoken(ChannelName::Say, Some("Examplar"), "Hail");
        assert_eq!(words(&say, "Other"), "Examplar says, 'Hail'");
        let tell = spoken(ChannelName::Tell, Some("Examplar"), "inc");
        assert_eq!(words(&tell, "Other"), "Examplar tells you, 'inc'");
        let ooc = spoken(ChannelName::Ooc, Some("Examplar"), "lfg");
        assert_eq!(
            words(&ooc, "Other"),
            "Examplar says out of character, 'lfg'"
        );
        let auction = spoken(ChannelName::Auction, Some("Examplar"), "WTS Rusty Dagger");
        assert_eq!(
            words(&auction, "Other"),
            "Examplar auctions, 'WTS Rusty Dagger'"
        );
        let raid = spoken(ChannelName::Raid, Some("Examplar"), "go");
        assert_eq!(words(&raid, "Other"), "Examplar tells the raid,  'go'");
        // The player's own speech, which the server repeats to them.
        assert_eq!(words(&say, "examplar"), "You say, 'Hail'");
        assert_eq!(
            words(&auction, "Examplar"),
            "You auction, 'WTS Rusty Dagger'"
        );
        // The game's own messages read as they are.
        let system = spoken(
            ChannelName::System,
            None,
            "You hit a rat for 5 points of damage.",
        );
        assert_eq!(
            words(&system, "Other"),
            "You hit a rat for 5 points of damage."
        );
    }
}
