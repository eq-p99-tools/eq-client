//! The names the official client shows for classes and deities, by the
//! numbers servers send for them.

/// A class's name, by its number, from 1 for a warrior to 16 for a berserker.
#[must_use]
pub const fn class_name(class: u32) -> Option<&'static str> {
    Some(match class {
        1 => "Warrior",
        2 => "Cleric",
        3 => "Paladin",
        4 => "Ranger",
        5 => "Shadow Knight",
        6 => "Druid",
        7 => "Monk",
        8 => "Bard",
        9 => "Rogue",
        10 => "Shaman",
        11 => "Necromancer",
        12 => "Wizard",
        13 => "Magician",
        14 => "Enchanter",
        15 => "Beastlord",
        16 => "Berserker",
        _ => return None,
    })
}

/// The string the official client's string table words a class at a level
/// with: the class's name below level 51, then its titles from 51, 55, 60 and
/// 65, as `/who` shows them. None for a class the table has no words for.
#[must_use]
pub const fn title_string(class: u32, level: u32) -> Option<u32> {
    // The table words shadow knights before rangers.
    let place = match class {
        4 => 4,
        5 => 3,
        1..=15 => class - 1,
        _ => return None,
    };
    let tier = match level {
        65.. => return Some(5755 + place),
        60.. => 3,
        55.. => 2,
        51.. => 1,
        _ => 0,
    };
    Some(1502 + place * 4 + tier)
}

/// A deity's name, by its number, from 201 for Bertoxxulous to 216 for
/// Veeshan; 140 and 396 are agnostics.
#[must_use]
pub const fn deity_name(deity: u32) -> Option<&'static str> {
    Some(match deity {
        140 | 396 => "Agnostic",
        201 => "Bertoxxulous",
        202 => "Brell Serilis",
        203 => "Cazic-Thule",
        204 => "Erollisi Marr",
        205 => "Bristlebane",
        206 => "Innoruuk",
        207 => "Karana",
        208 => "Mithaniel Marr",
        209 => "Prexus",
        210 => "Quellious",
        211 => "Rallos Zek",
        212 => "Rodcet Nife",
        213 => "Solusek Ro",
        214 => "The Tribunal",
        215 => "Tunare",
        216 => "Veeshan",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_and_deities_read_by_their_numbers() {
        assert_eq!(class_name(5), Some("Shadow Knight"));
        assert_eq!(class_name(0), None);
        assert_eq!(deity_name(208), Some("Mithaniel Marr"));
        assert_eq!(deity_name(396), Some("Agnostic"));
        assert_eq!(deity_name(1), None);
    }

    #[test]
    fn a_class_is_worded_by_its_title_from_level_51() {
        // Warrior, Champion, Myrmidon, Warlord and Overlord.
        let warrior: Vec<_> = [50, 51, 55, 60, 65]
            .map(|level| title_string(1, level))
            .into();
        assert_eq!(
            warrior,
            [Some(1502), Some(1503), Some(1504), Some(1505), Some(5755)]
        );
        // Shadow Knight and Grave Lord; Ranger and Warder; Beastlord.
        assert_eq!(title_string(5, 1), Some(1514));
        assert_eq!(title_string(5, 60), Some(1517));
        assert_eq!(title_string(4, 60), Some(1521));
        assert_eq!(title_string(4, 65), Some(5759));
        assert_eq!(title_string(15, 1), Some(1558));
        assert_eq!(title_string(16, 60), None);
    }
}
