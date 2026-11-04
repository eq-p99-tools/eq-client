use super::*;
use eq_client_assets::sidl::{
    Anchor, Anchors, Button, ButtonLook, Element, FrameLook, Label, Page, Piece, Slider, SliderLook,
};

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
        let mut bottom = 0.0_f32;
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
            bottom = bottom.max(rect.y + rect.height);
        }
        assert_eq!(content_height(page), Some(bottom + MARGIN));
        if size.x < 200.0 {
            assert!(
                bottom > size.y,
                "narrow pages grow into the vertical scroll range"
            );
        }
    }
}

#[test]
fn tall_scale_controls_extend_the_complete_content_extent() {
    let mut skin = source();
    let Element::Slider(slider) = &mut skin.pieces[2].1 else {
        panic!("slider")
    };
    slider.area.height = 80.0;
    let made = pages(
        std::slice::from_ref(&skin),
        WindowId::Options,
        Vec2::new(180.0, 100.0),
        None,
    );
    let page = made.last().unwrap();
    let (_, Element::Slider(slider)) = &page.pieces[page.pieces.len() - 2] else {
        panic!("slider")
    };
    assert_eq!(
        content_height(page),
        Some(slider.area.y + slider.area.height + MARGIN)
    );
}
