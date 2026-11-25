//! The frame diagnostics (`EQ_FRAME_LOG`, `EQ_FPS`): an on-screen frame
//! meter, and a log of the frame times every five seconds with what Rich
//! Lighting changed around each slow frame, for measuring its cost.

use super::{Carried, Lamp, Parked, RichMaterial, RichState};
use crate::theme;
use bevy::prelude::*;

/// A frame slower than this is a hitch: below 30 a second.
const HITCH: f32 = 1.0 / 30.0;
/// A frame slower than this is logged on its own line.
const SPIKE: f32 = 0.05;

/// Adds the meter when `EQ_FPS` or `EQ_FRAME_LOG` is set, and the log when
/// `EQ_FRAME_LOG` is.
pub(super) fn register(app: &mut App) {
    let log = std::env::var_os("EQ_FRAME_LOG").is_some();
    if log || std::env::var_os("EQ_FPS").is_some() {
        app.add_systems(Startup, spawn_meter)
            .add_systems(Last, show_meter);
    }
    if log {
        app.add_systems(Last, frame_log);
    }
}

#[derive(Component)]
struct Meter;

fn spawn_meter(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(4),
            right: px(8),
            padding: UiRect::axes(px(6), px(2)),
            ..default()
        },
        BackgroundColor(theme::SCRIM),
        GlobalZIndex(20),
        Pickable::IGNORE,
        theme::text("", theme::Size::Small, theme::INK_BRIGHT),
        Meter,
    ));
}

/// Frame times over a stretch of play.
#[derive(Default)]
struct Span {
    spent: f32,
    worst: f32,
    frames: u32,
    hitches: u32,
}

impl Span {
    fn add(&mut self, delta: f32) {
        self.spent += delta;
        self.worst = self.worst.max(delta);
        self.frames += 1;
        if delta > HITCH {
            self.hitches += 1;
        }
    }

    fn mean(&self) -> f32 {
        #[allow(clippy::cast_precision_loss)] // A frame count.
        let frames = self.frames.max(1) as f32;
        self.spent / frames
    }
}

/// Shows the last second's frame rate, mean and slowest frame, and how
/// many frames took longer than a thirtieth of a second.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn show_meter(
    time: Res<Time<Real>>,
    state: Res<RichState>,
    mut meter: Query<&mut Text, With<Meter>>,
    mut span: Local<Span>,
) {
    span.add(time.delta_secs());
    if span.spent < 1.0 {
        return;
    }
    let mean = span.mean();
    let line = format!(
        "{:.0} fps  {:.1} ms  worst {:.0} ms  hitches {}{}",
        1.0 / mean,
        mean * 1000.0,
        span.worst * 1000.0,
        span.hitches,
        if state.active { "  rich" } else { "" },
    );
    for mut text in &mut meter {
        if text.0 != line {
            text.0.clone_from(&line);
        }
    }
    *span = Span::default();
}

/// What Rich Lighting and the scene changed in one frame.
#[derive(Default, Clone, Copy, Debug)]
#[allow(dead_code)] // Read through Debug, in the log.
struct Changes {
    rich_added: usize,
    rich_modified: usize,
    rich_removed: usize,
    rich_unused: usize,
    plain_modified: usize,
    meshes: usize,
    swapped: usize,
    lamps_toggled: usize,
    carried_changed: usize,
}

#[derive(Default)]
struct Log {
    span: Span,
    spikes: u32,
    last: Changes,
}

/// Logs the frame times every five seconds: the mean, the slowest frame,
/// the hitches, and what Rich Lighting draws. A frame slower than a
/// twentieth of a second gets its own line, with what changed in it and
/// in the frame before, whose work it may be waiting on.
#[allow(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::too_many_arguments
)] // Bevy system parameters.
fn frame_log(
    time: Res<Time<Real>>,
    state: Res<RichState>,
    (mut materials, mut plain, mut meshes): (
        MessageReader<AssetEvent<RichMaterial>>,
        MessageReader<AssetEvent<StandardMaterial>>,
        MessageReader<AssetEvent<Mesh>>,
    ),
    (twice, lights, entities): (
        Query<
            (),
            (
                With<MeshMaterial3d<RichMaterial>>,
                With<MeshMaterial3d<StandardMaterial>>,
            ),
        >,
        Query<&ViewVisibility, With<PointLight>>,
        Query<()>,
    ),
    swapped: Query<(), Added<Parked>>,
    toggled: Query<(), (With<Lamp>, Changed<Visibility>)>,
    lamps: Query<&Visibility, With<Lamp>>,
    carried: Query<Ref<PointLight>, With<Carried>>,
    drawn: Query<(), With<MeshMaterial3d<RichMaterial>>>,
    mut log: Local<Log>,
) {
    let mut changes = Changes::default();
    for event in materials.read() {
        match event {
            AssetEvent::Added { .. } => changes.rich_added += 1,
            AssetEvent::Modified { .. } => changes.rich_modified += 1,
            AssetEvent::Removed { .. } => changes.rich_removed += 1,
            AssetEvent::Unused { .. } => changes.rich_unused += 1,
            AssetEvent::LoadedWithDependencies { .. } => {}
        }
    }
    let changes = Changes {
        plain_modified: plain
            .read()
            .filter(|event| matches!(event, AssetEvent::Modified { .. }))
            .count(),
        meshes: meshes.read().count(),
        swapped: swapped.iter().count(),
        lamps_toggled: toggled.iter().count(),
        carried_changed: carried.iter().filter(Ref::is_changed).count(),
        ..changes
    };
    let delta = time.delta_secs();
    if delta > SPIKE {
        log.spikes += 1;
        info!(
            "frame spike: {:.1} ms; rich {} interior {}; this frame {changes:?}; frame before {:?}",
            delta * 1000.0,
            state.active,
            state.interior,
            log.last,
        );
    }
    log.last = changes;
    log.span.add(delta);
    if log.span.spent < 5.0 {
        return;
    }
    let shown = lamps
        .iter()
        .filter(|shown| **shown != Visibility::Hidden)
        .count();
    let shadowed = carried
        .iter()
        .filter(|light| light.shadow_maps_enabled)
        .count();
    let mean = log.span.mean();
    info!(
        "frame log: {} frames, {:.1} fps, mean {:.2} ms, worst {:.2} ms, {} over 33 ms, {} over 50 ms; rich {} interior {}, lamps {shown}/{}, carried {} ({shadowed} shadowed), rich surfaces {}, drawn twice {}, point lights {} ({} in view), entities {}",
        log.span.frames,
        1.0 / mean,
        mean * 1000.0,
        log.span.worst * 1000.0,
        log.span.hitches,
        log.spikes,
        state.active,
        state.interior,
        lamps.iter().count(),
        carried.iter().count(),
        drawn.iter().count(),
        twice.iter().count(),
        lights.iter().count(),
        lights.iter().filter(|seen| seen.get()).count(),
        entities.iter().count(),
    );
    log.span = Span::default();
    log.spikes = 0;
}
