use super::*;
use eq_client_assets::sidl::{
    Anchor, Anchors, Button, ButtonLook, Element, FrameLook, Label, Page, Piece, Slider, SliderLook,
};
use eq_client_core::options::Level;

fn area(x: f32, y: f32, width: f32, height: f32) -> Area {
    Area {
        x,
        y,
        width,
        height,
    }
}

/// Invented skin geometry, including anchors which a transplanted control
/// must replace. No installed skin or captured artwork is included.
pub(in crate::skinned) fn source() -> Page {
    let anchors = Some(Anchors {
        left: Anchor::End(50.0),
        right: Anchor::End(5.0),
        top: Anchor::Start(700.0),
        bottom: Anchor::Start(720.0),
    });
    Page {
        name: "General".into(),
        title: Some("General".into()),
        area: None,
        template: None,
        icon: [None, None],
        title_colors: [None, None],
        tooltip: None,
        pieces: vec![
            (
                "checkbox".into(),
                Element::Button(Button {
                    id: None,
                    area: area(6.0, 700.0, 150.0, 20.0),
                    placed: false,
                    anchors,
                    look: ButtonLook::default(),
                    checkbox: true,
                    text: Some("Original".into()),
                    text_color: None,
                    font: Some(2),
                    decal: None,
                    decal_area: None,
                    tooltip: None,
                }),
            ),
            (
                "ExampleValueLabel".into(),
                Element::Label(Label {
                    area: area(160.0, 700.0, 40.0, 14.0),
                    anchors,
                    eq_type: None,
                    text: "100 %".into(),
                    color: None,
                    align: Align::Right,
                    font: Some(2),
                    wraps: false,
                }),
            ),
            (
                "ExampleSlider".into(),
                Element::Slider(Slider {
                    id: Some("ExampleSlider".into()),
                    area: area(10.0, 700.0, 150.0, 18.0),
                    look: SliderLook::default(),
                }),
            ),
        ],
    }
}

pub(in crate::skinned) fn page_frame() -> FrameLook {
    let piece = |width, height| {
        Some(Piece {
            texture: "synthetic-missing-frame.tga".into(),
            x: 0,
            y: 0,
            width,
            height,
        })
    };
    FrameLook {
        left: piece(7, 1),
        right: piece(9, 1),
        top: piece(1, 3),
        bottom: piece(1, 5),
        ..FrameLook::default()
    }
}

#[test]
fn generated_columns_fit_the_page_frame_and_keep_the_skins_controls() {
    for size in [
        Vec2::new(386.0, 430.0),
        Vec2::new(180.0, 100.0),
        Vec2::new(640.0, 220.0),
        Vec2::new(52.0, 80.0),
    ] {
        let skin = source();
        let frame = page_frame();
        let made = pages(
            std::slice::from_ref(&skin),
            WindowId::Options,
            size,
            Some(&frame),
        );
        assert_eq!(made[0], skin, "the imported page stays intact");
        let page = made.last().unwrap();
        let width = size.x - 16.0;
        let mut rects: Vec<Area> = Vec::new();
        for (_, element) in &page.pieces {
            let rect = match element {
                Element::Button(button) => {
                    assert!(button.anchors.is_none() && button.placed);
                    assert_eq!(button.font, Some(2));
                    assert_eq!(button.look, ButtonLook::default());
                    button.area
                }
                Element::Label(label) => {
                    assert!(label.anchors.is_none());
                    assert_eq!(label.font, Some(2));
                    label.area
                }
                Element::Slider(slider) => {
                    assert_eq!(slider.look, SliderLook::default());
                    slider.area
                }
                _ => panic!("generated setting control"),
            };
            assert!(rect.x >= 0.0 && rect.width >= 0.0);
            assert!(
                rect.x + rect.width <= width + 0.01,
                "{rect:?} inside {width}"
            );
            for other in &rects {
                assert!(
                    rect.x + rect.width <= other.x + 0.01
                        || other.x + other.width <= rect.x + 0.01
                        || rect.y + rect.height <= other.y + 0.01
                        || other.y + other.height <= rect.y + 0.01,
                    "{rect:?} overlaps {other:?}"
                );
            }
            rects.push(rect);
        }
        let bottom = rects
            .into_iter()
            .map(|rect| rect.y + rect.height)
            .fold(0.0, f32::max);
        assert_eq!(content_height(page), Some(bottom + MARGIN));
        if size.x < 200.0 {
            assert!(
                bottom > size.y,
                "narrow pages grow into the vertical scroll range"
            );
        }
    }
}

fn piece<'a>(page: &'a Page, id: &str) -> &'a Element {
    &page.pieces.iter().find(|(name, _)| name == id).unwrap().1
}

fn label<'a>(page: &'a Page, id: &str) -> &'a Label {
    let Element::Label(label) = piece(page, id) else {
        panic!("{id} must be a label")
    };
    label
}

fn slider<'a>(page: &'a Page, id: &str) -> &'a Slider {
    let Element::Slider(slider) = piece(page, id) else {
        panic!("{id} must be a slider")
    };
    slider
}

#[test]
fn dangerous_section_groups_the_existing_choices_and_run_speed() {
    let dangerous = [
        Fix::TakeEnvironmentalDamage,
        Fix::GillsForDays,
        Fix::Fly,
        Fix::JesusMode,
        Fix::ExitAtOnce,
    ];
    for size in [Vec2::new(640.0, 220.0), Vec2::new(180.0, 100.0)] {
        let skin = source();
        let made = pages(std::slice::from_ref(&skin), WindowId::Options, size, None);
        let page = made.last().unwrap();
        let ordinary = label(page, "EQC_QoL_SettingsCaption");
        let warning = label(page, "EQC_QoL_UnsafeCaption");
        assert_eq!(ordinary.text, "Quality of Life");
        assert_eq!(warning.text, "Dangerous");
        assert!((ordinary.area.width - (size.x - 2.0 * MARGIN)).abs() < f32::EPSILON);
        assert!((warning.area.width - ordinary.area.width).abs() < f32::EPSILON);

        let mut rightmost_checkbox = ordinary.area.x;
        for fix in Fix::settings() {
            let id = qol_checkbox_id(fix);
            let Element::Button(button) = piece(page, &id) else {
                panic!("{id} must be a checkbox")
            };
            assert_eq!(button.id.as_deref(), Some(id.as_str()));
            let area = button.area;
            rightmost_checkbox = rightmost_checkbox.max(area.x);
            if dangerous.contains(&fix) {
                assert!(area.y >= warning.area.y + warning.area.height);
            } else {
                assert!(area.y >= ordinary.area.y + ordinary.area.height);
                assert!(area.y + area.height < warning.area.y);
            }
        }
        assert_eq!(rightmost_checkbox > ordinary.area.x, size.x > 200.0);

        for (level, key, prefix, title) in [
            (
                Level::UiScale,
                "ui_scale",
                "EQC_QoL_UiScale",
                "Interface scale",
            ),
            (
                Level::RunSpeed,
                "run_boost",
                "EQC_QoL_RunSpeed",
                "Run a Little Faster",
            ),
        ] {
            let caption = label(page, &format!("{prefix}Label"));
            let slider_id = format!("{prefix}Slider");
            let slider = slider(page, &slider_id);
            let value_id = format!("{prefix}ValueLabel");
            let value = label(page, &value_id);
            assert_eq!(caption.text, title);
            assert_eq!(slider.id.as_deref(), Some(slider_id.as_str()));
            assert_eq!(level.key(), key);
            assert_eq!(
                controls::slider_sets(slider.id.as_deref(), WindowId::Options),
                Some(controls::Sets::Level(level))
            );
            assert!(matches!(
                controls::value_label(&value_id, WindowId::Options),
                Some(controls::ValueLabel::Shows(bound)) if bound == level
            ));
            assert!(slider.area.y >= caption.area.y + caption.area.height);
            if level == Level::UiScale {
                assert!(caption.area.y >= ordinary.area.y + ordinary.area.height);
                assert!(slider.area.y + slider.area.height < warning.area.y);
                assert!(value.area.y + value.area.height < warning.area.y);
            } else {
                assert!(caption.area.y >= warning.area.y + warning.area.height);
                assert!(value.area.y >= warning.area.y + warning.area.height);
            }
        }
    }
}

#[test]
fn section_headings_use_checkbox_typography_if_the_skin_has_no_label() {
    let mut skin = source();
    skin.pieces
        .retain(|(_, piece)| !matches!(piece, Element::Label(_)));
    let Element::Button(checkbox) = &mut skin.pieces[0].1 else {
        panic!("checkbox")
    };
    checkbox.text_color = Some([40, 150, 100]);

    let made = pages(&[skin], WindowId::Options, Vec2::new(180.0, 100.0), None).into_owned();
    let page = made.last().unwrap();
    for (id, text) in [
        ("EQC_QoL_SettingsCaption", "Quality of Life"),
        ("EQC_QoL_UnsafeCaption", "Dangerous"),
    ] {
        let heading = label(page, id);
        assert_eq!(heading.text, text);
        assert_eq!(heading.font, Some(2));
        assert_eq!(heading.color, Some([40, 150, 100]));
        assert!((heading.area.height - 20.0).abs() < f32::EPSILON);
        assert!(heading.eq_type.is_none() && heading.anchors.is_none());
    }
    assert_eq!(
        page.pieces
            .iter()
            .filter(|(_, piece)| matches!(piece, Element::Button(_)))
            .count(),
        Fix::settings().count()
    );
    assert!(
        page.pieces
            .iter()
            .all(|(_, piece)| !matches!(piece, Element::Slider(_)))
    );
}

#[test]
fn wrapped_slider_values_extend_the_sections_and_scroll_extent() {
    let mut skin = source();
    let Element::Slider(skin_slider) = &mut skin.pieces[2].1 else {
        panic!("slider")
    };
    skin_slider.area.height = 80.0;
    let made = pages(
        std::slice::from_ref(&skin),
        WindowId::Options,
        Vec2::new(76.0, 100.0),
        None,
    );
    let page = made.last().unwrap();
    let warning = label(page, "EQC_QoL_UnsafeCaption");
    let scale = slider(page, controls::UI_SCALE_SLIDER).area;
    let scale_value = label(page, controls::UI_SCALE_VALUE).area;
    let speed = slider(page, controls::RUN_SPEED_SLIDER).area;
    let speed_value = label(page, controls::RUN_SPEED_VALUE).area;
    assert!((scale.width - warning.area.width).abs() < f32::EPSILON);
    assert!((speed.width - warning.area.width).abs() < f32::EPSILON);
    assert!(scale_value.y > scale.y + scale.height);
    assert!(warning.area.y > scale_value.y + scale_value.height);
    assert!(speed_value.y > speed.y + speed.height);
    assert_eq!(
        content_height(page),
        Some(speed_value.y + speed_value.height + MARGIN)
    );
}
