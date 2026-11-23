//! The Titanium client's slash command names, in the order its parser
//! tries them, and how a typed word resolves among them (eqgame.exe: the
//! command table at 0x78a0cc, 336 entries of 28 bytes; the parser at
//! 0x498b78). The word is compared, letter case aside, as the start of each
//! name in turn, and the first name it starts wins, so an empty word, a bare
//! `/`, is the first entry's, `/who`. A first match on a game master's
//! command does nothing for a player, and the parser does not go on to a
//! later name (0x498eda-0x498ef0). Names and flags only; no help text. The
//! table's second `/quit`, which nothing reaches, and its empty terminator
//! are left out.
//!
//! The table matches the program's row for row (read R49-33e): names, the
//! order, the game master's rows (level byte +0xc above 0), and the names
//! that share a handler. No row has an alias (+4), so the program's alias
//! pass before its name pass finds nothing and the order is the names'.
//!
//! A word that starts no name is not the official client's. Only such a
//! word reaches this client's own commands (`/social`, `/pause` inside a
//! social, `/ranged`, `/harmtouch`, `/layonhands`, `/uiscale`). Any other,
//! and a game master's command for a player who isn't one, gets the
//! program's line 0x305c ([`unknown`], read R49-33 a, checklist 6.634).
//! The program's other answer, line 0x21a for a row without flag bit 1 run in
//! character states 3 or 4, waits on a read of that state byte [I]: not
//! built.

/// What a name in the table does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A command of its own.
    Own,
    /// Another name for the command named, sharing its handler.
    Same(&'static str),
    /// A game master's command: nothing for a player.
    Gm,
}

use Kind::{Gm, Own, Same};

/// The official names, in table order, without their slash.
const NAMES: &[(&str, Kind)] = &[
    ("who", Own),
    ("consider", Own),
    ("feedback", Own),
    ("consent", Own),
    ("played", Own),
    ("ignore", Own),
    ("anonymous", Own),
    ("afk", Own),
    ("filter", Own),
    ("assist", Own),
    ("duel", Own),
    ("decline", Own),
    ("split", Own),
    ("autosplit", Own),
    ("quit", Own),
    ("stopsong", Own),
    ("friends", Own),
    ("shield", Own),
    ("shownames", Own),
    ("shownpcnames", Gm),
    ("emote", Own),
    (":", Same("emote")),
    ("say", Own),
    ("'", Same("say")),
    ("tell", Own),
    ("msg", Same("tell")),
    ("send", Same("tell")),
    ("ttell", Own),
    ("gsay", Own),
    ("shout", Own),
    ("auction", Own),
    ("ooc", Own),
    ("reply", Own),
    ("channel", Own),
    ("guildsay", Own),
    ("guildinvite", Own),
    ("guildremove", Own),
    ("guilddelete", Own),
    ("guildstatus", Own),
    ("guildleader", Own),
    ("guildwar", Own),
    ("guildpeace", Own),
    ("pet", Own),
    ("petition", Own),
    ("private", Gm),
    ("find", Gm),
    ("goto", Gm),
    ("becomenpc", Gm),
    ("searchcorpse", Gm),
    ("yell", Own),
    ("motd", Own),
    ("corpse", Own),
    ("corpsedrag", Own),
    ("corpsedrop", Own),
    ("me", Same("emote")),
    ("location", Own),
    ("rewind", Own),
    ("melody", Own),
    ("adriver", Own),
    ("sdriver", Same("adriver")),
    ("chat", Own),
    ("join", Own),
    ("leave", Own),
    ("leaveall", Own),
    ("list", Own),
    ("announce", Own),
    ("1", Own),
    ("2", Own),
    ("3", Own),
    ("4", Own),
    ("5", Own),
    ("6", Own),
    ("7", Own),
    ("8", Own),
    ("9", Own),
    ("10", Own),
    ("autojoin", Own),
    ("keys", Own),
    ("mp3", Own),
    ("map", Own),
    ("claim", Own),
    ("indicator", Own),
    ("guidehelp", Own),
    ("stopcast", Own),
    ("netstats", Own),
    ("bandolier", Own),
    ("potionbelt", Own),
    ("veteranReward", Own),
    ("testbuffme", Own),
    ("beta", Own),
    ("betabuff", Own),
    ("deny", Own),
    ("emoteworld", Gm),
    ("system", Own),
    ("url", Own),
    ("www", Own),
    ("spellscribe", Own),
    ("smile", Own),
    ("hail", Own),
    ("frown", Own),
    ("laugh", Own),
    ("cry", Own),
    ("chuckle", Own),
    ("hug", Own),
    ("poke", Own),
    ("bow", Own),
    ("bounce", Own),
    ("panic", Own),
    ("nod", Own),
    ("rofl", Own),
    ("wave", Own),
    ("cheer", Own),
    ("gesture", Own),
    ("rude", Same("gesture")),
    ("flipoff", Same("gesture")),
    ("bird", Same("gesture")),
    ("finger", Same("gesture")),
    ("agree", Own),
    ("amaze", Own),
    ("apologize", Own),
    ("applaud", Own),
    ("plead", Own),
    ("bite", Own),
    ("bleed", Own),
    ("blink", Own),
    ("blush", Own),
    ("boggle", Own),
    ("bonk", Own),
    ("bored", Own),
    ("brb", Own),
    ("burp", Own),
    ("bye", Own),
    ("cackle", Own),
    ("calm", Own),
    ("clap", Own),
    ("comfort", Own),
    ("congratulate", Own),
    ("cough", Own),
    ("cringe", Own),
    ("curious", Own),
    ("dance", Own),
    ("drool", Own),
    ("duck", Own),
    ("eye", Own),
    ("gasp", Own),
    ("giggle", Own),
    ("glare", Own),
    ("grin", Own),
    ("groan", Own),
    ("grovel", Own),
    ("happy", Own),
    ("hungry", Own),
    ("introduce", Own),
    ("jk", Own),
    ("kneel", Own),
    ("lost", Own),
    ("massage", Own),
    ("moan", Own),
    ("mourn", Own),
    ("peer", Own),
    ("point", Own),
    ("ponder", Own),
    ("puzzle", Own),
    ("raise", Own),
    ("ready", Own),
    ("roar", Own),
    ("salute", Own),
    ("shiver", Own),
    ("shrug", Own),
    ("sigh", Own),
    ("smirk", Own),
    ("snarl", Own),
    ("snicker", Own),
    ("stare", Own),
    ("tap", Own),
    ("tease", Own),
    ("thank", Own),
    ("thirsty", Own),
    ("veto", Own),
    ("welcome", Own),
    ("whine", Own),
    ("whistle", Own),
    ("yawn", Own),
    ("smack", Own),
    ("pat", Own),
    ("fidget", Own),
    ("flex", Own),
    ("nudge", Own),
    ("purr", Own),
    ("kiss", Own),
    ("time", Own),
    ("discipline", Own),
    ("random", Own),
    ("summon", Gm),
    ("broadcast", Gm),
    ("lastname", Gm),
    ("zone", Gm),
    ("kick", Gm),
    ("kill", Gm),
    ("delcorpse", Gm),
    ("hideme", Gm),
    ("servers", Gm),
    ("height", Gm),
    ("exit", Own),
    ("loot", Own),
    ("selfkill", Gm),
    ("roleplay", Own),
    ("follow", Own),
    ("report", Own),
    ("surname", Own),
    ("log", Own),
    ("toggletell", Gm),
    ("inquire", Gm),
    ("praise", Gm),
    ("warn", Gm),
    ("movelog", Own),
    ("hidecorpses", Own),
    ("reverb", Own),
    ("fastdrop", Own),
    ("trackplayers", Own),
    ("stoptracking", Own),
    ("help", Own),
    ("lfg", Own),
    ("approval", Gm),
    ("sit", Own),
    ("stand", Own),
    ("book", Own),
    ("cast", Own),
    ("doability", Own),
    ("whotarget", Own),
    ("language", Own),
    ("target", Own),
    ("camp", Own),
    ("attack", Own),
    ("invite", Own),
    ("disband", Own),
    ("note", Own),
    ("usercolor", Own),
    ("resetwindows", Same("reverb")),
    ("mcicontrol", Own),
    ("popmail", Same("reverb")),
    ("dynamiclights", Own),
    ("chatfontsize", Own),
    ("lootnodrop", Own),
    ("reversesound", Own),
    ("emotezone", Gm),
    ("invwinlabels", Own),
    ("tracksort", Own),
    ("trackfilter", Own),
    ("mousespeed", Own),
    ("gems", Own),
    ("toggleinspect", Own),
    ("inspect", Own),
    ("open", Own),
    ("barter", Own),
    ("buyer", Own),
    ("showspelleffects", Own),
    ("combatmusic", Own),
    ("mixahead", Own),
    ("guildmotd", Own),
    ("charinfo", Own),
    ("safelock", Own),
    ("alternateadv", Own),
    ("targetgroupbuff", Own),
    ("tgb", Same("targetgroupbuff")),
    ("hotbutton", Own),
    ("particledensity", Own),
    ("facepick", Own),
    ("dismount", Own),
    ("rtarget", Own),
    ("showgrass", Own),
    ("dopropertinting", Own),
    ("makeleader", Own),
    ("fullscreen", Own),
    ("viewport", Own),
    ("clearchat", Own),
    ("bazaar", Own),
    ("trader", Own),
    ("bugreport", Own),
    ("loadskin", Own),
    ("getguildmotd", Own),
    ("memspellset", Own),
    ("autoinventory", Own),
    ("adventure", Gm),
    ("dzhelp", Own),
    ("dzlisttimers", Own),
    ("dzaddplayer", Own),
    ("dzremoveplayer", Own),
    ("dzswapplayer", Own),
    ("dzmakeleader", Own),
    ("dzplayerlist", Own),
    ("dzquit", Own),
    ("taskaddplayer", Own),
    ("taskremoveplayer", Own),
    ("taskplayerlist", Own),
    ("taskmakeleader", Own),
    ("tasktimers", Own),
    ("taskquit", Own),
    ("taskhelp", Own),
    ("gmarknpc", Own),
    ("rmarknpc", Own),
    ("marknpc", Same("gmarknpc")),
    ("clearmarks", Own),
    ("findpc", Own),
    ("inspectbuffs", Own),
    ("webupdate", Own),
    ("viewpetition", Own),
    ("deletepetition", Own),
    ("bodytint", Own),
    ("raidwindow", Own),
    ("makeraidleader", Own),
    ("raidinvite", Own),
    ("raiddecline", Own),
    ("raidaccept", Own),
    ("raiddisband", Own),
    ("setloottype", Own),
    ("addraidlooter", Own),
    ("removeraidlooter", Own),
    ("rsay", Own),
    ("summoncorpse", Gm),
    ("sheathe", Own),
    ("fontface", Own),
    ("aggressive", Own),
    ("outputfile", Own),
    ("vplay", Own),
    ("vtell", Own),
    ("vgroup", Own),
    ("vraid", Own),
    ("shadows", Own),
    ("autofire", Own),
    ("setstartcity", Own),
    ("changename", Own),
    ("resetaas", Own),
    ("resetCreationServer", Own),
];

/// This client's own commands, which no official name starts with.
const OURS: &[&str] = &[
    "boat",
    "social",
    "pause",
    "ranged",
    "harmtouch",
    "layonhands",
    "uiscale",
];

/// What a typed command word resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// The official command, by the name its handler goes by.
    Official(&'static str),
    /// One of this client's own commands, by its name.
    Ours(&'static str),
    /// A game master's command, or a word that is no command: nothing.
    Nothing,
}

/// The command a typed word, without its slash, resolves to.
pub(crate) fn resolve(word: &str) -> Resolved {
    match NAMES.iter().find(|(name, _)| starts(name, word)) {
        Some((name, Own) | (_, Same(name))) => Resolved::Official(name),
        Some((_, Gm)) => Resolved::Nothing,
        None => OURS
            .iter()
            .find(|name| name.eq_ignore_ascii_case(word))
            .map_or(Resolved::Nothing, |name| Resolved::Ours(name)),
    }
}

/// The program's line for a slash word with no command (0x498b78).
pub(crate) const UNKNOWN_LINE: u32 = 0x305c;
/// Its chat colour.
pub(crate) const UNKNOWN_COLOUR: u32 = 0xd;
/// This client's words for it where the installation lacks the line.
pub(crate) const UNKNOWN_WORDS: &str = "That is not a command.";

/// Whether a typed slash line's word is first matched by a game master's
/// command, which the program runs for a GM (spawn +0x256) and answers as
/// an unknown word for anyone else (0x498eda-0x498ef0).
pub(crate) fn game_masters(line: &str) -> bool {
    line.strip_prefix('/').is_some_and(|command| {
        let end = command.find(char::is_whitespace).unwrap_or(command.len());
        let word = &command[..end];
        NAMES
            .iter()
            .find(|(name, _)| starts(name, word))
            .is_some_and(|(_, kind)| *kind == Gm)
    })
}

/// Whether a slash line that resolved to nothing gets the program's line
/// 0x305c: always, but for a game master's command typed by a GM. The
/// program runs that one; its handlers are game master tools this client
/// hasn't built, so it passes over without a line [I] (a GM's `#` line
/// reaches the server instead). Titanium's line only: the Mac program's
/// answer is unread, so a Mac-era or RoF2 session stays silent as before.
pub(crate) fn unknown(line: &str, world: &eq_client_core::world::ClientWorld) -> bool {
    let titanium = matches!(world.generation(), eq_client_core::GameDialect::Titanium);
    let gm = world.player().is_some_and(crate::languages::game_master);
    titanium && line.starts_with('/') && !(gm && game_masters(line))
}

/// Whether `word` starts `name`, letter case aside.
fn starts(name: &str, word: &str) -> bool {
    name.len() >= word.len()
        && name.is_char_boundary(word.len())
        && name[..word.len()].eq_ignore_ascii_case(word)
}

/// A typed line as the commands read it: a slash command with its word
/// spelled out as the name it resolves to, the words after it unchanged.
/// None when the word resolves to nothing. A line that is no slash command
/// is unchanged.
pub(crate) fn spelled(line: &str) -> Option<std::borrow::Cow<'_, str>> {
    let Some(command) = line.strip_prefix('/') else {
        return Some(line.into());
    };
    let end = command.find(char::is_whitespace).unwrap_or(command.len());
    let (word, rest) = command.split_at(end);
    match resolve(word) {
        Resolved::Official(name) | Resolved::Ours(name) => Some(format!("/{name}{rest}").into()),
        Resolved::Nothing => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_the_first_name_it_starts() {
        let official = |word| match resolve(word) {
            Resolved::Official(name) => Some(name),
            _ => None,
        };
        assert_eq!(official(""), Some("who"));
        assert_eq!(official("s"), Some("split"));
        assert_eq!(official("SH"), Some("shield"));
        assert_eq!(official("sho"), Some("shownames"));
        assert_eq!(official("g"), Some("gsay"));
        assert_eq!(official("gu"), Some("guildsay"));
        assert_eq!(official("guild"), Some("guildsay"));
        assert_eq!(official("guildi"), Some("guildinvite"));
        assert_eq!(official("l"), Some("location"));
        assert_eq!(official("loc"), Some("location"));
        assert_eq!(official("raid"), Some("raidwindow"));
        assert_eq!(official("rs"), Some("rsay"));
        assert_eq!(official("e"), Some("emote"));
        assert_eq!(official("me"), Some("emote"));
        assert_eq!(official("m"), Some("tell"));
        assert_eq!(official("se"), Some("tell"));
        assert_eq!(official("disc"), Some("discipline"));
        assert_eq!(official("q"), Some("quit"));
        assert_eq!(official("ex"), Some("exit"));
        assert_eq!(official("fo"), Some("follow"));
        assert_eq!(official("au"), Some("autosplit"));
        assert_eq!(official("r"), Some("reply"));
        assert_eq!(official("rt"), Some("rtarget"));
        assert_eq!(official("tgb"), Some("targetgroupbuff"));
    }

    #[test]
    fn a_game_masters_command_or_no_command_is_nothing() {
        // The first match is a game master's, though a later name starts so too.
        for word in [
            "b", "z", "go", "su", "hi", "he", "del", "to", "shownp", "emotew",
        ] {
            assert_eq!(resolve(word), Resolved::Nothing, "{word}");
        }
        assert_eq!(resolve("group"), Resolved::Nothing);
        assert_eq!(resolve("whoever"), Resolved::Nothing);
        assert_eq!(resolve("splits"), Resolved::Nothing);
    }

    #[test]
    fn an_unknown_word_gets_the_programs_line_but_a_gms_own_command_runs() {
        use crate::online::{OnlineState, testing as session};
        use eq_client_core::GameDialect;
        let world = |gm: bool, generation| {
            let mut online = OnlineState::new(true);
            let mut player = session::player(7);
            player.appearance.titanium_gm = Some(u8::from(gm));
            session::admit(&mut online, 1, player);
            session::generation(&mut online, generation);
            online
        };
        // A player who isn't a GM: no command and a GM's command alike.
        let player = world(false, GameDialect::Titanium);
        for line in ["/zone qeynos", "/b", "/group hi", "/whoever", "/soc"] {
            assert!(unknown(line, player.world()), "{line}");
        }
        // A GM: the program runs the GM's command (its handler is unbuilt
        // here, so nothing is said); a word that is no command still gets
        // the line.
        let gm = world(true, GameDialect::Titanium);
        for line in ["/zone qeynos", "/b", "/shownp", "/summoncorpse"] {
            assert!(game_masters(line), "{line}");
            assert!(!unknown(line, gm.world()), "{line}");
        }
        assert!(unknown("/group hi", gm.world()));
        assert!(!game_masters("/who"));
        assert!(!game_masters("zone"));
        // Only a slash line, and only where the Titanium program is read.
        assert!(!unknown("zone", player.world()));
        for generation in [GameDialect::EqMac, GameDialect::Rof2] {
            assert!(!unknown("/whoever", world(false, generation).world()));
        }
    }

    #[test]
    fn only_a_word_no_official_name_starts_reaches_our_commands() {
        assert_eq!(resolve("social"), Resolved::Ours("social"));
        assert_eq!(resolve("Ranged"), Resolved::Ours("ranged"));
        assert_eq!(resolve("harmtouch"), Resolved::Ours("harmtouch"));
        assert_eq!(resolve("layonhands"), Resolved::Ours("layonhands"));
        assert_eq!(resolve("UiScale"), Resolved::Ours("uiscale"));
        // An official name comes first.
        assert_eq!(resolve("ran"), Resolved::Official("random"));
        assert_eq!(resolve("h"), Resolved::Official("hail"));
        // Our names are whole words only.
        assert_eq!(resolve("soc"), Resolved::Nothing);
    }

    #[test]
    fn a_line_is_spelled_with_its_commands_name() {
        assert_eq!(spelled("/s 1 2").as_deref(), Some("/split 1 2"));
        assert_eq!(spelled("/ all").as_deref(), Some("/who all"));
        assert_eq!(spelled("/").as_deref(), Some("/who"));
        assert_eq!(spelled("/msg Name  hi").as_deref(), Some("/tell Name  hi"));
        assert_eq!(spelled("hello").as_deref(), Some("hello"));
        assert_eq!(spelled("/zone qeynos"), None);
        assert_eq!(spelled("/group hi"), None);
    }

    #[test]
    fn a_spelled_line_spells_the_same_again() {
        // A typed line is spelled out, and a game command spells what it is
        // handed again, so each name must resolve to itself.
        for (name, kind) in NAMES {
            let line = format!("/{name} words");
            let once = spelled(&line).map(std::borrow::Cow::into_owned);
            match kind {
                Gm => assert_eq!(once, None, "{name}"),
                Own | Same(_) => {
                    let once = once.unwrap();
                    let target = match kind {
                        Same(target) => target,
                        _ => name,
                    };
                    assert_eq!(once, format!("/{target} words"), "{name}");
                    assert_eq!(spelled(&once).as_deref(), Some(once.as_str()), "{name}");
                }
            }
        }
        for name in OURS {
            let line = format!("/{name}");
            assert_eq!(spelled(&line).as_deref(), Some(line.as_str()));
        }
    }

    #[test]
    fn the_table_holds_each_name_once_and_aliases_name_commands() {
        for (index, (name, kind)) in NAMES.iter().enumerate() {
            assert!(
                !NAMES[..index].iter().any(|(earlier, _)| earlier == name),
                "{name}"
            );
            if let Same(own) = kind {
                assert!(
                    NAMES
                        .iter()
                        .any(|(other, kind)| other == own && *kind == Own)
                );
            }
        }
        for name in OURS {
            assert!(!NAMES.iter().any(|(official, _)| {
                official.len() >= name.len() && official[..name.len()].eq_ignore_ascii_case(name)
            }));
        }
    }
}
