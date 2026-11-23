//! The Titanium program's chat server channel commands (recording checklist
//! 6.509): `/join`, `/leave`, `/leaveall`, `/list`, `/announce`, `/1` to
//! `/10` and `/chat`. Each goes to the chat server as the words it takes,
//! on a server whose session offers chat channels; the server judges them.
//! `/chat` takes only the channel words ([`CHANNEL_WORDS`]): the chat
//! server's same handler also sends mail and keeps friends and ignore
//! lists, which this client does not reach. What the program itself checks
//! or says before it sends is pending read R49-12 e, so every refusal here
//! is this client's own wording, and `/autojoin`, a list the program keeps,
//! is not built.
use super::chat::{ChatState, Said};
use crate::outbox::{self, Outbox};
use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand};

/// The most chat server channels a character is on, and so the highest
/// `/N`.
const SLOTS: u8 = 10;

/// The chat server's channel words `/chat` passes on, compared whole and
/// regardless of case as `EQEmu`'s chat server compares them
/// (`ucs/clientlist.h`): joining, leaving and listing, and the owner's and
/// moderator's words, passed unchecked. Left out are its mail words
/// (`mailto`, `getheaders`, `getbody`, `setmessagestatus`, `selectmailbox`,
/// `setmailforwarding`), its friends and ignore lists (`buddy`,
/// `ignoreplayer`), `set`, which saves the channels joined at login, and
/// `afk` and `uptime`.
pub(crate) const CHANNEL_WORDS: [&str; 15] = [
    "join",
    "leave",
    "leaveall",
    "list",
    "listall",
    "announce",
    "setowner",
    "oplist",
    "invite",
    "grant",
    "moderate",
    "voice",
    "kick",
    "password",
    "toggleinvites",
];

/// What a typed line, spelled out, asks of the chat server: its words, or
/// this client's refusal. None for a line that is none of these commands.
pub(crate) fn typed(input: &str) -> Option<Result<String, String>> {
    let command = input.strip_prefix('/')?;
    let (name, rest) = command.split_once(' ').unwrap_or((command, ""));
    let rest = rest.trim();
    let name = name.to_ascii_lowercase();
    let needs = |words: &str, usage: &str| {
        if rest.is_empty() {
            Err(usage.to_owned())
        } else {
            Ok(format!("{words} {rest}"))
        }
    };
    let maybe = |words: &str| {
        Ok(if rest.is_empty() {
            words.to_owned()
        } else {
            format!("{words} {rest}")
        })
    };
    Some(match name.as_str() {
        "join" => needs("join", "Use /join channel, or channel:password"),
        "leave" => needs("leave", "Use /leave channel"),
        "leaveall" => Ok("leaveall".to_owned()),
        "list" => maybe("list"),
        "announce" => maybe("announce"),
        "chat" if rest.is_empty() => Err("Use /chat followed by the chat server's command".into()),
        "chat" => {
            let word = rest.split_whitespace().next().unwrap_or_default();
            if CHANNEL_WORDS
                .iter()
                .any(|channel| channel.eq_ignore_ascii_case(word))
            {
                Ok(rest.to_owned())
            } else {
                Err(format!("/chat does not send {word} to the chat server."))
            }
        }
        "autojoin" => Err("This client does not keep /autojoin channels yet.".into()),
        slot => {
            let slot = slot
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=SLOTS).contains(n))?;
            needs(
                &slot.to_string(),
                "Type a message after the channel command",
            )
        }
    })
}

/// Sends the chat channel commands the chat noted, or says why not.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn answers(
    mut chat: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    outbox: Res<Outbox>,
) {
    if chat.channel_asked.is_empty() {
        return;
    }
    let world = online.world();
    for text in std::mem::take(&mut chat.channel_asked) {
        if !outbox::offered(world, Capability::ChatChannels) {
            // This client's words; the program's own line is unread
            // (R49-12 d).
            chat.refuse(Said::own("You are not connected to the chat server."));
            continue;
        }
        let _ = outbox.post(world, |stamp| ClientCommand::ChatChannel {
            session_id: stamp.session_id,
            text,
            created: stamp.created,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::WorldEvent;

    fn admitted(capabilities: Vec<Capability>) -> OnlineState {
        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities,
                choices: Vec::new(),
                session_id: 5,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        testing::connect(&mut online, true);
        online
    }

    #[test]
    fn each_command_goes_as_the_chat_servers_words() {
        for (typed_line, words) in [
            ("/join general, trade:secret", "join general, trade:secret"),
            ("/leave general", "leave general"),
            ("/leaveall", "leaveall"),
            ("/list", "list"),
            ("/list general", "list general"),
            ("/announce", "announce"),
            ("/announce on", "announce on"),
            ("/1 hello there", "1 hello there"),
            ("/10 last slot", "10 last slot"),
            ("/chat password general secret", "password general secret"),
            ("/JOIN Loud", "join Loud"),
        ] {
            assert_eq!(
                typed(typed_line),
                Some(Ok(words.to_owned())),
                "{typed_line}"
            );
        }
    }

    #[test]
    fn what_needs_words_asks_for_them_and_the_rest_is_not_these_commands() {
        for line in ["/join", "/leave ", "/3", "/chat"] {
            assert!(matches!(typed(line), Some(Err(_))), "{line}");
        }
        // Only the channel words go: mail, friends, ignore and saved
        // channels never reach the chat server from here.
        for line in [
            "/chat mailto Someone \"subject\" body",
            "/chat MAILTO Someone",
            "/chat setmessagestatus 1 2",
            "/chat getbody 1",
            "/chat selectmailbox 0",
            "/chat buddy add Someone",
            "/chat ignoreplayer Someone",
            "/chat set general",
            "/chat 1 hello",
            "/chat #general hello",
        ] {
            let word = line.split_whitespace().nth(1).unwrap();
            assert_eq!(
                typed(line),
                Some(Err(format!(
                    "/chat does not send {word} to the chat server."
                ))),
                "{line}"
            );
        }
        assert_eq!(
            typed("/chat KICK general Someone"),
            Some(Ok("KICK general Someone".into()))
        );
        assert_eq!(
            typed("/autojoin general"),
            Some(Err(
                "This client does not keep /autojoin channels yet.".into()
            ))
        );
        for line in ["/11 hi", "/0 hi", "/say hi", "hello", "/joinx a"] {
            assert_eq!(typed(line), None, "{line}");
        }
        // A typed abbreviation reaches here spelled out, and /1 is the
        // table's own name, not a start of /10.
        assert_eq!(
            crate::commands::spelled("/leavea").as_deref(),
            Some("/leaveall")
        );
        assert_eq!(crate::commands::spelled("/1 hi").as_deref(), Some("/1 hi"));
    }

    #[test]
    fn channel_lines_read_with_their_slot_and_keep_the_servers_links() {
        use eq_client_core::world::{ChannelLine, Notice};
        let said = |line: ChannelLine| {
            let mut lines = crate::notices::wording(&Notice::Channel(line), None);
            assert_eq!(lines.len(), 1);
            lines.remove(0).1
        };
        let text = eq_client_core::chat::RichText::from("selling a sword");
        let message = |slot| ChannelLine::Message {
            channel: "trade".into(),
            slot,
            sender: "Other".into(),
            text: text.clone(),
        };
        let line = said(message(Some(2)));
        assert_eq!(line.words.text, "Other tells trade:2, 'selling a sword'");
        assert_eq!(line.source, eq_client_core::chat::Source::Server);
        assert_eq!(
            said(message(None)).words.text,
            "Other tells trade, 'selling a sword'"
        );
        assert_eq!(
            said(ChannelLine::Member {
                channel: "trade".into(),
                name: "Other".into(),
                joined: true,
            })
            .words
            .text,
            "Other has entered channel trade."
        );
        let notice = said(ChannelLine::Notice("Channels: general(3)".into()));
        assert_eq!(notice.words.text, "Channels: general(3)");
        assert_eq!(notice.source, eq_client_core::chat::Source::Server);
        assert_eq!(
            said(ChannelLine::Lost).source,
            eq_client_core::chat::Source::Client
        );
    }

    #[test]
    fn a_command_goes_only_where_the_session_offers_channels() {
        let mut app = App::new();
        let (queue, received) = std::sync::mpsc::sync_channel(4);
        app.insert_resource(admitted(vec![Capability::ChatChannels]))
            .insert_resource(Outbox::new(Some(queue)))
            .init_resource::<ChatState>()
            .add_systems(Update, answers);
        app.world_mut()
            .resource_mut::<ChatState>()
            .channel_asked
            .push("join general".into());
        app.update();
        match received.try_recv().unwrap() {
            ClientCommand::ChatChannel {
                session_id, text, ..
            } => {
                assert_eq!(session_id, 5);
                assert_eq!(text, "join general");
            }
            other => panic!("{other:?}"),
        }
        assert!(received.try_recv().is_err());

        // A zone whose world opened no link: refused, nothing sent.
        app.insert_resource(admitted(Vec::new()));
        app.world_mut()
            .resource_mut::<ChatState>()
            .channel_asked
            .push("1 hello".into());
        app.update();
        assert!(received.try_recv().is_err());
        assert_eq!(
            app.world().resource::<ChatState>().newest(),
            "You are not connected to the chat server."
        );
    }
}
