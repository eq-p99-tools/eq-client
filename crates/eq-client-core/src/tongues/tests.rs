use super::*;
use crate::chat::{ChannelName, Source};

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
        tongue: Some(Heard {
            language,
            skill,
            channel: 8,
            named: false,
        }),
    }
}

fn hearing(own: &[u8]) -> Hearing<'_> {
    Hearing {
        player: "Tester",
        own: Some(own),
        game_master: false,
        sender_game_master: false,
        listener_stage: false,
        letters: Letters::Titanium,
    }
}

fn link(text: &str, start: usize) -> ItemLink {
    ItemLink {
        body: "synthetic".into(),
        text: text.into(),
        item_id: 7,
        start: 0,
        end: 0,
        text_start: start,
        text_end: start + text.len(),
    }
}

#[test]
fn the_known_list_counts_languages_with_skill_in_order() {
    assert_eq!(known(&[100, 0, 0, 25, 0, 1]), [0, 3, 5]);
    assert_eq!(known(&[0; 40]), Vec::<u8>::new());
    let mut wide = vec![0; 40];
    wide[31] = 5;
    wide[33] = 5;
    assert_eq!(known(&wide), [31]);
}

#[test]
fn a_line_at_full_skill_with_the_listener_stage_off_is_untouched() {
    let mut line = said("hail, friend of the forest!", 3, 100);
    hear(&mut line, &hearing(&[100, 0, 0, 10]));
    assert_eq!(line.message.text, "hail, friend of the forest!");
    // Language 0 garbles like any other at a low skill (L1 dropped).
    let text = "the quick brown fox jumps over the lazy dog".repeat(3);
    let mut line = said(&text, COMMON, 1);
    hear(&mut line, &hearing(&[100]));
    assert_ne!(line.message.text, text);
}

#[test]
fn the_speaker_stage_drops_and_swaps_short_words_and_finishes_the_line() {
    // A word list of only listed words at skill 0: every roll beats it, so
    // dropped words go and swapped ones read plainer.
    let mut message = said("the a an of to is", 3, 0).message;
    speaker_stage(&mut message, 0, Letters::Titanium, &mut Rolls::new(1));
    assert_eq!(message.text, "");
    // At skill 99 no word roll (1 to 100) beats it but a 100, and no
    // letter roll passes 99 + 10: the line is only finished.
    let mut message = said("hello there friend.", 3, 99).message;
    speaker_stage(&mut message, 99, Letters::Titanium, &mut Rolls::new(7));
    assert!(message.text.starts_with('H'), "{}", message.text);
    assert!(message.text.ends_with('.'), "{}", message.text);
    assert!(!message.text.ends_with(' '));
}

#[test]
fn a_swapped_word_keeps_its_capital() {
    assert_eq!(swapped("They", false).as_deref(), Some("Them"));
    assert_eq!(swapped("this", false), None);
    assert_eq!(swapped("this", true).as_deref(), Some(""));
}

#[test]
fn letters_change_only_past_both_rolls() {
    // Skill 90 or more can't pass the second roll (above 100 is needed).
    let text = "abcdefghij".repeat(200);
    let changed = |skill, stage| {
        let mut word = text.clone().into_bytes();
        letters(
            &mut word,
            skill,
            stage,
            Letters::Titanium,
            &mut Rolls::new(0x5eed),
        );
        word.iter()
            .zip(text.bytes())
            .filter(|(a, b)| **a != *b)
            .count()
    };
    assert_eq!(changed(90, Stage::Speaker), 0);
    // At skill 0 the first roll passes 60 in 100 (41 to 100) and the
    // second 95 in 100 (6 to 100); a same-kind letter may come out the same.
    let low = changed(0, Stage::Speaker);
    assert!((900..=1_200).contains(&low), "{low}");
    // The listener's first roll passes 50 in 100.
    let listener = changed(0, Stage::Listener);
    assert!(listener < low, "{listener} {low}");
}

#[test]
fn a_random_letter_may_be_a_backtick_but_in_titaniums_listener_stage() {
    let text = "z".repeat(4_000);
    let draw = |stage, kind| {
        let mut word = text.clone().into_bytes();
        letters(&mut word, 0, stage, kind, &mut Rolls::new(3));
        word
    };
    let plain = |word: &[u8]| {
        word.iter()
            .all(|letter| *letter == b' ' || *letter == b'z' || (b'`'..=b'y').contains(letter))
    };
    // Titanium's listener stage: 'a' to 'y' only (R49-4).
    let listener = draw(Stage::Listener, Letters::Titanium);
    assert!(plain(&listener));
    assert!(!listener.contains(&b'`'));
    // Its speaker stage draws from the backtick (R49-4b), and so does the
    // Mac's in both (R49-4m, and inferred for its speaker stage).
    for (stage, kind) in [
        (Stage::Speaker, Letters::Titanium),
        (Stage::Speaker, Letters::EqMac),
        (Stage::Listener, Letters::EqMac),
    ] {
        let word = draw(stage, kind);
        assert!(plain(&word));
        assert!(word.contains(&b'`'), "{stage:?} {kind:?}");
    }
}

#[test]
fn a_space_may_take_a_words_last_letter_but_never_its_first() {
    // Every roll of 99 or 100 past the first letter, with no space beside
    // it, may make a space, the last letter included (R49-4b).
    let mut spaced_last = false;
    for seed in 0..400 {
        let mut word = b"zzzz".to_vec();
        letters(
            &mut word,
            0,
            Stage::Speaker,
            Letters::Titanium,
            &mut Rolls::new(seed),
        );
        assert_ne!(word[0], b' ');
        spaced_last |= word[3] == b' ' && word[2] != b' ';
    }
    assert!(spaced_last);
}

#[test]
fn the_gate_skips_channels_6_and_14_and_game_masters() {
    let text = "the quick brown fox jumps over the lazy dog".repeat(3);
    for channel in [6, 14] {
        let mut line = said(&text, 3, 0);
        line.tongue.as_mut().unwrap().channel = channel;
        hear(&mut line, &hearing(&[100]));
        assert_eq!(line.message.text, text);
    }
    let mut line = said(&text, 3, 0);
    hear(
        &mut line,
        &Hearing {
            game_master: true,
            ..hearing(&[100])
        },
    );
    assert_eq!(line.message.text, text);
    let mut line = said(&text, 3, 0);
    hear(
        &mut line,
        &Hearing {
            sender_game_master: true,
            ..hearing(&[100])
        },
    );
    assert_eq!(line.message.text, text);
    // The player's own line isn't garbled.
    let mut line = said(&text, 3, 0);
    line.sender = Some("tester".into());
    hear(&mut line, &hearing(&[100]));
    assert_eq!(line.message.text, text);
}

#[test]
fn the_listener_stage_runs_only_when_the_world_turns_it_on() {
    let text = "Mae govannen friend of the forest".repeat(4);
    let heard = |listener_stage, own: &[u8]| {
        let mut line = said(&text, 3, 100);
        hear(
            &mut line,
            &Hearing {
                listener_stage,
                ..hearing(own)
            },
        );
        line.message.text
    };
    assert_eq!(heard(false, &[100, 0, 0, 0]), text);
    let garbled = heard(true, &[100, 0, 0, 0]);
    assert_ne!(garbled, text);
    assert_eq!(garbled.len(), text.len());
    assert_eq!(heard(true, &[100, 0, 0, 0]), garbled);
    assert_eq!(heard(true, &[100, 0, 0, 100]), text);
    // No profile languages: the player's skill counts as 100.
    let mut line = said(&text, 3, 100);
    hear(
        &mut line,
        &Hearing {
            own: None,
            listener_stage: true,
            ..hearing(&[])
        },
    );
    assert_eq!(line.message.text, text);
}

#[test]
fn links_are_kept_whole_in_both_stages() {
    let text = "look Rusty Dagger here and the rest of it";
    let mut line = said(text, 3, 0);
    line.message.item_links.push(link("Rusty Dagger", 5));
    hear(
        &mut line,
        &Hearing {
            listener_stage: true,
            ..hearing(&[100, 0, 0, 0])
        },
    );
    let kept = &line.message.item_links[0];
    assert_eq!(
        &line.message.text[kept.text_start..kept.text_end],
        "Rusty Dagger"
    );
}

#[test]
fn the_tag_names_a_tongue_known_past_24_on_channel_14_or_from_a_game_master() {
    let named = |own: &[u8], channel, sender_game_master| {
        let mut line = said("hail", 3, 100);
        line.tongue.as_mut().unwrap().channel = channel;
        hear(
            &mut line,
            &Hearing {
                sender_game_master,
                ..hearing(own)
            },
        );
        line.tongue.unwrap().named
    };
    assert!(named(&[100, 0, 0, 25], 8, false));
    assert!(!named(&[100, 0, 0, 24], 8, false));
    assert!(named(&[100, 0, 0, 0], 14, false));
    assert!(named(&[100, 0, 0, 0], 8, true));
}
