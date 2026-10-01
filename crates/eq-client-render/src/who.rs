//! `/who` lists in the official client's words: each line's string from the
//! installed string table, filled with the player's level, class title,
//! name, race, guild and zone.
use super::hud::messages::Messages;
use eq_client_core::{
    classes::{class_name, title_string},
    races::race_name,
    who::{WhoList, WhoPlayer},
    zones,
};

/// `%T1[ANON (%2 %3)] %4 (%5) %6 %7 %8`: what an anonymous player hides,
/// shown to a game master.
const ANONYMOUS_TO_A_GAME_MASTER: u32 = 5022;
/// `%T1[ANONYMOUS] %2 %3 %4`: a roleplaying player, guild shown.
const ROLEPLAYING: u32 = 5023;
/// `%T1[ANONYMOUS] %2 %3`: an anonymous player.
const ANONYMOUS: u32 = 5024;

/// The lines a `/who all` answer prints: the heading and the rule under it,
/// a line per player, and the count.
pub(super) fn lines(list: &WhoList, messages: &Messages) -> Vec<String> {
    let mut lines = vec![messages.format(list.heading, &[]), list.rule.clone()];
    lines.extend(list.players.iter().map(|player| line(player, messages)));
    lines.push(messages.format(list.closing, &[list.count.to_string()]));
    lines
}

/// One player's line. A string number passed as the first argument is
/// worded by the line's `%T1`.
fn line(player: &WhoPlayer, messages: &Messages) -> String {
    let rank = player.rank.map(|rank| rank.to_string()).unwrap_or_default();
    let tag = player
        .tag
        .map(|tag| messages.format(tag, &[]))
        .unwrap_or_default();
    let level = player.level.to_string();
    let class = title_string(player.class, player.level).map_or_else(
        || class_name(player.class).unwrap_or_default().to_owned(),
        |title| messages.format(title, &[]),
    );
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
        .map(|(words, account)| messages.format(*words, std::slice::from_ref(account)))
        .unwrap_or_default();
    let name = player.name.clone();
    let guild = player.guild.clone();
    let arguments = match player.line {
        ANONYMOUS => vec![rank, name, tag],
        ROLEPLAYING => vec![rank, name, guild, tag],
        ANONYMOUS_TO_A_GAME_MASTER => vec![rank, level, class, name, race, guild, zone, tag],
        // 5025, `%T1[%2 %3] %4 (%5) %6 %7 %8 %9`: level, class, race and
        // zone shown.
        _ => vec![rank, level, class, name, race, guild, zone, tag, account],
    };
    messages
        .format(player.line, &arguments)
        .trim_end()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: u32 = 5025;

    fn messages() -> Messages {
        Messages::parse(
            "EQST0002
0 12
1514 Shadow Knight
1517 Grave Lord
1546 Wizard
5001 Players in EverQuest:
5006 ZONE: %1
5015 * GM-Admin *
5023 %T1[ANONYMOUS] %2 %3 %4
5024 %T1[ANONYMOUS] %2 %3
5025 %T1[%2 %3] %4 (%5) %6 %7 %8 %9
5036 There are %1 players in EverQuest.
12313  <LINKDEAD>
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
            lines(&list, &messages()),
            [
                "Players in EverQuest:".to_owned(),
                "-".repeat(27),
                "[60 Grave Lord] Tester (Dark Elf) <Seekers> ZONE: qeytoqrg".into(),
                "* GM-Admin *[20 Wizard] Guide (Gnome)  ZONE: poknowledge  <LINKDEAD>".into(),
                "[ANONYMOUS] Bard <Seekers>".into(),
                "[ANONYMOUS] Hidden".into(),
                "There are 4 players in EverQuest.".into(),
            ]
        );
    }
}
