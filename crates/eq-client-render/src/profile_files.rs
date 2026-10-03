//! The files this client keeps in the user's settings directory. As the
//! official client keeps its settings per character, each character on each
//! world has its own file; a shared one covers the screens before a
//! character enters and seeds characters that have none yet.
use bevy::prelude::*;
use std::path::Path;

/// The character playing and their world, by name: whose settings files are
/// read. It changes only when either does, so a reader of the official
/// client's settings for the character reads them again on that change
/// alone.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub(crate) struct Profile(Option<(String, String)>);

impl Profile {
    /// The character's and the world's names, once a character plays.
    pub(crate) fn names(&self) -> Option<(&str, &str)> {
        self.0
            .as_ref()
            .map(|(character, world)| (character.as_str(), world.as_str()))
    }

    /// The world's and the character's names, in the order this client's
    /// own files are named by them (see [`name`]).
    pub(crate) fn key(&self) -> Option<(String, String)> {
        self.0
            .as_ref()
            .map(|(character, world)| (world.clone(), character.clone()))
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
        profile.0 = current.map(|(character, world)| (character.to_owned(), world.to_owned()));
    }
}

/// The shared file's name, or `<prefix>-<world>-<character>.txt` with
/// anything but letters, digits, `-` and `_` replaced.
pub(crate) fn name(prefix: &str, shared: &str, profile: Option<&(String, String)>) -> String {
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
        |(world, character)| format!("{prefix}-{}-{}.txt", clean(world), clean(character)),
    )
}

/// Replaces the file in one step, so a crash never leaves half of it.
pub(crate) fn write(directory: &Path, path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let partial = path.with_extension("tmp");
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_character_on_each_world_has_a_file_of_its_own() {
        assert_eq!(name("windows", "windows.txt", None), "windows.txt");
        let profile = ("P1999Green".to_owned(), "Example".to_owned());
        assert_eq!(
            name("options", "options.txt", Some(&profile)),
            "options-P1999Green-Example.txt"
        );
        let odd = ("a/b".to_owned(), "..".to_owned());
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
