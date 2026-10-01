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
}
