//! Normal gem presses and targeted corpse opening from any slash input origin.
use crate::{
    chat::ChatState,
    online::OnlineState,
    outbox::{Outbox, Stamp},
};
use bevy::prelude::*;
use eq_client_core::world::ClientWorld;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Cast(u8),
    Loot {
        target: Option<u16>,
        revision: Option<u64>,
    },
}

/// A user request keeps its original admission/time until consumed once.
pub(crate) struct Asked {
    stamp: Stamp,
    action: Action,
}

/// Titanium's signed decimal prefix, including its 32-bit arithmetic.
fn number(words: &str) -> i32 {
    let mut bytes = words
        .trim_start_matches(|c: char| c.is_ascii_whitespace())
        .bytes()
        .peekable();
    let negative = bytes.peek() == Some(&b'-');
    if matches!(bytes.peek(), Some(b'+' | b'-')) {
        bytes.next();
    }
    let mut value = 0_i32;
    for digit in bytes.take_while(u8::is_ascii_digit) {
        value = value.wrapping_mul(10).wrapping_add(i32::from(digit - b'0'));
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

fn parse(input: &str, world: &ClientWorld) -> Option<Result<Action, String>> {
    let line = input.strip_prefix('/')?;
    let end = line.find(char::is_whitespace).unwrap_or(line.len());
    let (command, words) = line.split_at(end);
    if command.eq_ignore_ascii_case("cast") {
        let slot = number(words);
        Some(match slot {
            1..=8 => Ok(Action::Cast(
                u8::try_from(slot - 1).expect("eight supported gems"),
            )),
            9 => {
                Err("Spell gem 9 is not supported in this client yet; use /cast 1 through 8".into())
            }
            _ => Err("Use /cast 1 through 8".into()),
        })
    } else if command.eq_ignore_ascii_case("loot") {
        let target = world.target().selected;
        Some(Ok(Action::Loot {
            target,
            revision: target.and_then(|id| world.spawn(id).map(|spawn| spawn.revision)),
        }))
    } else {
        None
    }
}

/// Called by the ordinary typed, social, button and script slash dispatcher.
pub(super) fn request(
    input: &str,
    chat: &mut ChatState,
    world: &ClientWorld,
    outbox: &Outbox,
) -> Option<Result<(), String>> {
    Some(parse(input, world)?.and_then(|action| {
        if outbox
            .script_world(world)
            .is_some_and(|expected| Some(expected.admission_id) != world.session_id())
        {
            return Err(crate::outbox::Refusal::Away.text().into());
        }
        let stamp = outbox
            .stamp(world)
            .map_err(|reason| reason.text().to_owned())?;
        chat.cast_loot.push(Asked { stamp, action });
        Ok(())
    }))
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn run(
    mut chat: ResMut<ChatState>,
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    mut trade: ResMut<crate::trade::TradeState>,
    names: Res<crate::spellbook::SpellNames>,
    messages: Res<crate::hud::messages::Messages>,
    settings: Res<crate::ViewerSettings>,
    map: Res<crate::keys::KeyMap>,
    options: Res<crate::options::OptionsState>,
) {
    let mut sent_cast = false;
    for Asked { stamp, action } in std::mem::take(&mut chat.cast_loot) {
        let world = online.world();
        if world.session_id() != Some(stamp.session_id) || !world.in_world() || world.is_dead() {
            continue;
        }
        match action {
            Action::Cast(gem) => {
                if sent_cast {
                    continue;
                }
                online.clear_melody();
                let bound = outbox.for_request(stamp);
                sent_cast = crate::hud::press_gem(
                    (gem, false),
                    (&mut chat, &online, &bound),
                    (&names, &messages, &settings),
                    (&map, Some(&options)),
                );
            }
            Action::Loot { target: None, .. } => chat.refuse(
                messages
                    .said_text(13201, "You must first target a corpse to loot!")
                    .typed(0x111),
            ),
            Action::Loot {
                target: Some(target),
                revision,
            } => trade.slash_loot(&mut online, &outbox, stamp, target, revision),
        }
    }
}

#[cfg(test)]
mod tests;
