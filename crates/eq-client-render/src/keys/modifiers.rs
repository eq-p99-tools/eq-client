//! Preserve per-event modifier state when an entire chord arrives in one frame.
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};

pub(crate) const MODIFIERS: [Key; 4] = [Key::Control, Key::Shift, Key::Alt, Key::Super];

/// Whether a key event arrives as a chord rather than as typing: a modifier
/// other than Shift is held. Control and Alt held together are `AltGr`, which
/// Windows reports that way for the third-level characters of many
/// layouts (`@`, `{`, `€`), so that pair types.
#[must_use]
pub(crate) const fn chorded(held: [bool; 4]) -> bool {
    let alt_gr = held[0] && held[2] && !held[3];
    (held[0] || held[2] || held[3]) && !alt_gr
}

/// Infer the modifier state before this batch from its final state, then
/// associate each event with the state at that point in the batch.
pub(crate) fn snapshots(events: &[KeyboardInput], final_keys: &ButtonInput<Key>) -> Vec<[bool; 4]> {
    let mut held = MODIFIERS
        .each_ref()
        .map(|key| final_keys.pressed(key.clone()));
    for event in events.iter().rev().filter(|event| !event.repeat) {
        if let Some(index) = MODIFIERS.iter().position(|key| *key == event.logical_key) {
            held[index] = !event.state.is_pressed();
        }
    }
    events
        .iter()
        .map(|event| {
            if let Some(index) = MODIFIERS.iter().position(|key| *key == event.logical_key) {
                held[index] = event.state.is_pressed();
            }
            held
        })
        .collect()
}
