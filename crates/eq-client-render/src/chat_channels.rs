//! The Titanium program's chat server channel commands (recording checklist
//! 6.509, read R49-12, 6.616): `/join`, `/leave`, `/leaveall`, `/list`,
//! `/announce`, `/1` to `/10`, `/chat` and `/autojoin`, on a server whose
//! session offers chat channels; the server judges them.
//!
//! - `/join`, `/leave`, `/leaveall` and `/list` go as typed, slash and all,
//!   as 0x491438 sends them (R49-12 e); `/leave` alone goes as `/leave`.
//! - `/N message` goes as `;N message` (0x46cb65). The program first runs
//!   its stage-1 garble at skill 100, which leaves only its bad-word mask;
//!   this client has no such mask for any chat (see [`eq_client_core::tongues`]),
//!   so the words go as typed.
//! - The program checks nothing else before it sends: the chat server
//!   enforces channel counts, names and lengths.
//! - `/join` with no channel and `/N` with no message send nothing and say
//!   how to use them. The program's usage lines are plain text in it, not
//!   its string table, so these are this client's own words, in colour
//!   0x111 as the program prints its own.
//! - `/autojoin list` keeps the channels to join at each login, in this
//!   client's own per-character options (reviewer H2), and sends nothing
//!   now. At each login to the chat server the list is this client's,
//!   else the character's `[ChatManager]` `ChannelAutoJoin`, else
//!   `eqclient.ini`'s `[Defaults]` one, else `NONE`; any list but `NONE`
//!   is said (colour 0x122) and sent as `/join list` (R49-12 c).
//! - `/announce` goes as typed, `/announce` and its words: its handler
//!   (0x46cb10) is unread, and this is the form the server takes.
//! - `/chat` takes only the channel words ([`CHANNEL_WORDS`]): the chat
//!   server's same handler also sends mail and keeps friends and ignore
//!   lists, which this client does not reach. What the program sends for
//!   `/chat` is read only in part (R49-12 e: its text through the same
//!   sender, [I] with a `;` case at the start), so the words go after a `/`,
//!   the form the server takes: it skips a command's first byte.
//! - With no chat server link the program drops every command silently
//!   (R49-12 d). This client says "You are not connected to the chat
//!   server." instead: an always-on quality-of-life departure that only adds
//!   what the player would otherwise have to guess (reviewer Q3).
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

/// What a chat channel command asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// These words go to the chat server.
    Send(String),
    /// `/autojoin`: keep this list for the logins to come.
    AutoJoin(String),
}

/// The colour the program prints its own channel lines in, the usage and
/// `/autojoin`'s confirmation (R49-12 c, e).
pub(crate) const PLAIN_COLOUR: u32 = 0x111;
/// The colour of the program's autojoining line at login (R49-12 c).
const AUTOJOINING_COLOUR: u32 = 0x122;

/// What a typed line, spelled out, asks for: its words, or this client's
/// refusal. None for a line that is none of these commands.
pub(crate) fn typed(input: &str) -> Option<Result<Asked, String>> {
    let command = input.strip_prefix('/')?;
    let (name, rest) = command.split_once(' ').unwrap_or((command, ""));
    let rest = rest.trim();
    let name = name.to_ascii_lowercase();
    let needs = |words: String, usage: &str| {
        if rest.is_empty() {
            Err(usage.to_owned())
        } else {
            Ok(Asked::Send(format!("{words} {rest}")))
        }
    };
    let maybe = |words: &str| {
        Ok(Asked::Send(if rest.is_empty() {
            words.to_owned()
        } else {
            format!("{words} {rest}")
        }))
    };
    Some(match name.as_str() {
        "join" => needs(
            "/join".into(),
            "Use /join channel, /join channel:password, or several of them with commas between.",
        ),
        "leave" => maybe("/leave"),
        "leaveall" => Ok(Asked::Send("/leaveall".to_owned())),
        "list" => maybe("/list"),
        // Its handler (0x46cb10) is unread: the form the server takes.
        "announce" => maybe("/announce"),
        "chat" if rest.is_empty() => Err("Use /chat followed by the chat server's command".into()),
        "chat" => {
            let word = rest.split_whitespace().next().unwrap_or_default();
            if CHANNEL_WORDS
                .iter()
                .any(|channel| channel.eq_ignore_ascii_case(word))
            {
                Ok(Asked::Send(format!("/{rest}")))
            } else {
                Err(format!("/chat does not send {word} to the chat server."))
            }
        }
        "autojoin" if rest.is_empty() => Err(
            "Use /autojoin channel, /autojoin channel:password, or several of them with commas between, to join them each time you log in."
                .into(),
        ),
        "autojoin" => Ok(Asked::AutoJoin(rest.to_owned())),
        slot => {
            let slot = slot
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=SLOTS).contains(n))?;
            needs(format!(";{slot}"), "Type a message after the channel command.")
        }
    })
}

/// Sends the chat channel commands the chat noted, or says why not, and
/// keeps an `/autojoin` list.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn answers(
    mut chat: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    outbox: Res<Outbox>,
    mut options: Option<ResMut<crate::options::OptionsState>>,
) {
    if chat.channel_asked.is_empty() {
        return;
    }
    let world = online.world();
    for asked in std::mem::take(&mut chat.channel_asked) {
        let text = match asked {
            Asked::Send(text) => text,
            Asked::AutoJoin(list) => {
                // Kept for the logins to come; nothing is sent now (R49-12 c).
                match eq_client_core::options::AutoJoin::new(&list) {
                    Some(kept) => {
                        if let Some(options) = options.as_mut() {
                            options.options.channel_autojoin = Some(kept);
                        }
                        let line = if kept.as_str().trim().eq_ignore_ascii_case("NONE") {
                            "You will join no channels when you log in.".to_owned()
                        } else {
                            format!(
                                "You will join {} each time you log in.",
                                shown(kept.as_str())
                            )
                        };
                        chat.history.push(super::chat::system_line(
                            Said::own(line).typed(PLAIN_COLOUR),
                        ));
                    }
                    None => chat.refuse(Said::own("That autojoin list is too long to keep.")),
                }
                continue;
            }
        };
        if !outbox::offered(world, Capability::ChatChannels) || !world.chat_link_up() {
            // With no link up, the program drops it silently (R49-12 d);
            // saying so is this client's always-on departure (reviewer Q3).
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

/// An autojoin list as this client's lines show it: each `channel:password`
/// entry keeps its channel, and its password is never printed. Whether the
/// program's own lines show it is unread; ours keep it off the screen, the
/// log and scripts' chat.
fn shown(list: &str) -> String {
    list.split(',')
        .map(|entry| {
            entry
                .split_once(':')
                .map_or(entry, |(channel, _)| channel)
                .trim()
        })
        .filter(|channel| !channel.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The channels to join at a login to the chat server (R49-12 c): this
/// client's own list, else the character's in the installation, else the
/// installation's default, as given; none when the list found is `NONE`
/// (the program's default, compared regardless of case, inferred) or empty.
fn autojoin_list(
    ours: Option<&str>,
    character: Option<String>,
    defaults: Option<String>,
) -> Option<String> {
    let list = ours
        .map(str::to_owned)
        .or(character)
        .or(defaults)?
        .trim()
        .to_owned();
    (!list.is_empty() && !list.eq_ignore_ascii_case("NONE")).then_some(list)
}

/// Joins the autojoin channels once each time a chat server link takes the
/// login: the program's "autojoining" line, then `/join list` (R49-12 c).
/// Titanium only: only its installation's files are read, and only its
/// session opens a link.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn autojoin(
    mut chat: ResMut<ChatState>,
    online: Res<super::online::OnlineState>,
    outbox: Res<Outbox>,
    (settings, options): (
        Option<Res<crate::ViewerSettings>>,
        Option<Res<crate::options::OptionsState>>,
    ),
    mut seen: Local<u64>,
) {
    let world = online.world();
    let ups = world.chat_links_up();
    if ups == *seen {
        return;
    }
    // The link can take the login while the character is still entering
    // the world; the autojoin waits for the admission, whose character it
    // reads.
    if world.session_id().is_none() || !world.can(Capability::ChatChannels) {
        return;
    }
    *seen = ups;
    if ups == 0 || !world.chat_link_up() {
        return;
    }
    let playing = crate::profile_files::playing(world);
    let official = settings
        .as_ref()
        .and_then(|settings| settings.0.official_settings());
    let character = official.as_ref().and_then(|official| {
        let playing = playing.as_ref()?;
        official.channel_autojoin(&playing.character, &playing.world)
    });
    let defaults = official.and_then(|official| official.options().channel_autojoin);
    let ours = options
        .as_ref()
        .and_then(|options| options.of(playing.as_ref()).channel_autojoin);
    let Some(list) = autojoin_list(
        ours.as_ref().map(eq_client_core::options::AutoJoin::as_str),
        character,
        defaults,
    ) else {
        return;
    };
    chat.history.push(super::chat::system_line(
        // The program prints a plain line here whose words aren't read; these
        // are this client's, in its colour (R49-12 c).
        Said::own(format!("Autojoining channels: {}", shown(&list))).typed(AUTOJOINING_COLOUR),
    ));
    let _ = outbox.post(world, |stamp| ClientCommand::ChatChannel {
        session_id: stamp.session_id,
        text: format!("/join {list}"),
        created: stamp.created,
    });
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
    fn each_command_goes_as_the_program_sends_it() {
        // Slash and all for the channel commands, `;N` for a channel's
        // message (read R49-12 e).
        for (typed_line, words) in [
            ("/join general, trade:secret", "/join general, trade:secret"),
            ("/leave general", "/leave general"),
            ("/leave", "/leave"),
            ("/leave ", "/leave"),
            ("/leaveall", "/leaveall"),
            ("/list", "/list"),
            ("/list general", "/list general"),
            ("/announce", "/announce"),
            ("/announce on", "/announce on"),
            ("/1 hello there", ";1 hello there"),
            ("/10 last slot", ";10 last slot"),
            ("/chat password general secret", "/password general secret"),
            ("/chat KICK general Someone", "/KICK general Someone"),
            ("/JOIN Loud", "/join Loud"),
        ] {
            assert_eq!(
                typed(typed_line),
                Some(Ok(Asked::Send(words.to_owned()))),
                "{typed_line}"
            );
            // The server skips a command's first byte, so each starts with
            // the `/` or `;` the network library requires.
            assert!(words.starts_with(['/', ';']), "{words}");
        }
        // `/autojoin` keeps its list and sends nothing.
        assert_eq!(
            typed("/autojoin general, trade:secret"),
            Some(Ok(Asked::AutoJoin("general, trade:secret".into())))
        );
    }

    #[test]
    fn what_needs_words_asks_for_them_and_the_rest_is_not_these_commands() {
        for line in ["/join", "/3", "/3 ", "/chat", "/autojoin"] {
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
            Some(Ok(Asked::Send("/KICK general Someone".into())))
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
        let mut online = admitted(vec![Capability::ChatChannels]);
        testing::news(
            &mut online,
            [WorldEvent::ChannelLink(
                eq_network_game::chat_server::ChannelLink::Up,
            )],
        );
        app.insert_resource(online)
            .insert_resource(Outbox::new(Some(queue)))
            .init_resource::<ChatState>()
            .add_systems(Update, answers);
        app.world_mut()
            .resource_mut::<ChatState>()
            .channel_asked
            .push(Asked::Send("/join general".into()));
        app.update();
        match received.try_recv().unwrap() {
            ClientCommand::ChatChannel {
                session_id, text, ..
            } => {
                assert_eq!(session_id, 5);
                assert_eq!(text, "/join general");
            }
            other => panic!("{other:?}"),
        }
        assert!(received.try_recv().is_err());

        // A zone whose world opened no link: refused, nothing sent.
        app.insert_resource(admitted(Vec::new()));
        app.world_mut()
            .resource_mut::<ChatState>()
            .channel_asked
            .push(Asked::Send(";1 hello".into()));
        app.update();
        assert!(received.try_recv().is_err());
        // The program says nothing here; this line is the labelled,
        // always-on departure (reviewer Q3).
        assert_eq!(
            app.world().resource::<ChatState>().newest(),
            "You are not connected to the chat server."
        );
    }

    #[test]
    fn the_autojoin_list_is_ours_then_the_characters_then_the_installations() {
        let some = |text: &str| Some(text.to_owned());
        assert_eq!(
            autojoin_list(Some("ours"), some("character"), some("default")),
            some("ours")
        );
        assert_eq!(
            autojoin_list(None, some(" character "), some("default")),
            some("character")
        );
        assert_eq!(autojoin_list(None, None, some("default")), some("default"));
        // `NONE`, the program's default, joins nothing, whoever set it; so
        // does nothing set at all.
        assert_eq!(autojoin_list(None, some("none"), some("default")), None);
        assert_eq!(autojoin_list(Some("NONE"), some("character"), None), None);
        assert_eq!(autojoin_list(None, None, None), None);
    }

    /// An app with the channel systems, a session with a link offered,
    /// and what it sends.
    fn channel_app() -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
        let mut app = App::new();
        let (queue, received) = std::sync::mpsc::sync_channel(4);
        app.insert_resource(admitted(vec![Capability::ChatChannels]))
            .insert_resource(Outbox::new(Some(queue)))
            .init_resource::<ChatState>()
            .init_resource::<crate::profile_files::Profile>()
            .insert_resource(crate::options::OptionsState::default())
            .add_systems(Update, (answers, autojoin).chain());
        (app, received)
    }

    #[test]
    fn autojoin_keeps_the_list_in_our_options_and_sends_nothing_now() {
        let (mut app, received) = channel_app();
        app.world_mut()
            .resource_mut::<ChatState>()
            .channel_asked
            .push(Asked::AutoJoin("general, trade:secret".into()));
        app.update();
        assert!(received.try_recv().is_err());
        assert_eq!(
            app.world()
                .resource::<crate::options::OptionsState>()
                .options
                .channel_autojoin
                .map(|list| list.as_str().to_owned())
                .as_deref(),
            Some("general, trade:secret")
        );
        assert_eq!(
            app.world().resource::<ChatState>().newest(),
            "You will join general, trade each time you log in."
        );
    }

    #[test]
    fn a_login_taken_while_entering_the_world_joins_once_the_character_is_in() {
        // The chat server can take the login before the admission lands:
        // the autojoin waits for it, then reads that character's options.
        let (queue, received) = std::sync::mpsc::sync_channel(4);
        let mut app = App::new();
        app.insert_resource(OnlineState::new(true))
            .insert_resource(Outbox::new(Some(queue)))
            .init_resource::<ChatState>()
            .insert_resource(crate::options::OptionsState::default())
            .add_systems(Update, (answers, autojoin).chain());
        let up = WorldEvent::ChannelLink(eq_network_game::chat_server::ChannelLink::Up);
        testing::news(&mut app.world_mut().resource_mut::<OnlineState>(), [up]);
        app.update();
        app.update();
        assert!(received.try_recv().is_err());
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Entered {
                capabilities: vec![Capability::ChatChannels],
                choices: Vec::new(),
                session_id: 5,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        testing::connect(&mut app.world_mut().resource_mut::<OnlineState>(), true);
        keeping(&mut app, "general");
        app.update();
        match received.try_recv().unwrap() {
            ClientCommand::ChatChannel { text, .. } => assert_eq!(text, "/join general"),
            other => panic!("{other:?}"),
        }
        app.update();
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn with_no_link_up_a_command_says_not_connected_and_sends_nothing() {
        let (mut app, received) = channel_app();
        let link = |link| WorldEvent::ChannelLink(link);
        for news in [
            // Before the link takes the login.
            None,
            // After it has ended.
            Some(eq_network_game::chat_server::ChannelLink::Lost {
                reason: eq_network_game::chat_server::LinkLoss::Dropped,
            }),
        ] {
            if let Some(news) = news {
                testing::news(
                    &mut app.world_mut().resource_mut::<OnlineState>(),
                    [
                        link(eq_network_game::chat_server::ChannelLink::Up),
                        link(news),
                    ],
                );
                app.update();
                while received.try_recv().is_ok() {}
            }
            app.world_mut()
                .resource_mut::<ChatState>()
                .channel_asked
                .push(Asked::Send(";1 x".into()));
            app.update();
            assert!(received.try_recv().is_err());
            assert_eq!(
                app.world().resource::<ChatState>().newest(),
                "You are not connected to the chat server."
            );
        }
    }

    #[test]
    fn an_autojoin_of_none_says_no_channels() {
        let (mut app, _received) = channel_app();
        app.world_mut()
            .resource_mut::<ChatState>()
            .channel_asked
            .push(Asked::AutoJoin("NONE".into()));
        app.update();
        assert_eq!(
            app.world().resource::<ChatState>().newest(),
            "You will join no channels when you log in."
        );
    }

    /// The character's own options keep `list`, as after their load.
    fn keeping(app: &mut App, list: &str) {
        let playing = crate::profile_files::playing(app.world().resource::<OnlineState>().world());
        let mut options = app
            .world_mut()
            .resource_mut::<crate::options::OptionsState>();
        options.following(playing);
        options.options.channel_autojoin = eq_client_core::options::AutoJoin::new(list);
    }

    #[test]
    fn each_login_to_the_chat_server_joins_the_autojoin_list_once() {
        let (mut app, received) = channel_app();
        keeping(&mut app, "general, trade:secret");
        app.update();
        // Nothing before the link takes the login.
        assert!(received.try_recv().is_err());
        let up = || WorldEvent::ChannelLink(eq_network_game::chat_server::ChannelLink::Up);
        testing::news(&mut app.world_mut().resource_mut::<OnlineState>(), [up()]);
        app.update();
        app.update();
        match received.try_recv().unwrap() {
            // The password goes to the chat server, as the program sends it.
            ClientCommand::ChatChannel { text, .. } => {
                assert_eq!(text, "/join general, trade:secret");
            }
            other => panic!("{other:?}"),
        }
        assert!(received.try_recv().is_err());
        assert_eq!(
            app.world().resource::<ChatState>().newest(),
            "Autojoining channels: general, trade"
        );
        // A later link, after a loss, joins again, once.
        testing::news(&mut app.world_mut().resource_mut::<OnlineState>(), [up()]);
        app.update();
        assert!(matches!(
            received.try_recv().unwrap(),
            ClientCommand::ChatChannel { .. }
        ));
        assert!(received.try_recv().is_err());
    }
}
