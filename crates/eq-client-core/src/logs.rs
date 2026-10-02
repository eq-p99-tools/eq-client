//! Logs in the official client's format, which parsers and timers such as
//! GINA, `GamParse` and `EQLogParser` read: one file per character and server
//! in the installation's `Logs` folder, a line per message in the chat's
//! words, stamped with the local time.
use crate::chat::{ChannelName, ChatLine};

/// What the official client says as `/log` turns logging on (eqstr 13221).
pub const LOGGING_ON: &str = "Logging to 'eqlog.txt' is now *ON*.";
/// What it says as `/log` turns logging off (eqstr 13222).
pub const LOGGING_OFF: &str = "Logging to 'eqlog.txt' is now *OFF*.";

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
/// with who spoke and where (eqstr 1410 to 1422 and 5112), anything else as
/// it reads.
#[must_use]
pub fn words(line: &ChatLine) -> String {
    let text = &line.message.text;
    let Some(sender) = line.sender.as_deref().filter(|sender| !sender.is_empty()) else {
        return text.clone();
    };
    let said = match line.channel {
        ChannelName::Say => "says,",
        ChannelName::Tell => "tells you,",
        ChannelName::Group => "tells the group,",
        ChannelName::Guild => "tells the guild,",
        ChannelName::Ooc => "says out of character,",
        ChannelName::Shout => "shouts,",
        ChannelName::Auction => "auctions,",
        ChannelName::Raid => "tells the raid,",
        _ => return text.clone(),
    };
    format!("{sender} {said} '{text}'")
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
        assert_eq!(words(&say), "Examplar says, 'Hail'");
        let tell = spoken(ChannelName::Tell, Some("Examplar"), "inc");
        assert_eq!(words(&tell), "Examplar tells you, 'inc'");
        let ooc = spoken(ChannelName::Ooc, Some("Examplar"), "lfg");
        assert_eq!(words(&ooc), "Examplar says out of character, 'lfg'");
        let auction = spoken(ChannelName::Auction, Some("Examplar"), "WTS Rusty Dagger");
        assert_eq!(words(&auction), "Examplar auctions, 'WTS Rusty Dagger'");
        // The game's own messages read as they are.
        let system = spoken(
            ChannelName::System,
            None,
            "You hit a rat for 5 points of damage.",
        );
        assert_eq!(words(&system), "You hit a rat for 5 points of damage.");
    }
}
