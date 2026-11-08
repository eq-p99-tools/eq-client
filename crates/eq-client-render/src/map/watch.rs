//! Saved exact-name watches and appearance alerts, also active while the map is closed.
use crate::{
    chat::{ChatState, Said},
    online::OnlineState,
    profile_files::{Profile, Store},
};
use bevy::prelude::*;
use eq_client_core::{SpawnKind, maps::Watches, world::ClientWorld};
use std::{collections::BTreeSet, time::Instant};

const SHARED: &str = "map-watches.txt";

#[derive(Resource, Default)]
pub(crate) struct WatchState {
    /// Counts local appearance notices so the desktop can request attention.
    pub(crate) revision: u64,
    pub(super) names: Watches,
    pub(super) last_alert: String,
    pub(super) list_open: bool,
    dirty: bool,
    store: Store,
    tracker: Tracker,
}

impl WatchState {
    /// Whether the displayed name is tracked, independent of map search.
    pub(crate) fn tracks(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Whether any saved watch can match a scene entity.
    pub(crate) fn has_names(&self) -> bool {
        self.names.names().next().is_some()
    }

    pub(super) fn add(&mut self, name: &str) {
        self.dirty |= self.names.add(name);
    }

    pub(super) fn remove(&mut self, name: &str) {
        self.dirty |= self.names.remove(name);
    }
}

#[derive(Default)]
struct Tracker {
    admission: Option<u64>,
    seen: BTreeSet<(u16, u64)>,
}

impl Tracker {
    /// An appearance is an admission-scoped spawn revision, not a movement
    /// update. "Spotted" also covers an entity sent in the initial zone list.
    fn update(&mut self, world: &ClientWorld, watches: &Watches) -> Vec<String> {
        if self.admission != world.session_id() {
            self.admission = world.session_id();
            self.seen.clear();
        }
        let mut present = BTreeSet::new();
        let mut alerts = Vec::new();
        for spawn in world.spawns().values() {
            let state = &spawn.state;
            if !matches!(state.kind, SpawnKind::Npc | SpawnKind::Player)
                || !watches.contains(&state.name)
            {
                continue;
            }
            let identity = (state.spawn_id, spawn.revision);
            present.insert(identity);
            if !self.seen.contains(&identity) {
                alerts.push(format!(
                    "Tracked entity: {} spotted in {} (entity {}, X {:.1}, Y {:.1}, Z {:.1})",
                    eq_client_core::entities::display_name(&state.name),
                    world.zone(),
                    state.spawn_id,
                    state.position.x,
                    state.position.y,
                    state.position.z
                ));
            }
        }
        self.seen = present;
        alerts
    }
}

/// Load per-character watches and retry writes through the shared atomic store.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn persist(
    settings: Res<crate::ViewerSettings>,
    profile: Res<Profile>,
    mut state: ResMut<WatchState>,
) {
    let state = state.bypass_change_detection();
    state.store.retry(Instant::now());
    if state.store.loaded && state.store.profile == profile.key() {
        if state.dirty {
            save(state);
        }
        return;
    }
    if state.store.loaded {
        save(state);
    } else {
        state.store.open(settings.0.settings_directory.as_ref());
    }
    state.store.profile = profile.key();
    let text = state
        .store
        .read(&file_name(&state.store))
        .or_else(|| state.store.read(SHARED))
        .unwrap_or_default();
    state.names = Watches::read(&text);
    state.store.loaded_text(state.names.text());
    state.dirty = false;
    state.tracker = Tracker::default();
    state.last_alert.clear();
}

fn file_name(store: &Store) -> String {
    crate::profile_files::name("map-watches", SHARED, store.profile.as_ref())
}

fn save(state: &mut WatchState) {
    state
        .store
        .save(file_name(&state.store), state.names.text(), Instant::now());
    state.dirty = false;
}

/// Surface new appearances as local chat notices and in the map's banner.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn alert(
    online: Res<OnlineState>,
    mut state: ResMut<WatchState>,
    mut chat: ResMut<ChatState>,
) {
    let state = &mut *state;
    for alert in state.tracker.update(online.world(), &state.names) {
        state.revision = state.revision.wrapping_add(1);
        state.last_alert.clone_from(&alert);
        chat.history
            .push(crate::chat::system_line(Said::own(alert)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing as session;
    use eq_client_core::{WorldEvent, WorldPosition};

    #[test]
    fn watches_alert_once_per_appearance_then_again_on_respawn_and_admission() {
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(1));
        let mut watches = Watches::default();
        watches.add("Gabober");
        let mut tracker = Tracker::default();
        session::spawns(&mut online, vec![session::pet(2, 1)]);
        assert_eq!(tracker.update(online.world(), &watches).len(), 1);
        assert_eq!(
            tracker.update(online.world(), &watches),
            Vec::<String>::new()
        );
        session::news(
            &mut online,
            [WorldEvent::Position {
                spawn_id: 2,
                position: WorldPosition {
                    x: 20.0,
                    ..default()
                },
                velocity: [0.0; 3],
                forward: 0.0,
            }],
        );
        assert_eq!(
            tracker.update(online.world(), &watches),
            Vec::<String>::new()
        );
        session::news(&mut online, [WorldEvent::Despawn(2)]);
        session::spawns(&mut online, vec![session::pet(2, 1)]);
        assert_eq!(tracker.update(online.world(), &watches).len(), 1);
        session::admit(&mut online, 2, session::player(1));
        session::spawns(&mut online, vec![session::pet(2, 1)]);
        assert_eq!(tracker.update(online.world(), &watches).len(), 1);
        watches.remove("Gabober");
        assert_eq!(
            tracker.update(online.world(), &watches),
            Vec::<String>::new()
        );
    }
}
