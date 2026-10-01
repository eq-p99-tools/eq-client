//! Window placements kept between runs. As the official client keeps them per
//! character, each character on each world has its own file in the user's
//! settings directory; a shared file covers the screens before a character
//! enters and seeds characters that have none yet.
use super::layout::{Layouts, Saved};
use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    time::Duration,
};

/// Changed placements are written at most this often, and never mid-drag.
const SAVE_INTERVAL: Duration = Duration::from_secs(1);
const HEADER: &str = "# eq-client window layouts v1";
const SHARED: &str = "windows.txt";

/// The world and character whose placements are in use; None before one enters.
type Profile = Option<(String, String)>;

/// What the persisting system knows between frames.
#[derive(Default)]
pub(super) struct Store {
    /// Whether the directory is known and a profile's placements were loaded.
    loaded: bool,
    directory: Option<PathBuf>,
    profile: Profile,
    /// The text last read or written for this profile, to skip unchanged writes.
    written: String,
    since_check: Duration,
}

/// Loads the placements of whoever is playing and saves changes to their file.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn persist(
    time: Res<Time<Real>>,
    settings: Res<crate::ViewerSettings>,
    online: Res<crate::online::OnlineState>,
    drag: Res<super::DragState>,
    mut exits: MessageReader<AppExit>,
    mut layouts: ResMut<Layouts>,
    mut store: Local<Store>,
) {
    let current = online
        .world
        .player()
        .zip(online.world.world_name())
        .map(|(player, world)| (world, player.name.as_str()));
    let exiting = exits.read().count() > 0;
    let switched = store
        .profile
        .as_ref()
        .map(|(world, character)| (world.as_str(), character.as_str()))
        != current;
    if !store.loaded || switched {
        if store.loaded {
            save(&mut store, &layouts);
        } else {
            store.directory.clone_from(&settings.0.settings_directory);
            store.loaded = true;
        }
        store.profile = current.map(|(world, character)| (world.to_owned(), character.to_owned()));
        if let Some(directory) = &store.directory {
            let text = std::fs::read_to_string(directory.join(file_name(store.profile.as_ref())))
                .or_else(|_| std::fs::read_to_string(directory.join(SHARED)))
                .unwrap_or_default();
            layouts.0 = decode(&text);
        }
        store.written = encode(&layouts.0);
        store.since_check = Duration::ZERO;
        return;
    }
    store.since_check += time.delta();
    if (store.since_check >= SAVE_INTERVAL || exiting) && drag.active.is_none() {
        store.since_check = Duration::ZERO;
        save(&mut store, &layouts);
    }
}

/// Writes the placements to the profile's file when they changed since last time.
fn save(store: &mut Store, layouts: &Layouts) {
    let Some(directory) = &store.directory else {
        return;
    };
    let text = encode(&layouts.0);
    if text == store.written {
        return;
    }
    let path = directory.join(file_name(store.profile.as_ref()));
    match write(directory, &path, &text) {
        Ok(()) => store.written = text,
        Err(error) => {
            warn!(
                "Could not save window layouts to {}: {error}",
                path.display()
            );
            // Stop retrying every second; the next change tries again.
            store.written = text;
        }
    }
}

/// Replaces the file in one step, so a crash never leaves half a layout.
fn write(directory: &Path, path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let partial = path.with_extension("tmp");
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, path)
}

/// The shared file, or `windows-<world>-<character>.txt` with anything but
/// letters, digits, `-` and `_` replaced.
fn file_name(profile: Option<&(String, String)>) -> String {
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
        || SHARED.to_owned(),
        |(world, character)| format!("windows-{}-{}.txt", clean(world), clean(character)),
    )
}

/// One line per window: four edges, four margins, positioning, whether it is
/// minimized, then a tab and its title.
fn encode(layouts: &BTreeMap<String, Saved>) -> String {
    let mut text = format!("{HEADER}\n");
    for (key, saved) in layouts {
        // Windows left where they open follow the client's defaults instead.
        if !saved.placed || key.contains(['\t', '\n', '\r']) {
            continue;
        }
        let UiRect {
            left,
            right,
            top,
            bottom,
        } = saved.margin;
        let values: Vec<String> = saved
            .edges
            .iter()
            .chain(&[left, right, top, bottom])
            .map(|value| length(*value))
            .collect();
        let position = if saved.position_type == PositionType::Absolute {
            "absolute"
        } else {
            "relative"
        };
        let state = if saved.minimized { "minimized" } else { "open" };
        let _ = writeln!(text, "{} {position} {state}\t{key}", values.join(" "));
    }
    text
}

/// Reads [`encode`]'s lines, skipping any it cannot understand.
fn decode(text: &str) -> BTreeMap<String, Saved> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (fields, key) = line.split_once('\t')?;
            let fields: Vec<&str> = fields.split(' ').collect();
            let [l, t, r, b, ml, mr, mt, mb, position, state] = fields.as_slice() else {
                return None;
            };
            let value = |field: &str| parse_length(field);
            let saved = Saved {
                entity: Entity::PLACEHOLDER,
                edges: [value(l)?, value(t)?, value(r)?, value(b)?],
                margin: UiRect {
                    left: value(ml)?,
                    right: value(mr)?,
                    top: value(mt)?,
                    bottom: value(mb)?,
                },
                position_type: match *position {
                    "absolute" => PositionType::Absolute,
                    "relative" => PositionType::Relative,
                    _ => return None,
                },
                minimized: match *state {
                    "minimized" => true,
                    "open" => false,
                    _ => return None,
                },
                placed: true,
            };
            (!key.is_empty()).then(|| (key.to_owned(), saved))
        })
        .collect()
}

fn length(value: Val) -> String {
    match value {
        Val::Auto => "auto".into(),
        Val::Px(v) => format!("{v}px"),
        Val::Percent(v) => format!("{v}%"),
        Val::Vw(v) => format!("{v}vw"),
        Val::Vh(v) => format!("{v}vh"),
        Val::VMin(v) => format!("{v}vmin"),
        Val::VMax(v) => format!("{v}vmax"),
    }
}

fn parse_length(text: &str) -> Option<Val> {
    if text == "auto" {
        return Some(Val::Auto);
    }
    let number = |suffix: &str| {
        text.strip_suffix(suffix)?
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
    };
    // Longer suffixes first, so "vmin" is not read as a number ending in "min".
    number("vmin")
        .map(Val::VMin)
        .or_else(|| number("vmax").map(Val::VMax))
        .or_else(|| number("px").map(Val::Px))
        .or_else(|| number("vw").map(Val::Vw))
        .or_else(|| number("vh").map(Val::Vh))
        .or_else(|| number("%").map(Val::Percent))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(left: Val, minimized: bool) -> Saved {
        Saved {
            entity: Entity::PLACEHOLDER,
            edges: [left, px(93.5), Val::Auto, percent(10)],
            margin: UiRect {
                left: Val::Vw(1.0),
                right: Val::VMin(2.0),
                top: Val::VMax(3.0),
                bottom: Val::Vh(4.0),
            },
            position_type: PositionType::Absolute,
            minimized,
            placed: true,
        }
    }

    #[test]
    fn placements_survive_a_round_trip_through_the_file() {
        let layouts = BTreeMap::from([
            ("CHAT".to_owned(), saved(px(71), true)),
            ("SPELLBOOK [B]".to_owned(), saved(percent(50), false)),
        ]);
        let text = encode(&layouts);
        assert!(text.starts_with(HEADER));
        assert_eq!(decode(&text), layouts);
    }

    #[test]
    fn windows_the_player_never_moved_are_not_kept() {
        let untouched = Saved {
            placed: false,
            ..saved(px(5), false)
        };
        let text = encode(&BTreeMap::from([("BUFFS".to_owned(), untouched)]));
        assert!(decode(&text).is_empty());
    }

    #[test]
    fn unreadable_lines_are_skipped_without_losing_the_rest() {
        let good = encode(&BTreeMap::from([("CHAT".to_owned(), saved(px(1), false))]));
        let text = format!(
            "{good}broken line\n1px 2px auto auto 0px 0px 0px 0px sideways open\tTARGET\n\
             1px 2px auto auto 0px 0px 0px 0px absolute open\t\n"
        );
        assert_eq!(decode(&text).keys().collect::<Vec<_>>(), ["CHAT"]);
    }

    #[test]
    fn each_character_on_each_world_has_its_own_file() {
        assert_eq!(file_name(None), SHARED);
        let profile = ("P1999Green".to_owned(), "Example".to_owned());
        assert_eq!(file_name(Some(&profile)), "windows-P1999Green-Example.txt");
        let odd = ("a/b".to_owned(), "..".to_owned());
        assert_eq!(file_name(Some(&odd)), "windows-a_b-__.txt");
    }

    fn player(name: &str) -> eq_client_core::PlayerState {
        eq_client_core::PlayerState {
            name: name.into(),
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
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: None,
            appearance: eq_client_core::outfit::Appearance::default(),
        }
    }

    #[test]
    fn each_character_gets_its_own_placements_back_and_saves_changes() {
        let directory = std::env::temp_dir().join(format!("eq-windows-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let chat = BTreeMap::from([("CHAT".to_owned(), saved(px(71), false))]);
        std::fs::write(
            directory.join("windows-ExampleWorld-Example.txt"),
            encode(&chat),
        )
        .unwrap();
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::news(
            &mut online,
            [eq_client_core::WorldEvent::WorldName {
                short_name: "ExampleWorld".into(),
            }],
        );
        crate::online::testing::admit(&mut online, 1, player("Example"));
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .init_resource::<Layouts>()
            .init_resource::<super::super::DragState>()
            .insert_resource(online)
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig {
                settings_directory: Some(directory.clone()),
                ..Default::default()
            }))
            .add_systems(Update, persist);
        app.update();
        let loaded = app.world().resource::<Layouts>().0.clone();
        // The player moves the chat window; closing the client saves it.
        app.world_mut()
            .resource_mut::<Layouts>()
            .0
            .get_mut("CHAT")
            .unwrap()
            .edges[0] = px(12);
        app.world_mut().write_message(AppExit::Success);
        app.update();
        let written =
            std::fs::read_to_string(directory.join("windows-ExampleWorld-Example.txt")).unwrap();
        // Another character with no file of its own starts from the shared one.
        crate::online::testing::admit(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            2,
            player("Other"),
        );
        app.update();
        let other = app.world().resource::<Layouts>().0.clone();
        std::fs::remove_dir_all(&directory).unwrap();
        assert_eq!(loaded, chat);
        assert_eq!(decode(&written)["CHAT"].edges[0], px(12));
        assert!(other.is_empty());
    }
}
