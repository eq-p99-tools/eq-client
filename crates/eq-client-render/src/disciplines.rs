//! Disciplines, as Titanium uses one (reads R48, R48b and R48c). `/discipline`
//! (0x46fc1b), the Combat Abilities window (0x4f8aab) and a hotbutton
//! (0x51415b) all go through one use, 0x4bb39d, which this module stands in
//! for:
//!
//! - **The checks:** the spell is in the known list and is a discipline
//!   your class has at your level. A failed check is silent.
//! - **On its timer:** the time-left line, then refused.
//! - **A timed self discipline with the discipline slot taken:** its
//!   line, then refused.
//! - **Otherwise:** the ordinary cast from slot 10 at the current target,
//!   or 0 with none.
//!
//! `/discipline` with no word lists the known disciplines whose timers run.
//! With a word, it uses the first known discipline whose name the word
//! begins.
use crate::{
    chat::{ChatState, Said, system_line},
    hud::messages::Messages,
    online::OnlineState,
    outbox::Outbox,
    spellbook::{DisciplineSpell, SpellNames},
};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, disciplines::Active, world::ClientWorld};
use std::time::{Duration, Instant};

/// What asks for a discipline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// `/discipline` with the words after it.
    Typed(String),
    /// A discipline by its spell, from the Combat Abilities window, an
    /// Actions window button or a hotkey.
    Use(u32),
}

/// The classes `/discipline` serves (0x46fc1b): warrior, paladin, ranger,
/// shadow knight, monk, bard, rogue, beastlord and berserker.
const CLASSES: [u32; 9] = [1, 3, 4, 5, 7, 8, 9, 15, 16];

/// The line for a class `/discipline` does not serve (0x46fc1b).
const CLASS_GATE: u32 = 0x16d0;
/// The time left on a discipline's timer, with hours (`%1` name, then
/// hours, minutes and seconds).
const TIME_LEFT_HOURS: u32 = 0x16af;
/// The time left with no hours (`%1` name, then minutes and seconds).
const TIME_LEFT: u32 = 0x16b0;
/// The line for a timed self discipline while the discipline slot is taken
/// (0x4bb039).
const SLOT_TAKEN: u32 = 0x16bd;

/// The colour the timer and slot lines print in: Options colour row 15
/// (0x10f, read R48c 1).
const DISCIPLINE_COLOUR: u32 = 0x10f;
/// The colour of the class gate's line: Options colour row 17 (0x111, read
/// R48c 2).
const GATE_COLOUR: u32 = 0x111;

/// The buff slot 0x4bb039 tests: the program's last long buff slot, which
/// the slot finder gives a discipline (read R48b b). That our buff table's
/// slot of the same number is that slot is inferred (recording checklist
/// 6.488).
const DISCIPLINE_BUFF_SLOT: u32 = 24;

/// What a discipline request comes to.
#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    /// Nothing printed and nothing sent.
    Silent,
    /// Lines printed and nothing sent.
    Lines(Vec<Said>),
    /// The cast to send.
    Cast {
        spell_id: u32,
        timer: u32,
        target_id: u16,
        timed_self: bool,
    },
}

/// What a request comes to for this world and spell table at `now`.
pub(crate) fn decide(
    world: &ClientWorld,
    names: &SpellNames,
    messages: &Messages,
    asked: &Asked,
    now: Instant,
) -> Outcome {
    match asked {
        Asked::Use(spell) => use_one(world, names, messages, *spell, now),
        Asked::Typed(words) => typed(world, names, messages, words, now),
    }
}

/// `/discipline` (0x46fc1b): the class gate, then the list with no word or
/// the first known discipline the word begins. Only leading spaces are
/// skipped, as the program's argument starts at the first word.
fn typed(
    world: &ClientWorld,
    names: &SpellNames,
    messages: &Messages,
    words: &str,
    now: Instant,
) -> Outcome {
    let Some(player) = world.player() else {
        return Outcome::Silent;
    };
    if !player.class.is_some_and(|class| CLASSES.contains(&class)) {
        let mut line = messages.said_text(CLASS_GATE, "Your class has no disciplines.");
        line.message_type = Some(GATE_COLOUR);
        return Outcome::Lines(vec![line]);
    }
    let typed = words.trim_start();
    let known = known(world).filter_map(|spell| Some((spell, names.discipline(spell)?)));
    if typed.is_empty() {
        return Outcome::Lines(
            known
                .filter_map(|(_, spell)| {
                    let left = time_left(world, &spell, now)?;
                    Some(time_line(messages, &spell.name, left))
                })
                .collect(),
        );
    }
    let typed = typed.as_bytes();
    let mut known = known;
    let first = known.find(|(_, spell)| {
        spell
            .name
            .as_bytes()
            .get(..typed.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(typed))
    });
    match first {
        Some((spell, _)) => use_one(world, names, messages, spell, now),
        None => Outcome::Silent,
    }
}

/// 0x4bb39d: the checks, the timer, the discipline slot, then the cast.
fn use_one(
    world: &ClientWorld,
    names: &SpellNames,
    messages: &Messages,
    spell_id: u32,
    now: Instant,
) -> Outcome {
    let Some(player) = world.player() else {
        return Outcome::Silent;
    };
    let Some(spell) = names.discipline(spell_id) else {
        return Outcome::Silent;
    };
    // 0x4baca6 and 0x4bb2dd. Their further checks after the level were not
    // read (recording checklist 6.488).
    let level = player
        .class
        .and_then(|class| usize::try_from(class.checked_sub(1)?).ok())
        .and_then(|index| spell.class_levels.get(index).copied().flatten())
        .filter(|level| *level != u8::MAX);
    let usable = known(world).any(|known| known == spell_id)
        && spell.flagged
        && level.is_some_and(|level| player.level >= level);
    if !usable {
        return Outcome::Silent;
    }
    // The program refuses on a running timer only when the seconds part of
    // the time left is below the spell's recast seconds (read R48b d). This
    // client always refuses, the safe reading the plan chose.
    if let Some(left) = time_left(world, &spell, now) {
        return Outcome::Lines(vec![time_line(messages, &spell.name, left)]);
    }
    if spell.timed_self && slot_taken(world) {
        let mut line = messages.said_text(SLOT_TAKEN, "You can't use that discipline right now.");
        line.message_type = Some(DISCIPLINE_COLOUR);
        return Outcome::Lines(vec![line]);
    }
    Outcome::Cast {
        spell_id,
        timer: spell.timer,
        target_id: world.target().selected.unwrap_or(0),
        timed_self: spell.timed_self,
    }
}

/// The known disciplines in list order, empty slots skipped.
fn known(world: &ClientWorld) -> impl Iterator<Item = u32> + '_ {
    world
        .player()
        .and_then(|player| player.disciplines.as_deref())
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|spell| *spell != 0)
}

/// The time left on a discipline's own timer, while it runs.
fn time_left(world: &ClientWorld, spell: &DisciplineSpell, now: Instant) -> Option<Duration> {
    world
        .casting()
        .discipline_wait(spell.timer, now)
        .filter(|left| !left.is_zero())
}

/// Whether the discipline slot is taken: a discipline the server says is
/// active, or a spell in the buff slot 0x4bb039 tests.
fn slot_taken(world: &ClientWorld) -> bool {
    matches!(world.casting().active_discipline(), Active::Titanium(_))
        || world
            .buffs()
            .slots()
            .and_then(|slots| slots.get(&DISCIPLINE_BUFF_SLOT))
            .is_some_and(|buff| buff.spell_id != 0 && buff.spell_id != u32::MAX)
}

/// The time-left line (read R48c 1): whole seconds split into hours,
/// minutes and seconds, each in plain decimal. A part second counts as a
/// whole one, so a running timer never shows nothing left.
fn time_line(messages: &Messages, name: &str, left: Duration) -> Said {
    let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
    let (hours, minutes, seconds) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    let mut line = if hours > 0 {
        messages.said_or(
            TIME_LEFT_HOURS,
            &[
                name.to_owned(),
                hours.to_string(),
                minutes.to_string(),
                seconds.to_string(),
            ],
            &format!("{name} can be used again in {hours}h {minutes}m {seconds}s."),
        )
    } else {
        messages.said_or(
            TIME_LEFT,
            &[name.to_owned(), minutes.to_string(), seconds.to_string()],
            &format!("{name} can be used again in {minutes}m {seconds}s."),
        )
    };
    line.message_type = Some(DISCIPLINE_COLOUR);
    line
}

/// Runs the discipline requests: prints their lines, or sends the cast. The
/// Mac-era client keeps this client's earlier answer, as it has no
/// disciplines to read.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn run(
    mut chat: ResMut<ChatState>,
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    names: Res<SpellNames>,
    messages: Option<Res<Messages>>,
    settings: Option<Res<crate::ViewerSettings>>,
) {
    if chat.disciplines.is_empty() {
        return;
    }
    let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
    let empty = Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let now = Instant::now();
    for asked in std::mem::take(&mut chat.disciplines) {
        if !client.runs_disciplines() {
            chat.refuse(Said::own("This client does not take disciplines yet."));
            continue;
        }
        match decide(online.world(), &names, messages, &asked, now) {
            Outcome::Silent => (),
            Outcome::Lines(lines) => {
                for line in lines {
                    chat.history.push(system_line(line));
                }
            }
            Outcome::Cast {
                spell_id,
                timer,
                target_id,
                timed_self,
            } => {
                // The outbox shows why a cast did not go.
                let _ = outbox.post(online.world(), |stamp| ClientCommand::UseDiscipline {
                    session_id: stamp.session_id,
                    spell_id,
                    timer,
                    target_id,
                    timed_self,
                    created: stamp.created,
                });
            }
        }
    }
}

/// The name a discipline shows on a button or hotkey: its spell's name, or
/// its number where the spell table lacks it.
pub(crate) fn name(names: &SpellNames, spell: u32) -> String {
    names.label(spell)
}

/// The time left on a discipline's timer, while it runs.
pub(crate) fn time_left_of(
    world: &ClientWorld,
    names: &SpellNames,
    spell: u32,
    now: Instant,
) -> Option<Duration> {
    time_left(world, &names.discipline(spell)?, now)
}

/// Whether a discipline waits: its timer runs, or a timed self one finds
/// the discipline slot taken. A button or hotkey dims while it waits.
pub(crate) fn waits(world: &ClientWorld, names: &SpellNames, spell: u32, now: Instant) -> bool {
    names.discipline(spell).is_some_and(|discipline| {
        time_left(world, &discipline, now).is_some() || (discipline.timed_self && slot_taken(world))
    })
}

/// Whether the player knows a discipline, which is what a button or hotkey
/// holding it needs.
pub(crate) fn knows(world: &ClientWorld, spell: u32) -> bool {
    known(world).any(|known| known == spell)
}

/// What a hotkey or button holding a discipline says on hover: its name and
/// whether it waits.
pub(crate) fn detail(world: &ClientWorld, names: &SpellNames, spell: u32, now: Instant) -> String {
    let name = name(names, spell);
    let Some(discipline) = names.discipline(spell) else {
        return format!("{name}\nNot in the installed spell file");
    };
    let status = match world.casting().active_discipline() {
        Active::Titanium(active) if active == spell => "Active".to_owned(),
        _ => match time_left(world, &discipline, now) {
            Some(left) => format!("Ready in {:.0}s", left.as_secs_f32().ceil()),
            None if discipline.timed_self && slot_taken(world) => {
                "Another discipline is running".to_owned()
            }
            None => "Ready".to_owned(),
        },
    };
    format!("{name}\n{status}")
}

#[cfg(test)]
mod tests;
