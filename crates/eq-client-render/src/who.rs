//! `/who` lists in the official client's words: each line's string from the
//! installed string table, filled with the player's level, class title,
//! name, race, guild and zone. `/who all` lists what the world answered; a
//! plain `/who` lists the zone's players from what the client knows.
use super::{chat::Said, hud::messages::Messages};
use bevy::prelude::{Res, ResMut};
use eq_client_core::{
    classes::{class_name, title_string},
    listing::Anonymity,
    races::race_name,
    who::{WhoList, WhoPlayer},
    world::ZonePlayer,
    zones,
};

/// A player listed in full: level, class, name, race and zone, with nine
/// arguments.
const OPEN: u32 = 5025;
/// The zone list's heading.
const ZONE_HEADING: u32 = 12310;
/// What goes before an away player's line.
const AWAY: u32 = 12311;
/// What goes before a game master's line.
const GAME_MASTER: u32 = 12312;
/// What goes after a player looking for a group.
const LOOKING: u32 = 12314;
/// How many players the zone list counted, and in which zone.
const ZONE_COUNT: u32 = 12316;
/// The zone list matching no one, in a zone.
const ZONE_NONE: u32 = 12317;
/// The zone list counting a single player, in a zone.
const ZONE_ONE: u32 = 12318;
/// The rule under a heading.
const RULE_LENGTH: usize = 27;

/// What an anonymous player hides, shown to a game master: level, class,
/// name, race and zone.
const ANONYMOUS_TO_A_GAME_MASTER: u32 = 5022;
/// A roleplaying player: name and guild.
const ROLEPLAYING: u32 = 5023;
/// An anonymous player: name only.
const ANONYMOUS: u32 = 5024;

/// The lines a `/who all` answer prints: the heading and the rule under it,
/// a line per player, and the count.
pub(super) fn lines(list: &WhoList, messages: &Messages) -> Vec<Said> {
    let mut lines = vec![
        messages.said(list.heading, &[]),
        Said::official(list.rule.clone()),
    ];
    lines.extend(list.players.iter().map(|player| line(player, messages)));
    lines.push(messages.said(list.closing, &[list.count.to_string()]));
    lines
}

/// One player's line. A string number passed as the first argument is
/// worded by the line's `%T1`.
fn line(player: &WhoPlayer, messages: &Messages) -> Said {
    let rank = player.rank.map(|rank| rank.to_string()).unwrap_or_default();
    let tag = player
        .tag
        .map(|tag| messages.format(tag, &[]))
        .unwrap_or_default();
    let level = player.level.to_string();
    let class = class_words(player.class, player.level, messages);
    let race = race_name(player.race).unwrap_or_default().to_owned();
    let zone = player
        .zone
        .map(|(words, zone)| {
            let name = zones::short_name(zone).map_or_else(|| zone.to_string(), str::to_owned);
            messages.format(words, &[name])
        })
        .unwrap_or_default();
    let account = player
        .account
        .as_ref()
        .map(|(words, account)| {
            let status = player.status.map(|status| status.to_string());
            messages.format(*words, &[account.clone(), status.unwrap_or_default()])
        })
        .unwrap_or_default();
    let name = player.name.clone();
    let guild = player.guild.clone();
    let arguments = match player.line {
        ANONYMOUS => vec![rank, name, tag],
        ROLEPLAYING => vec![rank, name, guild, tag],
        ANONYMOUS_TO_A_GAME_MASTER => vec![rank, level, class, name, race, guild, zone, tag],
        _ => vec![rank, level, class, name, race, guild, zone, tag, account],
    };
    trimmed(messages.said(player.line, &arguments))
}

/// A line without the spaces its string leaves at its end where an
/// argument is empty.
fn trimmed(mut said: Said) -> Said {
    let end = said.text.trim_end().len();
    said.text.truncate(end);
    said
}

/// A class's words at a level: its title from level 51, from the installed
/// strings, or its name.
fn class_words(class: u32, level: u32, messages: &Messages) -> String {
    title_string(class, level).map_or_else(
        || class_name(class).unwrap_or_default().to_owned(),
        |title| messages.format(title, &[]),
    )
}

/// The zone's own `/who`, as the Titanium client words it: a heading, the
/// rule, a line per player and the count in the zone's long name; when no
/// one matches, that alone.
pub(super) fn zone_lines(players: &[ZonePlayer], zone: &str, messages: &Messages) -> Vec<Said> {
    let closing = match players.len() {
        0 => return vec![messages.said(ZONE_NONE, &[zone.to_owned()])],
        1 => messages.said(ZONE_ONE, &[zone.to_owned()]),
        count => messages.said(ZONE_COUNT, &[count.to_string(), zone.to_owned()]),
    };
    let mut lines = vec![
        messages.said(ZONE_HEADING, &[]),
        Said::official("-".repeat(RULE_LENGTH)),
    ];
    lines.extend(players.iter().map(|player| zone_line(player, messages)));
    lines.push(closing);
    lines
}

/// One player's line in the zone's `/who`: the same lines as `/who all`
/// gives, without a zone, flagged before for a game master or an away
/// player and after for one looking for a group.
fn zone_line(player: &ZonePlayer, messages: &Messages) -> Said {
    let listing = player.listing;
    let flag = if listing.game_master {
        GAME_MASTER.to_string()
    } else if listing.away {
        AWAY.to_string()
    } else {
        String::new()
    };
    let tag = if listing.looking {
        messages.format(LOOKING, &[])
    } else {
        String::new()
    };
    let name = player.name.clone();
    let guild = player
        .guild
        .as_ref()
        .map(|guild| format!("<{guild}>"))
        .unwrap_or_default();
    let (line, arguments) = match listing.anonymity {
        Anonymity::Anonymous => (ANONYMOUS, vec![flag, name, tag]),
        Anonymity::Roleplaying => (ROLEPLAYING, vec![flag, name, guild, tag]),
        Anonymity::Open => {
            let level = u32::from(player.level);
            let class = player
                .class
                .map(|class| class_words(class, level, messages))
                .unwrap_or_default();
            let race = race_name(player.race).unwrap_or_default().to_owned();
            (
                OPEN,
                vec![
                    flag,
                    level.to_string(),
                    class,
                    name,
                    race,
                    guild,
                    // No zone, and no account.
                    String::new(),
                    tag,
                    String::new(),
                ],
            )
        }
    };
    trimmed(messages.said(line, &arguments))
}

/// Prints the zone's own `/who` that chat asked for.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn zone_list(
    mut chat: ResMut<super::chat::ChatState>,
    online: Res<super::online::OnlineState>,
    messages: Option<Res<Messages>>,
) {
    let Some(filter) = chat.zone_who.take() else {
        return;
    };
    let world = online.world();
    let zone = world.zone_long_name();
    let empty = Messages::default();
    for line in zone_lines(
        &world.zone_who(&filter),
        zone,
        messages.as_deref().unwrap_or(&empty),
    ) {
        chat.history.push(super::chat::system_line(line));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines' words, each the official client's.
    fn official(lines: Vec<Said>) -> Vec<String> {
        lines
            .into_iter()
            .map(|said| {
                let source = eq_client_core::chat::Source::Official;
                assert_eq!(said.source, source, "{}", said.text);
                said.text
            })
            .collect()
    }

    fn messages() -> Messages {
        Messages::parse(
            "EQST0002
0 12
1514 Shadow Knight
1517 Grave Lord
1546 Wizard
5001 Everyone online:
5003 (account %1, id %2)
5006 in %1
5015 <admin>
5023 %T1{hidden} %2 %3 %4
5024 %T1{hidden} %2 %3
5025 %T1{%2 %3} %4 /%5/ %6 %7 %8 %9
5036 %1 online in all.
12310 Everyone here:
12311 (away) 
12312 <gm> 
12313  (lost)
12314  (group?)
12316 %1 here in %2.
12317 Nobody in %1 matches.
12318 Just one here in %1.
",
        )
    }

    fn player(line: u32, name: &str) -> WhoPlayer {
        WhoPlayer {
            line,
            name: name.into(),
            rank: None,
            guild: String::new(),
            tag: None,
            zone: None,
            class: 0,
            level: 0,
            race: 0,
            account: None,
            status: None,
        }
    }

    #[test]
    fn who_lines_read_as_the_official_client_words_them() {
        let open = WhoPlayer {
            guild: "<Seekers>".into(),
            zone: Some((5006, 4)),
            class: 5,
            level: 60,
            race: 6,
            ..player(OPEN, "Tester")
        };
        let master = WhoPlayer {
            rank: Some(5015),
            tag: Some(12313),
            zone: Some((5006, 202)),
            class: 12,
            level: 20,
            race: 12,
            account: Some((5003, "guide".into())),
            status: Some(100),
            ..player(OPEN, "Guide")
        };
        let roleplaying = WhoPlayer {
            guild: "<Seekers>".into(),
            ..player(ROLEPLAYING, "Bard")
        };
        let list = WhoList {
            heading: 5001,
            rule: "-".repeat(27),
            closing: 5036,
            count: 4,
            players: vec![open, master, roleplaying, player(ANONYMOUS, "Hidden")],
        };
        assert_eq!(
            official(lines(&list, &messages())),
            [
                "Everyone online:".to_owned(),
                "-".repeat(27),
                "{60 Grave Lord} Tester /Dark Elf/ <Seekers> in qeytoqrg".into(),
                concat!(
                    "<admin>{20 Wizard} Guide /Gnome/  in poknowledge  (lost) ",
                    "(account guide, id 100)"
                )
                .into(),
                "{hidden} Bard <Seekers>".into(),
                "{hidden} Hidden".into(),
                "4 online in all.".into(),
            ]
        );
    }

    #[test]
    fn the_zone_list_reads_as_the_titanium_client_words_it() {
        use eq_client_core::listing::Listing;
        let zone_player = |name: &str, listing| ZonePlayer {
            name: name.into(),
            level: 60,
            class: Some(5),
            race: 6,
            guild: Some("Seekers".into()),
            listing,
        };
        let players = [
            zone_player(
                "Ann",
                Listing {
                    away: true,
                    looking: true,
                    ..Listing::default()
                },
            ),
            zone_player(
                "Bea",
                Listing {
                    game_master: true,
                    anonymity: Anonymity::Roleplaying,
                    ..Listing::default()
                },
            ),
            zone_player(
                "Cid",
                Listing {
                    anonymity: Anonymity::Anonymous,
                    ..Listing::default()
                },
            ),
        ];
        assert_eq!(
            official(zone_lines(&players, "The Qeynos Hills", &messages())),
            [
                "Everyone here:".to_owned(),
                "-".repeat(27),
                "(away) {60 Grave Lord} Ann /Dark Elf/ <Seekers>   (group?)".into(),
                "<gm> {hidden} Bea <Seekers>".into(),
                "{hidden} Cid".into(),
                "3 here in The Qeynos Hills.".into(),
            ]
        );
        assert_eq!(
            official(zone_lines(&players[..1], "The Qeynos Hills", &messages())).last(),
            Some(&"Just one here in The Qeynos Hills.".to_owned())
        );
        assert_eq!(
            official(zone_lines(&[], "The Qeynos Hills", &messages())),
            ["Nobody in The Qeynos Hills matches."]
        );
    }
}
