//! Exercise the Map and generated skin checkbox against existing per-character files.
use super::*;
use crate::{
    clicks::testing::click,
    map,
    online::{OnlineState, testing},
};
use std::path::Path;

fn enter(app: &mut App, session: u64, name: &str) {
    let mut online = OnlineState::new(true);
    testing::news(
        &mut online,
        [eq_client_core::WorldEvent::WorldName {
            short_name: "Example".into(),
        }],
    );
    let mut player = testing::player(7);
    player.name = name.into();
    testing::admit(&mut online, session, player);
    app.insert_resource(online);
}

fn controls(directory: &Path) -> (App, Entity, Entity) {
    let mut app = crate::testing::app();
    let config = &mut app.world_mut().resource_mut::<crate::ViewerSettings>().0;
    config.settings_directory = Some(directory.to_owned());
    config.eq_directory = Some(directory.to_owned());
    crate::sheets::testing::sheet(&mut app, "checkbox.tga", image::RgbaImage::new(2, 1));
    app.add_systems(
        Startup,
        |mut commands: Commands, mut art: crate::sheets::Art| {
            let mut skin = qol::tests::source();
            let piece = |x| {
                Some(Piece {
                    texture: "checkbox.tga".into(),
                    x,
                    y: 0,
                    width: 1,
                    height: 1,
                })
            };
            let Element::Button(template) = &mut skin.pieces[0].1 else {
                panic!("checkbox template")
            };
            template.look.normal = piece(0);
            template.look.pressed = piece(1);
            let pages = qol::pages(
                std::slice::from_ref(&skin),
                WindowId::Options,
                Vec2::new(395.0, 432.0),
                None,
            );
            let (_, Element::Button(checkbox)) = pages
                .last()
                .unwrap()
                .pieces
                .iter()
                .find(|(id, _)| id == qol::OUTLINE_TRACKED)
                .unwrap()
            else {
                panic!("outline checkbox")
            };
            assert!(checkbox.checkbox);
            assert_eq!(checkbox.text.as_deref(), Some("Outline tracked names"));
            assert_eq!(checkbox.tooltip.as_deref(), Some(map::OUTLINES_TOOLTIP));
            commands.spawn(Node::default()).with_children(|parent| {
                button(
                    parent,
                    &mut art,
                    checkbox,
                    &checkbox.area,
                    WindowId::Options,
                );
            });
            commands
                .spawn(Node::default())
                .with_children(map::control_chrome);
        },
    )
    .add_systems(
        Update,
        (
            crate::profile_files::follow,
            map::presentation_actions,
            map::persist_presentation,
            map::present,
            buttons,
        )
            .chain(),
    );
    enter(&mut app, 1, "First");
    app.update();
    let world = app.world_mut();
    let map_button = world
        .query::<(Entity, &Text, &crate::tooltip::Tooltip)>()
        .iter(world)
        .find(|(_, text, tip)| {
            text.0.starts_with("Outline tracked: ") && tip.0 == map::OUTLINES_TOOLTIP
        })
        .unwrap()
        .0;
    let checkbox = world
        .query_filtered::<Entity, With<map::OutlineCheckbox>>()
        .single(world)
        .unwrap();
    (app, map_button, checkbox)
}

fn displays(app: &App, map_button: Entity, checkbox: Entity, on: bool) {
    let world = app.world();
    assert_eq!(
        world.get::<Text>(map_button).unwrap().0,
        if on {
            "Outline tracked: On"
        } else {
            "Outline tracked: Off"
        }
    );
    let x = if on { 1.0 } else { 0.0 };
    assert_eq!(
        world.get::<ImageNode>(checkbox).unwrap().rect,
        Some(Rect::new(x, 0.0, x + 1.0, 1.0))
    );
    assert_eq!(world.resource::<map::Presentation>().outlines(), on);
}

#[test]
fn map_and_qol_clicks_update_both_controls_and_the_existing_character_file() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "eq-outline-controls-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let first = dir.join("map-view-Example-First.txt");
    std::fs::write(
        &first,
        "opacity=60\nplain=true\nlocked=false\noutlines=true\n",
    )
    .unwrap();
    let (mut app, map_button, checkbox) = controls(&dir);
    displays(&app, map_button, checkbox, true);
    for (button, on) in [
        (checkbox, false),
        (map_button, true),
        (map_button, false),
        (checkbox, true),
    ] {
        click(&mut app, button);
        displays(&app, map_button, checkbox, on);
        assert_eq!(
            std::fs::read_to_string(&first).unwrap(),
            format!("opacity=60\nplain=true\nlocked=false\noutlines={on}\n")
        );
    }
    enter(&mut app, 2, "Second");
    app.update();
    displays(&app, map_button, checkbox, false);
    let (mut restarted, map_button, checkbox) = controls(&dir);
    displays(&restarted, map_button, checkbox, true);
    enter(&mut restarted, 2, "Second");
    restarted.update();
    displays(&restarted, map_button, checkbox, false);
    std::fs::remove_dir_all(dir).unwrap();
}
