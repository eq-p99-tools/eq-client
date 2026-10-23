//! The files this client keeps in the user's settings directory. As the
//! official client keeps its settings per character, each character on each
//! world has its own file; a shared one covers the screens before a
//! character enters and seeds characters that have none yet.
use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// Named identity shared by character-specific settings stores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CharacterProfile {
    pub(crate) world: String,
    pub(crate) character: String,
}

/// The character playing and their world, by name: whose settings files are
/// read. It changes only when either does, so a reader of the official
/// client's settings for the character reads them again on that change
/// alone.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub(crate) struct Profile(Option<CharacterProfile>);

impl Profile {
    /// The character's and the world's names, once a character plays.
    pub(crate) fn names(&self) -> Option<(&str, &str)> {
        self.0
            .as_ref()
            .map(|profile| (profile.character.as_str(), profile.world.as_str()))
    }

    /// The named identity used by this client's settings files.
    pub(crate) fn key(&self) -> Option<CharacterProfile> {
        self.0.clone()
    }
}

/// Keeps [`Profile`] on the character playing and their world, changing it
/// only when either changes.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn follow(online: Res<crate::online::OnlineState>, mut profile: ResMut<Profile>) {
    let current = online
        .world()
        .player()
        .map(|player| player.name.as_str())
        .zip(online.world().world_name());
    if profile.names() != current {
        profile.0 = current.map(|(character, world)| CharacterProfile {
            world: world.to_owned(),
            character: character.to_owned(),
        });
    }
}

/// The shared file's name, or `<prefix>-<world>-<character>.txt` with
/// anything but letters, digits, `-` and `_` replaced.
pub(crate) fn name(prefix: &str, shared: &str, profile: Option<&CharacterProfile>) -> String {
    let clean = |text: &str| -> String {
        text.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    profile.map_or_else(
        || shared.to_owned(),
        |profile| {
            format!(
                "{prefix}-{}-{}.txt",
                clean(&profile.world),
                clean(&profile.character)
            )
        },
    )
}

/// Retains dirty writes across profile switches and retries transient failures.
/// The observed text is only an input snapshot: pending entries remain dirty
/// until their atomic write succeeds.
#[derive(Default)]
pub(crate) struct Store {
    pub(crate) loaded: bool,
    pub(crate) directory: Option<PathBuf>,
    pub(crate) profile: Option<CharacterProfile>,
    observed: String,
    pending: BTreeMap<String, Pending>,
}

struct Pending {
    text: String,
    retry: Instant,
}

const RETRY: Duration = Duration::from_secs(1);

impl Store {
    /// Reads unsaved state before disk, including after returning to a profile.
    pub(crate) fn read(&self, name: &str) -> Option<String> {
        self.pending
            .get(name)
            .map(|pending| pending.text.clone())
            .or_else(|| std::fs::read_to_string(self.directory.as_ref()?.join(name)).ok())
    }

    /// Records the loaded codec's canonical form without scheduling a write.
    pub(crate) fn loaded_text(&mut self, text: String) {
        self.observed = text;
    }

    /// Queues a changed serialization and attempts its first write immediately.
    pub(crate) fn save(&mut self, name: String, text: String, now: Instant) {
        if self.directory.is_none() || self.observed == text {
            return;
        }
        self.observed.clone_from(&text);
        // Editing a dirty file keeps its backoff instead of retrying every frame.
        let retry = self.pending.get(&name).map_or(now, |pending| pending.retry);
        self.pending.insert(name, Pending { text, retry });
        self.retry(now);
    }

    /// Retries all dirty profiles at most once a second per file.
    pub(crate) fn retry(&mut self, now: Instant) {
        let Some(directory) = &self.directory else {
            return;
        };
        self.pending.retain(|name, pending| {
            if now < pending.retry {
                return true;
            }
            let path = directory.join(name);
            match write(directory, &path, &pending.text) {
                Ok(()) => false,
                Err(error) => {
                    warn!("Could not save settings to {}: {error}", path.display());
                    pending.retry = now + RETRY;
                    true
                }
            }
        });
    }
}

/// Atomically replaces a file using an exclusively created staging file, so
/// simultaneous client instances never share a temporary file.
pub(crate) fn write(directory: &Path, path: &Path, text: &str) -> std::io::Result<()> {
    use std::{
        fs::OpenOptions,
        io::Write,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(directory)?;
    let (partial, mut file) = loop {
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let partial = path.with_extension(format!("{}.{}.tmp", std::process::id(), number));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
        {
            Ok(file) => break (partial, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&partial, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(partial);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "eq-client-profile-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn failed_writes_recover_without_edits_and_survive_profile_changes() {
        let root = folder();
        let directory = root.join("settings");
        std::fs::write(&directory, "blocks directory creation").unwrap();
        let mut store = Store {
            directory: Some(directory.clone()),
            ..Default::default()
        };
        let now = Instant::now();
        store.save("first.txt".into(), "first changes".into(), now);
        store.loaded_text("second defaults".into());
        store.save("second.txt".into(), "second changes".into(), now);
        assert_eq!(store.pending.len(), 2);
        assert_eq!(store.read("first.txt").as_deref(), Some("first changes"));
        std::fs::remove_file(&directory).unwrap();
        std::fs::create_dir_all(&directory).unwrap();
        store.retry(now + RETRY / 2);
        assert!(
            !directory.join("first.txt").exists(),
            "backoff must still apply"
        );
        store.retry(now + RETRY);
        assert!(store.pending.is_empty());
        assert_eq!(
            std::fs::read_to_string(directory.join("first.txt")).unwrap(),
            "first changes"
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("second.txt")).unwrap(),
            "second changes"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn further_edits_replace_dirty_text_without_resetting_its_backoff() {
        let root = folder();
        let directory = root.join("settings");
        std::fs::write(&directory, "blocked").unwrap();
        let mut store = Store {
            directory: Some(directory.clone()),
            ..Default::default()
        };
        let now = Instant::now();
        store.save("options.txt".into(), "old".into(), now);
        store.save("options.txt".into(), "latest".into(), now + RETRY / 2);
        assert_eq!(store.pending["options.txt"].retry, now + RETRY);
        std::fs::remove_file(&directory).unwrap();
        store.retry(now + RETRY);
        assert_eq!(
            std::fs::read_to_string(directory.join("options.txt")).unwrap(),
            "latest"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_writers_publish_only_complete_files() {
        let directory = folder();
        let path = directory.join("options.txt");
        write(&directory, &path, &"0".repeat(32_768)).unwrap();
        std::thread::scope(|scope| {
            let handles: Vec<_> = (b'1'..=b'4')
                .map(|byte| {
                    let (directory, path) = (&directory, &path);
                    scope.spawn(move || {
                        let text = char::from(byte).to_string().repeat(32_768);
                        for _ in 0..8 {
                            write(directory, path, &text).unwrap();
                        }
                    })
                })
                .collect();
            while handles.iter().any(|handle| !handle.is_finished()) {
                let text = std::fs::read(&path).unwrap();
                assert_eq!(text.len(), 32_768);
                assert!(text.iter().all(|byte| *byte == text[0]));
                std::thread::yield_now();
            }
            for handle in handles {
                handle.join().unwrap();
            }
        });
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn each_character_on_each_world_has_a_file_of_its_own() {
        assert_eq!(name("windows", "windows.txt", None), "windows.txt");
        let profile = CharacterProfile {
            world: "P1999Green".into(),
            character: "Example".into(),
        };
        assert_eq!(
            name("options", "options.txt", Some(&profile)),
            "options-P1999Green-Example.txt"
        );
        let odd = CharacterProfile {
            world: "a/b".into(),
            character: "..".into(),
        };
        assert_eq!(
            name("options", "options.txt", Some(&odd)),
            "options-a_b-__.txt"
        );
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    use crate::online::{OnlineState, testing};

    #[test]
    fn the_profile_changes_only_when_the_character_or_world_does() {
        let mut app = App::new();
        app.init_resource::<Profile>()
            .insert_resource(OnlineState::new(true))
            .add_systems(Update, follow);
        app.update();
        assert_eq!(app.world().resource::<Profile>().names(), None);
        // The world names itself, and a character enters it.
        {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            testing::news(
                &mut online,
                [eq_client_core::WorldEvent::WorldName {
                    short_name: "testworld".into(),
                }],
            );
            testing::admit(&mut online, 1, testing::player(7));
        }
        app.update();
        let entered = app
            .world()
            .resource::<Profile>()
            .names()
            .map(|(_, world)| world);
        assert_eq!(entered, Some("testworld"));
        // Another frame with the same character changes nothing.
        let tick = app.world().resource_ref::<Profile>().last_changed();
        app.update();
        assert_eq!(app.world().resource_ref::<Profile>().last_changed(), tick);
    }
}
