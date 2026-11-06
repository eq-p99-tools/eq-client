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
    reopened.follow(Some(profile("fixture")));
    assert_eq!(reopened.saved, saved.saved);
    reopened.follow(Some(profile("otherworld")));
    assert!(reopened.saved.is_empty());
    reopened.follow(Some(profile("fixture")));
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
    sets.follow(Some(profile("fixture")));
    sets.panel.draft = "Travel".into();
    sets.save_current([None; 8]).unwrap();
    assert!(sets.checked_name(" travel ", None).is_err());
    assert!(sets.checked_name("travel", Some("Travel")).is_ok());
    assert!(sets.checked_name(&"a".repeat(33), None).is_err());

    let path = folder.0.join(sets.file_name().unwrap());
    let broken = r#"{"short":[null,42]}"#;
    std::fs::write(&path, broken).unwrap();
    sets.follow(Some(profile("otherworld")));
    sets.follow(Some(profile("fixture")));
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
        sets.follow(Some(profile(world)));
        assert!(sets.saved.is_empty());
        let name = sets.file_name().unwrap();
        // Block the rename after loading, so this profile stays dirty.
        std::fs::create_dir(folder.0.join(&name)).unwrap();
        sets.panel.draft = "Travel".into();
        sets.save_current(gems(id)).unwrap();
        assert!(sets.store.pending(&name));
    }
    for (world, id) in profiles {
        sets.follow(Some(profile(world)));
        assert_eq!(sets.saved["Travel"], gems(id));
        std::fs::remove_dir(folder.0.join(sets.file_name().unwrap())).unwrap();
    }
    sets.store
        .retry(Instant::now() + std::time::Duration::from_secs(120));
    let mut reopened = Sets::default();
    reopened.store.open(Some(&folder.0));
    for (world, id) in profiles {
        reopened.follow(Some(profile(world)));
        assert_eq!(reopened.saved["Travel"], gems(id));
    }
}

#[test]
fn a_confirming_gem_and_link_loss_in_one_receive_batch_keep_the_old_count() {
    use crate::online::{Updates, receive};
    use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
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
    for update in [
        WorldUpdate::Game(WorldEvent::BookAction(BookActionStatus::Submitted)),
        WorldUpdate::Game(WorldEvent::Spell(SpellUpdate::Slot {
            slot: 0,
            spell_id: 42,
            mode: 2,
        })),
        WorldUpdate::Connection(eq_client_core::world::Link::Ended),
    ] {
        updates.send(update).unwrap();
    }
    app.update();
    assert_eq!(
        app.world().resource::<Sets>().status,
        "Travel stopped. 7/8 match. The admission ended."
    );
    assert!(matches!(sent.try_recv(), Err(mpsc::TryRecvError::Empty)));
}
