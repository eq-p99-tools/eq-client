//! Which UI skin the windows follow: the one the player loaded with
//! `/loadskin` this session, else the one named on the command line, else
//! the one the character last loaded here, else the one the character last
//! chose in the official client, else the default skin.
pub(crate) mod browser;

use bevy::prelude::*;
use eq_client_assets::ui::DEFAULT_SKIN;

/// The skin windows read their layouts from.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub(super) struct UiSkin(pub String);

impl Default for UiSkin {
    fn default() -> Self {
        Self(DEFAULT_SKIN.into())
    }
}

/// A `/loadskin` typed, waiting to switch the skin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoadSkin {
    /// Load the skin of this name: a folder of the installation's
    /// `uifiles`.
    Skin(String),
    /// The words could not be read: no skin named, or more than a name and
    /// the official client's `0` or `1` after it.
    Usage,
}

impl LoadSkin {
    /// The request a typed line makes, if it is a `/loadskin`. The official
    /// client takes `/loadskin <skin> [0|1]`, the number choosing whether
    /// windows keep their saved places (1) or open where the skin puts them
    /// (0); this client keeps the player's own placements either way, so
    /// the number is read and set aside. How the official client reads the
    /// words is not read from the program.
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let mut words = input.split_whitespace();
        if !words.next()?.eq_ignore_ascii_case("/loadskin") {
            return None;
        }
        Some(match words.collect::<Vec<_>>().as_slice() {
            [name] | [name, "0" | "1"] => Self::Skin((*name).to_owned()),
            _ => Self::Usage,
        })
    }
}

/// What the player loaded, and the file that keeps it for the character.
#[derive(Resource, Default)]
pub(super) struct Loaded {
    files: crate::profile_files::Store,
    /// Loaded in game for the character playing: it wins over the command
    /// line until the profile changes, as when the player camps or plays
    /// another character or world. A zone change or a reconnect to the
    /// same character keeps it.
    session: Option<String>,
    /// The character's last loaded skin, from their file.
    saved: Option<String>,
    /// The skin the character chose in the official client.
    official: Option<String>,
}

/// The file prefix the loaded skin is kept under.
const FILE: &str = "skin";
/// The first line of that file.
const HEADER: &str = "# eq-client UI skin v1";

/// The skin a file keeps: its first line that is not a comment.
fn read_saved(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|name| {
            eq_client_assets::ui::skin_file(std::path::Path::new(""), name, "EQUI.xml").is_ok()
        })
        .map(str::to_owned)
}

/// The skin folder of the installation a name loads, matched without regard
/// to letter case as the official client's file system matches it.
fn installed_skin(eq_directory: &std::path::Path, name: &str) -> Option<String> {
    eq_client_assets::ui::skin_file(eq_directory, name, "EQUI.xml").ok()?;
    std::fs::read_dir(eq_directory.join("uifiles"))
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .find(|folder| folder.eq_ignore_ascii_case(name))
}

/// Keeps [`UiSkin`] current, before the windows that draw with it.
pub(super) fn register(app: &mut App) {
    app.init_resource::<Loaded>()
        .init_resource::<browser::Selection>()
        .add_systems(
            Update,
            (browser::input, load, follow, browser::fill)
                .chain()
                .in_set(super::Stage::Scene),
        );
}

/// Runs a typed `/loadskin`: the skin loads at once and stays the
/// character's, here, until another is loaded. What the official client
/// says, and what it does with a skin that is not installed, is not read
/// from the program; this client says what it did in its own words, and
/// keeps the skin it had when there is no such folder. The official client
/// keeps the choice in its own `UI_<character>_<world>.ini`; this client
/// keeps it in its settings folder instead and never writes the
/// installation's files.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn load(
    chat: Option<ResMut<super::chat::ChatState>>,
    settings: Res<super::ViewerSettings>,
    mut loaded: ResMut<Loaded>,
) {
    let Some(mut chat) = chat else {
        return;
    };
    let Some(asked) = chat.load_skin.take() else {
        return;
    };
    let said = match asked {
        LoadSkin::Usage => "Usage: /loadskin <skin name>".to_owned(),
        LoadSkin::Skin(name) => {
            match settings
                .0
                .eq_directory
                .as_deref()
                .and_then(|directory| installed_skin(directory, &name))
            {
                None => format!("There is no UI skin named {name} in the uifiles folder."),
                Some(folder) => {
                    let said = format!("Loading the {folder} UI skin.");
                    loaded.session = Some(folder.clone());
                    loaded.saved = Some(folder.clone());
                    let loaded = &mut *loaded;
                    if let Some(profile) = &loaded.files.profile {
                        let file = crate::profile_files::exact_name(FILE, profile);
                        loaded.files.save(
                            file,
                            format!("{HEADER}\n{folder}\n"),
                            std::time::Instant::now(),
                        );
                    }
                    said
                }
            }
        }
    };
    chat.history
        .push(super::chat::system_line(super::chat::Said::own(said)));
}

/// Picks the skin again when the character or world changes, reading the
/// character's settings files only then, and when the player loads one.
#[allow(clippy::needless_pass_by_value)]
fn follow(
    profile: Res<super::profile_files::Profile>,
    settings: Res<super::ViewerSettings>,
    mut loaded: ResMut<Loaded>,
    mut skin: ResMut<UiSkin>,
) {
    let loaded = &mut *loaded;
    loaded.files.retry(std::time::Instant::now());
    if !loaded.files.loaded || loaded.files.profile.as_ref() != profile.key_ref() {
        if !loaded.files.loaded {
            loaded.files.open(settings.0.settings_directory.as_ref());
        }
        loaded.files.profile = profile.key();
        loaded.session = None;
        loaded.saved = loaded.files.profile.as_ref().and_then(|profile| {
            loaded
                .files
                .read(&crate::profile_files::exact_name(FILE, profile))
                .and_then(|text| read_saved(&text))
        });
        loaded.official = profile
            .names()
            .zip(settings.0.official_settings())
            .and_then(|((character, world), official)| official.chosen_skin(character, world));
        let text = loaded
            .saved
            .as_ref()
            .map(|saved| format!("{HEADER}\n{saved}\n"))
            .unwrap_or_default();
        loaded.files.loaded_text(text);
    }
    let chosen = loaded
        .session
        .as_deref()
        .or(settings.0.ui_skin.as_deref())
        .or(loaded.saved.as_deref())
        .or(loaded.official.as_deref())
        .unwrap_or(DEFAULT_SKIN);
    if skin.0 != chosen {
        info!("UI skin: {chosen}");
        chosen.clone_into(&mut skin.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn app(install: &Path, ui_skin: Option<&str>) -> App {
        app_saving(install, ui_skin, None)
    }

    fn app_saving(install: &Path, ui_skin: Option<&str>, saves: Option<&Path>) -> App {
        let mut app = App::new();
        app.init_resource::<UiSkin>()
            .init_resource::<Loaded>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<super::super::profile_files::Profile>()
            .insert_resource(super::super::online::OnlineState::new(true))
            .insert_resource(super::super::ViewerSettings(super::super::ViewerConfig {
                eq_directory: Some(install.to_path_buf()),
                ui_skin: ui_skin.map(Into::into),
                settings_directory: saves.map(Path::to_path_buf),
                ..Default::default()
            }))
            .add_systems(
                Update,
                (super::super::profile_files::follow, load, follow).chain(),
            );
        app
    }

    fn enter(app: &mut App, name: &str) {
        crate::online::testing::admit(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            1,
            eq_client_core::PlayerState {
                name: name.into(),
                titanium_environment: None,
                base_attributes: None,
                deity: None,
                class: Some(1),
                spawn_id: 7,
                race: 1,
                gender: 0,
                level: 1,
                position: eq_client_core::WorldPosition::default(),
                mana: 0,
                endurance: None,
                skills: None,
                practice_points: None,
                languages: None,
                intoxication: None,
                disciplines: None,
                spell_refresh_ms: None,
                memorized_spells: [None; 8],
                size: 0.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                hp_percent: None,
                appearance: eq_client_core::outfit::Appearance::default(),
                listing: eq_client_core::listing::Listing::default(),
                name_parts: eq_client_core::names::NameParts::default(),
                posture: None,
            },
        );
    }

    fn skin(app: &App) -> &str {
        &app.world().resource::<UiSkin>().0
    }

    #[test]
    fn a_characters_chosen_skin_applies_once_its_world_is_known() {
        let install = std::env::temp_dir().join(format!("eq-skin-follow-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(
            install.join("UI_Example_ExampleWorld.ini"),
            "[Main]\nUISkin=velious\n",
        )
        .unwrap();
        let mut app = app(&install, None);
        enter(&mut app, "Example");
        app.update();
        assert_eq!(skin(&app), DEFAULT_SKIN);
        crate::online::testing::news(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            [eq_client_core::WorldEvent::WorldName {
                short_name: "ExampleWorld".into(),
            }],
        );
        app.update();
        let chosen = skin(&app).to_owned();
        // Another character without settings goes back to the default skin.
        enter(&mut app, "Other");
        app.update();
        let other = skin(&app).to_owned();
        let mut overridden = self::app(&install, Some("custom"));
        enter(&mut overridden, "Example");
        overridden.update();
        std::fs::remove_dir_all(&install).unwrap();
        assert_eq!(chosen, "velious");
        assert_eq!(other, DEFAULT_SKIN);
        // A skin named on the command line wins.
        assert_eq!(skin(&overridden), "custom");
    }

    fn world(app: &mut App, short_name: &str) {
        crate::online::testing::news(
            &mut app
                .world_mut()
                .resource_mut::<super::super::online::OnlineState>(),
            [eq_client_core::WorldEvent::WorldName {
                short_name: short_name.into(),
            }],
        );
    }

    fn load_skin(app: &mut App, line: &str) -> Vec<String> {
        let mut chat = app
            .world_mut()
            .resource_mut::<super::super::chat::ChatState>();
        chat.load_skin = LoadSkin::typed(line);
        app.update();
        app.world()
            .resource::<super::super::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::All)
            .iter()
            .map(|(_, line)| line.message.text.clone())
            .collect()
    }

    #[test]
    fn options_skin_browser_lists_selects_and_loads_through_the_session_loader() {
        use crate::{
            clicks::testing,
            windows::{Shown, WindowId},
        };

        let root =
            std::env::temp_dir().join(format!("eq-options-skin-browser-{}", std::process::id()));
        let ui = root.join("uifiles");
        for name in ["velious", "default"] {
            std::fs::create_dir_all(ui.join(name)).unwrap();
        }
        std::fs::write(ui.join("unrelated.txt"), "").unwrap();
        let mut app = app(&root, None);
        testing::install(&mut app);
        app.init_resource::<Shown>()
            .init_resource::<browser::Selection>()
            .add_systems(Update, browser::input.before(load))
            .add_systems(Update, browser::fill.after(follow));
        let open = app
            .world_mut()
            .spawn((browser::Action::Open, Interaction::None))
            .id();
        let apply = app
            .world_mut()
            .spawn((browser::Action::Apply, Interaction::None))
            .id();
        testing::click(&mut app, open);
        assert!(
            app.world()
                .resource::<Shown>()
                .displayed(WindowId::LoadSkin)
        );
        app.world_mut().spawn(browser::List);
        app.update();
        let mut q = app.world_mut().query::<(Entity, &browser::Folder)>();
        let rows: Vec<_> = q
            .iter(app.world())
            .map(|(e, row)| (e, row.0.clone()))
            .collect();
        assert_eq!(
            rows.iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>(),
            ["default", "velious"]
        );
        testing::click(&mut app, rows[1].0);
        assert_eq!(skin(&app), "default");
        testing::click(&mut app, apply);
        assert_eq!(skin(&app), "velious");
        assert!(
            !app.world()
                .resource::<Shown>()
                .displayed(WindowId::LoadSkin)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loadskin_reads_a_skin_name_and_the_official_clients_number() {
        assert_eq!(
            LoadSkin::typed("/loadskin default"),
            Some(LoadSkin::Skin("default".into()))
        );
        assert_eq!(
            LoadSkin::typed("/LoadSkin velious 1"),
            Some(LoadSkin::Skin("velious".into()))
        );
        assert_eq!(
            LoadSkin::typed("/loadskin velious 0"),
            Some(LoadSkin::Skin("velious".into()))
        );
        assert_eq!(LoadSkin::typed("/loadskin"), Some(LoadSkin::Usage));
        assert_eq!(LoadSkin::typed("/loadskin a b"), Some(LoadSkin::Usage));
        assert_eq!(LoadSkin::typed("/loadskin a 2"), Some(LoadSkin::Usage));
        assert_eq!(LoadSkin::typed("/loadskins default"), None);
        assert_eq!(LoadSkin::typed("/shownames 1"), None);
        // An abbreviation reaches it as the official name, as typed lines
        // do, and the chat line leaves it for the skin to load.
        let spelled = crate::commands::spelled("/loadsk default").unwrap();
        let mut chat = super::super::chat::ChatState::default();
        assert!(matches!(
            super::super::chat::client_request(&spelled, &mut chat),
            Some(Ok(()))
        ));
        assert_eq!(chat.load_skin, Some(LoadSkin::Skin("default".into())));
    }

    #[test]
    fn a_loaded_skin_applies_at_once_and_stays_the_characters() {
        let root = std::env::temp_dir().join(format!("eq-skin-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (install, saves) = (root.join("install"), root.join("saves"));
        for skin in ["default", "Velious"] {
            std::fs::create_dir_all(install.join("uifiles").join(skin)).unwrap();
        }
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(
            install.join("UI_Example_ExampleWorld.ini"),
            "[Main]\nUISkin=velious\n",
        )
        .unwrap();
        let mut app = app_saving(&install, None, Some(&saves));
        enter(&mut app, "Example");
        world(&mut app, "ExampleWorld");
        app.update();
        let official = skin(&app).to_owned();
        // A skin that is not installed changes nothing and says so.
        let missing = load_skin(&mut app, "/loadskin nosuch");
        let after_missing = skin(&app).to_owned();
        let usage = load_skin(&mut app, "/loadskin");
        let said = load_skin(&mut app, "/loadskin DEFAULT 1");
        let loaded = skin(&app).to_owned();
        // A new run for the same character starts in the skin loaded here,
        // over the official client's choice; a skin named on the command
        // line still wins at the start, and a skin loaded later wins over it.
        let mut again = app_saving(&install, None, Some(&saves));
        enter(&mut again, "Example");
        world(&mut again, "ExampleWorld");
        again.update();
        let kept = skin(&again).to_owned();
        let mut named = app_saving(&install, Some("velious"), Some(&saves));
        enter(&mut named, "Example");
        world(&mut named, "ExampleWorld");
        named.update();
        let from_command_line = skin(&named).to_owned();
        load_skin(&mut named, "/loadskin velious");
        let mut last = app_saving(&install, None, Some(&saves));
        enter(&mut last, "Example");
        world(&mut last, "ExampleWorld");
        last.update();
        let last_run = skin(&last).to_owned();
        let folder = std::fs::read_to_string(saves.join(crate::profile_files::exact_name(
            FILE,
            &crate::profile_files::CharacterProfile {
                world: "ExampleWorld".into(),
                character: "Example".into(),
            },
        )))
        .unwrap();
        // Another character keeps the official client's choice, or none.
        enter(&mut last, "Other");
        last.update();
        let other = skin(&last).to_owned();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(official, "velious");
        assert_eq!(after_missing, "velious");
        assert_eq!(
            missing.last().map(String::as_str),
            Some("There is no UI skin named nosuch in the uifiles folder.")
        );
        assert_eq!(
            usage.last().map(String::as_str),
            Some("Usage: /loadskin <skin name>")
        );
        // The installed folder's own spelling is the one loaded.
        assert_eq!(
            said.last().map(String::as_str),
            Some("Loading the default UI skin.")
        );
        assert_eq!(loaded, "default");
        assert_eq!(kept, "default");
        assert_eq!(from_command_line, "velious");
        assert_eq!(skin(&named), "Velious");
        assert_eq!(folder, "# eq-client UI skin v1\nVelious\n");
        assert_eq!(last_run, "Velious");
        assert_eq!(other, DEFAULT_SKIN);
    }
}
