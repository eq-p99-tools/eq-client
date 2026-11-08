//! Personal map locations. Stored coordinates are either entered explicitly
//! or copied from the local player's current displayed position.
use super::{MapMotion, MapView, controls::SearchBox, watch::WatchState};
use crate::{
    keys::Typing,
    online::OnlineState,
    profile_files::{self, CharacterProfile, Profile, Store},
};
use bevy::prelude::*;
use eq_client_core::WorldPosition;
use std::{collections::BTreeMap, time::Instant};

mod ui;
pub(super) use ui::chrome;
pub(crate) use ui::{List, draw, present};

const LIMIT: usize = 64;
const LABEL_BYTES: usize = 72;

#[derive(Debug, PartialEq, Eq)]
struct Scope {
    file: String,
    zone: String,
    admission: u64,
}

/// Saved locations for the current profile and zone, with local editor state.
#[derive(Resource, Default)]
pub(crate) struct Landmarks {
    store: Store,
    scope: Option<Scope>,
    places: BTreeMap<String, [f32; 3]>,
    selected: Option<String>,
    writable: bool,
    status: String,
    pub(super) draft: String,
    pub(super) open: bool,
    revision: u64,
}

impl Landmarks {
    #[cfg(test)]
    pub(super) fn preview() -> Self {
        Self {
            places: BTreeMap::from([("Meeting spot".into(), [75.0, -75.0, 0.0])]),
            ..default()
        }
    }

    pub(super) fn points(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.places.values().copied().map(map_at)
    }

    fn save(&mut self) {
        if let Some(scope) = &self.scope {
            self.store
                .save(scope.file.clone(), encode(&self.places), Instant::now());
        }
        self.revision = self.revision.wrapping_add(1);
    }
}

fn map_at([x, y, _]: [f32; 3]) -> Vec2 {
    Vec2::new(-x, -y)
}

fn file_name(profile: &CharacterProfile, zone: &str) -> String {
    use std::fmt::Write as _;
    let mut prefix = String::from("map-landmarks-");
    for byte in zone.bytes() {
        let _ = write!(prefix, "{byte:02x}");
    }
    profile_files::exact_name(&prefix, profile)
}

fn valid(label: &str, point: [f32; 3]) -> bool {
    !label.trim().is_empty()
        && label.len() <= LABEL_BYTES
        && !label.contains(char::is_control)
        && point.into_iter().all(f32::is_finite)
}

/// One JSON record per line keeps labels with spaces and quotes round-trippable.
fn decode(text: &str) -> BTreeMap<String, [f32; 3]> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<(String, [f32; 3])>(line).ok())
        .filter(|(name, point)| valid(name, *point))
        .take(LIMIT)
        .collect()
}

fn encode(places: &BTreeMap<String, [f32; 3]>) -> String {
    let mut text = String::new();
    for entry in places {
        if let Ok(line) = serde_json::to_string(&entry) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    text
}

/// Reuse the character profile store, with an exact zone key and no shared
/// fallback. A scope switch drops draft text and selections.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn follow(
    settings: Res<crate::ViewerSettings>,
    profile: Res<Profile>,
    online: Res<OnlineState>,
    view: Res<MapView>,
    mut typing: ResMut<Typing>,
    mut state: ResMut<Landmarks>,
) {
    let state = state.bypass_change_detection();
    state.store.retry(Instant::now());
    let world = online.world();
    let scope = profile
        .key_ref()
        .zip(world.session_id())
        .filter(|_| world.in_world() && !world.zone().is_empty());
    // The profile and short zone name can stay unchanged for many frames.
    if let Some((_, admission)) = scope {
        if state
            .scope
            .as_ref()
            .is_some_and(|old| old.admission == admission && old.zone == world.zone())
            && !profile.is_changed()
        {
            return;
        }
    } else if state.scope.is_none() {
        return;
    }
    let scope = scope.map(|(profile, admission)| Scope {
        file: file_name(profile, world.zone()),
        zone: world.zone().to_owned(),
        admission,
    });
    if state.scope == scope {
        return;
    }
    if view.field == SearchBox::Landmark {
        typing.map_search = false;
    }
    state.scope = scope;
    state.open = false;
    state.draft.clear();
    state.selected = None;
    state.status.clear();
    state.revision = state.revision.wrapping_add(1);
    if !state.store.loaded {
        state.store.open(settings.0.settings_directory.as_ref());
    }
    let loaded = state
        .scope
        .as_ref()
        .map(|scope| state.store.read_result(&scope.file));
    state.writable = state.store.directory.is_some() && loaded.as_ref().is_some_and(Result::is_ok);
    state.places = match loaded {
        Some(Ok(Some(text))) => decode(&text),
        Some(Err(error)) => {
            warn!("Could not read map landmarks: {error}");
            BTreeMap::new()
        }
        _ => BTreeMap::new(),
    };
    state.store.loaded_text(encode(&state.places));
}

#[derive(Component)]
pub(crate) enum Action {
    Toggle,
    Save,
    Locate(String),
    Remove,
}

/// Reads a label and optional explicit X Y Z. A plain name saves the current
/// location, if one is available.
fn draft(input: &str, player: Option<WorldPosition>) -> Option<(&str, [f32; 3])> {
    let (label, point) = if let Some((name, coordinates)) = input.split_once('|') {
        let mut words = coordinates.split_whitespace();
        let mut number = || words.next()?.parse::<f32>().ok();
        let point = [number()?, number()?, number()?];
        if words.next().is_some() {
            return None;
        }
        (name.trim(), point)
    } else {
        let position = player?;
        (input.trim(), [position.x, position.y, position.z])
    };
    valid(label, point).then_some((label, point))
}

/// Save, locate or delete a landmark without sending a game command.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn actions(
    clicks: crate::clicks::Clicks<Action>,
    online: Res<OnlineState>,
    motion: Res<MapMotion>,
    mut view: ResMut<MapView>,
    mut state: ResMut<Landmarks>,
    mut watches: ResMut<WatchState>,
) {
    let state = &mut *state;
    for action in clicks.iter() {
        if matches!(action, Action::Toggle) {
            state.open = !state.open;
            if state.open {
                watches.list_open = false;
            }
        } else if state.open && online.world().in_world() {
            match action {
                Action::Save if state.writable => {
                    if let Some((name, point)) = draft(&state.draft, motion.player(online.world()))
                    {
                        let name = name.to_owned();
                        if state.places.len() < LIMIT || state.places.contains_key(&name) {
                            let replaced = state.places.insert(name.clone(), point).is_some();
                            state.selected = Some(name);
                            state.draft.clear();
                            state.status = if replaced {
                                "Landmark updated"
                            } else {
                                "Landmark saved"
                            }
                            .into();
                            state.save();
                        } else {
                            state.status = "This zone's landmark list is full".into();
                        }
                    } else {
                        state.status = "Enter a name or Name | X Y Z".into();
                    }
                }
                Action::Locate(name) => {
                    if let Some(point) = state.places.get(name).copied() {
                        view.bounds = Some(super::nearby_bounds(map_at(point)));
                        view.pan = Vec2::ZERO;
                        view.zoom = 0;
                        state.selected = Some(name.clone());
                        state.status.clear();
                    }
                }
                Action::Remove if state.writable => {
                    if let Some(name) = state.selected.take() {
                        state.places.remove(&name);
                        state.status = "Landmark removed".into();
                        state.save();
                    }
                }
                _ => (),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_landmark_can_save_here_or_use_explicit_world_coordinates() {
        let here = WorldPosition {
            x: 3.0,
            y: -2.0,
            z: 7.0,
            ..default()
        };
        assert_eq!(
            draft("Meeting spot", Some(here)),
            Some(("Meeting spot", [3.0, -2.0, 7.0]))
        );
        assert_eq!(
            draft("Bridge | -100 50 2.5", None),
            Some(("Bridge", [-100.0, 50.0, 2.5]))
        );
        for bad in [
            "Empty",
            " | 1 2 3",
            "One | 1 2",
            "Four | 1 2 3 4",
            "Unbounded | 1 NaN 3",
        ] {
            assert_eq!(draft(bad, None), None, "{bad}");
        }
    }

    #[test]
    fn persisted_landmarks_are_bounded_and_collision_free_by_profile_and_zone() {
        let places = BTreeMap::from([("Inn \"East\"".to_owned(), [1.5, 2.0, -3.0])]);
        assert_eq!(
            decode(&(encode(&places) + "broken\n[\"bad\", [1, null, 0]]\n")),
            places
        );
        let overflow: BTreeMap<_, _> = (0..=LIMIT)
            .map(|index| (index.to_string(), [0.0; 3]))
            .collect();
        assert_eq!(decode(&encode(&overflow)).len(), LIMIT);
        let profile = |world: &str, character: &str| CharacterProfile {
            world: world.into(),
            character: character.into(),
        };
        let p = profile("local-eq", "somebody");
        assert_ne!(file_name(&p, "zone-a"), file_name(&p, "zone_a"));
        assert_ne!(
            file_name(&p, "zone-a"),
            file_name(&profile("local_eq", "somebody"), "zone-a")
        );
        assert_ne!(
            file_name(&p, "zone-a"),
            file_name(&profile("local-eq", "another"), "zone-a")
        );
    }

    #[test]
    fn projected_landmark_and_bearing_share_map_axes() {
        let player = WorldPosition::default();
        assert_eq!(map_at([0.0, 10.0, 200.0]), Vec2::new(0.0, -10.0));
        assert_eq!(
            ui::guidance(player, [0.0, 10.0, 200.0]),
            " · 10 units at 0°"
        );
        assert_eq!(
            ui::guidance(player, [-10.0, 0.0, 200.0]),
            " · 10 units at 90°"
        );
    }

    #[test]
    fn switching_worlds_clears_transient_editor_state_and_loads_exact_files() {
        use crate::online::testing as session;
        use eq_client_core::WorldEvent;
        let directory =
            std::env::temp_dir().join(format!("eq-landmark-scope-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut app = crate::testing::app();
        app.add_systems(Update, (profile_files::follow, follow).chain());
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .settings_directory = Some(directory.clone());
        let mut online = OnlineState::new(true);
        let player = session::player(1);
        let profile = CharacterProfile {
            world: "First".into(),
            character: player.name.clone(),
        };
        let server = |short_name: &str| WorldEvent::WorldName {
            short_name: short_name.into(),
        };
        session::news(&mut online, [server("First")]);
        session::admit(&mut online, 1, player);
        let places = BTreeMap::from([("Inn".to_owned(), [1.0, 2.0, 3.0])]);
        let path = directory.join(file_name(&profile, online.world().zone()));
        std::fs::write(path, encode(&places)).unwrap();
        app.insert_resource(online);
        app.update();
        {
            let mut state = app.world_mut().resource_mut::<Landmarks>();
            assert_eq!(state.places, places);
            assert!(state.writable);
            state.draft = "Unfinished".into();
            state.open = true;
            state.selected = Some("Inn".into());
        }
        session::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [server("Second")],
        );
        app.update();
        let state = app.world().resource::<Landmarks>();
        assert!(state.places.is_empty());
        assert_eq!(state.draft, "");
        assert!(!state.open);
        assert!(state.selected.is_none());
        session::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [server("First")],
        );
        app.update();
        assert_eq!(app.world().resource::<Landmarks>().places, places);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
