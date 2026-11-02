//! Local map searches, watch names and level-relative presentation.
use crate::{combat::ConColor, entities::display_name};
use std::collections::BTreeSet;

/// Case-insensitive displayed names, ignoring spawn suffixes and underscores.
pub fn name_key(name: &str) -> String {
    display_name(name.trim())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A substring search against the same name the map displays.
pub fn matches(name: &str, query: &str) -> bool {
    name_key(name).contains(&name_key(query))
}

/// Exact displayed names to watch, independent of ephemeral spawn IDs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watches(BTreeSet<String>);

impl Watches {
    /// Adds a nonempty exact name. Returns whether the list changed.
    pub fn add(&mut self, name: &str) -> bool {
        let key = name_key(name);
        !key.is_empty() && key.len() <= 128 && self.0.insert(key)
    }

    /// Stops watching this exact name.
    pub fn remove(&mut self, name: &str) -> bool {
        self.0.remove(&name_key(name))
    }

    /// Whether this spawn's displayed name is watched.
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains(&name_key(name))
    }

    /// Watched names in stable alphabetical order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// Reads one exact name per line; blank lines and comments are ignored.
    /// A leading `#` or backslash in a name is escaped with a backslash.
    pub fn read(text: &str) -> Self {
        let mut watches = Self::default();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with('#'))
        {
            let name = if line.starts_with("\\#") || line.starts_with("\\\\") {
                &line[1..]
            } else {
                line
            };
            watches.add(name);
        }
        watches
    }

    /// A portable settings file containing no session IDs or positions.
    pub fn text(&self) -> String {
        let mut text = String::from("# eq-client map watches v1\n");
        for name in &self.0 {
            if name.starts_with('#') || name.starts_with('\\') {
                text.push('\\');
            }
            text.push_str(name);
            text.push('\n');
        }
        text
    }
}

/// Level-only estimate for classic EQ or the Mac-era light-blue scale.
/// A server's actual consider reply takes precedence at the call site.
/// Mac thresholds follow the behavior documented by Quarm's `Mob::GetLevelCon`
/// in `zone/mob_ai.cpp`; classic has no light-blue band. This is presentation,
/// never a prediction of faction, aggression, experience or combat outcome.
/// References: <https://github.com/SecretsOTheP/EQMacEmu/blob/main/zone/mob_ai.cpp>
/// and <https://wiki.project1999.com/Consider> (classic observed color ranges).
pub fn level_con(player: u8, npc: u8, mac: bool) -> Option<ConColor> {
    if player == 0 || npc == 0 {
        return None;
    }
    let difference = i16::from(npc) - i16::from(player);
    Some(match difference {
        0 => ConColor::White,
        1..=2 => ConColor::Yellow,
        3.. => ConColor::Red,
        _ if mac => {
            let (green, light_blue) = match player {
                1..=7 => (4, 4),
                8 => (5, 4),
                9..=12 => (6, 4),
                13..=16 => (7, 5),
                17..=20 => (8, 6),
                21..=24 => (9, 7),
                25..=28 => (10, 8),
                29..=30 => (11, 9),
                31..=32 => (12, 9),
                33..=36 => (13, 10),
                37..=40 => (14, 11),
                41..=44 => (16, 12),
                45..=48 => (17, 13),
                49..=52 => (18, 14),
                53..=54 => (19, 15),
                55..=56 => (20, 15),
                57..=60 => (21, 16),
                61 => (19, 14),
                62 => (17, 12),
                _ => (16, 11),
            };
            if difference <= -green {
                ConColor::Green
            } else if difference <= -light_blue {
                ConColor::LightBlue
            } else {
                ConColor::Blue
            }
        }
        _ => {
            let green = match player {
                1..=12 => 4,
                13..=17 => 5,
                18..=21 => 6,
                22..=25 => 7,
                26..=29 => 8,
                30..=33 => 9,
                34..=37 => 10,
                38..=41 => 11,
                42..=45 => 12,
                46..=49 => 13,
                50..=53 => 14,
                54..=57 => 15,
                _ => 16,
            };
            if difference <= -green {
                ConColor::Green
            } else {
                ConColor::Blue
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_names_round_trip_without_becoming_file_comments() {
        let mut watches = Watches::default();
        for name in ["#Named_guard001", r"\Unusual", "ordinary"] {
            watches.add(name);
        }
        assert_eq!(Watches::read(&watches.text()), watches);
        assert_eq!(
            Watches::read("# a comment\n\nordinary\n")
                .names()
                .collect::<Vec<_>>(),
            ["ordinary"]
        );
    }

    #[test]
    fn searching_is_partial_but_watches_are_exact_and_survive_spawn_suffixes() {
        assert!(matches("A_dune_spider003", "DUNE SPIDER"));
        let mut watches = Watches::default();
        assert!(watches.add(" A_dune_spider001 "));
        assert!(!watches.add("a dune spider"));
        assert!(watches.contains("a_dune_spider017"));
        assert!(!watches.contains("a_giant_dune_spider001"));
        assert_eq!(Watches::read(&watches.text()), watches);
        assert!(watches.remove("a dune spider"));
        assert!(!watches.add("   "));
    }

    #[test]
    fn level_boundaries_and_unknowns_use_the_expected_bands() {
        for mac in [false, true] {
            assert_eq!(level_con(20, 20, mac), Some(ConColor::White));
            assert_eq!(level_con(20, 22, mac), Some(ConColor::Yellow));
            assert_eq!(level_con(20, 23, mac), Some(ConColor::Red));
            assert_eq!(level_con(20, 19, mac), Some(ConColor::Blue));
            assert_eq!(level_con(20, 0, mac), None);
            assert_eq!(level_con(0, 20, mac), None);
        }
        assert_eq!(level_con(12, 8, false), Some(ConColor::Green));
        assert_eq!(level_con(13, 9, false), Some(ConColor::Blue));
        assert_eq!(level_con(60, 44, false), Some(ConColor::Green));
        assert_eq!(level_con(60, 44, true), Some(ConColor::LightBlue));
        assert_eq!(level_con(60, 39, true), Some(ConColor::Green));
    }
}
