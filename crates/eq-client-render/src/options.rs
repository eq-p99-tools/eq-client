//! The player's options: loaded for whoever is playing, saved as they
//! change, and told to the session where it acts on them.
use bevy::prelude::*;
use eq_client_core::food::AutoEat;
use eq_client_core::options::{Options, Toggle};
use std::path::PathBuf;

/// The file a character with none of their own starts from.
const SHARED: &str = "options.txt";

/// The options in effect, and where they are kept.
#[derive(Resource)]
pub(crate) struct OptionsState {
    /// The options in effect.
    pub(crate) options: Options,
    /// What a character with no options of their own starts with.
    defaults: Options,
    store: Store,
}

/// What the persisting systems know between frames.
#[derive(Default)]
struct Store {
    /// Whether the directory is known and a profile's options were loaded.
    loaded: bool,
    directory: Option<PathBuf>,
    /// The world and character whose options are in use; None before one enters.
    profile: Option<(String, String)>,
    /// The text last read or written for this profile, to skip unchanged writes.
    written: String,
    /// The admission last told what the session may eat, and what it was told.
    told: Option<(u64, AutoEat)>,
}

impl Default for OptionsState {
    fn default() -> Self {
        Self::new(Options::default())
    }
}

impl OptionsState {
    /// Options that start from these defaults until a character's own load.
    pub(crate) fn new(defaults: Options) -> Self {
        Self {
            options: defaults,
            defaults,
            store: Store::default(),
        }
    }

    /// Whether the toggle is on.
    pub(crate) const fn on(&self, toggle: Toggle) -> bool {
        self.options.get(toggle)
    }
}

/// An Options window checkbox: a click turns its option the other way.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OptionCheckbox(pub(crate) Toggle);

/// Turns an option the other way when its checkbox is clicked.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn toggle(
    clicks: Query<(&Interaction, &OptionCheckbox), Changed<Interaction>>,
    online: Res<crate::online::OnlineState>,
    mut state: ResMut<OptionsState>,
) {
    for (interaction, OptionCheckbox(toggle)) in &clicks {
        // A greyed checkbox, for what the session does not offer, stays
        // as it is.
        let offered = toggle
            .needs()
            .is_none_or(|needs| crate::outbox::offered(online.world(), needs));
        if *interaction == Interaction::Pressed && offered {
            let on = state.on(*toggle);
            state.options.set(*toggle, !on);
        }
    }
}

/// Loads the options of whoever is playing, and saves a change to their file.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn persist(
    settings: Res<crate::ViewerSettings>,
    profile: Res<crate::profile_files::Profile>,
    mut state: ResMut<OptionsState>,
) {
    if state.store.loaded && !profile.is_changed() {
        if state.is_changed() {
            save(&mut state);
        }
        return;
    }
    if state.store.loaded {
        save(&mut state);
    } else {
        state
            .store
            .directory
            .clone_from(&settings.0.settings_directory);
        state.store.loaded = true;
    }
    let state = &mut *state;
    state.store.profile = profile.key();
    let read = |name: &str| {
        state
            .store
            .directory
            .as_ref()
            .and_then(|directory| std::fs::read_to_string(directory.join(name)).ok())
    };
    // A character with no options of their own starts from the shared file.
    let text = read(&file_name(state.store.profile.as_ref()))
        .or_else(|| read(SHARED))
        .unwrap_or_default();
    state.options = Options::read(&text, state.defaults);
    state.store.written = state.options.text();
}

/// Writes the options to the profile's file when they changed since last time.
fn save(state: &mut OptionsState) {
    let Some(directory) = &state.store.directory else {
        return;
    };
    let text = state.options.text();
    if text == state.store.written {
        return;
    }
    let path = directory.join(file_name(state.store.profile.as_ref()));
    if let Err(error) = crate::profile_files::write(directory, &path, &text) {
        warn!("Could not save options to {}: {error}", path.display());
    }
    // A failed write waits for the next change rather than retrying each frame.
    state.store.written = text;
}

/// The shared file, or the profile's own.
fn file_name(profile: Option<&(String, String)>) -> String {
    crate::profile_files::name("options", SHARED, profile)
}

/// Tells each admission what the session may eat and drink on its own, and
/// tells it again when the player changes that.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn tell_session(
    online: Res<crate::online::OnlineState>,
    outbox: Res<crate::outbox::Outbox>,
    mut state: ResMut<OptionsState>,
) {
    let Some(session_id) = outbox.peek(online.world()).map(|stamp| stamp.session_id) else {
        return;
    };
    let auto_eat = state.options.auto_eat();
    if state.store.told == Some((session_id, auto_eat)) {
        return;
    }
    let command = eq_client_core::ClientCommand::AutoEat {
        session_id,
        auto_eat,
    };
    if outbox.tell(online.world(), command) {
        state.store.told = Some((session_id, auto_eat));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};

    fn app(directory: &std::path::Path) -> App {
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .settings_directory = Some(directory.to_owned());
        app.insert_resource(OptionsState::new(Options::default()))
            .add_systems(
                Update,
                (crate::profile_files::follow, toggle, persist).chain(),
            );
        app
    }

    fn enter(app: &mut App, name: &str) {
        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::WorldName {
                short_name: "ExampleWorld".into(),
            }],
        );
        let mut player = testing::player(7);
        player.name = name.into();
        testing::admit(&mut online, 1, player);
        app.insert_resource(online);
    }

    #[test]
    fn each_character_keeps_their_own_options() {
        let directory = std::env::temp_dir().join(format!(
            "eq-client-options-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        let mut app = app(&directory);
        enter(&mut app, "Example");
        app.update();
        assert!(app.world().resource::<OptionsState>().options.wheel_zoom);
        // Nothing is written until the player changes something.
        assert!(std::fs::read_dir(&directory).is_err());
        let checkbox = app
            .world_mut()
            .spawn((Interaction::Pressed, OptionCheckbox(Toggle::WheelZoom)))
            .id();
        app.update();
        assert!(!app.world().resource::<OptionsState>().options.wheel_zoom);
        let file = directory.join("options-ExampleWorld-Example.txt");
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("wheel_zoom = false")
        );
        // Another character starts from the defaults again.
        app.world_mut().despawn(checkbox);
        enter(&mut app, "Another");
        app.update();
        assert!(app.world().resource::<OptionsState>().options.wheel_zoom);
        enter(&mut app, "Example");
        app.update();
        assert!(!app.world().resource::<OptionsState>().options.wheel_zoom);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_setting_for_what_the_session_does_not_offer_stays_as_it_is() {
        use eq_client_core::{Capability, WorldEvent, qol::Fix};
        let mut app = crate::testing::app();
        app.insert_resource(OptionsState::new(Options::default()))
            .add_systems(Update, toggle);
        // A session without the inventory eats nothing on its own, so the
        // food setting is greyed out there.
        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: vec![Capability::Talking],
                session_id: 1,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
            }],
        );
        testing::connect(&mut online, true);
        app.insert_resource(online);
        let food = Toggle::Qol(Fix::SkipModifiedFood);
        app.world_mut()
            .spawn((Interaction::Pressed, OptionCheckbox(food)));
        app.update();
        assert!(app.world().resource::<OptionsState>().on(food));
        // Where the session offers it, a click turns it off.
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 2, testing::player(7));
        app.insert_resource(online);
        app.world_mut()
            .spawn((Interaction::Pressed, OptionCheckbox(food)));
        app.update();
        assert!(!app.world().resource::<OptionsState>().on(food));
    }
}
