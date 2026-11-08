//! Adapter-backed probe of typed session ingress and the real banner node.
use super::*;
use crate::online::{self, OnlineState, Updates, testing as session};
use bevy::{
    app::AppExit,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};
use eq_client_core::{WorldEvent, WorldUpdate, world::Link};
use std::{
    path::PathBuf,
    sync::{Mutex, mpsc},
};

#[derive(Default)]
enum Step {
    #[default]
    Start,
    FadeIn,
    Hold(u8),
    Capture,
    FadeOut,
    Expired,
    Reset,
    Cleared,
}

#[derive(Resource)]
struct Probe {
    updates: mpsc::Sender<WorldUpdate>,
    screenshot: PathBuf,
    captured: bool,
}

const TEXT: &str = "Synthetic zone notice";

fn banner(text: &str) -> WorldUpdate {
    let interval = Duration::from_secs(4);
    WorldUpdate::Game(WorldEvent::Marquee(Marquee {
        message_type: 15,
        priority: 255,
        fade_in: interval,
        fade_out: interval,
        duration: interval,
        text: text.into(),
    }))
}

#[test]
#[ignore = "requires EQ_MARQUEE_PREVIEW_PNG and a graphics adapter; hidden offline window"]
fn capture() {
    let path = PathBuf::from(std::env::var_os("EQ_MARQUEE_PREVIEW_PNG").unwrap());
    assert!(
        path.is_absolute() && !path.exists(),
        "use a fresh absolute PNG path"
    );
    let mut app = crate::testing::hidden_graphics_probe("Offline marquee probe", (800, 500));
    crate::init_presentation(&mut app);
    let (updates, receiver) = mpsc::channel();
    updates.send(banner(TEXT)).unwrap();
    let mut online = OnlineState::new(false);
    session::admit(&mut online, 1, session::player(1));
    app.insert_resource(online)
        .insert_resource(Updates(Mutex::new(Some(receiver))))
        .insert_resource(Probe {
            updates,
            screenshot: path.clone(),
            captured: false,
        })
        .init_resource::<crate::ViewerSettings>()
        .insert_resource(ClearColor(crate::theme::COVER))
        .add_systems(Update, (online::receive, present, verify).chain());
    app.world_mut().spawn(Camera2d);
    assert_eq!(app.run(), AppExit::Success);
    assert!(path.is_file(), "the screenshot must actually be saved");
}

fn set_age(banners: &mut Banners, seconds: u64) {
    // Vary elapsed time without waiting for the GPU or changing the product clock.
    banners.active.as_mut().unwrap().0 = Instant::now() - Duration::from_secs(seconds);
}

#[allow(clippy::needless_pass_by_value)]
fn verify(
    mut step: Local<Step>,
    probe: Res<Probe>,
    mut banners: ResMut<Banners>,
    nodes: Query<(&Text, &TextColor, &BackgroundColor), With<BannerNode>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let alpha = |expected| {
        let (text, ink, background) = nodes.single().expect("one visible banner node");
        assert_eq!(text.0, expected);
        let alpha = ink.0.alpha();
        assert!((background.0.alpha() - alpha * 0.83).abs() < 0.001);
        alpha
    };
    *step = match *step {
        Step::Start => {
            assert!(alpha(TEXT).abs() < f32::EPSILON);
            set_age(&mut banners, 2);
            Step::FadeIn
        }
        Step::FadeIn => {
            assert!((0.45..0.65).contains(&alpha(TEXT)));
            set_age(&mut banners, 6);
            Step::Hold(0)
        }
        Step::Hold(frame) => {
            assert!((alpha(TEXT) - 1.0).abs() < f32::EPSILON);
            set_age(&mut banners, 6);
            if frame == 20 {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(probe.screenshot.clone()))
                    .observe(|_: On<ScreenshotCaptured>, mut probe: ResMut<Probe>| {
                        probe.captured = true;
                    });
                Step::Capture
            } else {
                Step::Hold(frame + 1)
            }
        }
        Step::Capture if probe.captured => {
            set_age(&mut banners, 10);
            Step::FadeOut
        }
        Step::Capture => {
            set_age(&mut banners, 6);
            Step::Capture
        }
        Step::FadeOut => {
            assert!((0.35..0.55).contains(&alpha(TEXT)));
            set_age(&mut banners, 13);
            Step::Expired
        }
        Step::Expired => {
            assert!(nodes.is_empty() && banners.active.is_none());
            for text in ["Visible before reset", "Waiting before reset"] {
                probe.updates.send(banner(text)).unwrap();
            }
            Step::Reset
        }
        Step::Reset => {
            alpha("Visible before reset");
            assert_eq!(banners.waiting.len(), 1);
            probe
                .updates
                .send(WorldUpdate::Connection(Link::Ended))
                .unwrap();
            Step::Cleared
        }
        Step::Cleared => {
            assert!(nodes.is_empty() && banners.active.is_none() && banners.waiting.is_empty());
            exit.write(AppExit::Success);
            Step::Cleared
        }
    };
}
