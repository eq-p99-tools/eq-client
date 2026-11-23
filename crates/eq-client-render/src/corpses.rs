//! `/hidecorpses`, as Titanium's handler runs it (0x469b01, read R46
//! section 2): local only, it hides corpses from the view by clearing each
//! one's draw flag. Two words hide the corpses present once; any other word
//! shows them all again. A third word (0x33f6) sets a standing mode (read
//! R49-8e): when the loot window closes (0x52aeb6), the corpse just looted
//! is hidden unless it carries the player's own name. It doesn't hide
//! corpses as they arrive. Words and lines are the installed client's
//! strings where it has them; this client's stand in otherwise.
//!
//! Inferred, checklist 6.490:
//! - the show-all word clears the mode as well as what's hidden (the flag's
//!   clearing is unread);
//! - a new zone session clears the mode (unread; the hidden ids go with the
//!   session because spawn ids are reused).
//!
//! A gap, checklist 6.490: the program also counts a hidden corpse as unseen
//! for its cast start and abilities (0x480409); this client doesn't. Whether
//! the mouse pick, the cycle keys and the name tags skip one is unread: here
//! it isn't drawn, so the mouse doesn't pick it, and `/target` still finds it.
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
        "looted hides each corpse you loot but your own; any other word shows them all.",
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
/// The word that sets the looted-corpse mode, and its line.
const LOOTED: (u32, &str) = (0x33f6, "looted");
const LOOTED_LINE: (u32, &str) = (
    0x33f7,
    "Each corpse you loot but your own is hidden when you close it.",
);
/// The answer on a client without R46's forms.
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
    /// Whether the looted-corpse mode (0x33f6) is set.
    after_loot: bool,
    /// The corpse the loot window shows, with its owner's name, so that
    /// its close can be seen whichever way the window closed.
    looting: Option<(u16, String)>,
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
            self.after_loot = false;
            self.looting = None;
        }
        self.closed_loot(world);
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

impl HiddenCorpses {
    /// Hides the corpse whose loot window just closed, in the looted-corpse
    /// mode, unless it is the player's own (0x52aeb6), and notes the corpse
    /// the window shows now.
    fn closed_loot(&mut self, world: &ClientWorld) {
        let open = world.loot().map(|loot| loot.corpse_id);
        if let Some((id, owner)) = self.looting.take_if(|(id, _)| open != Some(*id))
            && self.after_loot
            && !own_name(world).is_some_and(|mine| same(&mine, &owner))
            && corpses(world).any(|(corpse, _)| corpse == id)
        {
            self.hidden.insert(id);
        }
        if let Some(open) = open
            && let Some(owner) = corpses(world)
                .find(|(corpse, _)| *corpse == open)
                .map(|(_, owner)| owner)
        {
            self.looting = Some((open, owner));
        }
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

/// Whether `owner` is in the player's group: the six group names, as the
/// program's test 0x4bb226 reads them (R49-8e).
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
        if client.runs_r46_forms() {
            for line in answer(&mut hidden, world, &words, messages) {
                chat.history.push(system_line(line));
            }
        } else {
            chat.refuse(Said::own(NOT_YET));
        }
    }
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
    } else if is(LOOTED) {
        corpses_hidden.after_loot = true;
        vec![said(LOOTED_LINE)]
    } else {
        corpses_hidden.hidden.clear();
        corpses_hidden.after_loot = false;
        vec![said(SHOWN_LINE)]
    }
}

#[cfg(test)]
mod tests;
