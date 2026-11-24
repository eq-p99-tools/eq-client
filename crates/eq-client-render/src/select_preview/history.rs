//! Private last-known positions for selection scenes, never gameplay placement.
use crate::{ViewerSettings, login::FrontEnd, online::OnlineState, profile_files};
use bevy::prelude::*;
use eq_client_core::WorldPosition;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    time::{Duration, Instant},
};

const FILE_BYTES: usize = 4096;
const SAVE_INTERVAL: Duration = Duration::from_secs(1);
// Well beyond supported zone bounds; also excludes finite corrupt values that
// would overflow scene/camera calculations. Zero is a legitimate world point.
const COORDINATE_LIMIT: f32 = 1_000_000.0;

type Key = (String, String, String);

#[derive(Clone, Debug, PartialEq)]
struct Location {
    zone: String,
    position: WorldPosition,
}

struct Known {
    store: profile_files::Store,
    file: String,
    location: Option<Location>,
    dirty: bool,
    saved: Option<Instant>,
    writable: bool,
}

impl Known {
    fn flush(&mut self, now: Instant) {
        if self.dirty
            && self.writable
            && let Some(location) = &self.location
        {
            self.store.save(self.file.clone(), encode(location), now);
            self.saved = Some(now);
            self.dirty = false;
        }
        self.store.retry(now);
    }
}

/// Exact profile records remain separate, including pending atomic writes.
#[derive(Resource, Default)]
pub(crate) struct History {
    known: BTreeMap<Key, Known>,
    active: Option<Key>,
}

impl History {
    fn known(&mut self, root: Option<&std::path::PathBuf>, key: &Key) -> &mut Known {
        self.known.entry(key.clone()).or_insert_with(|| {
            let profile = profile_files::CharacterProfile {
                world: key.1.clone(),
                character: key.2.clone(),
            };
            let file = profile_files::exact_name("last-location", &profile);
            let directory = root.map(|root| {
                let mut directory = root.join("select-locations-v1");
                // Exact bytes, split into ordinary path components. No sanitizer
                // collisions, process-dependent hashes, or account-wide fallback.
                let mut hex = String::new();
                for byte in key.0.bytes() {
                    let _ = write!(hex, "{byte:02x}");
                }
                for part in hex.as_bytes().chunks(96) {
                    directory.push(std::str::from_utf8(part).expect("hex is ASCII"));
                }
                directory
            });
            let mut store = profile_files::Store::default();
            store.open(directory.as_ref());
            store.protect_existing_size(FILE_BYTES);
            let read = store.read_bounded(&file, FILE_BYTES);
            let writable = read.is_ok();
            let location = read.ok().flatten().and_then(|text| decode(&text));
            store.loaded_text(location.as_ref().map_or_else(String::new, encode));
            Known {
                store,
                file,
                location,
                dirty: false,
                saved: None,
                writable,
            }
        })
    }

    /// No history from another login profile, server, character or zone is used.
    pub(super) fn position(
        &mut self,
        root: Option<&std::path::PathBuf>,
        scope: &str,
        profile: &profile_files::CharacterProfile,
        zone: &str,
    ) -> Option<WorldPosition> {
        if scope.is_empty() {
            return None;
        }
        let key = (
            scope.to_owned(),
            profile.world.clone(),
            profile.character.clone(),
        );
        self.known(root, &key)
            .location
            .as_ref()
            .filter(|location| location.zone == zone)
            .map(|location| location.position)
    }

    fn observe(
        &mut self,
        root: Option<&std::path::PathBuf>,
        next: Option<(Key, Location)>,
        now: Instant,
    ) {
        let key = next.as_ref().map(|(key, _)| key);
        if self.active.as_ref() != key {
            if let Some(previous) = self.active.take().and_then(|key| self.known.get_mut(&key)) {
                previous.flush(now);
            }
            self.active = key.cloned();
        }
        if let Some((key, location)) = next {
            let known = self.known(root, &key);
            if known.location.as_ref() != Some(&location) {
                known.location = Some(location);
                known.dirty = true;
            }
            if known
                .saved
                .is_none_or(|at| now.saturating_duration_since(at) >= SAVE_INTERVAL)
            {
                known.flush(now);
            }
        }
        for known in self.known.values_mut() {
            known.store.retry(now);
        }
    }
}

fn valid(location: &Location) -> bool {
    eq_client_core::zones::long_name(&location.zone).is_some()
        && [
            location.position.x,
            location.position.y,
            location.position.z,
        ]
        .into_iter()
        .all(|value| value.is_finite() && value.abs() <= COORDINATE_LIMIT)
        && location.position.heading.is_finite()
        && (0.0..512.0).contains(&location.position.heading)
}

fn encode(location: &Location) -> String {
    let p = location.position;
    serde_json::to_string(&(1, &location.zone, [p.x, p.y, p.z, p.heading]))
        .expect("a valid location is JSON")
}

fn decode(text: &str) -> Option<Location> {
    let (version, zone, [x, y, z, heading]): (u8, String, [f32; 4]) =
        serde_json::from_str(text).ok()?;
    let location = Location {
        zone,
        position: WorldPosition { x, y, z, heading },
    };
    (version == 1 && valid(&location)).then_some(location)
}

/// Read the current admission's actual session position, never the scene's
/// default origin, a selection model, or a delayed interpolation from another
/// admission. Accepted local movement is last-known client state, not a server ACK.
fn current(
    online: &OnlineState,
    front: &FrontEnd,
    scene: Option<&crate::SceneInfo>,
) -> Option<(Key, Location)> {
    let world = online.world();
    if !online.enabled
        || !world.in_world()
        || online.scene_loading()
        || front.screen(online) != crate::login::Screen::World
        || scene.is_none_or(|scene| scene.zone_name != world.zone())
        || online.server_placement().is_none()
    {
        return None;
    }
    let profile = profile_files::playing(world)?;
    let key = (
        front
            .location_scope()
            .filter(|scope| !scope.is_empty())?
            .to_owned(),
        profile.world,
        profile.character,
    );
    let location = Location {
        zone: world.zone().to_owned(),
        position: world.player()?.position,
    };
    valid(&location).then_some((key, location))
}

/// Coalesce moving samples, flush the last observation on camp, zoning or a
/// profile change, and retry failed saves through the ordinary settings store.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn remember(
    settings: Res<ViewerSettings>,
    online: Res<OnlineState>,
    front: Res<FrontEnd>,
    scene: Option<Res<crate::SceneInfo>>,
    mut history: ResMut<History>,
) {
    history.observe(
        settings.0.settings_directory.as_ref(),
        current(&online, &front, scene.as_deref()),
        Instant::now(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;

    fn profile(world: &str, character: &str) -> profile_files::CharacterProfile {
        profile_files::CharacterProfile {
            world: world.into(),
            character: character.into(),
        }
    }

    fn point(zone: &str, x: f32) -> Location {
        Location {
            zone: zone.into(),
            position: WorldPosition {
                x,
                y: 12.0,
                z: 6.0,
                heading: 128.0,
            },
        }
    }

    fn key(scope: &str, world: &str, character: &str) -> Key {
        (scope.into(), world.into(), character.into())
    }

    fn folder() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "select-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn exact_profile_and_zone_reuse_survives_restart_without_shared_fallback() {
        let root = folder();
        let now = Instant::now();
        let location = point("qeynos", 80.0);
        let mut history = History::default();
        history.observe(
            Some(&root),
            Some((key("endpoint/account", "World", "One"), location.clone())),
            now,
        );
        drop(history);
        let mut restarted = History::default();
        assert_eq!(
            restarted.position(
                Some(&root),
                "endpoint/account",
                &profile("World", "One"),
                "qeynos"
            ),
            Some(location.position)
        );
        for (scope, world, character, zone) in [
            ("endpoint/account", "World", "One", "erudnint"),
            ("endpoint/account", "World", "Two", "qeynos"),
            ("endpoint/account", "world", "One", "qeynos"),
            ("endpoint/other-account", "World", "One", "qeynos"),
            ("other-endpoint/account", "World", "One", "qeynos"),
        ] {
            assert!(
                restarted
                    .position(Some(&root), scope, &profile(world, character), zone)
                    .is_none()
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sanitized_name_collisions_and_multiple_dirty_profiles_remain_separate() {
        let root = folder();
        let mut history = History::default();
        let now = Instant::now();
        for (scope, world, character, x) in [
            ("a/b", "W/a", "One", 1.0),
            ("a_b", "W/a", "One", 2.0),
            ("a/b", "W_a", "One", 3.0),
            ("a/b", "W/a", "one", 4.0),
        ] {
            history.observe(
                Some(&root),
                Some((key(scope, world, character), point("qeynos", x))),
                now,
            );
        }
        let mut restarted = History::default();
        for (scope, world, character, x) in [
            ("a/b", "W/a", "One", 1.0),
            ("a_b", "W/a", "One", 2.0),
            ("a/b", "W_a", "One", 3.0),
            ("a/b", "W/a", "one", 4.0),
        ] {
            assert_eq!(
                restarted
                    .position(Some(&root), scope, &profile(world, character), "qeynos")
                    .unwrap()
                    .x,
                x
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn camp_or_transfer_flushes_the_latest_sample_before_the_save_interval() {
        let root = folder();
        let mut history = History::default();
        let now = Instant::now();
        let identity = key("scope", "World", "One");
        history.observe(
            Some(&root),
            Some((identity.clone(), point("qeynos", 1.0))),
            now,
        );
        history.observe(
            Some(&root),
            Some((identity, point("qeynos", 9.0))),
            now + SAVE_INTERVAL / 2,
        );
        history.observe(Some(&root), None, now + SAVE_INTERVAL / 2);
        let mut restarted = History::default();
        assert_eq!(
            restarted
                .position(Some(&root), "scope", &profile("World", "One"), "qeynos")
                .unwrap()
                .x,
            9.0
        );
        // A new zone replaces this character's last zone; it cannot revive an
        // older location when the server's list names a different one.
        history.observe(
            Some(&root),
            Some((key("scope", "World", "One"), point("erudnint", 20.0))),
            now + SAVE_INTERVAL,
        );
        assert!(
            history
                .position(Some(&root), "scope", &profile("World", "One"), "qeynos")
                .is_none()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_future_unknown_or_nonfinite_history_is_not_a_preview_position() {
        for text in [
            "",
            "not json",
            "[2,\"qeynos\",[1,2,3,4]]",
            "[1,\"unknown-zone\",[1,2,3,4]]",
            "[1,\"qeynos\",[1e38,2,3,4]]",
            "[1,\"qeynos\",[1,2,3,NaN]]",
            "[1,\"qeynos\",[1,2,3,512]]",
        ] {
            assert!(decode(text).is_none(), "{text}");
        }
        assert!(decode("[1,\"qeynos\",[0,0,0,0]]").is_some());
        let mut location = point("qeynos", f32::NAN);
        assert!(!valid(&location));
        location.position.x = 1.0;
        location.position.heading = f32::INFINITY;
        assert!(!valid(&location));
    }

    #[test]
    fn only_the_received_current_admission_can_supply_a_location() {
        let mut online = OnlineState::new(true);
        let front = crate::login::testing::scoped("scope");
        let scene = crate::SceneInfo {
            zone_name: "qeytoqrg".into(),
        };
        assert!(current(&online, &front, Some(&scene)).is_none());
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::WorldName {
                short_name: "World".into(),
            }],
        );
        let mut player = testing::player(7);
        player.position = point("qeytoqrg", 50.0).position;
        testing::admit(&mut online, 1, player.clone());
        assert_eq!(
            current(&online, &front, Some(&scene)).unwrap().1.position.x,
            50.0
        );
        assert!(current(&online, &front, None).is_none());
        assert!(current(&online, &FrontEnd::default(), Some(&scene)).is_none());
        let other_scene = crate::SceneInfo {
            zone_name: "erudnint".into(),
        };
        assert!(current(&online, &front, Some(&other_scene)).is_none());
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::ZoneTransfer(
                eq_client_core::ZoneOffer {
                    zone_id: 24,
                    instance_id: 0,
                    position: WorldPosition::default(),
                    reason: 0,
                    to_bind: false,
                    solicited: true,
                },
            )],
        );
        assert!(current(&online, &front, Some(&scene)).is_none());
        // An obsolete correction cannot furnish the next admission's pose.
        testing::admit(&mut online, 2, player);
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::MotionSent {
                session_id: 1,
                created: Instant::now(),
                position: WorldPosition::default(),
                refused: None,
            }],
        );
        assert_eq!(
            current(&online, &front, Some(&scene)).unwrap().1.position.x,
            50.0
        );
        testing::place_axis(&mut online, |p| p.x = 90.0);
        assert_eq!(
            current(&online, &front, Some(&scene)).unwrap().1.position.x,
            90.0
        );
        testing::connect(&mut online, false);
        assert!(current(&online, &front, Some(&scene)).is_none());
    }
}
