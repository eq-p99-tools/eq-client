//! Spoken languages: `/language`, the tongue every typed line goes out in,
//! and the names heard lines are shown with (parity item 9, recording
//! checklist 6.495).
//!
//! The official client's `/language` is not read yet (program read
//! R49-4a). Its forms here are inferred from the official help list's
//! numbering, which counts the common tongue as 1: `/language` alone lists
//! the languages the character knows, and `/language N` chooses the Nth
//! where the character knows it. Every line about it is in this client's
//! words. The choice is kept per character in the options.
use crate::{
    chat::{ChatState, Said, system_line},
    hud::messages::Messages,
    online::OnlineState,
    options::OptionsState,
    outbox::Outbox,
};
use bevy::prelude::*;
use eq_client_assets::skills::SkillRules;
use eq_client_core::{chat::Tongue, tongues};

/// The name a heard tongue is shown with: the installed client's name for
/// the language where the installation has it (the names after 24, and
/// `EQEmu`'s unknown tongue 27, share one), else this client's words.
pub(crate) fn tongue_name(messages: Option<&Messages>, language: u8) -> String {
    SkillRules::language_name_string(language, None)
        .and_then(|id| messages?.argument_free(id))
        .map_or_else(
            || {
                if language > tongues::LAST_CHOSEN {
                    "an unknown tongue".to_owned()
                } else {
                    format!("Language {language}")
                }
            },
            str::to_owned,
        )
}

/// A language's name for the `/language` list, by its number: the common
/// tongue by the installed name the world's selector picks, the others as
/// heard lines name them.
fn listed_name(online: &OnlineState, messages: Option<&Messages>, language: u8) -> String {
    if language != tongues::COMMON {
        return tongue_name(messages, language);
    }
    let selector = online
        .world()
        .language_zero_selector()
        .map(eq_client_core::rules::LanguageZeroSelector::alternate_name);
    SkillRules::language_name_string(language, selector.or(Some(false)))
        .and_then(|id| messages?.argument_free(id))
        .map_or_else(|| "Common Tongue".to_owned(), str::to_owned)
}

/// Answers a `/language`: no word lists the languages the character knows
/// and the one spoken; a number chooses one the character knows; anything
/// else says how it is used.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn command(
    mut chat: ResMut<ChatState>,
    mut options: ResMut<OptionsState>,
    online: Res<OnlineState>,
    messages: Option<Res<Messages>>,
) {
    let Some(words) = chat.language_asked.take() else {
        return;
    };
    let messages = messages.as_deref();
    let Some(player) = online.world().player() else {
        chat.refuse(Said::own("Choose a language once in the world."));
        return;
    };
    let known: Vec<(u8, u8)> = player
        .languages
        .as_deref()
        .unwrap_or_default()
        .iter()
        .take(usize::from(tongues::LAST_CHOSEN) + 1)
        .enumerate()
        .filter(|(_, skill)| **skill > 0)
        .map(|(language, skill)| (u8::try_from(language).expect("below 25"), *skill))
        .collect();
    let word = words.trim();
    if word.is_empty() {
        let spoken = options.options.language;
        for (language, skill) in known {
            let marker = if language == spoken {
                " (speaking)"
            } else {
                ""
            };
            let name = listed_name(&online, messages, language);
            chat.history.push(system_line(Said::own(format!(
                "{} - {name} ({skill}){marker}",
                u16::from(language) + 1
            ))));
        }
        return;
    }
    let chosen = word
        .parse::<u8>()
        .ok()
        .and_then(|number| number.checked_sub(1))
        .filter(|&language| language <= tongues::LAST_CHOSEN);
    let Some(language) = chosen else {
        chat.refuse(Said::own(
            "Use /language to list your languages, or /language and a number to speak one.",
        ));
        return;
    };
    if Tongue::spoken(player, language).is_none() {
        chat.refuse(Said::own("You do not know that language."));
        return;
    }
    options.options.language = language;
    let name = listed_name(&online, messages, language);
    chat.history.push(system_line(Said::own(format!(
        "You are now speaking {name}."
    ))));
}

/// Keeps the outbox speaking the character's chosen language.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn speak(options: Res<OptionsState>, outbox: Res<Outbox>) {
    outbox.set_language(options.options.language);
}

#[cfg(test)]
mod tests;
