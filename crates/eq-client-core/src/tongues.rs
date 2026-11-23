//! Spoken languages: the tongue a heard line came in, the program's two
//! garble stages, and the tag the line is shown with (parity item 9,
//! recording checklist 6.495).
//!
//! The Titanium program's rules are read (R49-4 (c), R49-4b). A heard
//! channel line in a tongue is garbled as it arrives, once, unless it came
//! on channel 6 or 14, or the player or the sender is a game master:
//!
//! - **Stage 1, by the skill the line carries** (the speaker's): when that
//!   skill is under 100, each word of seven letters or fewer may be dropped
//!   or swapped for a plainer form, then each letter of each word may change
//!   ([`speaker_stage`]). The line is then finished: its last punctuation
//!   kept at the end, no trailing space, the first letter capitalised.
//! - **Stage 2, by the player's own skill**: only when the world turns it on
//!   (Titanium's log-server word at 16, `EQMac`'s `gibberish` at the same
//!   place, read R49-4m) and the player's skill is under 100, each letter
//!   may change again ([`listener_stage`]).
//!
//! The program's own list of short words and its vowel table are not
//! copied: the word list here is this client's own, written fresh, and the
//! vowels are the six letters a, e, i, o, u and y. The program's bad-word
//! mask belongs to a profanity filter this client doesn't have, so it is
//! left out. A program flag read at the gate (0x923d74) isn't labelled and
//! is taken as 0 (inferred). The `EQMac` program's speaker stage isn't read,
//! so the same rules are used there (inferred); its listener stage is read
//! (R49-4m) and its random letter may be a backtick.
use crate::chat::{ChatLine, ItemLink, Message};

/// The common tongue's number.
pub const COMMON: u8 = 0;
/// The number past which the profile holds no language (the program builds
/// its list from languages 0 to 31).
pub const LANGUAGES: usize = 32;

/// The tongue a heard channel line came in, as the server sent it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heard {
    /// The language, by the servers' number.
    pub language: u8,
    /// The skill the line carries in it: the speaker's on TAKP, the lower of
    /// the two on `EQEmu`.
    pub skill: u8,
    /// The channel's wire number.
    pub channel: u32,
    /// Whether the line names its tongue (string 0x583) rather than calling
    /// it unknown (0x584); set as the line is heard ([`hear`]).
    pub named: bool,
}

/// The languages the player knows, in the program's order: each language
/// from 0 to 31 the profile gives skill in (read R49-4 (a)). `/language N`
/// chooses the Nth of these, counting from 1.
#[must_use]
pub fn known(languages: &[u8]) -> Vec<u8> {
    languages
        .iter()
        .take(LANGUAGES)
        .zip(0..)
        .filter(|(skill, _)| **skill > 0)
        .map(|(_, language)| language)
        .collect()
}

/// Which program's letter draw to use: Titanium's random letter is 'a' to
/// 'y'; TAKP's may also be a backtick (read R49-4m).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Letters {
    /// The Titanium program's.
    Titanium,
    /// The `EQMac` program's, as TAKP's reads.
    EqMac,
}

/// Everything the program weighs when it hears a line in a tongue.
#[derive(Clone, Copy, Debug)]
pub struct Hearing<'a> {
    /// The player's name, whose own lines aren't garbled.
    pub player: &'a str,
    /// The player's skill in each language, None when the profile gave none
    /// (then taken as 100, inferred).
    pub own: Option<&'a [u8]>,
    /// The player is a game master.
    pub game_master: bool,
    /// The sender is a game master in the zone, found by name (inferred:
    /// the program's own lookup isn't read).
    pub sender_game_master: bool,
    /// The world turned on the listener stage.
    pub listener_stage: bool,
    /// Which program's letters.
    pub letters: Letters,
}

impl Hearing<'_> {
    /// The player's own skill in a language: 100 when the profile gave no
    /// languages (inferred), 0 for one it lacks.
    #[must_use]
    pub fn own_skill(&self, language: u8) -> u8 {
        self.own.map_or(100, |own| {
            own.get(usize::from(language)).copied().unwrap_or(0)
        })
    }
}

/// Garbles a heard line and tags it as the program does, once, as it
/// arrives. Not the player's own line (inferred: which path the program
/// shows those on isn't read), not one with no tongue.
pub fn hear(line: &mut ChatLine, hearing: &Hearing) {
    let Some(mut heard) = line.tongue else {
        return;
    };
    let own = hearing.own_skill(heard.language);
    // The tag (R49-4 (c)): the tongue's name when the player knows it past
    // 24, on channel 14, or from a game master; else unknown.
    heard.named = own > 24 || heard.channel == 14 || hearing.sender_game_master;
    line.tongue = Some(heard);
    if line
        .sender
        .as_deref()
        .is_some_and(|sender| sender.eq_ignore_ascii_case(hearing.player))
    {
        return;
    }
    // The gate: channels 6 and 14, and a game master on either side.
    if matches!(heard.channel, 6 | 14) || hearing.game_master || hearing.sender_game_master {
        return;
    }
    let mut rolls = Rolls::new(seed(line));
    if heard.skill < 100 {
        speaker_stage(&mut line.message, heard.skill, hearing.letters, &mut rolls);
    }
    if hearing.listener_stage && own < 100 {
        listener_stage(&mut line.message, own, hearing.letters, &mut rolls);
    }
}

/// The seed a line garbles by: its speaker, tongue and words, so one line
/// garbles the same way each time.
#[must_use]
pub fn seed(line: &ChatLine) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.sender.hash(&mut hasher);
    line.tongue
        .map(|heard| (heard.language, heard.skill, heard.channel))
        .hash(&mut hasher);
    line.message.text.hash(&mut hasher);
    hasher.finish()
}

/// The program's rolls, drawn from a seed (xorshift64*).
pub struct Rolls(u64);

impl Rolls {
    /// Rolls from a seed.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32
    }

    /// A roll of 1 to 100.
    fn percent(&mut self) -> u32 {
        u32::try_from(self.next() % 100).unwrap_or(0) + 1
    }

    /// A pick of 0 to `count - 1`.
    fn pick(&mut self, count: usize) -> usize {
        usize::try_from(self.next() % count as u64).unwrap_or(0)
    }
}

/// This client's own short words a poor speaker drops (an empty form) or
/// says plainer, written fresh: the program's list isn't copied (G4).
const SHORT_WORDS: &[(&str, &str)] = &[
    ("the", ""),
    ("a", ""),
    ("an", ""),
    ("of", ""),
    ("to", ""),
    ("is", ""),
    ("are", ""),
    ("am", ""),
    ("be", ""),
    ("and", ""),
    ("i", "me"),
    ("my", "me"),
    ("we", "us"),
    ("our", "us"),
    ("your", "you"),
    ("they", "them"),
    ("their", "them"),
    ("he", "him"),
    ("his", "him"),
    ("she", "her"),
    ("was", "is"),
    ("were", "is"),
    ("has", "have"),
    ("does", "do"),
    ("did", "do"),
];

/// This client's own further words, which join when the roll beats the
/// skill by more than 20.
const MORE_WORDS: &[(&str, &str)] = &[
    ("this", ""),
    ("that", ""),
    ("with", ""),
    ("for", ""),
    ("will", ""),
    ("can", ""),
];

/// The six vowels, written fresh.
const VOWELS: &[u8; 6] = b"aeiouy";

fn is_vowel(letter: u8) -> bool {
    VOWELS.contains(&letter.to_ascii_lowercase())
}

/// One word or one item link of a line.
enum Token {
    Word(String),
    Link(ItemLink),
}

/// Splits a message into its words and links, a link kept whole.
fn tokens(message: &mut Message) -> Vec<Token> {
    let text = std::mem::take(&mut message.text);
    let mut links = std::mem::take(&mut message.item_links);
    links.sort_by_key(|link| link.text_start);
    let mut links = links.into_iter().peekable();
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut index = 0;
    while index < text.len() {
        if let Some(link) = links.next_if(|link| link.text_start == index) {
            if !word.is_empty() {
                tokens.push(Token::Word(std::mem::take(&mut word)));
            }
            index = link.text_end.max(index + 1);
            tokens.push(Token::Link(link));
            continue;
        }
        let Some(character) = text[index..].chars().next() else {
            break;
        };
        index += character.len_utf8();
        if character == ' ' {
            if !word.is_empty() {
                tokens.push(Token::Word(std::mem::take(&mut word)));
            }
        } else {
            word.push(character);
        }
    }
    if !word.is_empty() {
        tokens.push(Token::Word(word));
    }
    // A link the text didn't hold where it said is kept, at the end.
    tokens.extend(links.map(Token::Link));
    tokens
}

/// Joins words and links with one space each, the links' places mended.
fn join(message: &mut Message, tokens: Vec<Token>) {
    let mut text = String::new();
    for token in tokens {
        if !text.is_empty() {
            text.push(' ');
        }
        match token {
            Token::Word(word) => text.push_str(&word),
            Token::Link(mut link) => {
                link.text_start = text.len();
                text.push_str(&link.text);
                link.text_end = text.len();
                message.item_links.push(link);
            }
        }
    }
    message.text = text;
}

/// The plainer form of a short word, keeping a capital first letter.
fn swapped(word: &str, more: bool) -> Option<String> {
    let lower = word.to_ascii_lowercase();
    let (_, form) = SHORT_WORDS
        .iter()
        .chain(more.then_some(MORE_WORDS).into_iter().flatten())
        .find(|(short, _)| *short == lower)?;
    let mut form = (*form).to_owned();
    if word.starts_with(|first: char| first.is_ascii_uppercase())
        && let Some(first) = form.get_mut(..1)
    {
        first.make_ascii_uppercase();
    }
    Some(form)
}

/// The per-letter rule both stages share (R49-4b): a letter is kept unless
/// the first roll passes `threshold` and the second is above 5 and above the
/// skill plus 10; then a 99 or 100 inside a word is a space, a second roll
/// within 30 of the skill a letter of the same kind, and anything else a
/// random letter.
fn letters(word: &mut [u8], skill: u8, threshold: u32, kind: Letters, rolls: &mut Rolls) {
    let skill = u32::from(skill);
    for index in 0..word.len() {
        let letter = word[index];
        if !letter.is_ascii_alphabetic() {
            continue;
        }
        if rolls.percent() < threshold {
            continue;
        }
        let second = rolls.percent();
        if second <= 5 || second <= skill + 10 {
            continue;
        }
        let inside = index > 0
            && word[index - 1] != b' '
            && word.get(index + 1).is_some_and(|next| *next != b' ');
        word[index] = if second >= 99 && inside {
            b' '
        } else if second - skill < 31 {
            if is_vowel(letter) {
                VOWELS[rolls.pick(VOWELS.len())]
            } else {
                loop {
                    let consonant = b'b' + u8::try_from(rolls.pick(24)).unwrap_or(0);
                    if !is_vowel(consonant) {
                        break consonant;
                    }
                }
            }
        } else {
            match kind {
                Letters::Titanium => b'a' + u8::try_from(rolls.pick(25)).unwrap_or(0),
                Letters::EqMac => b'`' + u8::try_from(rolls.pick(26)).unwrap_or(0),
            }
        };
    }
}

fn garble_words(word: &str, skill: u8, threshold: u32, kind: Letters, rolls: &mut Rolls) -> String {
    let mut bytes = word.as_bytes().to_vec();
    if word.is_ascii() {
        letters(&mut bytes, skill, threshold, kind, rolls);
    }
    // Only ASCII letters were swapped for ASCII letters or spaces.
    String::from_utf8(bytes).unwrap_or_else(|_| word.to_owned())
}

/// Stage 1, by the speaker's `skill` (R49-4 (c), R49-4b): word drops and
/// swaps, each surviving word's letters, then the line's finish.
pub fn speaker_stage(message: &mut Message, skill: u8, kind: Letters, rolls: &mut Rolls) {
    let last = message
        .text
        .chars()
        .last()
        .filter(|last| !last.is_alphanumeric() && *last != ' ');
    let ends_in_link = message
        .item_links
        .iter()
        .any(|link| link.text_end >= message.text.len());
    let mut tokens = tokens(message);
    // The last punctuation goes to the end of the finished line.
    let punctuation = match (last, ends_in_link, tokens.last_mut()) {
        (Some(last), false, Some(Token::Word(word))) => {
            word.pop();
            if word.is_empty() {
                tokens.pop();
            }
            Some(last)
        }
        _ => None,
    };
    let skill_now = u32::from(skill);
    let mut kept = Vec::with_capacity(tokens.len());
    for token in tokens {
        let Token::Word(word) = token else {
            kept.push(token);
            continue;
        };
        let word = if word.chars().count() <= 7 {
            let roll = rolls.percent();
            if roll > skill_now {
                match swapped(&word, roll - skill_now > 20) {
                    Some(form) if form.is_empty() => continue,
                    Some(form) => form,
                    None => word,
                }
            } else {
                word
            }
        } else {
            word
        };
        kept.push(Token::Word(garble_words(&word, skill, 41, kind, rolls)));
    }
    join(message, kept);
    if let Some(punctuation) = punctuation {
        message.text.push(punctuation);
    }
    if let Some(first) = message.text.get_mut(..1)
        && !message.item_links.iter().any(|link| link.text_start == 0)
    {
        first.make_ascii_uppercase();
    }
}

/// Stage 2, by the listener's own `skill` (R49-4 (c), R49-4m): each letter
/// outside a link, by the same rule with a first roll of 51 or more.
pub fn listener_stage(message: &mut Message, skill: u8, kind: Letters, rolls: &mut Rolls) {
    let mut links: Vec<_> = message
        .item_links
        .iter()
        .map(|link| link.text_start..link.text_end)
        .collect();
    links.sort_by_key(|link| link.start);
    let mut bytes = std::mem::take(&mut message.text).into_bytes();
    let mut start = 0;
    for link in links
        .iter()
        .cloned()
        .chain(std::iter::once(bytes.len()..bytes.len()))
    {
        let end = link.start.min(bytes.len()).max(start);
        let mut plain = bytes[start..end].to_vec();
        if plain.is_ascii() {
            letters(&mut plain, skill, 51, kind, rolls);
            bytes[start..end].copy_from_slice(&plain);
        }
        start = link.end.max(end);
    }
    // Only ASCII letters were swapped for ASCII letters or spaces.
    message.text = String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
}

#[cfg(test)]
mod tests;
