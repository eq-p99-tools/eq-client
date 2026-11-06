//! Generated SIDL/art through the production border and Bevy layout/clip paths.
//! Pixel sampling below is a CPU UV check, not a GPU/native render comparison.
mod capture;
mod capture_pixels;
mod tabbed;

use super::*;
use bevy::{
    app::{HierarchyPropagatePlugin, PropagateSet, TaskPoolPlugin},
    camera::{ComputedCameraValues, RenderTargetInfo, Viewport},
    ecs::system::RunSystemOnce,
    math::Affine2,
    ui::{
        ui_layout_system,
        ui_surface::UiSurface,
        update::{propagate_ui_target_cameras, update_clipping_system},
    },
};
use std::{collections::BTreeMap, fmt::Write as _};

const SHEET: &str = "synthetic-border.tga";

const TAGS: [(&str, u16, u16); 12] = [
    ("TopLeft", 7, 3),
    ("Top", 2, 2),
    ("TopRight", 9, 4),
    ("RightTop", 3, 5),
    ("Right", 2, 2),
    ("RightBottom", 4, 3),
    ("BottomRight", 6, 5),
    ("Bottom", 2, 3),
    ("BottomLeft", 8, 4),
    ("LeftTop", 4, 6),
    ("Left", 3, 2),
    ("LeftBottom", 5, 2),
];

fn fixture() -> (eq_client_assets::sidl::Border, image::RgbaImage) {
    let mut animations = String::from("<XML>");
    let mut pieces = String::new();
    let mut pixels = image::RgbaImage::from_pixel(16, 104, image::Rgba([253, 0, 251, 255]));
    for (index, (tag, width, height)) in TAGS.iter().enumerate() {
        let row = u16::try_from(index).unwrap() * 8 + 1;
        write!(
            animations,
            "<Ui2DAnimation item=\"{tag}\"><Frames><Texture>{SHEET}</Texture>\
            <Location><X>2</X><Y>{row}</Y></Location>\
            <Size><CX>{width}</CX><CY>{height}</CY></Size></Frames></Ui2DAnimation>"
        )
        .unwrap();
        write!(pieces, "<{tag}>{tag}</{tag}>").unwrap();
        let color = image::Rgba([u8::try_from(index * 17 + 1).unwrap(), 40, 90, 255]);
        for y in 0..u32::from(*height) {
            for x in 0..u32::from(*width) {
                pixels.put_pixel(2 + x, u32::from(row) + y, color);
            }
        }
    }
    // Natural transparent margin and opaque black inside the very same cut.
    for y in 1..4 {
        pixels.put_pixel(2, y, image::Rgba([0, 0, 0, 0]));
    }
    pixels.put_pixel(3, 2, image::Rgba([0, 0, 0, 255]));
    animations.push_str("</XML>");
    let library = Library::parse(&animations, &format!(
        "<XML><WindowDrawTemplate item=\"Border\"><Border>{pieces}</Border></WindowDrawTemplate></XML>"
    )).unwrap();
    let screen = library
        .screen(
            "<XML><Screen item=\"Example\"><Size><CX>40</CX><CY>32</CY></Size>\
        <DrawTemplate>Border</DrawTemplate><Style_Border>true</Style_Border></Screen></XML>",
            "Example",
        )
        .unwrap();
    (screen.template.unwrap().border, pixels)
}

fn app(scale: f32, pixels: image::RgbaImage) -> App {
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default())
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
            PostUpdate,
        ))
        .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
            PostUpdate,
        ))
        .init_resource::<UiSurface>()
        .init_resource::<bevy::text::TextPipeline>()
        .init_resource::<bevy::text::FontCx>()
        .init_resource::<bevy::text::ScaleCx>()
        .init_resource::<crate::sheets::Sheets>()
        .init_resource::<Assets<Image>>()
        .init_resource::<crate::skin::UiSkin>()
        .insert_resource(UiScale(scale))
        .insert_resource(crate::ViewerSettings(crate::ViewerConfig {
            eq_directory: Some("synthetic-no-files".into()),
            ..default()
        }))
        .add_systems(
            PostUpdate,
            (
                propagate_ui_target_cameras,
                reflow::update,
                ui_layout_system,
                pixel_edges::snap,
                update_clipping_system,
            )
                .chain(),
        )
        .configure_sets(
            PostUpdate,
            PropagateSet::<ComputedUiTargetCamera>::default()
                .after(propagate_ui_target_cameras)
                .before(ui_layout_system),
        )
        .configure_sets(
            PostUpdate,
            PropagateSet::<ComputedUiRenderTargetInfo>::default()
                .after(propagate_ui_target_cameras)
                .before(ui_layout_system),
        );
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(512, 256),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::new(512, 256),
                ..default()
            }),
            ..default()
        },
    ));
    crate::sheets::testing::sheet(&mut app, SHEET, pixels);
    app
}

fn spawn(app: &mut App, look: eq_client_assets::sidl::Border, size: Vec2) -> (Entity, Area) {
    app.world_mut()
        .run_system_once(move |mut commands: Commands, mut art: crate::sheets::Art| {
            let mut inside = Area {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            };
            let root = commands
                .spawn(at(7.0, 11.0, size.x, size.y))
                .with_children(|root| {
                    inside = border(root, &mut art, &look, (size.x, size.y));
                })
                .id();
            (root, inside)
        })
        .unwrap()
}

fn bounds(world: &World, entity: Entity) -> Rect {
    let node = world.get::<ComputedNode>(entity).unwrap();
    let transform = world.get::<UiGlobalTransform>(entity).unwrap();
    Rect::from_center_size(Affine2::from(*transform).translation, node.size())
}

fn images(world: &mut World) -> BTreeMap<String, Entity> {
    world
        .query::<(Entity, &ImageNode, &ComputedNode)>()
        .iter(world)
        .filter(|(_, _, node)| node.size().cmpgt(Vec2::ZERO).all())
        .map(|(entity, image, _)| {
            let rect = image.rect.unwrap();
            let row = rect.min.y;
            let tag = TAGS
                .iter()
                .enumerate()
                .find(|(index, _)| {
                    let index = u16::try_from(*index).unwrap();
                    (f32::from(index * 8 + 1) - row).abs() < f32::EPSILON
                })
                .unwrap()
                .1
                .0;
            (tag.to_owned(), entity)
        })
        .collect()
}

#[test]
#[allow(clippy::too_many_lines)]
fn resize_reflows_real_nested_chat_widgets_clips_and_perimeter_without_replacement() {
    use eq_client_assets::sidl::{Sizable, TextBox, View, WindowTemplate};
    for scale in [0.75, 1.25, 2.0] {
        let (look, pixels) = fixture();
        let mut app = app(scale, pixels);
        app.init_resource::<crate::keys::Typing>();
        let outer = WindowTemplate {
            border: look.clone(),
            title: [
                look.top_left.clone(),
                look.top.clone(),
                look.top_right.clone(),
            ],
            close_box: Some(ButtonLook {
                normal: look.top_left.clone(),
                ..default()
            }),
            minimize_box: Some(ButtonLook {
                normal: look.top_right.clone(),
                ..default()
            }),
            ..default()
        };
        let text = |id: &str, anchors| {
            Element::TextBox(TextBox {
                id: Some(id.into()),
                area: Area {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 50.0,
                },
                anchors: Some(anchors),
                template: None,
                color: None,
                font: None,
                scrollbar: (id == "CWChatOutput")
                    .then(eq_client_assets::sidl::ScrollbarLook::default),
            })
        };
        let screen = Screen {
            name: "SyntheticChat".into(),
            title: None,
            title_color: None,
            font: None,
            area: Area {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 250.0,
            },
            sizable: Sizable::Enabled,
            template: Some(outer.clone()),
            border: true,
            title_bar: Some(eq_client_assets::sidl::TitleBar {
                close_box: true,
                minimize_box: true,
            }),
            transparent: false,
            tooltip: None,
            tab_frame: None,
            pieces: vec![
                (
                    "Label".into(),
                    Element::Label(eq_client_assets::sidl::Label {
                        area: screen_area(),
                        anchors: Some(Anchors {
                            left: Anchor::Start(12.0),
                            top: Anchor::Start(42.0),
                            right: Anchor::End(12.0),
                            bottom: Anchor::Start(62.0),
                        }),
                        eq_type: None,
                        text: "retained label".into(),
                        color: None,
                        align: Align::Left,
                        font: None,
                        wraps: false,
                    }),
                ),
                (
                    "Output".into(),
                    text(
                        "CWChatOutput",
                        reflow::Geometry::inset(8.0, 8.0, 8.0, 40.0).0,
                    ),
                ),
                (
                    "Nested".into(),
                    Element::View(Box::new(View {
                        name: "Nested".into(),
                        area: screen_area(),
                        anchors: Some(reflow::Geometry::inset(20.0, 80.0, 20.0, 10.0).0),
                        template: Some(outer),
                        border: true,
                        tooltip: None,
                        scrollbar: None,
                        pieces: vec![(
                            "Input".into(),
                            text(
                                "CWChatInput",
                                Anchors {
                                    left: Anchor::Start(6.0),
                                    top: Anchor::End(26.0),
                                    right: Anchor::End(6.0),
                                    bottom: Anchor::End(6.0),
                                },
                            ),
                        )],
                    })),
                ),
            ],
        };
        let root = app
            .world_mut()
            .run_system_once(move |mut commands: Commands, mut art: crate::sheets::Art| {
                commands
                    .spawn((
                        at(7.0, 11.0, 400.0, 250.0),
                        crate::windows::Frame::default(),
                    ))
                    .with_children(|root| {
                        draw(
                            root,
                            &screen,
                            &mut art,
                            &Context {
                                id: WindowId::Chat,
                                paperdoll: None,
                                depth: 0,
                                tab_frame: None,
                            },
                        );
                    })
                    .id()
            })
            .unwrap();
        app.update();
        let world = app.world_mut();
        let input = world
            .query_filtered::<Entity, With<crate::chat::InputBox>>()
            .single(world)
            .unwrap();
        let label = world
            .query_filtered::<Entity, With<crate::chat::InputLabel>>()
            .single(world)
            .unwrap();
        world.get_mut::<Text>(label).unwrap().0 = "a retained draft".into();
        let lines = world
            .query_filtered::<Entity, With<crate::chat::Viewport>>()
            .single(world)
            .unwrap();
        world.get_mut::<ScrollPosition>(lines).unwrap().y = 37.0;
        world.resource_mut::<crate::keys::Typing>().composing = true;
        let entities: Vec<_> = world
            .query_filtered::<Entity, With<Node>>()
            .iter(world)
            .collect();
        let old_input = bounds(world, input);
        let old_lines = bounds(world, lines);
        let label_box = world
            .query::<(&Text, &ChildOf)>()
            .iter(world)
            .find(|(text, _)| text.0 == "retained label")
            .unwrap()
            .1
            .parent();
        let old_label = bounds(world, label_box);
        {
            let mut node = world.get_mut::<Node>(root).unwrap();
            node.width = px(600);
            node.height = px(370);
        }
        app.update();
        let world = app.world_mut();
        let new_input = bounds(world, input);
        let new_lines = bounds(world, lines);
        let near = |actual: f32, expected: f32| {
            assert!(
                (actual - expected).abs() <= 1.0,
                "{actual} versus {expected}"
            );
        };
        near(new_input.width() - old_input.width(), 200.0 * scale);
        near(new_input.min.y - old_input.min.y, 120.0 * scale);
        near(new_input.height(), old_input.height());
        near(new_lines.width() - old_lines.width(), 200.0 * scale);
        near(new_lines.height() - old_lines.height(), 120.0 * scale);
        near(
            bounds(world, label_box).width() - old_label.width(),
            200.0 * scale,
        );
        assert_eq!(world.get::<ScrollPosition>(lines).unwrap().y, 37.0);
        assert_eq!(world.get::<Text>(label).unwrap().0, "a retained draft");
        assert!(world.resource::<crate::keys::Typing>().composing);
        assert_eq!(
            world
                .query_filtered::<Entity, With<Node>>()
                .iter(world)
                .collect::<std::collections::BTreeSet<_>>(),
            entities.into_iter().collect()
        );
        let clip = world.get::<CalculatedClip>(input).unwrap().clip;
        assert!(clip.max.cmple(bounds(world, root).max).all());
        let perimeter = world
            .query::<(&reflow::Geometry, &pixel_edges::Piece)>()
            .iter(world)
            .count();
        assert!(perimeter >= 24); // Both the owner and nested view keep all twelve pieces.
    }
}

fn screen_area() -> Area {
    Area {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 100.0,
    }
}

fn pixel(image: &Image, at: UVec2) -> [u8; 4] {
    let index = usize::try_from((at.y * image.size().x + at.x) * 4).unwrap();
    image.data.as_ref().unwrap()[index..index + 4]
        .try_into()
        .unwrap()
}

fn assert_cut(world: &World, entity: Entity, tag: &str, destination: Rect) {
    let image_node = world.get::<ImageNode>(entity).unwrap();
    assert!(matches!(image_node.image_mode, NodeImageMode::Stretch));
    let source = image_node.rect.unwrap();
    let (_, source_width, source_height) = TAGS.iter().find(|(name, _, _)| *name == tag).unwrap();
    assert_eq!(source.min.x, 2.0);
    assert_eq!(
        source.size(),
        Vec2::new(f32::from(*source_width), f32::from(*source_height))
    );
    let image = world
        .resource::<Assets<Image>>()
        .get(&image_node.image)
        .unwrap();
    // Nearest UV samples stay within this cut, never in its bright
    // neighboring cells, even when its destination is stretched.
    for yy in 0..destination.size().as_uvec2().y {
        for xx in 0..destination.size().as_uvec2().x {
            let fraction = (UVec2::new(xx, yy).as_vec2() + Vec2::splat(0.5)) / destination.size();
            let sample = pixel(
                image,
                (source.min + source.size() * fraction).floor().as_uvec2(),
            );
            assert_ne!(sample, [253, 0, 251, 255]);
        }
    }
}

fn assert_joins(world: &World, entities: &BTreeMap<String, Entity>) {
    // These independently rounded shared edges must still meet exactly.
    assert_eq!(
        bounds(world, entities["TopLeft"]).max.x,
        bounds(world, entities["Top"]).min.x
    );
    assert_eq!(
        bounds(world, entities["TopRight"]).max.y,
        bounds(world, entities["RightTop"]).min.y
    );
    assert_eq!(
        bounds(world, entities["RightTop"]).max.y,
        bounds(world, entities["Right"]).min.y
    );
    assert_eq!(
        bounds(world, entities["Left"]).max.y,
        bounds(world, entities["LeftBottom"]).min.y
    );
    for (before, after, vertical) in [
        ("Top", "TopRight", false),
        ("Right", "RightBottom", true),
        ("RightBottom", "BottomRight", true),
        ("Bottom", "BottomRight", false),
        ("BottomLeft", "Bottom", false),
        ("LeftBottom", "BottomLeft", true),
        ("LeftTop", "Left", true),
        ("TopLeft", "LeftTop", true),
    ] {
        let before = bounds(world, entities[before]).max;
        let after = bounds(world, entities[after]).min;
        assert_eq!(
            if vertical { before.y } else { before.x },
            if vertical { after.y } else { after.x }
        );
    }
}

fn assert_frame_bounds(
    world: &World,
    root: Entity,
    entities: &BTreeMap<String, Entity>,
    clip: Rect,
) {
    let frame = bounds(world, root);
    for (tag, entity) in entities {
        let piece = bounds(world, *entity);
        assert!(
            piece.min.cmpge(frame.min).all() && piece.max.cmple(frame.max).all(),
            "{tag}: {piece:?} outside {frame:?}"
        );
        assert_eq!(world.get::<CalculatedClip>(*entity).unwrap().clip, clip);
        assert_cut(world, *entity, tag, piece);
    }
}

fn geometry(world: &World, entities: &BTreeMap<String, Entity>) -> Vec<(Rect, Rect)> {
    entities
        .values()
        .map(|entity| {
            (
                bounds(world, *entity),
                world.get::<CalculatedClip>(*entity).unwrap().clip,
            )
        })
        .collect()
}

#[test]
fn perimeter_joins_and_uv_margins_survive_fractional_physical_scales() {
    // Golden rectangles use a deliberately asymmetric synthetic frame.
    let expected = [
        ("TopLeft", [0.0, 0.0, 7.0, 3.0]),
        ("Top", [7.0, 0.0, 24.0, 2.0]),
        ("TopRight", [31.0, 0.0, 9.0, 4.0]),
        ("RightTop", [37.0, 4.0, 3.0, 5.0]),
        ("Right", [38.0, 9.0, 2.0, 15.0]),
        ("RightBottom", [36.0, 24.0, 4.0, 3.0]),
        ("BottomRight", [34.0, 27.0, 6.0, 5.0]),
        ("Bottom", [8.0, 29.0, 26.0, 3.0]),
        ("BottomLeft", [0.0, 28.0, 8.0, 4.0]),
        ("LeftTop", [0.0, 3.0, 4.0, 6.0]),
        ("Left", [0.0, 9.0, 3.0, 17.0]),
        ("LeftBottom", [0.0, 26.0, 5.0, 2.0]),
    ];
    for scale in [1.0, 1.25, 1.5, 2.25] {
        let (look, pixels) = fixture();
        let mut app = app(scale, pixels);
        let (root, inside) = spawn(&mut app, look, Vec2::new(40.0, 32.0));
        assert_eq!(
            (inside.x, inside.y, inside.width, inside.height),
            (5.0, 4.0, 31.0, 23.0)
        );
        app.update();
        let entities = images(app.world_mut());
        assert_eq!(entities.len(), 12);
        let root_bounds = bounds(app.world(), root);
        assert_frame_bounds(app.world(), root, &entities, root_bounds);
        // UI global transforms already use physical coordinates from the
        // target's top left; there is no camera-center conversion here.
        assert_eq!(
            root_bounds.min,
            Vec2::new((7.0 * scale).round(), (11.0 * scale).round())
        );
        for (tag, [x, y, width, height]) in expected {
            let entity = entities[tag];
            let rect = bounds(app.world(), entity);
            let physical = rect.min;
            assert_eq!(
                physical,
                Vec2::new(((7.0 + x) * scale).round(), ((11.0 + y) * scale).round()),
                "{tag} at {scale}"
            );
            let end = rect.max;
            assert_eq!(
                end,
                Vec2::new(
                    ((7.0 + x + width) * scale).round(),
                    ((11.0 + y + height) * scale).round()
                ),
                "{tag} at {scale}"
            );
            let clip = app.world().get::<CalculatedClip>(entity).unwrap().clip;
            assert_eq!(clip, root_bounds);
            assert_cut(app.world(), entity, tag, rect);
        }
        let node = app.world().get::<ImageNode>(entities["TopLeft"]).unwrap();
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&node.image)
            .unwrap();
        assert_eq!(pixel(image, UVec2::new(2, 1)), [0, 0, 0, 0]);
        assert_eq!(pixel(image, UVec2::new(3, 2)), [0, 0, 0, 255]);
        assert_joins(app.world(), &entities);
    }
}

#[test]
fn perimeter_moved_parents_scale_changes_and_repeated_frames_keep_shared_edges() {
    let (look, pixels) = fixture();
    let mut app = app(1.0, pixels);
    let (root, _) = spawn(&mut app, look, Vec2::new(40.0, 32.0));
    let parent = app
        .world_mut()
        .spawn(Node {
            overflow: Overflow::clip(),
            ..at(1.2, 2.4, 30.0, 25.0)
        })
        .id();
    app.world_mut().entity_mut(parent).add_child(root);
    // An ordinary control-sized node deliberately lacks the perimeter marker.
    let unmarked = app.world_mut().spawn(at(38.0, 9.0, 2.0, 15.0)).id();
    app.world_mut().entity_mut(root).add_child(unmarked);
    for (camera_scale, ui_scale, position) in [
        (1.0, 1.0, Vec2::new(1.2, 2.4)),
        (1.25, 1.0, Vec2::new(3.25, 5.75)),
        (1.5, 1.0, Vec2::new(11.5, 9.25)),
        (1.5, 1.5, Vec2::new(1.2, 2.4)),
        (1.0, 1.25, Vec2::new(11.5, 9.25)),
        (1.0, 1.0, Vec2::new(3.25, 5.75)),
    ] {
        app.world_mut().resource_mut::<UiScale>().0 = ui_scale;
        {
            let world = app.world_mut();
            let mut cameras = world.query::<&mut Camera>();
            cameras
                .single_mut(world)
                .unwrap()
                .computed
                .target_info
                .as_mut()
                .unwrap()
                .scale_factor = camera_scale;
            let mut node = world.get_mut::<Node>(parent).unwrap();
            node.left = px(position.x);
            node.top = px(position.y);
        }
        app.update();
        let entities = images(app.world_mut());
        let expected_clip = bounds(app.world(), parent).intersect(bounds(app.world(), root));
        assert_eq!(entities.len(), 12);
        assert_joins(app.world(), &entities);
        assert_frame_bounds(app.world(), root, &entities, expected_clip);
        let initial = geometry(app.world(), &entities);
        let control = bounds(app.world(), unmarked);
        // A second finalization without another layout must be idempotent.
        app.world_mut().run_system_once(pixel_edges::snap).unwrap();
        assert_eq!(geometry(app.world(), &entities), initial);
        assert_eq!(bounds(app.world(), unmarked), control);
        for _ in 0..4 {
            app.update();
            assert_eq!(geometry(app.world(), &entities), initial);
            assert_eq!(bounds(app.world(), unmarked), control);
        }
    }
}

#[test]
fn perimeter_small_and_empty_windows_keep_nonnegative_clients_and_clipped_art() {
    for scale in [1.0, 1.25] {
        for size in [
            Vec2::new(17.0, 18.0),
            Vec2::new(9.0, 8.0),
            Vec2::new(0.0, 32.0),
            Vec2::new(40.0, 0.0),
        ] {
            let (look, pixels) = fixture();
            let mut app = app(scale, pixels);
            let (root, inside) = spawn(&mut app, look, size);
            assert!(inside.width >= 0.0 && inside.height >= 0.0);
            app.update();
            let entities = images(app.world_mut());
            if size.x == 0.0 || size.y == 0.0 {
                assert_eq!(entities.len(), 0);
                continue;
            }
            let root = bounds(app.world(), root);
            for entity in entities.values() {
                let drawn = bounds(app.world(), *entity);
                assert!(drawn.width() > 0.0 && drawn.height() > 0.0);
                assert_eq!(
                    app.world().get::<CalculatedClip>(*entity).unwrap().clip,
                    root
                );
            }
            if size.x < 16.0 {
                assert!(!entities.contains_key("Top") && !entities.contains_key("Bottom"));
                assert!(!entities.contains_key("Left") && !entities.contains_key("Right"));
            }
        }
    }
}

#[test]
fn perimeter_zero_size_caps_preserve_the_corner_anchor_without_drawing() {
    for scale in [1.0, 1.25] {
        for present in [true, false] {
            let (mut look, pixels) = fixture();
            if present {
                let cap = look.left_top.as_mut().unwrap();
                cap.width = 0;
                cap.height = 0;
            } else {
                look.left_top = None;
            }
            let mut app = app(scale, pixels);
            spawn(&mut app, look, Vec2::new(40.0, 32.0));
            app.update();
            let entities = images(app.world_mut());
            assert!(!entities.contains_key("LeftTop"));
            let start = bounds(app.world(), entities["Left"]).min.y;
            let logical = if present { 14.0 } else { 11.0 };
            assert_eq!(start, (logical * scale).round());
        }
    }
}
