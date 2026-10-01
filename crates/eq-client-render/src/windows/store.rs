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
    (time, windows): (
        Res<Time<Real>>,
        Query<&Window, With<bevy::window::PrimaryWindow>>,
    ),
    settings: Res<crate::ViewerSettings>,
    online: Res<crate::online::OnlineState>,
    drag: Res<super::DragState>,
    mut exits: MessageReader<AppExit>,
    mut layouts: ResMut<Layouts>,
    mut store: Local<Store>,
) {
    let current = online
        .world()
        .player()
        .zip(online.world().world_name())
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
        let own = store.directory.as_ref().and_then(|directory| {
            std::fs::read_to_string(directory.join(file_name(store.profile.as_ref()))).ok()
        });
        let seeded = own.is_none();
        if let Some(directory) = &store.directory {
            let text = own
                .or_else(|| std::fs::read_to_string(directory.join(SHARED)).ok())
                .unwrap_or_default();
            layouts.0 = decode(&text);
        }
        // A character this client has not placed windows for yet starts where
        // the official client last put them.
        if seeded
            && let Some((world, character)) = &store.profile
            && let Some(install) = settings.0.eq_directory.as_deref()
            && let Ok(window) = windows.single()
        {
            seed(
                &mut layouts.0,
                &eq_client_assets::ui::window_positions(install, character, world),
                Vec2::new(window.width(), window.height()),
            );
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

/// Places the windows the official client placed for a character, where
/// this client has no placement of its own: at the position it saved for
/// this screen size, or else scaled from the largest screen it saved one for.
fn seed(
    layouts: &mut BTreeMap<super::WindowId, Saved>,
    positions: &[eq_client_assets::ui::WindowPosition],
    viewport: Vec2,
) {
    if viewport.min_element() <= 0.0 {
        return;
    }
    for id in super::WindowId::ALL
        .into_iter()
        .chain(super::WindowId::bags())
    {
        let Some(name) = id.section() else {
            continue;
        };
        if layouts.contains_key(&id) || !id.describe().persists {
            continue;
        }
        let saved: Vec<_> = positions
            .iter()
            .filter(|position| position.window.eq_ignore_ascii_case(&name))
            .filter(|position| position.screen.0 > 0 && position.screen.1 > 0)
            .collect();
        let size = |position: &&eq_client_assets::ui::WindowPosition| {
            UVec2::new(position.screen.0, position.screen.1)
        };
        let Some(position) = saved
            .iter()
            .find(|position| size(position).as_vec2() == viewport)
            .or_else(|| {
                saved.iter().max_by_key(|position| {
                    u64::from(position.screen.0) * u64::from(position.screen.1)
                })
            })
        else {
            continue;
        };
        #[allow(clippy::cast_precision_loss, reason = "screen positions are small")]
        let at =
            Vec2::new(position.x as f32, position.y as f32) * viewport / size(position).as_vec2();
        layouts.insert(
            id,
            Saved {
                entity: Entity::PLACEHOLDER,
                edges: [px(at.x), px(at.y), Val::Auto, Val::Auto],
                margin: UiRect::ZERO,
                position_type: PositionType::Absolute,
                minimized: false,
                placed: true,
            },
        );
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
/// minimized, then a tab and the window's name.
fn encode(layouts: &BTreeMap<super::WindowId, Saved>) -> String {
    let mut text = format!("{HEADER}\n");
    for (id, saved) in layouts {
        // Windows left where they open follow the client's defaults instead.
        if !saved.placed || !id.describe().persists {
            continue;
        }
        let key = id.key();
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

/// Reads [`encode`]'s lines, skipping any it cannot understand. A window
/// saved under its old title, before windows had ids, keeps its placement.
fn decode(text: &str) -> BTreeMap<super::WindowId, Saved> {
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
            Some((super::WindowId::saved_under(key)?, saved))
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

    #[test]
    fn a_new_character_starts_where_the_official_client_put_its_windows() {
        use super::super::WindowId;
        use eq_client_assets::ui::WindowPosition;
        let at = |window: &str, screen, x, y| WindowPosition {
            window: window.into(),
            screen,
            x,
            y,
        };
        let mut layouts = BTreeMap::new();
        let mut chat = Saved {
            entity: Entity::PLACEHOLDER,
            edges: [px(1), px(2), Val::Auto, Val::Auto],
            margin: UiRect::ZERO,
            position_type: PositionType::Absolute,
            minimized: false,
            placed: true,
        };
        layouts.insert(WindowId::Chat, chat);
        seed(
            &mut layouts,
            &[
                at("PlayerWindow", (2560, 1600), 2000, 400),
                at("TargetWindow", (2560, 1600), 1200, 20),
                at("TargetWindow", (1280, 800), 500, 16),
                at("ChatWindow", (1280, 800), 9, 9),
                at("CastingWindow", (1280, 800), 9, 9),
            ],
            Vec2::new(1280.0, 800.0),
        );
        let edges = |id| layouts[&id].edges[..2].to_vec();
        // Scaled from the only screen size saved for it.
        assert_eq!(edges(WindowId::Player), [px(1000), px(200)]);
        // This screen's own size wins over a larger one.
        assert_eq!(edges(WindowId::Target), [px(500), px(16)]);
        // This client's own placement stays.
        chat.entity = Entity::PLACEHOLDER;
        assert_eq!(layouts[&WindowId::Chat], chat);
        // Windows whose placement is never kept are left where they open.
        assert!(!layouts.contains_key(&WindowId::CastBar));
    }

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
            (super::super::WindowId::Chat, saved(px(71), true)),
            (super::super::WindowId::Spellbook, saved(percent(50), false)),
        ]);
        let text = encode(&layouts);
        assert!(text.starts_with(HEADER));
        assert!(text.contains("\tspellbook\n"));
        assert_eq!(decode(&text), layouts);
    }

    #[test]
    fn placements_saved_under_old_titles_are_kept() {
        let text = format!(
            "{HEADER}\n1px 2px auto auto 0px 0px 0px 0px absolute open\tSPELLBOOK [B]\n\
             3px 4px auto auto 0px 0px 0px 0px absolute minimized\tBUFFS\n"
        );
        let layouts = decode(&text);
        assert_eq!(layouts[&super::super::WindowId::Spellbook].edges[0], px(1));
        assert!(layouts[&super::super::WindowId::Effects].minimized);
    }

    #[test]
    fn windows_the_player_never_moved_are_not_kept() {
        let untouched = Saved {
            placed: false,
            ..saved(px(5), false)
        };
        let text = encode(&BTreeMap::from([(
            super::super::WindowId::Effects,
            untouched,
        )]));
        assert!(decode(&text).is_empty());
    }

    #[test]
    fn unreadable_lines_are_skipped_without_losing_the_rest() {
        let good = encode(&BTreeMap::from([(
            super::super::WindowId::Chat,
            saved(px(1), false),
        )]));
        let text = format!(
            "{good}broken line\n1px 2px auto auto 0px 0px 0px 0px sideways open\tTARGET\n\
             1px 2px auto auto 0px 0px 0px 0px absolute open\t\n\
             1px 2px auto auto 0px 0px 0px 0px absolute open\tno such window\n"
        );
        assert_eq!(
            decode(&text).keys().collect::<Vec<_>>(),
            [&super::super::WindowId::Chat]
        );
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
        let chat = BTreeMap::from([(super::super::WindowId::Chat, saved(px(71), false))]);
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
            .get_mut(&super::super::WindowId::Chat)
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
        assert_eq!(
            decode(&written)[&super::super::WindowId::Chat].edges[0],
            px(12)
        );
        assert!(other.is_empty());
    }
}
