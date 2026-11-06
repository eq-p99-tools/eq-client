//! Named local layouts for the eight server-owned gems.
mod apply;
pub(crate) mod ui;

use crate::profile_files::{self, CharacterProfile, Store};
use bevy::prelude::*;
use eq_client_core::{Capability, world::ClientWorld};
use std::{collections::BTreeMap, fmt::Write as _, time::Instant};

type Gems = [Option<u32>; 8];
pub(crate) type AdmissionGems = (u64, Gems);
const MAX_SETS: usize = 32;

#[derive(Resource, Default)]
pub(crate) struct Sets {
    store: Store,
    saved: BTreeMap<String, Gems>,
    load_error: bool,
    panel: ui::Panel,
    active: Option<apply::Apply>,
    status: String,
}

impl Sets {
    /// Discard all unsent intent, including when resets and new admissions
    /// arrive in the same frame.
    pub(crate) fn reset(&mut self, old_gems: Option<AdmissionGems>) {
        if let Some(active) = self.active.as_mut() {
            active.invalidate(old_gems);
        }
        self.panel = ui::Panel::default();
    }

    fn follow(&mut self, profile: Option<CharacterProfile>) {
        if self.store.profile == profile {
            return;
        }
        self.reset(None);
        self.store.profile = profile;
        let result = self.file_name().map_or_else(
            || Ok(BTreeMap::new()),
            |name| {
                self.store
                    .read_result(&name)?
                    .as_deref()
                    .map_or_else(|| Ok(BTreeMap::new()), decode)
            },
        );
        self.load_error = result.is_err();
        self.saved = result.unwrap_or_default();
        self.status.clear();
        self.store.loaded_text(encode(&self.saved));
    }

    fn file_name(&self) -> Option<String> {
        self.store.profile.as_ref().map(|profile| {
            format!(
                "spell-sets-v1-{}-{}.txt",
                byte_hex(&profile.world),
                byte_hex(&profile.character)
            )
        })
    }

    fn writable(&self) -> bool {
        self.store.directory.is_some() && self.store.profile.is_some() && !self.load_error
    }

    fn save(&mut self) {
        if let Some(name) = self.file_name() {
            self.store.save(name, encode(&self.saved), Instant::now());
        }
    }

    fn checked_name(&self, input: &str, renaming: Option<&str>) -> Result<String, &'static str> {
        let name = valid_name(input.trim())?;
        let lower = name.to_lowercase();
        if self
            .saved
            .keys()
            .any(|saved| Some(saved.as_str()) != renaming && saved.to_lowercase() == lower)
        {
            return Err("A set already has that name");
        }
        Ok(name.to_owned())
    }
}

/// Use exact, separately delimited components on case-insensitive filesystems.
fn byte_hex(text: &str) -> String {
    let mut hex = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        write!(hex, "{byte:02x}").expect("writing to a String succeeds");
    }
    hex
}

/// Keep the last layout visible in the old admission, even when the next
/// event replaces that admission before the set's input system runs.
pub(crate) fn snapshot(world: &ClientWorld) -> Option<AdmissionGems> {
    world
        .session_id()
        .filter(|_| world.in_world())
        .map(|session| (session, world.gems()))
}

fn valid_name(name: &str) -> Result<&str, &'static str> {
    if name.is_empty() || name.chars().count() > 32 || name.chars().any(char::is_control) {
        return Err("Use a name of 1–32 characters");
    }
    Ok(name)
}

/// An invalid file stays intact until repaired. Never load only part of a layout.
fn decode(text: &str) -> anyhow::Result<BTreeMap<String, Gems>> {
    use anyhow::ensure;
    let saved: BTreeMap<String, Gems> = serde_json::from_str(text)?;
    ensure!(saved.len() <= MAX_SETS, "too many spell sets");
    let mut names = std::collections::BTreeSet::new();
    for (name, gems) in &saved {
        ensure!(
            name.trim() == name && valid_name(name).is_ok() && names.insert(name.to_lowercase()),
            "invalid or duplicate spell set name"
        );
        ensure!(
            gems.iter()
                .flatten()
                .all(|id| !matches!(*id, 0 | 0xffff | u32::MAX)),
            "invalid spell ID"
        );
    }
    Ok(saved)
}

fn encode(saved: &BTreeMap<String, Gems>) -> String {
    serde_json::to_string_pretty(saved).expect("spell sets contain only names and gem IDs")
}

/// Load the active character's settings before the book takes keyboard input.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn follow(
    settings: Res<crate::ViewerSettings>,
    profile: Res<profile_files::Profile>,
    mut sets: ResMut<Sets>,
) {
    if !sets.store.loaded {
        sets.store.open(settings.0.settings_directory.as_ref());
    }
    sets.store.retry(Instant::now());
    sets.follow(profile.key());
}

fn eligible(world: &ClientWorld, client: eq_client_assets::ui::InstalledClient) -> bool {
    world.in_world()
        && world.can(Capability::Spellbook)
        && world.spell_book().is_some()
        && world.player().is_some_and(|player| {
            player
                .class
                .is_none_or(|class| client.spellbook_class(class))
        })
}
