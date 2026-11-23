//! `/hidecorpses`, as Titanium's handler runs it (0x469b01, read R46
//! section 2): local only, it hides corpses from the view by clearing each
//! one's draw flag. Two words hide the corpses present once; any other word
//! shows them all again. A third word (0x33f6) sets a flag whose meaning is
//! unread (R49-8e: it may hide corpses once looted rather than every new
//! one), so it is held: this client doesn't take it yet. Words and lines are
//! the installed client's strings where it has them; this client's stand in
//! otherwise.
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
/// The flag word, held until R49-8e reads what its flag hides (its line
/// is 0x33f7).
const HELD: (u32, &str) = (0x33f6, "new");
/// The answer on a client without R46's forms, and to the held word.
const NOT_YET: &str = "This client does not take /hidecorpses that way yet.";
/// The line for any other word: every corpse shows again.
const SHOWN_LINE: (u32, &str) = (0x3399, "Every corpse is shown.");

/// Which corpses `/hidecorpses` hid, in which zone session.
#[derive(Resource, Default)]
pub(crate) struct HiddenCorpses {
    hidden: BTreeSet<u16>,
    /// The zone session the hidden ids belong to: spawn ids are reused in
    /// the next zone.
    session: Option<u64>,
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

    /// Forgets every hidden corpse when the zone session changes, and a
    /// corpse gone from the zone.
    fn follow(&mut self, world: &ClientWorld) {
        let session = world.session_id();
        if self.session != session {
            self.session = session;
            self.hidden.clear();
        }
        if self.hidden.is_empty() {
            return;
        }
        let spawns = world.spawns();
        self.hidden.retain(|id| {
            spawns.get(id).is_some_and(|spawn| {
                matches!(
                    spawn.state.kind,
                    SpawnKind::PlayerCorpse | SpawnKind::NpcCorpse
                )
            })
        });
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

/// Answers a `/hidecorpses` the chat noted, and forgets hidden corpses the
/// zone no longer holds.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn run(
    mut chat: ResMut<ChatState>,
    online: Res<OnlineState>,
    messages: Option<Res<Messages>>,
    settings: Option<Res<crate::ViewerSettings>>,
    mut hidden: ResMut<HiddenCorpses>,
) {
    let world = online.world();
    hidden.follow(world);
    if let Some(words) = chat.hide_corpses.take() {
        let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
        let empty = Messages::default();
        let messages = messages.as_deref().unwrap_or(&empty);
        match client
            .runs_r46_forms()
            .then(|| answer(&mut hidden, world, &words, messages))
            .flatten()
        {
            Some(lines) => {
                for line in lines {
                    chat.history.push(system_line(line));
                }
            }
            None => chat.refuse(Said::own(NOT_YET)),
        }
    }
}

/// What a `/hidecorpses` with these words does, and the lines it says;
/// None for the held flag word, which this client doesn't take yet.
fn answer(
    corpses_hidden: &mut HiddenCorpses,
    world: &ClientWorld,
    words: &str,
    messages: &Messages,
) -> Option<Vec<Said>> {
    let said = |(id, ours): (u32, &str)| messages.said_text(id, ours);
    let Some(typed) = words.split_whitespace().next() else {
        return Some(HELP.into_iter().map(said).collect());
    };
    let is = |(id, ours): (u32, &str)| said((id, ours)).words.text.eq_ignore_ascii_case(typed);
    let mine = own_name(world);
    let own = |owner: &str| mine.as_deref().is_some_and(|mine| same(mine, owner));
    Some(if is(OTHERS) {
        corpses_hidden.sweep(world, own);
        vec![said(OTHERS_LINE)]
    } else if is(GROUP) {
        corpses_hidden.sweep(world, |owner| own(owner) || grouped(world, owner));
        vec![said(GROUP_LINE)]
    } else if is(HELD) {
        return None;
    } else {
        corpses_hidden.hidden.clear();
        vec![said(SHOWN_LINE)]
    })
}

#[cfg(test)]
mod tests;
