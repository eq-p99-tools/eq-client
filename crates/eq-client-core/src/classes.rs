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

/// The string the official client names a class with on a character's line
/// at character select, by the class's number as the list carries it; every
/// number has one, a number with no name of its own the table's fallback,
/// 2456 (from the program, 0x491e5b).
#[must_use]
pub const fn select_name_string(class: u8) -> u32 {
    match class {
        // The table words shadow knights before rangers.
        1..=3 | 6..=15 => 1502 + (class as u32 - 1) * 4,
        4 => 1518,
        5 => 1514,
        16 => 5791,
        20 => 1487,
        21 => 1488,
        22 => 1489,
        23 => 1491,
        24 => 1490,
        25..=34 => 1492 + (class as u32 - 25),
        35 => 5792,
        40 => 1486,
        41 => 1485,
        66 => 6110,
        _ => 2456,
    }
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
    fn a_class_at_character_select_is_named_by_the_programs_string() {
        let ids: Vec<_> = (0..=21).map(select_name_string).collect();
        assert_eq!(
            ids,
            [
                2456, 1502, 1506, 1510, 1518, 1514, 1522, 1526, 1530, 1534, 1538, 1542, 1546, 1550,
                1554, 1558, 5791, 2456, 2456, 2456, 1487, 1488
            ]
        );
        let above: Vec<_> = (22..=35).map(select_name_string).collect();
        assert_eq!(
            above,
            [
                1489, 1491, 1490, 1492, 1493, 1494, 1495, 1496, 1497, 1498, 1499, 1500, 1501, 5792
            ]
        );
        assert_eq!(
            [36, 40, 41, 66, 67, 255].map(select_name_string),
            [2456, 1486, 1485, 6110, 2456, 2456]
        );
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
