//! `/who`: who is online in the world (`/who all`), or in the zone. The
//! words after the command narrow the list as the official client's do: a
//! class, a race, a level or a range of levels, `gm`, and the start of a
//! name, guild or zone.
pub use eq_network_game::who::{MAX_TEXT, WhoFilter, WhoList, WhoPlayer};

/// What a `/who` asks: the whole world or only the zone, and about whom.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WhoRequest {
    /// `/who all` asks the world; a plain `/who` lists the zone's players.
    pub everywhere: bool,
    /// Which players.
    pub filter: WhoFilter,
}

/// Class numbers by the words `/who` takes for them: each class's name and
/// its three-letter short form.
const CLASSES: [(&str, &str, u8); 16] = [
    ("warrior", "war", 1),
    ("cleric", "clr", 2),
    ("paladin", "pal", 3),
    ("ranger", "rng", 4),
    ("shadowknight", "shd", 5),
    ("druid", "dru", 6),
    ("monk", "mnk", 7),
    ("bard", "brd", 8),
    ("rogue", "rog", 9),
    ("shaman", "shm", 10),
    ("necromancer", "nec", 11),
    ("wizard", "wiz", 12),
    ("magician", "mag", 13),
    ("enchanter", "enc", 14),
    ("beastlord", "bst", 15),
    ("berserker", "ber", 16),
];

/// Race numbers by the words `/who` takes for them: each playable race's name
/// without spaces and its three-letter short form.
const RACES: [(&str, &str, u32); 16] = [
    ("human", "hum", 1),
    ("barbarian", "bar", 2),
    ("erudite", "eru", 3),
    ("woodelf", "elf", 4),
    ("highelf", "hie", 5),
    ("darkelf", "def", 6),
    ("halfelf", "hef", 7),
    ("dwarf", "dwf", 8),
    ("troll", "trl", 9),
    ("ogre", "ogr", 10),
    ("halfling", "hfl", 11),
    ("gnome", "gnm", 12),
    ("iksar", "iks", 128),
    ("vahshir", "vah", 130),
    ("froglok", "frg", 330),
    ("drakkin", "drk", 522),
];

/// Reads the words after `/who`.
///
/// # Errors
/// Says why when the levels are not levels or the name, guild or zone start
/// is too long to ask about.
pub fn parse(words: &str) -> Result<WhoRequest, String> {
    let mut request = WhoRequest::default();
    let mut levels = Vec::new();
    let mut text = Vec::new();
    let words: Vec<String> = words
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let mut index = 0;
    while let Some(word) = words.get(index) {
        // Two-word classes and races, such as "dark elf", read as one.
        let joined = words.get(index + 1).map(|next| format!("{word}{next}"));
        let known = |word: &str| {
            CLASSES.iter().any(|(name, _, _)| *name == word)
                || RACES.iter().any(|(name, _, _)| *name == word)
        };
        let word = match joined {
            Some(joined) if !known(word) && known(&joined) => {
                index += 1;
                joined
            }
            _ => word.clone(),
        };
        index += 1;
        if index == 1 && word == "all" {
            request.everywhere = true;
        } else if word == "gm" {
            request.filter.game_masters = true;
        } else if let Some((_, _, class)) = CLASSES
            .iter()
            .find(|(name, short, _)| *name == word || *short == word)
        {
            request.filter.class = Some(*class);
        } else if let Some((_, _, race)) = RACES
            .iter()
            .find(|(name, short, _)| *name == word || *short == word)
        {
            request.filter.race = Some(*race);
        } else if word.bytes().all(|byte| byte.is_ascii_digit()) {
            levels.push(
                word.parse::<u8>()
                    .map_err(|_| format!("{word} is not a level"))?,
            );
        } else {
            text.push(word);
        }
    }
    request.filter.levels = match levels[..] {
        [] => None,
        [level] => Some((level, level)),
        [first, second] => Some((first.min(second), first.max(second))),
        _ => return Err("Give one level, or two for a range".into()),
    };
    request.filter.text = text.join(" ");
    if request.filter.text.len() > MAX_TEXT {
        return Err(format!(
            "A name, guild or zone to look for is at most {MAX_TEXT} letters"
        ));
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn who_words_narrow_the_list() {
        assert_eq!(parse("").unwrap(), WhoRequest::default());
        let request = parse("all Dark Elf wiz 50 40 gm").unwrap();
        assert!(request.everywhere);
        assert_eq!(
            request.filter,
            WhoFilter {
                text: String::new(),
                race: Some(6),
                class: Some(12),
                levels: Some((40, 50)),
                game_masters: true,
            }
        );
        let request = parse("all shadow knight 60 qeynos hills").unwrap();
        assert_eq!(request.filter.class, Some(5));
        assert_eq!(request.filter.levels, Some((60, 60)));
        assert_eq!(request.filter.text, "qeynos hills");
        // "all" only asks the world as the first word.
        let request = parse("Allmighty").unwrap();
        assert!(!request.everywhere);
        assert_eq!(request.filter.text, "allmighty");
        assert!(parse("all 1 2 3").is_err());
        assert!(parse("all 300").is_err());
        assert!(parse(&format!("all {}", "x".repeat(64))).is_err());
    }
}
