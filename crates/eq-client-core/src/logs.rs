//! Logs in the official client's format, which parsers and timers such as
//! GINA, `GamParse` and `EQLogParser` read: one file per character and server
//! in the installation's `Logs` folder, a line per message in the chat's
//! words, stamped with the local time.
use crate::chat::{ChannelName, ChatLine, Source};

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
pub fn line(time: jiff::civil::DateTime, words: &str) -> String {
    format!("[{}] {words}\r\n", time.strftime("%a %b %d %H:%M:%S %Y"))
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

/// The installed client's line for the guild's message of the day.
const GUILD_MOTD: u32 = 0x1d4;

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
/// by the installed client's string for who spoke and where, with the tongue
/// it came in where the string has a place for one, and anything else as it
/// reads. A tongue other than the common one fills that place as `, in`
/// and its name, which `tongue_name` gives by its number; that form is this
/// client's, pending program read R49-4c (recording checklist 6.54, 6.495). The server repeats the player's
/// own speech to them under their name, and echoes a tell they sent with
/// whom they told; a tell is always heard, even one sent to oneself.
/// A line in this client's own words has none: the official client never
/// writes it, so its log leaves it out.
#[must_use]
pub fn words(line: &ChatLine, player: &str, tongue_name: &dyn Fn(u8) -> String) -> Option<Words> {
    if line.source == Source::Client {
        return None;
    }
    let text = &line.message.text;
    let spoken = line
        .sender
        .as_deref()
        .filter(|sender| !sender.is_empty())
        .and_then(|sender| {
            // The guild's message of the day names its author, then its text
            // (program read R11, 0x40a7fd: line 0x1d4), asked for or not.
            if matches!(
                line.channel,
                ChannelName::GuildMotd | ChannelName::GuildMotdReply
            ) {
                return Some((
                    GUILD_MOTD,
                    vec![sender.to_owned(), text.clone()],
                    format!("Guild MOTD ({sender})"),
                ));
            }
            if line.channel == ChannelName::TellEcho {
                let target = line.target.as_deref()?;
                return Some((
                    1400,
                    vec![target.to_owned(), text.clone()],
                    format!("You (tell {target})"),
                ));
            }
            if line.channel == ChannelName::Tell || !sender.eq_ignore_ascii_case(player) {
                let spoken = line
                    .tongue
                    .map(|heard| heard.language)
                    .filter(|&language| language != crate::tongues::COMMON)
                    .map(tongue_name);
                return others_speech(line.channel).map(|(id, tongue, place)| {
                    let arguments = if tongue {
                        let place = spoken
                            .as_deref()
                            .map_or_else(String::new, |name| format!(", in {name}"));
                        vec![sender.to_owned(), place, text.clone()]
                    } else {
                        vec![sender.to_owned(), text.clone()]
                    };
                    let who = match (&spoken, tongue) {
                        (Some(name), true) => format!("{sender} ({place}, {name})"),
                        _ => format!("{sender} ({place})"),
                    };
                    (id, arguments, who)
                });
            }
            own_speech(line.channel)
                .map(|(id, place)| (id, vec![text.clone()], format!("You ({place})")))
        });
    Some(match spoken {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(language: u8) -> String {
        format!("Tongue {language}")
    }
    use crate::chat::Message;

    fn spoken(channel: ChannelName, sender: Option<&str>, text: &str) -> ChatLine {
        ChatLine {
            channel,
            message_type: None,
            sender: sender.map(str::to_owned),
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: text.to_owned(),
                item_links: vec![],
            },
            source: Source::Server,
            tongue: None,
        }
    }

    #[test]
    fn lines_read_as_the_official_clients_do() {
        let time = jiff::civil::date(2026, 10, 2).at(2, 41, 5, 0);
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

    #[test]
    fn log_dates_keep_english_names_padding_and_whole_seconds() {
        for (time, expected) in [
            (
                jiff::civil::date(1970, 1, 1).at(0, 0, 0, 0),
                "[Thu Jan 01 00:00:00 1970] words\r\n",
            ),
            (
                jiff::civil::date(1969, 12, 31).at(23, 59, 59, 999_999_999),
                "[Wed Dec 31 23:59:59 1969] words\r\n",
            ),
            (
                jiff::civil::date(2024, 2, 29).at(2, 4, 5, 987_654_321),
                "[Thu Feb 29 02:04:05 2024] words\r\n",
            ),
        ] {
            assert_eq!(line(time, "words"), expected);
        }
    }

    #[test]
    fn local_dates_keep_timezone_offsets_and_daylight_saving() {
        for (instant, log, earth) in [
            (
                "2024-07-04T02:04:05.987Z",
                "[Wed Jul 03 22:04:05 2024] words\r\n",
                "Wednesday, July 03, 2024 22:04:05",
            ),
            (
                "2024-01-01T02:04:05.987Z",
                "[Sun Dec 31 21:04:05 2023] words\r\n",
                "Sunday, December 31, 2023 21:04:05",
            ),
        ] {
            let local = instant
                .parse::<jiff::Timestamp>()
                .unwrap()
                .in_tz("America/New_York")
                .unwrap()
                .datetime();
            assert_eq!(line(local, "words"), log);
            assert_eq!(crate::whereabouts::earth(local), earth);
        }
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
    fn speech_in_another_tongue_names_it_in_the_tongues_place() {
        let heard = |language| {
            let mut line = spoken(ChannelName::Say, Some("Examplar"), "mae govannen");
            line.tongue = Some(crate::tongues::Heard {
                language,
                skill: 50,
            });
            line
        };
        let words = words(&heard(3), "Other", &named).unwrap();
        assert_eq!(words.string_id, Some(1410));
        assert_eq!(
            words.arguments,
            ["Examplar", ", in Tongue 3", "mae govannen"]
        );
        assert_eq!(words.text, "Examplar (say, Tongue 3): mae govannen");
        // The common tongue leaves the place empty, as before.
        let common = super::words(&heard(0), "Other", &named).unwrap();
        assert_eq!(common.arguments, ["Examplar", "", "mae govannen"]);
        // Out-of-character speech has no place for one.
        let mut ooc = heard(3);
        ooc.channel = ChannelName::Ooc;
        let ooc = super::words(&ooc, "Other", &named).unwrap();
        assert_eq!(ooc.arguments, ["Examplar", "mae govannen"]);
    }

    #[test]
    fn speech_names_who_spoke_and_where() {
        let say = spoken(ChannelName::Say, Some("Examplar"), "Hail");
        assert_eq!(
            words(&say, "Other", &named).unwrap(),
            heard(1410, &["Examplar", "", "Hail"], "Examplar (say): Hail")
        );
        let tell = spoken(ChannelName::Tell, Some("Examplar"), "inc");
        assert_eq!(
            words(&tell, "Other", &named).unwrap(),
            heard(1416, &["Examplar", "", "inc"], "Examplar (tell): inc")
        );
        // Out-of-character speech has no place for a tongue.
        let ooc = spoken(ChannelName::Ooc, Some("Examplar"), "lfg");
        assert_eq!(
            words(&ooc, "Other", &named).unwrap(),
            heard(1422, &["Examplar", "lfg"], "Examplar (ooc): lfg")
        );
        let auction = spoken(ChannelName::Auction, Some("Examplar"), "WTS Rusty Dagger");
        assert_eq!(
            words(&auction, "Other", &named).unwrap(),
            heard(
                1421,
                &["Examplar", "", "WTS Rusty Dagger"],
                "Examplar (auction): WTS Rusty Dagger"
            )
        );
        let raid = spoken(ChannelName::Raid, Some("Examplar"), "go");
        assert_eq!(
            words(&raid, "Other", &named).unwrap(),
            heard(5112, &["Examplar", "", "go"], "Examplar (raid): go")
        );
        // The player's own speech, which the server repeats to them.
        assert_eq!(
            words(&say, "examplar", &named).unwrap(),
            heard(12344, &["Hail"], "You (say): Hail")
        );
        assert_eq!(
            words(&auction, "Examplar", &named).unwrap(),
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
            words(&told, "Examplar", &named).unwrap(),
            heard(1400, &["Friend", "inc"], "You (tell Friend): inc")
        );
        // The guild's message of the day names its author.
        assert_eq!(
            words(
                &spoken(ChannelName::GuildMotd, Some("Leader"), "Raid at 8"),
                "Examplar",
                &named
            )
            .unwrap(),
            heard(
                0x1d4,
                &["Leader", "Raid at 8"],
                "Guild MOTD (Leader): Raid at 8"
            )
        );
        // A tell the player sent themselves is heard as any tell is.
        let to_self = ChatLine {
            target: Some("Examplar".into()),
            ..spoken(ChannelName::Tell, Some("Examplar"), "inc")
        };
        assert_eq!(
            words(&to_self, "Examplar", &named).unwrap(),
            heard(1416, &["Examplar", "", "inc"], "Examplar (tell): inc")
        );
        // The game's own messages read as they are.
        let system = spoken(ChannelName::System, None, "A rat squeaks from the shadows.");
        assert_eq!(
            words(&system, "Other", &named).unwrap(),
            Words {
                string_id: None,
                arguments: Vec::new(),
                text: "A rat squeaks from the shadows.".into(),
            }
        );
        // So do the official client's own lines, but never this client's.
        let official = ChatLine {
            source: Source::Official,
            ..system.clone()
        };
        assert_eq!(
            words(&official, "Other", &named).map(|words| words.text),
            Some(system.message.text.clone())
        );
        let own = ChatLine {
            source: Source::Client,
            ..system
        };
        assert_eq!(words(&own, "Other", &named), None);
    }

    #[test]
    fn the_asked_for_guild_motd_is_heard_as_the_guild_motd() {
        assert_eq!(
            words(
                &spoken(ChannelName::GuildMotdReply, Some("Leader"), "Raid at 8"),
                "Examplar",
                &named
            )
            .unwrap(),
            heard(
                0x1d4,
                &["Leader", "Raid at 8"],
                "Guild MOTD (Leader): Raid at 8"
            )
        );
    }
}
