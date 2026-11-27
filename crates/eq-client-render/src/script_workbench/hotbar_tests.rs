use super::*;
use crate::hud::hotbar::{Action, Bindings, carry::Carry};
use bevy::ecs::system::RunSystemOnce;

#[test]
fn the_scripts_hotbutton_control_carries_only_a_saved_clean_file() {
    let mut app = crate::testing::app();
    app.init_resource::<Workbench>()
        .init_resource::<InputFocus>()
        .init_resource::<Assets<Font>>()
        .init_resource::<RunStatus>()
        .add_systems(Startup, view::spawn)
        .add_systems(Update, controls);
    let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
    crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
    crate::online::testing::news(
        &mut online,
        [eq_client_core::WorldEvent::WorldName {
            short_name: "FixtureWorld".into(),
        }],
    );
    app.world_mut()
        .run_system_once(crate::profile_files::follow)
        .unwrap();
    let path = PathBuf::from("C:/fixture/Scripts/saved.eqscript");
    app.world_mut().resource_mut::<Workbench>().document =
        Document::new("wait(100)\n".into(), Some(path.clone()));
    app.world_mut().resource_mut::<Workbench>().library.root =
        Some(PathBuf::from("C:/fixture/Scripts"));
    app.world_mut().resource_mut::<Workbench>().path = "saved.eqscript".into();
    app.update();
    let button = app
        .world_mut()
        .query::<(Entity, &view::Action)>()
        .iter(app.world())
        .find_map(|(entity, action)| matches!(action, view::Action::Hotbutton).then_some(entity))
        .unwrap();
    crate::clicks::testing::click(&mut app, button);
    let Some(binding @ Action::Script(id)) = app.world().resource::<Carry>().hotkey() else {
        panic!("script shortcut")
    };
    assert_eq!(
        app.world().resource::<Bindings>().script_path(id),
        Some(path.as_path())
    );
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(
        app.world().resource::<Bindings>().0[0],
        Some(Action::Gem(0))
    );
    assert!(
        app.world()
            .resource::<Workbench>()
            .message
            .contains("hotbar slot")
    );
    *app.world_mut().resource_mut::<Carry>() = Carry::default();
    app.world_mut()
        .resource_mut::<Workbench>()
        .document
        .text
        .push_str("wait(200)\n");
    crate::clicks::testing::click(&mut app, button);
    assert_eq!(app.world().resource::<Carry>().hotkey(), None);
    assert!(
        app.world()
            .resource::<Workbench>()
            .message
            .contains("Save your edits")
    );
    app.world_mut().resource_mut::<Workbench>().document =
        Document::new("wait(100)\n".into(), Some(path));
    app.world_mut().resource_mut::<Workbench>().path = "renamed.eqscript".into();
    crate::clicks::testing::click(&mut app, button);
    assert_eq!(app.world().resource::<Carry>().hotkey(), None);
    assert!(
        app.world()
            .resource::<Workbench>()
            .message
            .contains("Save or reload")
    );
    app.world_mut().resource_mut::<Workbench>().path = "saved.eqscript".into();
    crate::clicks::testing::click(&mut app, button);
    assert_eq!(app.world().resource::<Carry>().hotkey(), Some(binding));
}
