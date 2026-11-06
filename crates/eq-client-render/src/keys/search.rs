//! Single-line local search editing, shared by the map and spellbook.
use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};

/// Finishing leaves the query in place. Escape also claims the window key.
#[derive(PartialEq, Eq)]
pub(crate) enum Finish {
    Enter,
    Escape,
}

pub(crate) fn edit(
    query: &mut String,
    events: &[KeyboardInput],
    keys: &ButtonInput<Key>,
    mut clipboard: Option<&mut Clipboard>,
    pending: &mut Option<ClipboardRead>,
) -> Option<Finish> {
    if let Some(result) = pending.as_mut().and_then(ClipboardRead::poll_result) {
        if let Ok(text) = result {
            append(query, &text);
        }
        *pending = None;
    }
    let modifiers = super::modifiers::snapshots(events, keys);
    for (event, held) in events
        .iter()
        .zip(modifiers)
        .filter(|(event, _)| event.state.is_pressed())
    {
        if super::modifiers::chorded(held) {
            if held[0]
                && !held[2]
                && !held[3]
                && matches!(&event.logical_key, Key::Character(text) if text.eq_ignore_ascii_case("v"))
                && let Some(clipboard) = clipboard.as_mut()
            {
                *pending = Some(clipboard.fetch_text());
            }
            continue;
        }
        match event.logical_key {
            Key::Escape => return Some(Finish::Escape),
            Key::Enter => return Some(Finish::Enter),
            Key::Backspace => {
                query.pop();
            }
            Key::Delete => {
                query.clear();
                *pending = None;
            }
            _ => append(query, event.text.as_deref().unwrap_or_default()),
        }
    }
    None
}

/// Bound input on UTF-8 boundaries and ignore multiline/control content.
pub(crate) fn append(query: &mut String, text: &str) {
    for c in text.chars().filter(|c| !c.is_control()) {
        if query.len() + c.len_utf8() > 128 {
            break;
        }
        query.push(c);
    }
}
