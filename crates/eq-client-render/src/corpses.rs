//! `/hidecorpses`, as Titanium's handler runs it (0x469b01, read R46
//! section 2): local only, it hides corpses from the view by clearing each
//! one's draw flag. Two words hide the corpses present once; a third sets a
//! flag that hides corpses as they come; any other word shows them all
//! again. Words and lines are the installed client's strings where it has
//! them; this client's stand in otherwise.
use super::chat::{ChatState, Said, system_line};
use super::hud::messages::Messages;
use super::online::OnlineState;
use bevy::prelude::*;
use eq_client_core::{SpawnKind, world::ClientWorld};
use std::collections::BTreeSet;

/// The help lines with no word.
const HELP: [(u32, &str); 3] = [
    (0x3392, "Use /hidecorpses with a word:"),
    (
        0x3393,
        "others hides every corpse but yours; group keeps your group's too;",
    ),
    (
        0x3394,
        "new hides corpses as they come; any other word shows them all.",
    ),
];
/// The word that hides every corpse but the player's own, and its line.
const OTHERS: (u32, &str) = (0x3395, "others");
const OTHERS_LINE: (u32, &str) = (0x3396, "Corpses other than yours are hidden.");
/// The word that keeps the corpses of the player's group too, and its line.
const GROUP: (u32, &str) = (0x3397, "group");
const GROUP_LINE: (u32, &str) = (
    0x3398,
    "Corpses other than yours and your group's are hidden.",
);
/// The word that hides corpses as they come, and its line.
const LATER: (u32, &str) = (0x33f6, "new");
const LATER_LINE: (u32, &str) = (0x33f7, "New corpses will be hidden.");
/// The line for any other word: every corpse shows again.
const SHOWN_LINE: (u32, &str) = (0x3399, "Every corpse is shown.");

/// Which corpses `/hidecorpses` hid, and whether it hides those to come.
#[derive(Resource, Default)]
pub(crate) struct HiddenCorpses {
    hidden: BTreeSet<u16>,
    /// The mode flag 0x33f6 sets ([0x905d24]+0x14).
    later: bool,
    /// The corpses present when last looked at, so that a corpse that comes
    /// afterwards is known as new.
    seen: BTreeSet<u16>,
}

impl HiddenCorpses {
    /// Whether the spawn is a corpse `/hidecorpses` keeps out of the view.
    pub(crate) fn hides(&self, spawn_id: u16) -> bool {
        self.hidden.contains(&spawn_id)
    }

    /// Hides the corpses present that `keep` doesn't keep: one sweep, as
    /// the handler walks the corpses once.
    fn sweep(&mut self, world: &ClientWorld, keep: impl Fn(&str) -> bool) {
        for (id, owner) in corpses(world) {
            if !keep(&owner) {
                self.hidden.insert(id);
            }
        }
    }

    /// Hides the corpses that came since last looked at, while the flag
    /// holds, keeping the player's own (inferred: which corpses the flag
    /// hides is unread, recording checklist 6.490), and forgets corpses
    /// gone from the zone.
    fn follow(&mut self, world: &ClientWorld) {
        let present: BTreeSet<u16> = corpses(world).map(|(id, _)| id).collect();
        if self.later {
            let mine = own_name(world);
            for (id, owner) in corpses(world) {
                if !self.seen.contains(&id)
                    && !mine.as_deref().is_some_and(|mine| same(mine, &owner))
                {
                    self.hidden.insert(id);
                }
            }
        }
        self.hidden.retain(|id| present.contains(id));
        self.seen = present;
    }
}

/// The corpses the client holds, each with the name of its owner: the
/// corpse's name less its "'s corpse" ending.
fn corpses(world: &ClientWorld) -> impl Iterator<Item = (u16, String)> + '_ {
    world.spawns().values().filter_map(|spawn| {
        let state = &spawn.state;
        matches!(state.kind, SpawnKind::PlayerCorpse | SpawnKind::NpcCorpse).then(|| {
            let shown = eq_client_core::entities::display_name(&state.name);
            let owner = shown.strip_suffix("'s corpse").unwrap_or(&shown).to_owned();
            (state.spawn_id, owner)
        })
    })
}

fn own_name(world: &ClientWorld) -> Option<String> {
    world.player().map(|player| player.name.clone())
}

fn same(name: &str, owner: &str) -> bool {
    name.eq_ignore_ascii_case(owner)
}

/// Whether `owner` is in the player's group. It stands in for the
/// program's test 0x4bb226, which is unread (6.490).
fn grouped(world: &ClientWorld, owner: &str) -> bool {
    (0..6)
        .filter_map(|place| world.group_place(place))
        .any(|member| same(member, owner))
}

/// Answers a `/hidecorpses` the chat noted, and hides corpses that come
/// while its flag holds.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn run(
    mut chat: ResMut<ChatState>,
    online: Res<OnlineState>,
    messages: Option<Res<Messages>>,
    settings: Option<Res<crate::ViewerSettings>>,
    mut hidden: ResMut<HiddenCorpses>,
) {
    let world = online.world();
    if let Some(words) = chat.hide_corpses.take() {
        let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
        if client.runs_r46_forms() {
            let empty = Messages::default();
            let messages = messages.as_deref().unwrap_or(&empty);
            for line in answer(&mut hidden, world, &words, messages) {
                chat.history.push(system_line(line));
            }
        } else {
            chat.refuse(Said::own(
                "This client does not take /hidecorpses that way yet.",
            ));
        }
    }
    hidden.follow(world);
}

/// What a `/hidecorpses` with these words does, and the lines it says.
fn answer(
    corpses_hidden: &mut HiddenCorpses,
    world: &ClientWorld,
    words: &str,
    messages: &Messages,
) -> Vec<Said> {
    let said = |(id, ours): (u32, &str)| messages.said_text(id, ours);
    let Some(typed) = words.split_whitespace().next() else {
        return HELP.into_iter().map(said).collect();
    };
    let is = |(id, ours): (u32, &str)| said((id, ours)).words.text.eq_ignore_ascii_case(typed);
    let mine = own_name(world);
    let own = |owner: &str| mine.as_deref().is_some_and(|mine| same(mine, owner));
    if is(OTHERS) {
        corpses_hidden.sweep(world, own);
        vec![said(OTHERS_LINE)]
    } else if is(GROUP) {
        corpses_hidden.sweep(world, |owner| own(owner) || grouped(world, owner));
        vec![said(GROUP_LINE)]
    } else if is(LATER) {
        corpses_hidden.later = true;
        vec![said(LATER_LINE)]
    } else {
        corpses_hidden.later = false;
        corpses_hidden.hidden.clear();
        vec![said(SHOWN_LINE)]
    }
}

#[cfg(test)]
mod tests;
