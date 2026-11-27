//! Existing named skin controls use the same Give/Cancel handlers as a click.
use super::{background, frame_at, laid_out};
use crate::{
    give::{self, GiveButton},
    online::{OnlineState, testing},
    outbox::{Needs, Outbox},
    script::{InputMode, Phase, Program, RunStatus, Script},
    skinned::{Closes, SkinId},
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, SpawnKind, WorldEvent, exchange::ExchangeUpdate};
use std::{path::Path, sync::mpsc};

#[derive(Clone, Copy)]
struct Control {
    window: WindowId,
    id: &'static str,
    accepts: bool,
}

const CONTROLS: [Control; 4] = [
    Control {
        window: WindowId::Give,
        id: "GVW_Give_Button",
        accepts: true,
    },
    Control {
        window: WindowId::Give,
        id: "GVW_Cancel_Button",
        accepts: false,
    },
    Control {
        window: WindowId::Trade,
        id: "TRDW_Trade_Button",
        accepts: true,
    },
    Control {
        window: WindowId::Trade,
        id: "TRDW_Cancel_Button",
        accepts: false,
    },
];

fn fixture(control: Control, source: &str) -> (App, mpsc::Receiver<ClientCommand>, Entity) {
    let (mut app, _) = background(Vec::new());
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 7, testing::player(9));
    let mut partner = testing::pet(42, 0);
    partner.kind = if control.window == WindowId::Give {
        SpawnKind::Npc
    } else {
        SpawnKind::Player
    };
    testing::spawns(&mut online, vec![partner]);
    online.offer_trade(42);
    testing::news(
        &mut online,
        [WorldEvent::Exchange(ExchangeUpdate::Opened { with: 42 })],
    );
    let (sender, receiver) = mpsc::sync_channel(8);
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(sender)))
        .init_resource::<crate::inventory::InventoryState>()
        .init_resource::<crate::escape::Escape>()
        .insert_resource(
            Script::from_program(Program::parse(source, Path::new(".")).unwrap())
                .input_mode(InputMode::Background),
        )
        .add_systems(
            Update,
            (crate::skinned::close, give::buttons, give::window).chain(),
        );
    app.world_mut().resource_mut::<Shown>().open(control.window);
    let mut button = app.world_mut().spawn((
        Button,
        SkinId {
            window: control.window,
            id: control.id.into(),
        },
        laid_out(Vec2::new(240., 360.), 40., 1),
    ));
    if control.accepts {
        button.insert(GiveButton);
    } else {
        button.insert(Closes(control.window));
    }
    let button = button.id();
    (app, receiver, button)
}

fn finish(app: &mut App) {
    for ms in (0..=1_000).step_by(10) {
        frame_at(app, ms);
    }
}

#[test]
fn named_give_and_trade_buttons_dispatch_the_ordinary_accept_or_cancel_once() {
    for control in CONTROLS {
        let source = format!(
            "click(\"button\", \"{}\", \"{}\")\n",
            control.window.key(),
            control.id
        );
        let (mut app, receiver, _) = fixture(control, &source);
        finish(&mut app);
        assert_eq!(
            app.world().resource::<RunStatus>().phase(),
            Phase::Completed
        );
        let sent: Vec<_> = receiver.try_iter().collect();
        if control.accepts {
            assert!(
                matches!(
                    sent.as_slice(),
                    [ClientCommand::AcceptTrade { session_id: 7, .. }]
                ),
                "{source}: {sent:?}"
            );
            assert!(
                app.world()
                    .resource::<OnlineState>()
                    .world()
                    .exchange()
                    .unwrap()
                    .given
            );
            assert!(app.world().resource::<Shown>().is_open(control.window));
        } else {
            assert!(
                matches!(
                    sent.as_slice(),
                    [ClientCommand::CancelTrade { session_id: 7 }]
                ),
                "{source}: {sent:?}"
            );
            assert!(
                app.world()
                    .resource::<OnlineState>()
                    .world()
                    .exchange()
                    .is_none()
            );
            assert!(!app.world().resource::<Shown>().is_open(control.window));
        }
    }
}

#[test]
fn done_give_and_done_trade_resolve_the_cancel_body_button() {
    for control in CONTROLS.into_iter().filter(|control| !control.accepts) {
        let source = format!("click(\"done\", \"{}\")\n", control.window.key());
        let (mut app, receiver, _) = fixture(control, &source);
        finish(&mut app);
        assert_eq!(
            app.world().resource::<RunStatus>().phase(),
            Phase::Completed
        );
        assert!(matches!(
            receiver.try_iter().collect::<Vec<_>>().as_slice(),
            [ClientCommand::CancelTrade { session_id: 7 }]
        ));
        assert!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .exchange()
                .is_none()
        );
    }
}

#[test]
fn named_exchange_buttons_refuse_disabled_hidden_and_wrong_window_controls() {
    for control in CONTROLS {
        for refusal in 0..3 {
            let key = if refusal == 2 {
                "bank".into()
            } else {
                control.window.key()
            };
            let source = format!("click(\"button\", \"{key}\", \"{}\")\n", control.id);
            let (mut app, receiver, button) = fixture(control, &source);
            match refusal {
                0 => {
                    app.world_mut().entity_mut(button).insert(Needs::Missing);
                }
                1 => {
                    app.world_mut().get_mut::<Node>(button).unwrap().display = Display::None;
                }
                _ => (),
            }
            finish(&mut app);
            let status = app.world().resource::<RunStatus>();
            assert_eq!(status.phase(), Phase::Failed, "{source}, control={refusal}");
            let reason = &status.events().back().unwrap().message;
            if refusal == 0 {
                assert!(reason.contains("greyed out"), "{reason}");
            } else {
                assert!(reason.contains("not visible"), "{reason}");
            }
            assert!(receiver.try_recv().is_err(), "{source} must not send");
            assert!(
                !app.world()
                    .resource::<OnlineState>()
                    .world()
                    .exchange()
                    .unwrap()
                    .given
            );
            assert!(app.world().resource::<Shown>().is_open(control.window));
        }
    }
}
