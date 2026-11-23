use super::*;
use std::collections::BTreeMap;

/// A string table of made-up words in the installed layout: a no-target
/// line starts its verb against `%1`, a with-target line puts a space
/// after it.
struct Table(BTreeMap<u32, &'static str>);

impl Strings for Table {
    fn line(&self, id: u32, arguments: &[String]) -> Option<String> {
        let mut text = (*self.0.get(&id)?).to_owned();
        for (index, argument) in arguments.iter().enumerate().rev() {
            text = text.replace(&format!("%{}", index + 1), argument);
        }
        Some(text)
    }

    fn word(&self, id: u32) -> Option<String> {
        self.0.get(&id).map(|word| (*word).to_owned())
    }
}

fn table() -> Table {
    Table(BTreeMap::from([
        (0x30d4, "%1grins"),
        (0x30d5, "%1 grins at %2"),
        (0x319c, "Grin"),
        (0x319d, "Grin at %1"),
        // /boggle: own pronoun words.
        (0x3105, "%1scratches %2 head"),
        (0x31cd, "Scratch %1 head"),
        (0x2fb1, "W1"),
        (0x2fb2, "W2"),
        (0x300d, "R0"),
        (0x300b, "R1"),
        (0x300e, "Thou"),
        (0x300f, "thy"),
        (0x3010, "thyself"),
        // /dance with a target: the target's second word.
        (0x3120, "%1 twirls %2 round %3"),
        (0x31e8, "%1 twirl %2 round %3"),
        (0x3009, "O0"),
        (0x300a, "O1"),
        (0x3011, "Thou %1"),
        (0x317a, "%1 nudges %2"),
        (0x3242, "Nudge %1"),
        (0x317b, "%1fidgets"),
        (0x3243, "Fidget"),
    ]))
}

#[test]
fn the_table_holds_every_case_once_in_order() {
    assert_eq!(EMOTES.len(), 88);
    for (index, emote) in EMOTES.iter().enumerate() {
        assert_eq!(usize::from(emote.case), index);
        assert_eq!(named(emote.name), Some(emote));
        assert!(
            emote.sent[1].is_some() || emote.case == 0x54,
            "{}",
            emote.name
        );
        assert!(
            emote.sent[0].is_some() || emote.case == 0x53,
            "{}",
            emote.name
        );
        assert_eq!(emote.sent[0].is_none(), emote.local[0].is_none());
        assert_eq!(emote.sent[1].is_none(), emote.local[1].is_none());
    }
    let still: Vec<_> = EMOTES
        .iter()
        .filter(|emote| emote.stands_still)
        .map(|emote| emote.name)
        .collect();
    assert_eq!(still, ["frown", "wave", "cheer", "gesture"]);
    assert_eq!(
        EMOTES
            .iter()
            .filter(|emote| emote.animation.is_some())
            .count(),
        63
    );
    // R47b's corrected order for the cases that end in `break`.
    assert_eq!(named("agree").unwrap().sent[1].unwrap().id, 0x30f2);
    assert_eq!(named("agree").unwrap().sent[0].unwrap().id, 0x30f3);
    assert_eq!(named("smile").unwrap().sent[0].unwrap().id, 0x30d4);
    assert_eq!(named("hail"), None);
}

#[test]
fn the_sent_text_has_no_name_and_the_own_line_is_the_second_set() {
    let smile = named("smile").unwrap();
    let spoken = speak(smile, 0, None, "", &table()).unwrap();
    assert_eq!(spoken.sent, " grins.");
    assert_eq!(spoken.local, "Grin.");
    assert!(spoken.official);
    let target = Target {
        name: "Fippy",
        gender: 1,
    };
    let spoken = speak(smile, 0, Some(target), "", &table()).unwrap();
    assert_eq!(spoken.sent, " grins at Fippy.");
    assert_eq!(spoken.local, "Grin at Fippy.");
    // Pronoun words by the player's gender in the sent text, the
    // second-person words in the player's own line.
    let boggle = named("boggle").unwrap();
    let spoken = speak(boggle, 1, None, "", &table()).unwrap();
    assert_eq!(spoken.sent, " scratches R1 head.");
    assert_eq!(spoken.local, "Scratch thyself head.");
    // The target's own words come from the target's gender.
    let dance = named("dance").unwrap();
    let spoken = speak(dance, 0, Some(target), "", &table()).unwrap();
    assert_eq!(spoken.sent, " twirls Fippy round O1.");
    assert_eq!(spoken.local, "Thou twirl Fippy round O1.");
}

#[test]
fn extra_words_replace_the_lines_and_one_character_does_not() {
    let smile = named("smile").unwrap();
    let target = Target {
        name: "Fippy",
        gender: 0,
    };
    let spoken = speak(smile, 0, Some(target), "  waves hello ", &table()).unwrap();
    assert_eq!(spoken.sent, " waves hello.");
    assert_eq!(spoken.local, "Thou waves hello.");
    let spoken = speak(smile, 0, Some(target), "x", &table()).unwrap();
    assert_eq!(spoken.sent, " grins at Fippy.");
    // Ending in punctuation already, no full stop is added.
    let spoken = speak(smile, 0, None, "hums!", &table()).unwrap();
    assert_eq!(spoken.sent, " hums!.");
}

#[test]
fn nudge_has_only_a_target_line_and_fidget_only_a_lone_one() {
    let nudge = named("nudge").unwrap();
    let spoken = speak(nudge, 0, None, "", &table()).unwrap();
    // The empty name leaves a trailing space, so no full stop.
    assert_eq!(spoken.sent, " nudges ");
    assert_eq!(spoken.local, "Nudge ");
    let fidget = named("fidget").unwrap();
    let target = Target {
        name: "Fippy",
        gender: 0,
    };
    assert_eq!(speak(fidget, 0, Some(target), "", &table()), None);
    assert_eq!(
        speak(fidget, 0, None, "", &table()).unwrap().sent,
        " fidgets."
    );
}

#[test]
fn a_missing_installation_takes_this_clients_words() {
    let empty = Table(BTreeMap::new());
    let bow = named("bow").unwrap();
    let spoken = speak(bow, 0, None, "", &empty).unwrap();
    assert_eq!(spoken.sent, " uses /bow.");
    assert_eq!(spoken.local, "You use /bow.");
    assert!(!spoken.official);
    let target = Target {
        name: "Fippy",
        gender: 0,
    };
    let spoken = speak(bow, 0, Some(target), "", &empty).unwrap();
    assert_eq!(spoken.sent, " uses /bow on Fippy.");
    assert_eq!(spoken.local, "You use /bow on Fippy.");
    let spoken = speak(bow, 0, Some(target), "salutes", &empty).unwrap();
    assert_eq!(spoken.local, "You salutes.");
}

#[test]
fn pronoun_sets_follow_the_gender() {
    assert_eq!(pronouns(0), [0x2fb1, 0x3009, 0x300d]);
    assert_eq!(pronouns(1), [0x2fb2, 0x300a, 0x300b]);
    assert_eq!(pronouns(2), [0x2fb3, 0x2fb3, 0x300c]);
    assert_eq!(pronouns(7), pronouns(2));
}
