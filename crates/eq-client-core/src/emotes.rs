//! The 88 social emotes Titanium's chat runs, as its worker 0x49e0c1 does
//! them (reads R47 and R47b): `/smile`, `/bow`, `/wave` and the rest, each a
//! case of one switch. Names, case numbers, animation numbers and string ids
//! only; every line comes from the installed string table at run time, and
//! this client's own words stand in only where the installation lacks one.
//!
//! The worker formats each emote twice. The text it sends has an empty
//! name slot, so it starts with a space and the server puts the sender's
//! name in front; the line the player sees is a second line set, in the
//! second person.

/// What fills a line's `%1`, `%2` and so on, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// The player: empty in the sent text, word 0x300e in the player's own
    /// line.
    Own,
    /// The target's name.
    Name,
    /// The player's first pronoun word (0x2fb1 to 0x2fb3 by gender; 0x300f
    /// in the player's own line). Inferred: a possessive.
    OwnP1,
    /// The player's second pronoun word (0x3009, 0x300a or 0x2fb3; 0x300f
    /// in the player's own line). Inferred: an object word.
    OwnP2,
    /// The player's third pronoun word (0x300d, 0x300b or 0x300c; 0x3010
    /// in the player's own line). Inferred: a reflexive.
    OwnP3,
    /// The target's first pronoun word, by the target's gender.
    TargetP1,
    /// The target's second pronoun word.
    TargetP2,
    /// The target's third pronoun word.
    TargetP3,
}

use Slot::{Name, Own, OwnP1, OwnP2, OwnP3, TargetP1, TargetP2, TargetP3};

/// A line of the installed string table and what fills its places.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Line {
    /// The string's id.
    pub id: u32,
    /// What fills `%1`, `%2` and so on.
    pub slots: &'static [Slot],
}

#[allow(clippy::unnecessary_wraps)] // The table holds optional lines; this fills one.
const fn line(id: u32, slots: &'static [Slot]) -> Option<Line> {
    Some(Line { id, slots })
}

/// One emote command: a case of the worker's switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Emote {
    /// The case number, 0x00 to 0x57.
    pub case: u8,
    /// The command's name as the command table resolves it, without the
    /// slash (`/gesture` also answers to its other names there).
    pub name: &'static str,
    /// The servers' animation number sent and played with it, if it has one.
    pub animation: Option<u8>,
    /// Whether its own stub asks the player to stand still first (0x330d):
    /// `/frown`, `/wave`, `/cheer` and `/gesture` (R47b g).
    pub stands_still: bool,
    /// The sent text's line without a target, then with one.
    pub sent: [Option<Line>; 2],
    /// The player's own line without a target, then with one.
    pub local: [Option<Line>; 2],
}

const fn emote(
    case: u8,
    name: &'static str,
    animation: Option<u8>,
    stands_still: bool,
    sent: [Option<Line>; 2],
    local: [Option<Line>; 2],
) -> Emote {
    Emote {
        case,
        name,
        animation,
        stands_still,
        sent,
        local,
    }
}

/// Every emote, by case number (R47b's corrected table).
pub const EMOTES: [Emote; 88] = [
    emote(
        0x0,
        "smile",
        Some(0x4d),
        false,
        [line(0x30d4, &[Own]), line(0x30d5, &[Own, Name])],
        [line(0x319c, &[]), line(0x319d, &[Name])],
    ),
    emote(
        0x1,
        "frown",
        Some(0x1c),
        true,
        [line(0x30d6, &[Own]), line(0x30d7, &[Own, Name])],
        [line(0x319e, &[]), line(0x319f, &[Name])],
    ),
    emote(
        0x2,
        "laugh",
        Some(0x3f),
        false,
        [line(0x30d8, &[Own]), line(0x30d9, &[Own, Name])],
        [line(0x31a0, &[]), line(0x31a1, &[Name])],
    ),
    emote(
        0x3,
        "cry",
        Some(0x1c),
        false,
        [line(0x30da, &[Own]), line(0x30db, &[Own, Name])],
        [line(0x31a2, &[]), line(0x31a3, &[Name])],
    ),
    emote(
        0x4,
        "chuckle",
        Some(0x36),
        false,
        [line(0x30dc, &[Own]), line(0x30dd, &[Own, Name])],
        [line(0x31a4, &[]), line(0x31a5, &[Name])],
    ),
    emote(
        0x5,
        "hug",
        None,
        false,
        [line(0x30de, &[Own, OwnP2]), line(0x30df, &[Own, Name])],
        [line(0x31a6, &[]), line(0x31a7, &[Name])],
    ),
    emote(
        0x6,
        "poke",
        None,
        false,
        [line(0x30e0, &[Own, OwnP2]), line(0x30e1, &[Own, Name])],
        [line(0x31a8, &[]), line(0x31a9, &[Name])],
    ),
    emote(
        0x7,
        "bow",
        Some(0x46),
        false,
        [line(0x30e2, &[Own]), line(0x30e3, &[Own, Name])],
        [line(0x31aa, &[]), line(0x31ab, &[Name])],
    ),
    emote(
        0x8,
        "bounce",
        None,
        false,
        [line(0x30e4, &[Own]), line(0x30e5, &[Own, Name])],
        [line(0x31ac, &[]), line(0x31ad, &[Name])],
    ),
    emote(
        0x9,
        "panic",
        None,
        false,
        [line(0x30e6, &[Own]), line(0x30e7, &[Own, Name])],
        [line(0x31ae, &[]), line(0x31af, &[Name])],
    ),
    emote(
        0xa,
        "nod",
        Some(0x30),
        false,
        [line(0x30e8, &[Own]), line(0x30e9, &[Own, Name])],
        [line(0x31b0, &[]), line(0x31b1, &[Name])],
    ),
    emote(
        0xb,
        "rofl",
        None,
        false,
        [line(0x30ea, &[Own]), line(0x30eb, &[Own, Name])],
        [line(0x31b2, &[]), line(0x31b3, &[Name])],
    ),
    emote(
        0xc,
        "wave",
        Some(0x1d),
        true,
        [line(0x30ec, &[Own]), line(0x30ed, &[Own, Name])],
        [line(0x31b4, &[]), line(0x31b5, &[Name])],
    ),
    emote(
        0xd,
        "cheer",
        Some(0x1b),
        true,
        [line(0x30ee, &[Own]), line(0x30ef, &[Own, Name])],
        [line(0x31b6, &[]), line(0x31b7, &[Name])],
    ),
    emote(
        0xe,
        "gesture",
        Some(0x1e),
        true,
        [line(0x30f0, &[Own]), line(0x30f1, &[Own, Name])],
        [line(0x31b8, &[]), line(0x31b9, &[Name])],
    ),
    emote(
        0xf,
        "agree",
        Some(0x30),
        false,
        [line(0x30f3, &[Own, OwnP2]), line(0x30f2, &[Own, Name])],
        [line(0x31bb, &[]), line(0x31ba, &[Name])],
    ),
    emote(
        0x10,
        "amaze",
        Some(0x31),
        false,
        [line(0x30f5, &[Own]), line(0x30f4, &[Own, Name])],
        [line(0x31bd, &[]), line(0x31bc, &[Own, Name])],
    ),
    emote(
        0x11,
        "apologize",
        Some(0x32),
        false,
        [line(0x30f7, &[Own]), line(0x30f6, &[Own, Name])],
        [line(0x31bf, &[]), line(0x31be, &[Own, Name])],
    ),
    emote(
        0x12,
        "applaud",
        Some(0x33),
        false,
        [line(0x30f9, &[Own]), line(0x30f8, &[Own, Name])],
        [line(0x31c1, &[]), line(0x31c0, &[Own, Name])],
    ),
    emote(
        0x13,
        "plead",
        Some(0x32),
        false,
        [line(0x30fb, &[Own, OwnP2]), line(0x30fa, &[Own, Name])],
        [line(0x31c3, &[]), line(0x31c2, &[Own, Name])],
    ),
    emote(
        0x14,
        "bite",
        None,
        false,
        [line(0x30fd, &[Own]), line(0x30fc, &[Own, Name])],
        [line(0x31c5, &[]), line(0x31c4, &[Own, Name])],
    ),
    emote(
        0x15,
        "bleed",
        Some(0x34),
        false,
        [line(0x30ff, &[Own]), line(0x30fe, &[Own, Name])],
        [line(0x31c7, &[]), line(0x31c6, &[Name])],
    ),
    emote(
        0x16,
        "blink",
        Some(0x3b),
        false,
        [line(0x3101, &[Own]), line(0x3100, &[Own, Name])],
        [line(0x31c9, &[]), line(0x31c8, &[Own, Name])],
    ),
    emote(
        0x17,
        "blush",
        Some(0x35),
        false,
        [line(0x3103, &[Own]), line(0x3102, &[Own, Name])],
        [line(0x31cb, &[]), line(0x31ca, &[Name])],
    ),
    emote(
        0x18,
        "boggle",
        Some(0x31),
        false,
        [
            line(0x3105, &[Own, OwnP3]),
            line(0x3104, &[Own, Name, OwnP3]),
        ],
        [line(0x31cd, &[OwnP3]), line(0x31cc, &[Own, Name, OwnP3])],
    ),
    emote(
        0x19,
        "bonk",
        None,
        false,
        [line(0x3107, &[Own]), line(0x3106, &[Own, Name])],
        [line(0x31cf, &[]), line(0x31ce, &[Own, Name])],
    ),
    emote(
        0x1a,
        "bored",
        Some(0x1f),
        false,
        [
            line(0x3109, &[Own, OwnP1]),
            line(0x3108, &[Own, Name, OwnP1]),
        ],
        [line(0x31d1, &[OwnP1]), line(0x31d0, &[Own, Name, OwnP1])],
    ),
    emote(
        0x1b,
        "brb",
        Some(0x1d),
        false,
        [
            line(0x310b, &[Own, OwnP1]),
            line(0x310a, &[Own, Name, OwnP1]),
        ],
        [line(0x31d3, &[OwnP1]), line(0x31d2, &[Own, Name, OwnP1])],
    ),
    emote(
        0x1c,
        "burp",
        Some(0x37),
        false,
        [line(0x310d, &[Own]), line(0x310c, &[Own, Name])],
        [line(0x31d5, &[]), line(0x31d4, &[Name])],
    ),
    emote(
        0x1d,
        "bye",
        Some(0x1d),
        false,
        [line(0x310f, &[Own]), line(0x310e, &[Own, Name])],
        [line(0x31d7, &[]), line(0x31d6, &[Name])],
    ),
    emote(
        0x1e,
        "cackle",
        Some(0x36),
        false,
        [line(0x3111, &[Own]), line(0x3110, &[Own, Name])],
        [line(0x31d9, &[]), line(0x31d8, &[Name])],
    ),
    emote(
        0x1f,
        "calm",
        None,
        false,
        [line(0x3113, &[Own]), line(0x3112, &[Own, Name])],
        [line(0x31db, &[]), line(0x31da, &[Name])],
    ),
    emote(
        0x20,
        "clap",
        Some(0x33),
        false,
        [line(0x3115, &[Own, OwnP3]), line(0x3114, &[Own, Name])],
        [line(0x31dd, &[OwnP3]), line(0x31dc, &[Own, Name])],
    ),
    emote(
        0x21,
        "comfort",
        None,
        false,
        [line(0x3117, &[Own]), line(0x3116, &[Own, Name])],
        [line(0x31df, &[]), line(0x31de, &[Name])],
    ),
    emote(
        0x22,
        "congratulate",
        Some(0x1b),
        false,
        [line(0x3119, &[Own, OwnP2]), line(0x3118, &[Own, Name])],
        [line(0x31e1, &[OwnP2]), line(0x31e0, &[Own, Name])],
    ),
    emote(
        0x23,
        "cough",
        Some(0x37),
        false,
        [line(0x311b, &[Own]), line(0x311a, &[Own, Name])],
        [line(0x31e3, &[]), line(0x31e2, &[Name])],
    ),
    emote(
        0x24,
        "cringe",
        Some(0x38),
        false,
        [line(0x311d, &[Own]), line(0x311c, &[Own, Name])],
        [line(0x31e5, &[]), line(0x31e4, &[Name])],
    ),
    emote(
        0x25,
        "curious",
        Some(0x39),
        false,
        [line(0x311f, &[Own, OwnP2]), line(0x311e, &[Own, Name])],
        [line(0x31e7, &[OwnP2]), line(0x31e6, &[Own, Name])],
    ),
    emote(
        0x26,
        "dance",
        Some(0x3a),
        false,
        [
            line(0x3121, &[Own, OwnP3]),
            line(0x3120, &[Own, Name, TargetP2]),
        ],
        [line(0x31e9, &[OwnP3]), line(0x31e8, &[Own, Name, TargetP2])],
    ),
    emote(
        0x27,
        "drool",
        Some(0x3d),
        false,
        [line(0x3123, &[Own, OwnP2]), line(0x3122, &[Own, Name])],
        [line(0x31eb, &[OwnP2]), line(0x31ea, &[Name])],
    ),
    emote(
        0x28,
        "duck",
        Some(0x38),
        false,
        [line(0x3125, &[Own]), line(0x3124, &[Own, Name])],
        [line(0x31ed, &[]), line(0x31ec, &[Name])],
    ),
    emote(
        0x29,
        "eye",
        Some(0x3c),
        false,
        [line(0x3127, &[Own]), line(0x3126, &[Own, Name])],
        [line(0x31ef, &[]), line(0x31ee, &[Name])],
    ),
    emote(
        0x2a,
        "gasp",
        Some(0x31),
        false,
        [line(0x3129, &[Own]), line(0x3128, &[Own, Name])],
        [line(0x31f1, &[]), line(0x31f0, &[Own, Name])],
    ),
    emote(
        0x2b,
        "giggle",
        Some(0x36),
        false,
        [line(0x312b, &[Own]), line(0x312a, &[Own, Name])],
        [line(0x31f3, &[]), line(0x31f2, &[Name])],
    ),
    emote(
        0x2c,
        "glare",
        Some(0x3c),
        false,
        [line(0x312d, &[Own]), line(0x312c, &[Own, Name])],
        [line(0x31f5, &[]), line(0x31f4, &[Name])],
    ),
    emote(
        0x2d,
        "grin",
        None,
        false,
        [line(0x312f, &[Own]), line(0x312e, &[Own, Name])],
        [line(0x31f7, &[]), line(0x31f6, &[Name])],
    ),
    emote(
        0x2e,
        "groan",
        Some(0x1c),
        false,
        [line(0x3131, &[Own]), line(0x3130, &[Own, Name])],
        [line(0x31f9, &[]), line(0x31f8, &[Name])],
    ),
    emote(
        0x2f,
        "grovel",
        Some(0x32),
        false,
        [line(0x3133, &[Own]), line(0x3132, &[Own, Name])],
        [line(0x31fb, &[]), line(0x31fa, &[Name])],
    ),
    emote(
        0x30,
        "happy",
        Some(0x1b),
        false,
        [line(0x3135, &[Own]), line(0x3134, &[Own, Name])],
        [line(0x31fd, &[]), line(0x31fc, &[Name])],
    ),
    emote(
        0x31,
        "hungry",
        Some(0x34),
        false,
        [line(0x3137, &[Own]), line(0x3136, &[Own, Name, OwnP1])],
        [line(0x31ff, &[]), line(0x31fe, &[Own, Name, OwnP1])],
    ),
    emote(
        0x32,
        "introduce",
        Some(0x1d),
        false,
        [line(0x3139, &[Own, OwnP2]), line(0x3138, &[Own, Name])],
        [line(0x3201, &[OwnP2]), line(0x3200, &[Own, Name])],
    ),
    emote(
        0x33,
        "jk",
        None,
        false,
        [line(0x313b, &[Own]), line(0x313a, &[Own, Name, OwnP1])],
        [line(0x3203, &[]), line(0x3202, &[Own, Name, OwnP1])],
    ),
    emote(
        0x34,
        "kneel",
        Some(0x3e),
        false,
        [line(0x313d, &[Own]), line(0x313c, &[Own, Name])],
        [line(0x3205, &[]), line(0x3204, &[Own, Name])],
    ),
    emote(
        0x35,
        "lost",
        None,
        false,
        [line(0x313f, &[Own]), line(0x313e, &[Own, Name, OwnP1])],
        [line(0x3207, &[]), line(0x3206, &[Own, Name, OwnP1])],
    ),
    emote(
        0x36,
        "massage",
        None,
        false,
        [line(0x3141, &[Own]), line(0x3140, &[Own, Name])],
        [line(0x3209, &[]), line(0x3208, &[Own, Name])],
    ),
    emote(
        0x37,
        "moan",
        None,
        false,
        [line(0x3143, &[Own]), line(0x3142, &[Own, Name])],
        [line(0x320b, &[]), line(0x320a, &[Name])],
    ),
    emote(
        0x38,
        "mourn",
        Some(0x1c),
        false,
        [
            line(0x3145, &[Own, OwnP3]),
            line(0x3144, &[Own, OwnP3, Name]),
        ],
        [line(0x320d, &[OwnP3]), line(0x320c, &[Own, OwnP3, Name])],
    ),
    emote(
        0x39,
        "peer",
        Some(0x3d),
        false,
        [line(0x3147, &[Own]), line(0x3146, &[Own, Name, TargetP2])],
        [line(0x320f, &[]), line(0x320e, &[Own, Name, TargetP2])],
    ),
    emote(
        0x3a,
        "point",
        Some(0x40),
        false,
        [line(0x3149, &[Own]), line(0x3148, &[Own, Name])],
        [line(0x3211, &[]), line(0x3210, &[Own, Name])],
    ),
    emote(
        0x3b,
        "ponder",
        Some(0x41),
        false,
        [line(0x314b, &[Own]), line(0x314a, &[Own, Name, TargetP2])],
        [line(0x3213, &[]), line(0x3212, &[Own, Name, TargetP2])],
    ),
    emote(
        0x3c,
        "puzzle",
        Some(0x39),
        false,
        [line(0x314d, &[Own]), line(0x314c, &[Own, Name])],
        [line(0x3215, &[]), line(0x3214, &[Own, Name])],
    ),
    emote(
        0x3d,
        "raise",
        Some(0x42),
        false,
        [
            line(0x314f, &[Own, OwnP3]),
            line(0x314e, &[Own, Name, OwnP3]),
        ],
        [line(0x3217, &[OwnP3]), line(0x3216, &[Own, Name, OwnP3])],
    ),
    emote(
        0x3e,
        "ready",
        Some(0x42),
        false,
        [
            line(0x3151, &[Own, OwnP1]),
            line(0x3150, &[Own, Name, TargetP1]),
        ],
        [line(0x3219, &[OwnP1]), line(0x3218, &[Own, Name, TargetP1])],
    ),
    emote(
        0x3f,
        "roar",
        Some(0x1b),
        false,
        [line(0x3153, &[Own]), line(0x3152, &[Own, Name])],
        [line(0x321b, &[]), line(0x321a, &[Name])],
    ),
    emote(
        0x40,
        "salute",
        Some(0x43),
        false,
        [line(0x3155, &[Own]), line(0x3154, &[Own, Name])],
        [line(0x321d, &[]), line(0x321c, &[Own, Name])],
    ),
    emote(
        0x41,
        "shiver",
        Some(0x44),
        false,
        [line(0x3157, &[Own]), line(0x3156, &[Own, Name])],
        [line(0x321f, &[]), line(0x321e, &[Own, Name])],
    ),
    emote(
        0x42,
        "shrug",
        Some(0x41),
        false,
        [line(0x3159, &[Own]), line(0x3158, &[Own, Name])],
        [line(0x3221, &[]), line(0x3220, &[Own, Name])],
    ),
    emote(
        0x43,
        "sigh",
        Some(0x1c),
        false,
        [line(0x315b, &[Own]), line(0x315a, &[Own, Name])],
        [line(0x3223, &[]), line(0x3222, &[Own, Name])],
    ),
    emote(
        0x44,
        "smirk",
        None,
        false,
        [line(0x315d, &[Own]), line(0x315c, &[Own, Name])],
        [line(0x3225, &[]), line(0x3224, &[Own, Name])],
    ),
    emote(
        0x45,
        "snarl",
        Some(0x3c),
        false,
        [line(0x315f, &[Own, OwnP3]), line(0x315e, &[Own, Name])],
        [line(0x3227, &[OwnP3]), line(0x3226, &[Own, Name])],
    ),
    emote(
        0x46,
        "snicker",
        Some(0x36),
        false,
        [line(0x3161, &[Own]), line(0x3160, &[Own, Name])],
        [line(0x3229, &[]), line(0x3228, &[Own, Name])],
    ),
    emote(
        0x47,
        "stare",
        Some(0x39),
        false,
        [line(0x3163, &[Own]), line(0x3162, &[Own, Name, TargetP3])],
        [line(0x322b, &[]), line(0x322a, &[Own, Name, TargetP3])],
    ),
    emote(
        0x48,
        "tap",
        Some(0x45),
        false,
        [
            line(0x3165, &[Own, OwnP3]),
            line(0x3164, &[Own, OwnP3, OwnP1, Name]),
        ],
        [
            line(0x322d, &[OwnP3]),
            line(0x322c, &[Own, OwnP3, OwnP1, Name]),
        ],
    ),
    emote(
        0x49,
        "tease",
        None,
        false,
        [line(0x3167, &[Own]), line(0x3166, &[Own, Name])],
        [line(0x322f, &[]), line(0x322e, &[Own, Name])],
    ),
    emote(
        0x4a,
        "thank",
        Some(0x46),
        false,
        [line(0x3169, &[Own]), line(0x3168, &[Own, Name])],
        [line(0x3231, &[]), line(0x3230, &[Own, Name])],
    ),
    emote(
        0x4b,
        "thirsty",
        None,
        false,
        [line(0x316b, &[Own]), line(0x316a, &[Own, Name, OwnP1])],
        [line(0x3233, &[]), line(0x3232, &[Own, Name, OwnP1])],
    ),
    emote(
        0x4c,
        "veto",
        Some(0x3b),
        false,
        [line(0x316d, &[Own]), line(0x316c, &[Own, Name])],
        [line(0x3235, &[]), line(0x3234, &[Own, Name])],
    ),
    emote(
        0x4d,
        "welcome",
        Some(0x1d),
        false,
        [line(0x316f, &[Own]), line(0x316e, &[Own, Name])],
        [line(0x3237, &[]), line(0x3236, &[Own, Name])],
    ),
    emote(
        0x4e,
        "whine",
        None,
        false,
        [line(0x3171, &[Own]), line(0x3170, &[Own, Name])],
        [line(0x3239, &[]), line(0x3238, &[Own, Name])],
    ),
    emote(
        0x4f,
        "whistle",
        Some(0x3d),
        false,
        [line(0x3173, &[Own]), line(0x3172, &[Own, Name])],
        [line(0x323b, &[]), line(0x323a, &[Own, Name])],
    ),
    emote(
        0x50,
        "yawn",
        Some(0x1f),
        false,
        [
            line(0x3175, &[Own, OwnP3]),
            line(0x3174, &[Own, Name, OwnP1]),
        ],
        [line(0x323d, &[OwnP3]), line(0x323c, &[Own, Name])],
    ),
    emote(
        0x51,
        "smack",
        None,
        false,
        [line(0x3176, &[Own, OwnP2]), line(0x3177, &[Own, Name])],
        [line(0x323e, &[]), line(0x323f, &[Name])],
    ),
    emote(
        0x52,
        "pat",
        None,
        false,
        [line(0x3178, &[Own, OwnP2]), line(0x3179, &[Own, Name])],
        [line(0x3240, &[]), line(0x3241, &[Name])],
    ),
    emote(
        0x53,
        "nudge",
        None,
        false,
        [None, line(0x317a, &[Own, Name])],
        [None, line(0x3242, &[Name])],
    ),
    emote(
        0x54,
        "fidget",
        None,
        false,
        [line(0x317b, &[Own]), None],
        [line(0x3243, &[]), None],
    ),
    emote(
        0x55,
        "purr",
        None,
        false,
        [line(0x317e, &[Own, OwnP2]), line(0x317f, &[Own, Name])],
        [line(0x3246, &[]), line(0x3247, &[Name])],
    ),
    emote(
        0x56,
        "kiss",
        None,
        false,
        [line(0x3180, &[Own, OwnP2]), line(0x3181, &[Own, Name])],
        [line(0x3248, &[OwnP2]), line(0x3249, &[Name])],
    ),
    emote(
        0x57,
        "flex",
        None,
        false,
        [line(0x317c, &[Own, OwnP3]), line(0x317d, &[Own, Name])],
        [line(0x3244, &[]), line(0x3245, &[Name])],
    ),
];

/// The first word that keeps the current target instead of naming a spawn.
pub const MY_TARGET_WORD: u32 = 0x3007;
/// No spawn has the name given.
pub const NOT_FOUND: u32 = 0x3008;
/// The target is out of range.
pub const TOO_FAR: u32 = 0x3021;
/// A still emote while the player isn't standing still.
pub const STAND_STILL: u32 = 0x330d;
/// The player's own line for extra words.
pub const OWN_EXTRA: u32 = 0x3011;
/// The player's name slot in the player's own line.
pub const YOU: u32 = 0x300e;
/// The first and second pronoun slots in the player's own line.
pub const YOUR: u32 = 0x300f;
/// The third pronoun slot in the player's own line.
pub const YOURSELF: u32 = 0x3010;

/// How far the target may be: the float at 0x640dac, compared with the
/// distance helper 0x439d16 (R47b c).
pub const RANGE: f32 = 60.0;
/// The speed under which a still emote counts the player as still: the
/// float at 0x640e38 (R47b g).
pub const STILL_SPEED: f32 = 0.1;

/// The three pronoun words for a gender (spawn +0x24e: 0, 1 or other), in
/// slot order (R47b a).
#[must_use]
pub const fn pronouns(gender: u32) -> [u32; 3] {
    match gender {
        0 => [0x2fb1, 0x3009, 0x300d],
        1 => [0x2fb2, 0x300a, 0x300b],
        _ => [0x2fb3, 0x2fb3, 0x300c],
    }
}

/// The emote a resolved command name runs.
#[must_use]
pub fn named(name: &str) -> Option<&'static Emote> {
    EMOTES.iter().find(|emote| emote.name == name)
}

/// The installed strings an emote is worded from.
pub trait Strings {
    /// The string with these arguments in its places, or None where the
    /// installation lacks it.
    fn line(&self, id: u32, arguments: &[String]) -> Option<String>;
    /// An argument-free word, or None where the installation lacks it.
    fn word(&self, id: u32) -> Option<String>;
}

/// The emote's target, as the worker has it after its lookup.
#[derive(Clone, Copy, Debug)]
pub struct Target<'a> {
    /// Its name, as shown.
    pub name: &'a str,
    /// Its gender (spawn +0x24e).
    pub gender: u32,
}

/// What an emote says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spoken {
    /// The text sent, starting with a space; the server puts the player's
    /// name in front.
    pub sent: String,
    /// The player's own line.
    pub local: String,
    /// Whether the player's own line is the installed client's words.
    pub official: bool,
}

/// A full stop where the text ends in a letter or digit (0x574800).
fn stopped(mut text: String) -> String {
    if text.chars().last().is_some_and(char::is_alphanumeric) {
        text.push('.');
    }
    text
}

/// The words of an emote: the sent text and the player's own line, as the
/// worker's two passes make them (R47b b and the local line). `rest` is
/// what follows the target word; two or more characters of it replace the
/// emote's own words. None where the emote has nothing to say, as
/// `/fidget` with a target.
#[must_use]
pub fn speak(
    emote: &Emote,
    gender: u32,
    target: Option<Target<'_>>,
    rest: &str,
    strings: &dyn Strings,
) -> Option<Spoken> {
    let rest = rest.trim();
    let extra = rest.chars().count() >= 2;
    let index = usize::from(target.is_some());
    let target_name = target.map_or("", |target| target.name);
    let own_words = pronouns(gender);
    let target_words = pronouns(target.map_or(2, |target| target.gender));
    let fill = |line: Option<Line>, local: bool| -> Option<Option<String>> {
        let line = line?;
        let word = |id: u32| strings.word(id).unwrap_or_default();
        let arguments: Vec<String> = line
            .slots
            .iter()
            .map(|slot| match (slot, local) {
                (Own, false) => String::new(),
                (Own, true) => word(YOU),
                (Name, _) => target_name.to_owned(),
                (OwnP1 | OwnP2, true) => word(YOUR),
                (OwnP3, true) => word(YOURSELF),
                (OwnP1, false) => word(own_words[0]),
                (OwnP2, false) => word(own_words[1]),
                (OwnP3, false) => word(own_words[2]),
                (TargetP1, _) => word(target_words[0]),
                (TargetP2, _) => word(target_words[1]),
                (TargetP3, _) => word(target_words[2]),
            })
            .collect();
        Some(strings.line(line.id, &arguments))
    };
    // This client's words where the installation lacks a line.
    // Laid out as the installed lines are: a lone sent line starts with
    // its verb, a sent line with a target with a space.
    let ours = |local: bool| match (local, target_name.is_empty()) {
        (false, true) => format!("uses /{}", emote.name),
        (false, false) => format!(" uses /{} on {target_name}", emote.name),
        (true, true) => format!("You use /{}", emote.name),
        (true, false) => format!("You use /{} on {target_name}", emote.name),
    };
    let chosen = |lines: [Option<Line>; 2], local: bool| -> Option<(String, bool)> {
        // Without a target, a no-target line that says something; else the
        // with-target line, as both passes choose.
        if target.is_none()
            && let Some(words) = fill(lines[0], local)
        {
            let words = words.map_or_else(|| (ours(local), false), |words| (words, true));
            if !words.0.trim().is_empty() {
                return Some(words);
            }
        }
        if lines[1].is_none() && index == 1 {
            return None;
        }
        let words = fill(lines[1].or(lines[0]), local)?;
        Some(words.map_or_else(|| (ours(local), false), |words| (words, true)))
    };
    let sent = if extra {
        format!(" {rest}.")
    } else {
        let (words, _) = chosen(emote.sent, false)?;
        // A lone line follows the empty name and a space (0x49faf4).
        if target.is_none() && emote.sent[0].is_some() {
            format!(" {words}")
        } else {
            words
        }
    };
    if sent.trim().is_empty() {
        return None;
    }
    let (local, official) = if extra {
        strings
            .line(OWN_EXTRA, &[rest.to_owned()])
            .map_or_else(|| (format!("You {rest}"), false), |words| (words, true))
    } else {
        chosen(emote.local, true)?
    };
    Some(Spoken {
        sent: stopped(sent),
        local: stopped(local),
        official,
    })
}

/// Why an emote is refused, with nothing sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A still emote while the player isn't standing still (0x330d).
    StandStill,
    /// No spawn has the name given (0x3008).
    NotFound,
    /// The target is further than [`RANGE`] (0x3021).
    TooFar,
}

impl Refusal {
    /// The line it prints, and this client's words where the installation
    /// lacks it.
    #[must_use]
    pub const fn line(self) -> (u32, &'static str) {
        match self {
            Self::StandStill => (STAND_STILL, "Stand still to do that."),
            Self::NotFound => (NOT_FOUND, "Nobody by that name is near."),
            Self::TooFar => (TOO_FAR, "Your target is too far away for that."),
        }
    }
}

#[cfg(test)]
mod tests;
