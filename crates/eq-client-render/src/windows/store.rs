//! Window placements and chat groups kept between runs. As the official client keeps layouts per
//! character, each character on each world has its own file in the user's
//! settings directory; a shared file covers the screens before a character
//! enters and seeds characters that have none yet.
use super::layout::{Layouts, Saved};
use crate::profile_files::{CharacterProfile, Store as Files};
use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    time::{Duration, Instant},
};

/// Changed placements are written at most this often, and never mid-drag.
const SAVE_INTERVAL: Duration = Duration::from_secs(1);
const HEADER: &str = "# eq-client window layouts v2";
const SHARED: &str = "windows.txt";

/// Placement-specific scheduling around the shared profile persistence state.
#[derive(Default)]
pub(super) struct Store {
    files: Files,
    since_check: Duration,
}

/// Loads the current character's window layout and saves changes to their file.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn persist(
    (time, windows): (
        Res<Time<Real>>,
        Query<&Window, With<bevy::window::PrimaryWindow>>,
    ),
    (settings, profile): (
        Res<crate::ViewerSettings>,
        Res<crate::profile_files::Profile>,
    ),
    (mut drag, mut exits): (ResMut<super::DragState>, MessageReader<AppExit>),
    mut layouts: ResMut<Layouts>,
    mut store: Local<Store>,
    mut frames: Query<(&mut super::Frame, &mut Node)>,
    mut chat: Option<ResMut<crate::chat::ChatState>>,
) {
    let exiting = exits.read().count() > 0;
    if exiting {
        drag.active = None;
        drag.cursor = None;
    }
    if drag.active.is_none() {
        store.files.retry(Instant::now());
    }
    if !store.files.loaded || profile.is_changed() {
        for (mut frame, mut node) in &mut frames {
            frame.clear_size(&mut node);
        }
        if store.files.loaded {
            save(&mut store, &layouts, chat.as_ref().map(|chat| &chat.groups));
        } else {
            store.files.open(settings.0.settings_directory.as_ref());
        }
        store.files.profile = profile.key();
        let own = store.files.read(&file_name(store.files.profile.as_ref()));
        let seeded = own.is_none();
        if store.files.directory.is_some() {
            let text = own.or_else(|| store.files.read(SHARED)).unwrap_or_default();
            layouts.0 = decode(&text);
            if let Some(chat) = &mut chat {
                chat.groups = crate::chat::Groups::read(&text);
            }
        } else {
            for saved in layouts.0.values_mut() {
                saved.size = None;
            }
        }
        // A character this client has not placed windows for yet starts where
        // the official client last put them.
        if seeded
            && let Some(CharacterProfile { world, character }) = &store.files.profile
            && let Some(official) = settings.0.official_settings()
            && let Ok(window) = windows.single()
        {
            seed(
                &mut layouts.0,
                &official.window_positions(character, world),
                Vec2::new(window.width(), window.height()),
            );
        }
        let text = profile_text(&layouts, chat.as_ref().map(|chat| &chat.groups));
        store.files.loaded_text(text);
        store.since_check = Duration::ZERO;
        return;
    }
    store.since_check += time.delta();
    if (store.since_check >= SAVE_INTERVAL || exiting) && drag.active.is_none() {
        store.since_check = Duration::ZERO;
        save(&mut store, &layouts, chat.as_ref().map(|chat| &chat.groups));
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
                size: None,
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
fn save(store: &mut Store, layouts: &Layouts, groups: Option<&crate::chat::Groups>) {
    let name = file_name(store.files.profile.as_ref());
    store
        .files
        .save(name, profile_text(layouts, groups), Instant::now());
}

fn profile_text(layouts: &Layouts, groups: Option<&crate::chat::Groups>) -> String {
    let mut text = encode_placements(&layouts.0, groups);
    if let Some(groups) = groups {
        groups.write(&mut text);
    }
    text
}

/// The shared file, or the profile's own.
fn file_name(profile: Option<&CharacterProfile>) -> String {
    crate::profile_files::name("windows", SHARED, profile)
}

/// One line per window: four edges, four margins, positioning, whether it is
/// minimized, then a tab and the window's name.
#[cfg(test)]
fn encode(layouts: &BTreeMap<super::WindowId, Saved>) -> String {
    encode_placements(layouts, None)
}

fn encode_placements(
    layouts: &BTreeMap<super::WindowId, Saved>,
    groups: Option<&crate::chat::Groups>,
) -> String {
    let mut text = format!("{HEADER}\n");
    for (id, saved) in layouts {
        // Windows left where they open follow the client's defaults instead.
        if !saved.placed
            || !id.describe().persists
            || (matches!(id, super::WindowId::ChatGroup(_))
                && groups.is_some_and(|groups| !groups.contains(*id)))
        {
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
        let size = saved.size.filter(|size| size.valid()).map_or_else(
            || "default".to_owned(),
            |size| {
                format!(
                    "size:{:016x}:{}:{}",
                    size.scope, size.dimensions.x, size.dimensions.y
                )
            },
        );
        let _ = writeln!(
            text,
            "{} {position} {state} {size}\t{key}",
            values.join(" ")
        );
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
            let fields: Vec<&str> = fields.split_whitespace().collect();
            let [l, t, r, b, ml, mr, mt, mb, position, state, rest @ ..] = fields.as_slice() else {
                return None;
            };
            if rest.len() > 1 {
                return None;
            }
            let size = rest.first().and_then(|field| parse_size(field));
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
                size,
            };
            Some((super::WindowId::saved_under(key)?, saved))
        })
        .collect()
}

fn parse_size(field: &str) -> Option<super::resize::Size> {
    let mut parts = field.strip_prefix("size:")?.split(':');
    let size = super::resize::Size {
        scope: u64::from_str_radix(parts.next()?, 16).ok()?,
        dimensions: Vec2::new(parts.next()?.parse().ok()?, parts.next()?.parse().ok()?),
    };
    (parts.next().is_none() && size.valid()).then_some(size)
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
            size: None,
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
                at("MainChat", (1280, 800), 9, 9),
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

    #[test]
    fn the_main_chat_starts_where_the_chat_manager_put_it() {
        use super::super::WindowId;
        use eq_client_assets::ui::WindowPosition;
        let at = |window: &str, x, y| WindowPosition {
            window: window.into(),
            screen: (1280, 800),
            x,
            y,
        };
        let mut layouts = BTreeMap::new();
        // The main chat's place is the chat manager's, not the section named
        // after the skin's chat window.
        seed(
            &mut layouts,
            &[at("ChatWindow", 9, 9), at("MainChat", 640, 600)],
            Vec2::new(1280.0, 800.0),
        );
        assert_eq!(layouts[&WindowId::Chat].edges[..2], [px(640), px(600)]);
    }

    fn saved(left: Val, minimized: bool) -> Saved {
        Saved {
            size: None,
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
    fn resize_sizes_round_trip_while_legacy_and_invalid_sizes_keep_placements() {
        let mut chat = saved(px(35), true);
        chat.size = Some(super::super::resize::Size {
            scope: 0x1234,
            dimensions: Vec2::new(610.0, 320.0),
        });
        let layouts = BTreeMap::from([(super::super::WindowId::Chat, chat)]);
        assert_eq!(decode(&encode(&layouts)), layouts);
        for size in [
            "",
            " default",
            " size:1234:NaN:200",
            " size:1234:500:inf",
            " size:1234:-10:200",
            " size:1234:0:200",
            " size:1234:160:99",
            " size:xyz:400:200",
            " size:1234:400:200:extra",
        ] {
            let line = format!(
                "# eq-client window layouts v1\n35px 93.5px auto auto 0px 0px 0px 0px absolute minimized{size}\tchat\n"
            );
            let loaded = decode(&line);
            let loaded = &loaded[&super::super::WindowId::Chat];
            assert_eq!(loaded.edges[..2], [px(35), px(93.5)]);
            assert!(loaded.minimized);
            assert!(loaded.size.is_none(), "{size}");
        }
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
        let profile = CharacterProfile {
            world: "P1999Green".into(),
            character: "Example".into(),
        };
        assert_eq!(file_name(Some(&profile)), "windows-P1999Green-Example.txt");
        let odd = CharacterProfile {
            world: "a/b".into(),
            character: "..".into(),
        };
        assert_eq!(file_name(Some(&odd)), "windows-a_b-__.txt");
    }

    fn player(name: &str) -> eq_client_core::PlayerState {
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
        }
    }

    #[test]
    fn each_character_gets_its_own_placements_back_and_saves_changes() {
        use super::super::WindowId;

        let directory = std::env::temp_dir().join(format!("eq-windows-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let detached = WindowId::ChatGroup(1);
        let chat = BTreeMap::from([
            (WindowId::Chat, saved(px(71), false)),
            (detached, saved(px(255), false)),
        ]);
        let groups = crate::chat::Groups::read("chat-tabs\tchat-group-1 Guild Guild Tell\n");
        std::fs::write(
            directory.join("windows-ExampleWorld-Example.txt"),
            profile_text(&Layouts(chat.clone()), Some(&groups)),
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
            .init_resource::<crate::profile_files::Profile>()
            .init_resource::<crate::chat::ChatState>()
            .insert_resource(online)
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig {
                settings_directory: Some(directory.clone()),
                ..Default::default()
            }))
            .add_systems(Update, (crate::profile_files::follow, persist).chain());
        app.update();
        let loaded = app.world().resource::<Layouts>().0.clone();
        assert_eq!(
            app.world().resource::<crate::chat::ChatState>().groups,
            groups
        );
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
        assert_eq!(decode(&written)[&detached].edges[0], px(255));
        assert_eq!(crate::chat::Groups::read(&written), groups);
        assert!(other.is_empty());
        assert_eq!(
            app.world().resource::<crate::chat::ChatState>().groups,
            crate::chat::Groups::default()
        );
    }
}
