use super::*;
use crate::{
    clicks::testing as clicks,
    keys::Typing,
    online::testing as session,
    profile_files::{self, CharacterProfile},
    spellbook::{self, search},
};
use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput},
};
use eq_client_core::{
    BookActionStatus, ClientCommand, SpellBook, SpellUpdate, WorldEvent, WorldUpdate,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "eq-spell-sets-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        if let Ok(files) = std::fs::read_dir(&self.0) {
            for file in files.flatten() {
                if file.file_type().is_ok_and(|kind| kind.is_dir()) {
                    let _ = std::fs::remove_dir(file.path());
                } else {
                    let _ = std::fs::remove_file(file.path());
                }
            }
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn profile(world: &str) -> CharacterProfile {
    CharacterProfile {
        world: world.into(),
        character: "Example".into(),
    }
}

fn test_app(folder: &Folder, class: u32) -> (App, mpsc::Receiver<ClientCommand>) {
    let mut app = crate::testing::app();
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .settings_directory = Some(folder.0.clone());
    let mut online = OnlineState::new(true);
    session::news(
        &mut online,
        [WorldEvent::WorldName {
            short_name: "fixture".into(),
        }],
    );
    let mut player = session::player(7);
    player.class = Some(class);
    player.memorized_spells = [Some(42), None, None, Some(123_456), None, None, None, None];
    session::admit(&mut online, 9, player);
    session::book(&mut online, SpellBook::default());
    let (queue, sent) = mpsc::sync_channel(4);
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(queue)))
        .add_message::<KeyboardInput>()
        .add_systems(Startup, |mut commands: Commands| {
            spellbook::spawn(&mut commands);
        })
        .add_systems(
            Update,
            (
                profile_files::follow,
                super::super::follow,
                search::focus,
                search::capture,
                crate::chat::input,
                search::input,
                input,
                present,
            )
                .chain(),
        );
    app.world_mut()
        .resource_mut::<Shown>()
        .open(WindowId::Spellbook);
    app.update();
    (app, sent)
}

fn control(app: &mut App, id: &str) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &SkinId)>()
        .iter(world)
        .find_map(|(entity, skin)| (skin.id == id).then_some(entity))
        .unwrap()
}

fn click(app: &mut App, id: &str) {
    let button = control(app, id);
    clicks::click(app, button);
}

fn key(app: &mut App, key_code: KeyCode, logical_key: Key, text: Option<&str>) {
    let world = app.world_mut();
    world.resource_mut::<ButtonInput<KeyCode>>().press(key_code);
    world.write_message(KeyboardInput {
        key_code,
        logical_key,
        text: text.map(Into::into),
        repeat: false,
        state: ButtonState::Pressed,
        window: Entity::PLACEHOLDER,
    });
}

fn install_background_book_script(app: &mut App) -> (Entity, Entity) {
    use crate::script::{InputMode, Program};
    use bevy::window::PrimaryWindow;

    let program = Program::parse(
        "click button spellbook SpellSets_Name\npress b\npress enter\n\
         click button spellbook SpellSets_Save\nclick button spellbook TestSearch\n\
         hold w 160\npress enter\nclick button spellbook TestSearch\nwait 2000",
        std::path::Path::new("."),
    )
    .unwrap();
    let world = app.world_mut();
    world
        .resource_mut::<crate::ViewerSettings>()
        .0
        .script_input_mode = InputMode::Background;
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap();
    world.get_mut::<Window>(window).unwrap().focused = false;
    let field = world
        .query_filtered::<Entity, With<search::Field>>()
        .single(world)
        .unwrap();
    world.entity_mut(field).insert(script_id("TestSearch"));
    for (id, mut node) in world
        .query_filtered::<(&WindowId, &mut Node), With<Frame>>()
        .iter_mut(world)
    {
        if *id == WindowId::Spellbook {
            node.display = Display::Flex;
        }
    }
    for (index, id) in [
        (1_u16, "SpellSets_Name"),
        (2, "SpellSets_Save"),
        (3, "TestSearch"),
    ] {
        let button = control(app, id);
        app.world_mut().entity_mut(button).insert((
            InheritedVisibility::VISIBLE,
            ComputedNode {
                size: Vec2::splat(32.0),
                ..default()
            },
            bevy::ui::UiGlobalTransform::from_xy(f32::from(index) * 140.0, 180.0),
            bevy::ui::ComputedStackIndex(u32::from(index)),
        ));
    }
    app.init_resource::<Time<Real>>()
        .init_resource::<crate::windows::Moves>()
        .add_message::<bevy::input::keyboard::KeyboardFocusLost>()
        .add_message::<bevy::input::mouse::MouseButtonInput>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_message::<bevy::window::WindowFocused>()
        .add_systems(
            PreUpdate,
            (
                bevy::input::keyboard::keyboard_input_system,
                bevy::input::mouse::mouse_button_input_system,
            )
                .in_set(bevy::input::InputSystems),
        )
        .add_systems(Update, search::present.after(search::input));
    crate::install_script(
        app,
        (Some(program), None),
        true,
        false,
        InputMode::Background,
    );
    (window, field)
}

fn script_frame(app: &mut App, ms: u64) {
    let mut time = app.world_mut().resource_mut::<Time<Real>>();
    let startup = time.startup();
    time.update_with_instant(startup + std::time::Duration::from_millis(ms));
    app.update();
}

#[test]
fn background_script_types_in_both_book_fields_until_native_input_reclaims_ownership() {
    use crate::script::{Phase, RunStatus};
    use bevy::input::keyboard::KeyboardFocusLost;

    let folder = Folder::new();
    let (mut app, sent) = test_app(&folder, 2);
    click(&mut app, "SpellSets_Toggle");
    let (window, search) = install_background_book_script(&mut app);
    let mut passed_enter = false;
    let mut saw_text = false;
    let mut next_frame = 0;
    for ms in (0..2_000).step_by(20) {
        script_frame(&mut app, ms);
        next_frame = ms + 20;
        let world = app.world();
        assert!(
            !world
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
        match world.get::<Text>(search).unwrap().0.as_str() {
            "w|" if passed_enter => break,
            "w|" if !saw_text => {
                saw_text = true;
                app.world_mut().write_message(KeyboardFocusLost);
            }
            "w" => passed_enter = true,
            _ => (),
        }
    }
    let world = app.world();
    assert!(saw_text && passed_enter);
    assert!(world.resource::<Typing>().spell_search);
    assert_eq!(world.get::<Text>(search).unwrap().0, "w|");
    assert_eq!(
        world.resource::<Sets>().saved["b"],
        world.resource::<OnlineState>().world().gems()
    );

    // Let the last click release before the script waits for its next action.
    script_frame(&mut app, next_frame);
    script_frame(&mut app, next_frame + 20);
    // Native input takes ownership while the window is unfocused. Neither
    // that first repeat nor the next one belongs to the editor or gameplay.
    for ms in [next_frame + 40, next_frame + 60] {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyW,
            logical_key: Key::Character("w".into()),
            text: Some("w".into()),
            repeat: true,
            state: ButtonState::Pressed,
            window,
        });
        script_frame(&mut app, ms);
        let world = app.world();
        assert_eq!(
            world.resource::<RunStatus>().phase(),
            Phase::WaitingForInput
        );
        let typing = world.resource::<Typing>();
        assert!(!typing.spell_search && !typing.spell_set_name && !typing.composing);
        assert!(
            !world
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
        assert_eq!(world.get::<Text>(search).unwrap().0, "w");
        assert_eq!(world.resource::<ChatState>().draft(), "");
    }
    assert!(matches!(sent.try_recv(), Err(mpsc::TryRecvError::Empty)));
}

#[test]
fn typed_names_save_rename_and_delete_the_selected_layout() {
    let folder = Folder::new();
    let (mut app, sent) = test_app(&folder, 2);
    click(&mut app, "SpellSets_Toggle");
    click(&mut app, "SpellSets_Name");
    assert!(app.world().resource::<Typing>().spell_set_name);
    key(
        &mut app,
        KeyCode::Slash,
        Key::Character("/".into()),
        Some("/ travel "),
    );
    key(&mut app, KeyCode::Escape, Key::Escape, None);
    app.update();
    let typing = app.world().resource::<Typing>();
    assert!(!typing.spell_set_name && typing.escape_consumed && !typing.composing);
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::Slash)
    );
    click(&mut app, "SpellSets_Save");
    let saved = app.world().resource::<Sets>();
    assert_eq!(
        saved.saved["/ travel"],
        app.world().resource::<OnlineState>().world().gems()
    );

    click(&mut app, "SpellSets_Name");
    key(&mut app, KeyCode::Delete, Key::Delete, None);
    key(
        &mut app,
        KeyCode::KeyR,
        Key::Character("r".into()),
        Some("Rest"),
    );
    app.update();
    click(&mut app, "SpellSets_Rename");
    let saved = app.world().resource::<Sets>();
    assert_eq!(
        saved.saved.keys().map(String::as_str).collect::<Vec<_>>(),
        ["Rest"]
    );
    let mut reopened = Sets::default();
    reopened.store.open(Some(&folder.0));
    reopened.follow(Some(&profile("fixture")));
    assert_eq!(reopened.saved, saved.saved);
    reopened.follow(Some(&profile("otherworld")));
    assert!(reopened.saved.is_empty());
    reopened.follow(Some(&profile("fixture")));
    assert_eq!(reopened.saved, saved.saved);
    click(&mut app, "SpellSets_Delete");
    assert_eq!(app.world().resource::<Sets>().saved.len(), 1);
    click(&mut app, "SpellSets_Delete");
    assert!(app.world().resource::<Sets>().saved.is_empty());
    assert!(matches!(sent.try_recv(), Err(mpsc::TryRecvError::Empty)));
}

#[test]
fn noncasters_cannot_open_the_panel_even_if_a_book_event_arrives() {
    let folder = Folder::new();
    let (mut app, sent) = test_app(&folder, 1);
    let toggle = control(&mut app, "SpellSets_Toggle");
    assert_eq!(
        app.world().get::<Node>(toggle).unwrap().display,
        Display::None
    );
    clicks::click(&mut app, toggle);
    assert!(!app.world().resource::<Sets>().panel.expanded);
    assert!(sent.try_recv().is_err());
}

#[test]
fn malformed_data_stays_intact_and_name_collisions_are_rejected() {
    let folder = Folder::new();
    let mut sets = Sets::default();
    sets.store.open(Some(&folder.0));
    sets.follow(Some(&profile("fixture")));
    sets.panel.draft = "Travel".into();
    sets.save_current([None; 8]).unwrap();
    assert!(sets.checked_name(" travel ", None).is_err());
    assert!(sets.checked_name("travel", Some("Travel")).is_ok());
    assert!(sets.checked_name(&"a".repeat(33), None).is_err());

    let path = folder.0.join(sets.file_name().unwrap());
    let broken = r#"{"short":[null,42]}"#;
    std::fs::write(&path, broken).unwrap();
    sets.follow(Some(&profile("otherworld")));
    sets.follow(Some(&profile("fixture")));
    assert!(sets.load_error && !sets.writable() && sets.saved.is_empty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), broken);
}

#[test]
fn exact_world_names_keep_their_own_deferred_writes() {
    let folder = Folder::new();
    let mut sets = Sets::default();
    sets.store.open(Some(&folder.0));
    let profiles = [("test+world", 42), ("test_world", 73), ("Test_World", 99)];
    let gems = |id| [Some(id), None, None, None, None, None, None, None];
    for (world, id) in profiles {
        sets.follow(Some(&profile(world)));
        assert!(sets.saved.is_empty());
        let name = sets.file_name().unwrap();
        // Block the rename after loading, so this profile stays dirty.
        std::fs::create_dir(folder.0.join(&name)).unwrap();
        sets.panel.draft = "Travel".into();
        sets.save_current(gems(id)).unwrap();
        assert!(sets.store.pending(&name));
    }
    for (world, id) in profiles {
        sets.follow(Some(&profile(world)));
        assert_eq!(sets.saved["Travel"], gems(id));
        std::fs::remove_dir(folder.0.join(sets.file_name().unwrap())).unwrap();
    }
    sets.store
        .retry(Instant::now() + std::time::Duration::from_secs(120));
    let mut reopened = Sets::default();
    reopened.store.open(Some(&folder.0));
    for (world, id) in profiles {
        reopened.follow(Some(&profile(world)));
        assert_eq!(reopened.saved["Travel"], gems(id));
    }
}

#[test]
fn confirming_gems_in_a_reset_batch_keep_the_old_count_and_the_right_panel() {
    use crate::online::{Updates, receive};
    use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
    use eq_client_core::world::Link;

    for (reset, reason, retains_panel) in [
        (
            vec![WorldUpdate::Game(WorldEvent::Death(
                eq_client_core::Death {
                    spawn_id: 7,
                    killer_id: 9,
                    corpse_id: 8,
                    bind_zone_id: 9,
                    corpse_name: None,
                },
            ))],
            "The character died.",
            true,
        ),
        (
            vec![WorldUpdate::Game(WorldEvent::ZoneTransfer(
                eq_client_core::ZoneOffer {
                    zone_id: 2,
                    instance_id: 0,
                    position: eq_client_core::WorldPosition::default(),
                    reason: 0,
                    to_bind: false,
                    solicited: true,
                },
            ))],
            "Zone transfer started.",
            true,
        ),
        (
            vec![
                WorldUpdate::Connection(Link::Zoning),
                WorldUpdate::Connection(Link::Connected),
            ],
            "The connection was interrupted.",
            true,
        ),
        (
            vec![WorldUpdate::Connection(Link::Ended)],
            "The admission ended.",
            false,
        ),
    ] {
        let folder = Folder::new();
        let (mut app, sent) = test_app(&folder, 2);
        let (updates, incoming) = mpsc::channel();
        app.insert_resource(Updates(std::sync::Mutex::new(Some(incoming))))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, receive.before(profile_files::follow));
        app.world_mut()
            .resource_mut::<Sets>()
            .saved
            .insert("Travel".into(), [None; 8]);
        click(&mut app, "SpellSets_Toggle");
        click(&mut app, "SpellSets_Apply");
        app.update();
        assert!(matches!(
            sent.try_recv(),
            Ok(ClientCommand::ForgetSpell {
                gem: 0,
                spell_id: 42,
                ..
            })
        ));
        app.world_mut().resource_mut::<Sets>().panel.draft = "In progress".into();
        for update in [
            WorldUpdate::Game(WorldEvent::BookAction(BookActionStatus::Submitted)),
            WorldUpdate::Game(WorldEvent::Spell(SpellUpdate::Slot {
                slot: 0,
                spell_id: 42,
                mode: 2,
            })),
        ]
        .into_iter()
        .chain(reset)
        {
            updates.send(update).unwrap();
        }
        app.update();
        let sets = app.world().resource::<Sets>();
        assert_eq!(sets.status, format!("Travel stopped. 7/8 match. {reason}"));
        assert_eq!(sets.panel.expanded, retains_panel);
        assert_eq!(
            sets.panel.selected.as_deref(),
            retains_panel.then_some("Travel")
        );
        assert_eq!(
            sets.panel.draft,
            if retains_panel { "In progress" } else { "" }
        );
        assert!(matches!(sent.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }
}
