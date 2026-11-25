//! The original Titanium `/fastdrop on|off|never` preference.
use crate::chat::{ChatState, Said};
use bevy::prelude::*;
use eq_client_core::options::NoSafeDrop;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Asked(Option<NoSafeDrop>);

impl Asked {
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let mut words = input.strip_prefix('/')?.split_whitespace();
        if !words.next()?.eq_ignore_ascii_case("fastdrop") {
            return None;
        }
        let mode = match words.next().map(str::to_ascii_lowercase).as_deref() {
            Some("on") => Some(NoSafeDrop::Fast),
            Some("off") => Some(NoSafeDrop::Ask),
            Some("never") => Some(NoSafeDrop::Never),
            // [I] Bare and unknown forms leave the choice unchanged and
            // show usage; the Titanium read only names on/off/never.
            _ => None,
        };
        Some(Self(mode))
    }
}

fn response(mode: Option<NoSafeDrop>) -> (u32, &'static str) {
    match mode {
        Some(NoSafeDrop::Fast) => (0x333c, "Items will drop without asking."),
        Some(NoSafeDrop::Ask) => (0x333d, "Items will ask before dropping."),
        Some(NoSafeDrop::Never) => (0x333e, "Items will never drop."),
        // [I] The usage line ID is not established by the Titanium read,
        // which names only the three mode lines 0x333c/0x333d/0x333e.
        None => (0x333f, "Usage /fastdrop on|off|never"),
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn answers(
    mut chat: ResMut<ChatState>,
    mut options: ResMut<crate::options::OptionsState>,
    messages: Option<Res<crate::hud::messages::Messages>>,
) {
    let Some(Asked(mode)) = chat.fastdrop_asked.take() else {
        return;
    };
    if let Some(mode) = mode {
        options.options.no_safe_drop = mode;
    }
    let (id, fallback) = response(mode);
    let said = messages.as_ref().map_or_else(
        || Said::own(fallback),
        |messages| messages.said_text(id, fallback),
    );
    chat.history
        .push(crate::chat::system_line(said.typed(0x111)));
}

#[cfg(test)]
mod tests;
