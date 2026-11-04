//! The skin's real factories reach the same pointer and page controllers.
use super::*;
use crate::hud::hotbar::{carry::pointer_tests, page};
use eq_client_assets::sidl::{Button, ButtonLook, Element, Label};

fn button(id: String, area: Area) -> Element {
    Element::Button(Button {
        id: Some(id),
        area,
        placed: true,
        anchors: None,
        look: ButtonLook::default(),
        checkbox: false,
        text: None,
        text_color: None,
        font: None,
        decal: None,
        decal_area: None,
        tooltip: None,
    })
}

fn fixture() -> pointer_tests::Fixture {
    let mut pieces: Vec<_> = (0_u16..10)
        .map(|index| {
            (
                format!("slot{index}"),
                button(
                    format!("HB_Button{}", index + 1),
                    Area {
                        x: 10.0 + f32::from(index) * 50.0,
                        y: 40.0,
                        width: 40.0,
                        height: 40.0,
                    },
                ),
            )
        })
        .collect();
    for (id, x) in [("HB_PageLeftButton", 18.0), ("HB_PageRightButton", 68.0)] {
        pieces.push((
            id.into(),
            button(
                id.into(),
                Area {
                    x,
                    y: 108.0,
                    width: 24.0,
                    height: 24.0,
                },
            ),
        ));
    }
    pieces.push((
        "HB_CurrentPageLabel".into(),
        Element::Label(Label {
            area: Area {
                x: 110.0,
                y: 108.0,
                width: 20.0,
                height: 24.0,
            },
            anchors: None,
            eq_type: None,
            text: "1".into(),
            color: None,
            align: Align::Center,
            font: None,
            wraps: false,
        }),
    ));
    let screen = Screen {
        name: "HotButtonWnd".into(),
        title: None,
        title_color: None,
        font: None,
        area: Area {
            x: 0.0,
            y: 0.0,
            width: 540.0,
            height: 160.0,
        },
        template: None,
        title_bar: None,
        border: false,
        tooltip: None,
        pieces,
        tab_frame: None,
        transparent: false,
    };
    let mut app = pointer_tests::app();
    app.add_systems(
        Startup,
        move |mut commands: Commands, mut art: crate::sheets::Art| {
            let context = Context {
                id: WindowId::Actions,
                paperdoll: None,
                depth: 0,
                tab_frame: None,
            };
            commands.spawn(Node::default()).with_children(|window| {
                draw(window, &screen, &mut art, &context);
            });
        },
    );
    pointer_tests::finish(app)
}

#[test]
fn skinned_hotbuttons_pick_up_show_swap_and_remove_shortcuts() {
    pointer_tests::pickup_move_remove(fixture());
}

#[test]
fn skinned_controls_select_wrap_activate_bind_and_clear_the_current_page() {
    page::tests::controls(fixture());
}

#[test]
fn skinned_holds_cannot_cross_identical_pages_or_an_away_and_back_replacement() {
    for picked in [false, true] {
        for round_trip in [false, true] {
            page::tests::hold_context(fixture(), picked, round_trip);
        }
    }
}
