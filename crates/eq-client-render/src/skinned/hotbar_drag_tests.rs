//! The skin's real button factory must reach the same pointer/hold controller.
use super::*;
use crate::hud::hotbar::carry::pointer_tests;
use eq_client_assets::sidl::{Button, ButtonLook, Element};

#[test]
fn skinned_hotbuttons_pick_up_show_swap_and_remove_shortcuts() {
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
        pieces: (0_u16..10)
            .map(|index| {
                (
                    format!("slot{index}"),
                    Element::Button(Button {
                        id: Some(format!("HB_Button{}", index + 1)),
                        area: Area {
                            x: 10.0 + f32::from(index) * 50.0,
                            y: 40.0,
                            width: 40.0,
                            height: 40.0,
                        },
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
                    }),
                )
            })
            .collect(),
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
    pointer_tests::pickup_move_remove(pointer_tests::finish(app));
}
