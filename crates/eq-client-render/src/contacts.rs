//! The friends and ignore lists and `/reply` in the chat, as the Titanium
//! client has them (program read R6; the lists themselves are
//! [`eq_client_core::contacts`]).
//!
//! Slash commands match by the official parser's prefix rule (0x498b78): a
//! typed word runs the first command, in table order, whose name it begins.
//! So `/r` to `/reply` reply, `/rt` to `/rtarget` target the last teller,
//! `/fr` to `/friends` and `/ig` to `/ignore` change the lists, each down to
//! the shortest form the read lists. `/who all friends` asks the world
//! which friends are online.
//!
//! The lists are saved per character in this client's own file after every
//! change. A character with no file of their own starts from the lists the
//! official client saved for them in the installation (R9), or, where it
//! has no settings file for them either, from the 20-name defaults in its
//! `eqclient.ini`, as it starts a new character (R8). The installation's
//! files are only read.
use crate::chat::{ChatState, Said, system_line};
use crate::hud::messages::Messages;
use crate::profile_files::Store;
use bevy::prelude::*;
use eq_client_core::contacts::{
    Contacts, ENTRY_LIMIT, FRIEND_NAME_LIMIT, LIST_LIMIT, NameList, Toggled, strings,
};
use eq_client_core::world::ClientWorld;
use std::time::Instant;

/// A contacts command the chat runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// `/reply` with its words.
    Reply(String),
    /// `/rtarget`.
    ReplyTarget,
    /// `/friends`, naming someone or listing them all.
    Friends(Option<String>),
    /// `/ignore`, naming someone or listing them all.
    Ignore(Option<String>),
    /// `/who all friends`.
    WhoFriends,
}

/// Whether a typed command word runs the command `name` by the official
/// parser's prefix rule, given the shortest form that reaches it first.
pub(crate) fn abbreviates(typed: &str, name: &str, shortest: usize) -> bool {
    typed.len() >= shortest
        && name
            .get(..typed.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(typed))
}

impl Command {
    /// The contacts command a typed line is; None for any other line.
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let command = input.strip_prefix('/')?.trim();
        let (name, words) = command
            .split_once(char::is_whitespace)
            .unwrap_or((command, ""));
        let words = words.trim();
        // The lists take the rest of the line as the name (R8).
        let named = || (!words.is_empty()).then(|| words.to_owned());
        if abbreviates(name, "reply", 1) {
            Some(Self::Reply(words.to_owned()))
        } else if abbreviates(name, "rtarget", 2) {
            Some(Self::ReplyTarget)
        } else if abbreviates(name, "friends", 2) {
            Some(Self::Friends(named()))
        } else if abbreviates(name, "ignore", 2) {
            Some(Self::Ignore(named()))
        } else if name.eq_ignore_ascii_case("who") && is_all_friends(words) {
            Some(Self::WhoFriends)
        } else {
            None
        }
    }
}

/// Whether `/who`'s words ask for friends everywhere. How the official
/// client reads `friends` among other `/who` words is not read; this takes
/// exactly `all friends` (inferred, recording checklist 6.412).
fn is_all_friends(words: &str) -> bool {
    let mut words = words.split_whitespace();
    matches!(
        (words.next(), words.next(), words.next()),
        (Some(all), Some(friends), None)
            if all.eq_ignore_ascii_case("all") && friends.eq_ignore_ascii_case("friends")
    )
}

/// The lists the chat keeps, whether they changed since they were saved,
/// and what was asked of them.
#[derive(Default)]
pub(crate) struct Book {
    /// The friends, ignore and reply lists.
    pub(crate) lists: Contacts,
    /// Whether the friends or ignore list changed since it was saved.
    pub(crate) changed: bool,
    /// A `/friends`, `/ignore`, `/reply` or `/rtarget`, waiting to be
    /// answered in the official client's words.
    pub(crate) asked: Option<Asked>,
}

/// What the chat noted for the lists to answer, in the official client's
/// words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// `/reply` with no tell received.
    NoReply,
    /// `/rtarget` with no tell received.
    NoReplyTarget,
    /// `/friends`.
    Friends(Option<String>),
    /// `/ignore`.
    Ignore(Option<String>),
}

/// The lines that answer what the player asked, changing the lists as it
/// says; true in the second place when a list changed.
fn answer(
    asked: Asked,
    contacts: &mut Contacts,
    world: &ClientWorld,
    messages: &Messages,
) -> (Vec<Said>, bool) {
    match asked {
        Asked::NoReply => (
            vec![messages.said_text(strings::NO_REPLY, "You have no one to reply to.")],
            false,
        ),
        Asked::NoReplyTarget => (
            vec![messages.said_text(strings::NO_REPLY_TARGET, "You have no one to target.")],
            false,
        ),
        Asked::Friends(None) => (friends_listing(&contacts.friends, messages), false),
        Asked::Ignore(None) => (ignore_listing(&contacts.ignored, messages), false),
        Asked::Friends(Some(name)) => {
            let said = match contacts.friends.toggle(&name, FRIEND_NAME_LIMIT) {
                Toggled::Added(name) => messages.said_or(
                    strings::FRIEND_ADDED,
                    std::slice::from_ref(&name),
                    &format!("{name} is now your friend."),
                ),
                Toggled::Removed(name) => messages.said_or(
                    strings::FRIEND_REMOVED,
                    std::slice::from_ref(&name),
                    &format!("{name} is no longer your friend."),
                ),
                Toggled::Full => messages.said_or(
                    strings::FRIENDS_FULL,
                    &[LIST_LIMIT.to_string()],
                    &format!("Your friends list is full at {LIST_LIMIT} names."),
                ),
                // The official client adds nothing and says nothing.
                Toggled::Invalid => return (Vec::new(), false),
            };
            (vec![said], true)
        }
        Asked::Ignore(Some(name)) => {
            if !contacts.ignored.contains(&name) && world.game_master_in_zone(&name) {
                let refused = messages.said_or(
                    strings::IGNORE_REFUSED,
                    std::slice::from_ref(&name),
                    "You cannot ignore a game master.",
                );
                return (vec![refused], false);
            }
            let said = match contacts.ignored.toggle(&name, ENTRY_LIMIT) {
                Toggled::Added(name) => messages.said_or(
                    strings::IGNORE_ADDED,
                    std::slice::from_ref(&name),
                    &format!("You are now ignoring {name}."),
                ),
                Toggled::Removed(name) => messages.said_or(
                    strings::IGNORE_REMOVED,
                    std::slice::from_ref(&name),
                    &format!("You are no longer ignoring {name}."),
                ),
                Toggled::Full => messages.said_or(
                    strings::IGNORE_FULL,
                    &[LIST_LIMIT.to_string()],
                    &format!("Your ignore list is full at {LIST_LIMIT} names."),
                ),
                // The official client adds nothing and says nothing.
                Toggled::Invalid => return (Vec::new(), false),
            };
            (vec![said], true)
        }
    }
}

/// `/friends` alone: a header, the names and their count, or two lines for
/// an empty list. The official client writes the names itself, so they are
/// its lines and its log keeps them.
fn friends_listing(list: &NameList, messages: &Messages) -> Vec<Said> {
    if list.names().is_empty() {
        let [first, second] = strings::FRIENDS_EMPTY;
        return vec![
            messages.said_text(first, "Your friends list is empty."),
            messages.said_text(second, "Use /friends Name to add a friend."),
        ];
    }
    let count = list.names().len().to_string();
    std::iter::once(messages.said_text(strings::FRIENDS_HEADER, "Your friends:"))
        .chain(
            list.names()
                .iter()
                .map(|name| Said::official(name.as_str())),
        )
        .chain(std::iter::once(messages.said_or(
            strings::FRIENDS_COUNT,
            std::slice::from_ref(&count),
            &format!("{count} friends."),
        )))
        .collect()
}

/// `/ignore` alone: a header and the names.
fn ignore_listing(list: &NameList, messages: &Messages) -> Vec<Said> {
    std::iter::once(messages.said_text(strings::IGNORE_HEADER, "You are ignoring:"))
        .chain(
            list.names()
                .iter()
                .map(|name| Said::official(name.as_str())),
        )
        .collect()
}

/// Answers a `/friends`, `/ignore`, `/reply` or `/rtarget` the chat noted.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn answers(
    mut chat: ResMut<ChatState>,
    online: Res<crate::online::OnlineState>,
    messages: Option<Res<Messages>>,
) {
    let Some(asked) = chat.contacts.asked.take() else {
        return;
    };
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    if matches!(asked, Asked::Friends(_) | Asked::Ignore(_)) && online.world().player().is_none() {
        chat.history.push(system_line("Enter the world first"));
        return;
    }
    let (lines, changed) = answer(asked, &mut chat.contacts.lists, online.world(), messages);
    chat.contacts.changed |= changed;
    for line in lines {
        chat.history.push(system_line(line));
    }
}

/// One file owner for the lists.
#[derive(Resource, Default)]
pub(crate) struct Files(Store);

/// The file prefix the lists are saved under.
const FILE: &str = "contacts";

/// Loads the lists of whoever is playing and saves each change to their
/// own file; the reply list stays through a change of character, as the
/// official client keeps it for its run.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn persist(
    settings: Res<crate::ViewerSettings>,
    profile: Res<crate::profile_files::Profile>,
    mut chat: ResMut<ChatState>,
    mut files: ResMut<Files>,
) {
    let store = &mut files.0;
    store.retry(Instant::now());
    if store.loaded && store.profile == profile.key() {
        if std::mem::take(&mut chat.contacts.changed) {
            save(store, &chat.contacts.lists);
        }
        return;
    }
    if store.loaded {
        save(store, &chat.contacts.lists);
    } else {
        store.open(settings.0.settings_directory.as_ref());
    }
    chat.contacts.changed = false;
    store.profile = profile.key();
    let saved = store.profile.as_ref().and_then(|profile| {
        match store.read_result(&crate::profile_files::exact_name(FILE, profile)) {
            Ok(text) => text,
            Err(error) => {
                warn!("Could not read the character's friends and ignore lists: {error}");
                None
            }
        }
    });
    let lists = match saved {
        Some(text) => Contacts::read(&text),
        None if let Some(profile) = &store.profile => {
            let (friends, ignored) = settings
                .0
                .official_settings()
                .map(|official| {
                    official
                        .character_contacts(&profile.character, &profile.world)
                        .unwrap_or_else(|| official.default_contacts())
                })
                .unwrap_or_default();
            Contacts {
                friends: NameList::read(&friends.join("\n")),
                ignored: NameList::read(&ignored.join("\n")),
                ..Contacts::default()
            }
        }
        None => Contacts::default(),
    };
    chat.contacts.lists.friends = lists.friends;
    chat.contacts.lists.ignored = lists.ignored;
    store.loaded_text(chat.contacts.lists.text());
}

/// Writes the lists to the profile's file when they changed.
fn save(store: &mut Store, contacts: &Contacts) {
    let Some(profile) = &store.profile else {
        return;
    };
    let name = crate::profile_files::exact_name(FILE, profile);
    store.save(name, contacts.text(), Instant::now());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_follow_the_official_prefix_rule() {
        for typed in ["/r hi", "/re hi", "/rep hi", "/reply hi", "/REPLY hi"] {
            assert_eq!(
                Command::typed(typed),
                Some(Command::Reply("hi".into())),
                "{typed}"
            );
        }
        assert_eq!(Command::typed("/rt"), Some(Command::ReplyTarget));
        assert_eq!(Command::typed("/rtarget"), Some(Command::ReplyTarget));
        assert_eq!(
            Command::typed("/fr Alpha"),
            Some(Command::Friends(Some("Alpha".into())))
        );
        assert_eq!(
            Command::typed("/friends Two Words "),
            Some(Command::Friends(Some("Two Words".into())))
        );
        assert_eq!(Command::typed("/friends"), Some(Command::Friends(None)));
        assert_eq!(
            Command::typed("/ig Pest"),
            Some(Command::Ignore(Some("Pest".into())))
        );
        assert_eq!(
            Command::typed("/who all friends"),
            Some(Command::WhoFriends)
        );
        for other in [
            "/f",
            "/i",
            "/frown",
            "/random",
            "/who all",
            "/who friends",
            "hello",
        ] {
            assert_eq!(Command::typed(other), None, "{other}");
        }
    }

    #[test]
    fn the_lists_answer_in_the_official_words_and_game_masters_stay_unignored() {
        let messages = Messages::parse(
            "EQST0002
0 3
12364 nobody
12983 full %1
12984 added %1
13243 ignoring %1
",
        );
        let mut contacts = Contacts::default();
        let world = ClientWorld::default();
        let (lines, changed) = answer(Asked::NoReply, &mut contacts, &world, &messages);
        assert_eq!((lines[0].words.text.as_str(), changed), ("nobody", false));
        let (lines, changed) = answer(
            Asked::Friends(Some("alpha".into())),
            &mut contacts,
            &world,
            &messages,
        );
        assert_eq!(
            (lines[0].words.text.as_str(), changed),
            ("added alpha", true)
        );
        let (lines, _) = answer(Asked::Friends(None), &mut contacts, &world, &messages);
        assert_eq!(lines.len(), 3, "header, the name and the count");
        let (lines, changed) = answer(
            Asked::Ignore(Some("Pest".into())),
            &mut contacts,
            &world,
            &messages,
        );
        assert_eq!(
            (lines[0].words.text.as_str(), changed),
            ("ignoring Pest", true)
        );
        assert!(contacts.ignored.contains("pest"));
        // A friend's name of 16 bytes or more goes on no list, unsaid.
        let (lines, changed) = answer(
            Asked::Friends(Some("a".repeat(16))),
            &mut contacts,
            &world,
            &messages,
        );
        assert_eq!((lines.len(), changed), (0, false));
    }
}
