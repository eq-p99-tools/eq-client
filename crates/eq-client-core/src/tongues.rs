//! Spoken languages: the tongue a heard line came in, how much of it the
//! player follows, and the garble of what they don't (parity item 9,
//! recording checklist 6.495).
//!
//! What the official client does with a heard tongue is not read yet
//! (program read R49-4c). Until then the rule here is inferred: the skill
//! that counts is the lower of the one the line carries and the player's
//! own, so it reads alike from `EQEmu` (which already sends the lower of the
//! two, and 27 for a tongue the listener barely knows) and TAKP (which sends
//! the speaker's); each letter is then swapped as `EQEmu`'s own garble for
//! books swaps it (`Client::GarbleMessage`), with a chance of
//! (101 - skill) in 116: a draw of 0 to 115 at most 100 - skill.
use crate::chat::{ChatLine, Message};

/// The common tongue's number.
pub const COMMON: u8 = 0;
/// The number `EQEmu` sends for a tongue the listener knows too little of.
pub const UNKNOWN: u8 = 27;
/// The highest number a language the player can choose has (1 to 24 are the
/// others the profile lists).
pub const LAST_CHOSEN: u8 = 24;

/// The tongue a heard channel line came in, as the server sent it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heard {
    /// The language, by the servers' number.
    pub language: u8,
    /// The skill the line carries in it.
    pub skill: u8,
}

/// How much of a heard line the player follows, 0 to 100: all of it for the
/// common tongue whatever skill the line carries (servers send their own
/// lines as channel messages), else the lower of the line's skill and the
/// player's own, which is 0 for a language the profile lacks and for
/// `EQEmu`'s unknown tongue. Inferred, pending R49-4c.
#[must_use]
pub fn followed(heard: Heard, own: Option<&[u8]>) -> u8 {
    if heard.language == COMMON {
        return 100;
    }
    let own = own
        .and_then(|languages| languages.get(usize::from(heard.language)))
        .copied()
        .unwrap_or(0);
    heard.skill.min(own).min(100)
}

/// The letters `EQEmu`'s garble swaps in.
const LETTERS: &[u8; 52] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Garbles a heard line for a player who follows `followed` of its tongue:
/// nothing at 100; below, each ASCII letter outside the item links' text is
/// swapped with a chance of (101 - followed) in 116 (`EQEmu`'s draw of 0 to
/// 115, taken when at most 100 - followed). The draw is seeded from
/// the line, so one line garbles the same way each time; the length stays,
/// so the links' places hold.
pub fn garble(message: &mut Message, followed: u8, seed: u64) {
    if followed >= 100 {
        return;
    }
    let variance = u64::from(100 - followed);
    let mut state = seed | 1;
    let mut next = || {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32
    };
    let links: Vec<_> = message
        .item_links
        .iter()
        .map(|link| link.text_start..link.text_end)
        .collect();
    let mut bytes = std::mem::take(&mut message.text).into_bytes();
    for (index, byte) in bytes.iter_mut().enumerate() {
        if !byte.is_ascii_alphabetic() || links.iter().any(|link| link.contains(&index)) {
            continue;
        }
        if next() % 116 <= variance {
            let pick = next() % 52;
            *byte = LETTERS
                .iter()
                .zip(0..)
                .find_map(|(letter, index)| (index == pick).then_some(*letter))
                .unwrap_or(*byte);
        }
    }
    // Only ASCII letters were swapped for ASCII letters.
    message.text = String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
}

/// The seed a line garbles by: its speaker, tongue and words.
#[must_use]
pub fn seed(line: &ChatLine) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.sender.hash(&mut hasher);
    line.tongue
        .map(|heard| (heard.language, heard.skill))
        .hash(&mut hasher);
    line.message.text.hash(&mut hasher);
    hasher.finish()
}

/// Garbles a heard line as the player follows it, once, as it arrives: not
/// the player's own line, not one in the common tongue, not one with no
/// tongue. The chat and the log then show the same words.
pub fn hear(line: &mut ChatLine, player: &str, own: Option<&[u8]>) {
    let Some(heard) = line.tongue else {
        return;
    };
    if line
        .sender
        .as_deref()
        .is_some_and(|sender| sender.eq_ignore_ascii_case(player))
    {
        return;
    }
    let seed = seed(line);
    garble(&mut line.message, followed(heard, own), seed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{ChannelName, ItemLink, Source};

    fn said(text: &str, language: u8, skill: u8) -> ChatLine {
        ChatLine {
            channel: ChannelName::Say,
            sender: Some("Examplar".into()),
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: text.into(),
                item_links: Vec::new(),
            },
            source: Source::Server,
            message_type: None,
            tongue: Some(Heard { language, skill }),
        }
    }

    #[test]
    fn the_common_tongue_is_followed_whatever_skill_it_carries() {
        let common = Heard {
            language: COMMON,
            skill: 0,
        };
        assert_eq!(followed(common, None), 100);
        let mut line = said("Welcome to the zone", COMMON, 0);
        hear(&mut line, "Tester", Some(&[0; 25]));
        assert_eq!(line.message.text, "Welcome to the zone");
    }

    #[test]
    fn the_lower_of_the_lines_skill_and_the_players_counts() {
        let own = [100, 0, 0, 40];
        let elvish = |skill| Heard { language: 3, skill };
        assert_eq!(followed(elvish(100), Some(&own)), 40);
        assert_eq!(followed(elvish(12), Some(&own)), 12);
        // A language the profile lacks, and EQEmu's unknown tongue.
        assert_eq!(
            followed(
                Heard {
                    language: 9,
                    skill: 100
                },
                Some(&own)
            ),
            0
        );
        assert_eq!(
            followed(
                Heard {
                    language: UNKNOWN,
                    skill: 100
                },
                Some(&own)
            ),
            0
        );
        assert_eq!(followed(elvish(100), None), 0);
    }

    #[test]
    fn a_line_fully_followed_is_untouched_and_one_not_followed_garbles_its_letters_only() {
        let mut line = said("Hail, friend! 42", 3, 100);
        hear(&mut line, "Tester", Some(&[100, 0, 0, 100]));
        assert_eq!(line.message.text, "Hail, friend! 42");

        let text = "Mae govannen, friend of the forest! 1 2 3".repeat(4);
        let mut line = said(&text, 3, 100);
        hear(&mut line, "Tester", Some(&[100]));
        assert_ne!(line.message.text, text);
        assert_eq!(line.message.text.len(), text.len());
        for (heard, sent) in line.message.text.bytes().zip(text.bytes()) {
            assert_eq!(heard.is_ascii_alphabetic(), sent.is_ascii_alphabetic());
            if !sent.is_ascii_alphabetic() {
                assert_eq!(heard, sent);
            }
        }
    }

    #[test]
    fn a_letter_swaps_with_a_chance_of_101_less_the_skill_in_116() {
        let text = "abcdefghij".repeat(400);
        let changed = |followed| {
            let mut message = said(&text, 3, 100).message;
            garble(&mut message, followed, 0x5eed);
            message
                .text
                .bytes()
                .zip(text.bytes())
                .filter(|(heard, sent)| heard != sent)
                .count()
        };
        // A swap may draw the same letter (1 in 52), so the share changed is
        // the chance times 51/52: about 0.85 at no skill, 0.017 at 99.
        let none = changed(0);
        assert!((3_200..=3_600).contains(&none), "{none}");
        let nearly = changed(99);
        assert!((20..=120).contains(&nearly), "{nearly}");
        assert_eq!(changed(100), 0);
    }

    #[test]
    fn a_line_garbles_the_same_way_each_time() {
        let once = || {
            let mut line = said("The same words spoken twice", 5, 30);
            hear(&mut line, "Tester", Some(&[100, 0, 0, 0, 0, 60]));
            line.message.text
        };
        assert_eq!(once(), once());
    }

    #[test]
    fn the_players_own_line_and_link_text_are_never_garbled() {
        let mut own = said("my own words", 3, 1);
        own.sender = Some("tester".into());
        hear(&mut own, "Tester", Some(&[100]));
        assert_eq!(own.message.text, "my own words");

        let text = "look Rusty Dagger here".repeat(3);
        let mut line = said(&text, 3, 0);
        line.message.item_links.push(ItemLink {
            body: "synthetic".into(),
            text: "Rusty Dagger".into(),
            item_id: 7,
            start: 0,
            end: 0,
            text_start: 5,
            text_end: 17,
        });
        hear(&mut line, "Tester", Some(&[100]));
        assert_eq!(&line.message.text[5..17], "Rusty Dagger");
        // At no skill nearly every other letter changes.
        assert_ne!(&line.message.text[..4], "look");
    }
}
