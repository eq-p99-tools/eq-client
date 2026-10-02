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
/// then the words, ended with a carriage return and a line feed, as the
/// official client ends every line of its logs. The words go as they are,
/// non-ASCII text in the UTF-8 the server sent it in.
#[must_use]
pub fn line(time: chrono::NaiveDateTime, words: &str) -> String {
    format!("[{}] {words}\r\n", time.format("%a %b %d %H:%M:%S %Y"))
}

/// What a chat line says in a log: the installed client's string for it,
/// with what the string names, and the line in this client's words for an
/// installation without the string table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Words {
    /// The installed client's string for a player's speech; None for a line
    /// logged as it reads.
    pub string_id: Option<u32>,
    /// What the string names, in its placeholders' order.
    pub arguments: Vec<String>,
    /// The line in this client's words, or as it reads.
    pub text: String,
}

/// The installed client's string for the player's own speech in a channel,
/// which names the text, and the channel in this client's words.
const fn own_speech(channel: ChannelName) -> Option<(u32, &'static str)> {
    Some(match channel {
        ChannelName::Say => (12344, "say"),
        ChannelName::Group => (12346, "group"),
        ChannelName::Guild => (12263, "guild"),
        ChannelName::Ooc => (12352, "ooc"),
        ChannelName::Shout => (12348, "shout"),
        ChannelName::Auction => (12350, "auction"),
        ChannelName::Raid => (5109, "raid"),
        _ => return None,
    })
}

/// The installed client's string for another player's speech in a channel,
/// which names the speaker, then the tongue where it has a place for one
/// (true; empty for the common tongue), then the text; and the channel in
/// this client's words.
const fn others_speech(channel: ChannelName) -> Option<(u32, bool, &'static str)> {
    Some(match channel {
        ChannelName::Say => (1410, true, "say"),
        ChannelName::Group => (1414, true, "group"),
        ChannelName::Guild => (1415, true, "guild"),
        ChannelName::Tell => (1416, true, "tell"),
        ChannelName::Shout => (1420, true, "shout"),
        ChannelName::Auction => (1421, true, "auction"),
        ChannelName::Ooc => (1422, false, "ooc"),
        ChannelName::Raid => (5112, true, "raid"),
        _ => return None,
    })
}

/// What a chat line says in the official client's words: a player's speech
/// by the installed client's string for who spoke and where, in the common
/// tongue, and anything else as it reads. The server repeats the player's
/// own speech to them under their name, and echoes a tell they sent with
/// whom they told; a tell is always heard, even one sent to oneself.
#[must_use]
pub fn words(line: &ChatLine, player: &str) -> Words {
    let text = &line.message.text;
    let spoken = line
        .sender
        .as_deref()
        .filter(|sender| !sender.is_empty())
        .and_then(|sender| {
            if line.channel == ChannelName::TellEcho {
                let target = line.target.as_deref()?;
                return Some((
                    1400,
                    vec![target.to_owned(), text.clone()],
                    format!("You (tell {target})"),
                ));
            }
            if line.channel == ChannelName::Tell || !sender.eq_ignore_ascii_case(player) {
                return others_speech(line.channel).map(|(id, tongue, place)| {
                    let arguments = if tongue {
                        vec![sender.to_owned(), String::new(), text.clone()]
                    } else {
                        vec![sender.to_owned(), text.clone()]
                    };
                    (id, arguments, format!("{sender} ({place})"))
                });
            }
            own_speech(line.channel)
                .map(|(id, place)| (id, vec![text.clone()], format!("You ({place})")))
        });
    match spoken {
        Some((string_id, arguments, who)) => Words {
            string_id: Some(string_id),
            arguments,
            text: format!("{who}: {text}"),
        },
        None => Words {
            string_id: None,
            arguments: Vec::new(),
            text: text.clone(),
        },
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
            line(time, "Arrived in The Qeynos Hills."),
            "[Fri Oct 02 02:41:05 2026] Arrived in The Qeynos Hills.\r\n"
        );
        assert_eq!(
            line(time, "\u{c9}p\u{e9}e \u{2026}").as_bytes(),
            b"[Fri Oct 02 02:41:05 2026] \xC3\x89p\xC3\xA9e \xE2\x80\xA6\r\n"
        );
        assert_eq!(
            file_name("Examplar", "ExampleWorld"),
            "eqlog_Examplar_ExampleWorld.txt"
        );
    }

    fn heard(string_id: u32, arguments: &[&str], text: &str) -> Words {
        Words {
            string_id: Some(string_id),
            arguments: arguments
                .iter()
                .map(|&argument| argument.to_owned())
                .collect(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn speech_names_who_spoke_and_where() {
        let say = spoken(ChannelName::Say, Some("Examplar"), "Hail");
        assert_eq!(
            words(&say, "Other"),
            heard(1410, &["Examplar", "", "Hail"], "Examplar (say): Hail")
        );
        let tell = spoken(ChannelName::Tell, Some("Examplar"), "inc");
        assert_eq!(
            words(&tell, "Other"),
            heard(1416, &["Examplar", "", "inc"], "Examplar (tell): inc")
        );
        // Out-of-character speech has no place for a tongue.
        let ooc = spoken(ChannelName::Ooc, Some("Examplar"), "lfg");
        assert_eq!(
            words(&ooc, "Other"),
            heard(1422, &["Examplar", "lfg"], "Examplar (ooc): lfg")
        );
        let auction = spoken(ChannelName::Auction, Some("Examplar"), "WTS Rusty Dagger");
        assert_eq!(
            words(&auction, "Other"),
            heard(
                1421,
                &["Examplar", "", "WTS Rusty Dagger"],
                "Examplar (auction): WTS Rusty Dagger"
            )
        );
        let raid = spoken(ChannelName::Raid, Some("Examplar"), "go");
        assert_eq!(
            words(&raid, "Other"),
            heard(5112, &["Examplar", "", "go"], "Examplar (raid): go")
        );
        // The player's own speech, which the server repeats to them.
        assert_eq!(
            words(&say, "examplar"),
            heard(12344, &["Hail"], "You (say): Hail")
        );
        assert_eq!(
            words(&auction, "Examplar"),
            heard(
                12350,
                &["WTS Rusty Dagger"],
                "You (auction): WTS Rusty Dagger"
            )
        );
        // The echo of a tell the player sent names whom they told.
        let told = ChatLine {
            target: Some("Friend".into()),
            ..spoken(ChannelName::TellEcho, Some("Examplar"), "inc")
        };
        assert_eq!(
            words(&told, "Examplar"),
            heard(1400, &["Friend", "inc"], "You (tell Friend): inc")
        );
        // A tell the player sent themselves is heard as any tell is.
        let to_self = ChatLine {
            target: Some("Examplar".into()),
            ..spoken(ChannelName::Tell, Some("Examplar"), "inc")
        };
        assert_eq!(
            words(&to_self, "Examplar"),
            heard(1416, &["Examplar", "", "inc"], "Examplar (tell): inc")
        );
        // The game's own messages read as they are.
        let system = spoken(ChannelName::System, None, "A rat squeaks from the shadows.");
        assert_eq!(
            words(&system, "Other"),
            Words {
                string_id: None,
                arguments: Vec::new(),
                text: "A rat squeaks from the shadows.".into(),
            }
        );
    }
}
