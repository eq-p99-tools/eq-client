//! Preserve per-event modifier state when an entire chord arrives in one frame.
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};

pub(crate) const MODIFIERS: [Key; 4] = [Key::Control, Key::Shift, Key::Alt, Key::Super];

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
