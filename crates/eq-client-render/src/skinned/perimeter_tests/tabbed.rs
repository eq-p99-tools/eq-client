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
    let root = app
        .world_mut()
        .run_system_once(move |mut commands: Commands, mut art: crate::sheets::Art| {
            commands
                .spawn(Node {
                    overflow: Overflow::clip(),
                    ..at(0.0, 0.0, 386.0, 180.0)
                })
                .with_children(|root| {
                    tree::tabbed(
                        root,
                        &mut art,
                        &tab_box,
                        &Area {
                            x: 0.0,
                            y: 0.0,
                            width: 386.0,
                            height: 180.0,
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
