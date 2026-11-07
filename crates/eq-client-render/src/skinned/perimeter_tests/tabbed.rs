use super::*;

fn spawn_tabs(app: &mut App, owner: WindowId, count: usize) -> (Entity, Vec<(usize, Entity)>) {
    let pages = (0..count)
        .map(|index| eq_client_assets::sidl::Page {
            name: format!("page{index}"),
            title: Some(if index == 8 {
                "QoL".into()
            } else {
                format!("Tab{index}")
            }),
            area: None,
            template: None,
            pieces: Vec::new(),
            icon: [None, None],
            title_colors: [None, None],
            tooltip: None,
        })
        .collect();
    let tab_box = eq_client_assets::sidl::TabBox {
        name: "OptionsTabs".into(),
        area: None,
        pages,
        tab_frame: None,
        page_frame: None,
    };
    spawn_tab_box(app, owner, tab_box, Vec2::new(386.0, 180.0))
}

fn spawn_tab_box(
    app: &mut App,
    owner: WindowId,
    tab_box: eq_client_assets::sidl::TabBox,
    size: Vec2,
) -> (Entity, Vec<(usize, Entity)>) {
    let root = app
        .world_mut()
        .run_system_once(move |mut commands: Commands, mut art: crate::sheets::Art| {
            commands
                .spawn(Node {
                    overflow: Overflow::clip(),
                    ..at(0.0, 0.0, size.x, size.y)
                })
                .with_children(|root| {
                    tree::tabbed(
                        root,
                        &mut art,
                        &tab_box,
                        &Area {
                            x: 0.0,
                            y: 0.0,
                            width: size.x,
                            height: size.y,
                        },
                        &Context {
                            id: owner,
                            paperdoll: None,
                            depth: 0,
                            tab_frame: None,
                        },
                    );
                })
                .id()
        })
        .unwrap();
    let mut query = app.world_mut().query::<(Entity, &SkinTab)>();
    let mut controls: Vec<_> = query
        .iter(app.world())
        .map(|(entity, tab)| (tab.index, entity))
        .collect();
    controls.sort_by_key(|(index, _)| *index);
    for (_, entity) in &controls {
        app.world_mut().get_mut::<Node>(*entity).unwrap().width = px(48);
    }
    (root, controls)
}

/// Fixed intrinsic widths stand in for shaped text; the production Bevy layout
/// and clip systems decide whether all nine tabs are reachable.
#[test]
fn options_tabs_wrap_inside_the_window_and_keep_page_contents_reachable() {
    for scale in [1.0, 1.5] {
        let (_, pixels) = fixture();
        let mut app = app(scale, pixels);
        crate::clicks::testing::install(&mut app);
        app.init_resource::<Tabs>().add_systems(Update, tabs);
        let (root, controls) = spawn_tabs(&mut app, WindowId::Options, 9);
        app.update();
        app.update();
        let window = bounds(app.world(), root);
        for (_, entity) in &controls {
            let tab = bounds(app.world(), *entity);
            assert!(
                tab.min.x >= window.min.x && tab.max.x <= window.max.x + 0.01,
                "every tab must remain inside the window: {tab:?} in {window:?}"
            );
            let clip = app.world().get::<CalculatedClip>(*entity).unwrap().clip;
            assert!(
                clip.contains(tab.center()),
                "the product click target must be unclipped"
            );
        }
        let first = bounds(app.world(), controls[0].1);
        let last = bounds(app.world(), controls[8].1);
        assert!(
            last.min.y >= first.max.y,
            "the ninth tab wraps to another row"
        );
        crate::clicks::testing::click(&mut app, controls[8].1);
        let mut page_query = app.world_mut().query::<(Entity, &SkinPage)>();
        let page = page_query
            .iter(app.world())
            .find(|(_, page)| page.0.index == 8)
            .unwrap()
            .0;
        let viewport = bounds(app.world(), page);
        assert!(
            viewport.min.y >= last.max.y,
            "the page starts below all tab rows"
        );
        assert!((viewport.max.y - window.max.y).abs() < 0.01);
        let computed = app.world().get::<ComputedNode>(page).unwrap();
        assert!(
            computed.content_size().y > computed.size().y,
            "wrapping preserves the authored page height as scrollable content"
        );
        assert!(app.world().get::<ScrolledView>(page).is_some());
    }
}

#[test]
fn actions_tabs_keep_their_single_row_and_authored_page_geometry() {
    let (_, pixels) = fixture();
    let mut app = app(1.0, pixels);
    app.init_resource::<Tabs>().add_systems(Update, tabs);
    let (root, controls) = spawn_tabs(&mut app, WindowId::ActionsWindow, 3);
    app.update();
    app.update();
    let window = bounds(app.world(), root);
    let first = bounds(app.world(), controls[0].1);
    for (_, control) in controls {
        assert_eq!(bounds(app.world(), control).min.y, first.min.y);
    }
    let mut query = app.world_mut().query::<(Entity, &SkinPage)>();
    let page = query
        .iter(app.world())
        .find(|(_, page)| page.0.index == 0)
        .unwrap()
        .0;
    let shown = bounds(app.world(), page);
    assert_eq!(shown.min.y - window.min.y, 20.0);
    assert_eq!(shown.max, window.max);
    assert!(app.world().get::<ScrolledView>(page).is_none());
}

/// Generated controls use real page-frame insets and the measured viewport
/// below two tab rows. Their source anchors must not escape either boundary.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one production layout and scroll regression across sizes and scales"
)]
fn qol_controls_fit_the_width_and_scroll_fully_below_wrapped_tabs() {
    use crate::options::OptionCheckbox;
    use crate::windows::pointer::{TakesWheel, Wheel};
    use eq_client_core::options::{Level, Toggle};
    use eq_client_core::qol::Fix;

    for scale in [1.0, 1.5] {
        for size in [Vec2::new(386.0, 408.0), Vec2::new(180.0, 180.0)] {
            let (_, pixels) = fixture();
            let mut app = app(scale, pixels);
            crate::clicks::testing::install(&mut app);
            app.init_resource::<Tabs>()
                .init_resource::<Wheel>()
                .add_systems(Update, (tabs, controls::scroll_lists));
            let pages = (0..8)
                .map(|index| eq_client_assets::sidl::Page {
                    name: format!("page{index}"),
                    title: Some(if index == 7 {
                        "Particles".into()
                    } else {
                        format!("Tab{index}")
                    }),
                    ..qol::tests::source()
                })
                .collect();
            let tab_box = eq_client_assets::sidl::TabBox {
                name: "OptionsTabs".into(),
                area: None,
                pages,
                tab_frame: None,
                page_frame: Some(qol::tests::page_frame()),
            };
            let (root, tabs) = spawn_tab_box(&mut app, WindowId::Options, tab_box, size);
            app.update();
            app.update();
            let first_tab = bounds(app.world(), tabs[0].1);
            let last_tab = bounds(app.world(), tabs[8].1);
            assert!(last_tab.min.y >= first_tab.max.y);
            crate::clicks::testing::click(&mut app, tabs[8].1);
            let mut query = app.world_mut().query::<(Entity, &SkinPage)>();
            let page = query
                .iter(app.world())
                .find(|(_, page)| page.0.index == 8)
                .unwrap()
                .0;
            let viewport = bounds(app.world(), page);
            assert!(viewport.min.y >= last_tab.max.y);
            assert!(app.world().get::<TakesWheel>(page).is_some());

            let mut query = app.world_mut().query::<(Entity, &OptionCheckbox)>();
            let checkboxes: Vec<_> = query
                .iter(app.world())
                .filter(|(_, checkbox)| matches!(checkbox.0, Toggle::Qol(_)))
                .map(|(entity, checkbox)| (entity, checkbox.0))
                .collect();
            assert_eq!(checkboxes.len(), Fix::settings().count());
            for (entity, _) in &checkboxes {
                let control = bounds(app.world(), *entity);
                assert!(control.min.x >= viewport.min.x && control.max.x <= viewport.max.x);
            }
            if size.x < 200.0 {
                assert!(
                    checkboxes
                        .iter()
                        .any(|(entity, _)| bounds(app.world(), *entity).max.y > viewport.max.y),
                    "the compact viewport must scroll to reach every checkbox"
                );
            }
            let mut sliders = app.world_mut().query::<(Entity, &SkinSlider)>();
            let mut labels = app.world_mut().query::<(&ChildOf, &controls::LevelValue)>();
            let mut words = app.world_mut().query::<(&ChildOf, &Text)>();
            for (level, text) in [
                (Level::UiScale, "Interface scale"),
                (Level::RunSpeed, "Run a Little Faster"),
            ] {
                let slider = sliders
                    .iter(app.world())
                    .find(|(_, slider)| slider.sets == Sets::Level(level))
                    .unwrap()
                    .0;
                let value = labels
                    .iter(app.world())
                    .find(|(_, label)| label.0 == level)
                    .unwrap()
                    .0
                    .parent();
                let caption = words
                    .iter(app.world())
                    .find(|(_, label)| label.0 == text)
                    .unwrap()
                    .0
                    .parent();
                let controls = [slider, value, caption];
                for control in controls {
                    let rect = bounds(app.world(), control);
                    assert!(rect.min.x >= viewport.min.x && rect.max.x <= viewport.max.x);
                }

                // Reach Interface scale within its section, then exercise
                // the end of the page's scroll extent for the run speed slider.
                let pixels = if level == Level::UiScale {
                    let bottom_edge = controls
                        .into_iter()
                        .map(|control| bounds(app.world(), control).max.y)
                        .fold(viewport.max.y, f32::max);
                    (viewport.max.y - bottom_edge) / scale - 2.0
                } else {
                    -100_000.0
                };
                *app.world_mut().resource_mut::<Wheel>() = Wheel {
                    surface: Some(page),
                    pixels,
                    lines: 0.0,
                };
                app.update();
                app.world_mut().resource_mut::<Wheel>().pixels = 0.0;
                app.update();
                for control in controls {
                    let rect = bounds(app.world(), control);
                    let clip = app.world().get::<CalculatedClip>(control).unwrap().clip;
                    assert!(
                        rect.min.cmpge(clip.min).all() && rect.max.cmple(clip.max).all(),
                        "complete control {rect:?} inside {clip:?}"
                    );
                }
            }
            let mut wheel_owners = app.world_mut().query_filtered::<Entity, With<TakesWheel>>();
            assert!(
                wheel_owners
                    .iter(app.world())
                    .all(|entity| app.world().get::<SkinPage>(entity).is_some()),
                "one wheel owner per page, with no nested QoL scroller"
            );
            assert_eq!(bounds(app.world(), root).size(), size * scale);
        }
    }
}
