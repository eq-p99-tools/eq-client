//! Chase camera placement, input arbitration and collision-aware zoom.
use crate::{
    Collision, Player, PlayerBody, ProjectionStyle, chat, inventory, items, online, options,
    sounds, windows,
};
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
#[derive(Component)]
pub(super) struct OrbitCamera {
    pub(super) focus: Vec3,
    pub(super) radius: f32,
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    /// The distance shown while a roof or wall holds the camera short of
    /// `radius`; None once it is back out at the chosen zoom.
    pub(super) pulled: Option<f32>,
}

/// The chase view's pitch as it first opens: a little above the player,
/// looking down past them. Inferred, item 6.240.
pub(super) const CHASE_PITCH: f32 = -20.0_f32.to_radians();

/// The orbit yaw that puts the camera behind whatever faces the render yaw
/// `facing`, looking along it, so W walks that way: the camera sits opposite
/// the direction it looks.
pub(super) fn behind(facing: f32) -> f32 {
    facing + std::f32::consts::PI
}

/// Turns the camera to start behind a placed player, looking the way they
/// face, keeping the pitch and zoom the player chose.
pub(super) fn start_behind(camera: &mut OrbitCamera, placed: &Transform) {
    camera.yaw = behind(placed.rotation.to_euler(EulerRot::YXZ).0);
}

pub(super) fn spawn_camera(
    commands: &mut Commands,
    placed: Transform,
    radius: f32,
    projection_style: ProjectionStyle,
) {
    let mut orbit = OrbitCamera {
        focus: placed.translation,
        radius,
        yaw: 0.0,
        pitch: CHASE_PITCH,
        pulled: None,
    };
    // The first zone is entered before this camera exists, so it starts
    // behind the player here; later entries turn it in `Scene::enter`.
    start_behind(&mut orbit, &placed);
    let mut projection = match projection_style {
        ProjectionStyle::Perspective => Projection::Perspective(PerspectiveProjection {
            near: 1.0,
            ..default()
        }),
        ProjectionStyle::Orthographic => {
            Projection::Orthographic(OrthographicProjection::default_3d())
        }
    };
    fit_projection(&mut projection, radius, None);
    commands.spawn((
        Camera3d::default(),
        projection,
        orbit_transform(&orbit),
        orbit,
        sounds::listener(),
    ));
}

pub(super) fn camera_relative_direction(horizontal: f32, vertical: f32, yaw: f32) -> Vec3 {
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(-forward.z, 0.0, forward.x);
    (right * horizontal + forward * vertical).normalize_or_zero()
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn orbit_camera(
    collision: Option<Res<Collision>>,
    time: Res<Time>,
    chat: Res<chat::ChatState>,
    items: Res<items::ItemState>,
    inventory: Res<inventory::InventoryState>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    (wheel, options): (Res<windows::pointer::Wheel>, Res<options::OptionsState>),
    online: Res<online::OnlineState>,
    mut cameras: Query<(&mut OrbitCamera, &mut Transform, Option<&mut Projection>)>,
    bodies: Query<&PlayerBody, With<Player>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    ui: windows::pointer::PointerUi,
) {
    let head = bodies.single().map_or(0.0, |body| body.height * HEAD_LINE);
    let accepts_input = windows.single().is_ok_and(|window| {
        window.focused
            && !window
                .physical_cursor_position()
                .is_some_and(|cursor| ui.contains(cursor))
    });
    let drag = if accepts_input
        && !chat.hovered
        && !items.hovered
        && !inventory.hovered
        && mouse_buttons.pressed(MouseButton::Right)
    {
        motion.read().map(|event| event.delta).sum::<Vec2>()
    } else {
        motion.clear();
        Vec2::ZERO
    };
    // A turn over a window scrolls the window, never the camera too; with
    // the wheel's zoom turned off, it does nothing here.
    let scroll = if !accepts_input
        || !options.options.wheel_zoom
        || wheel.surface.is_some()
        || chat.hovered
        || items.hovered
        || inventory.hovered
    {
        0.0
    } else {
        wheel.lines
    };

    let rise = if options.options.invert_y { -1.0 } else { 1.0 };
    let clip = options.options.clip_distance(online.world().far_clip());
    for (mut camera, mut transform, projection) in &mut cameras {
        let turn = options.options.turn_per_pixel();
        camera.yaw -= drag.x * turn;
        camera.pitch = (camera.pitch - drag.y * turn * rise).clamp(-1.45, -0.15);
        let radius = (camera.radius * (-scroll * 0.12).exp()).clamp(20.0, 20_000.0);
        camera.radius = radius;
        if let Some(mut projection) = projection
            && !fitted(&projection, radius, clip)
        {
            fit_projection(&mut projection, radius, clip);
        }
        *transform = unobstructed_orbit(
            &mut camera,
            collision.as_ref().and_then(|world| world.0.as_ref()),
            head,
            time.delta_secs(),
        );
    }
}

/// How far the view reaches: the clip distance past the player the camera
/// orbits, or, where the zone gives no clip, ten times the orbit distance,
/// so zooming out never pushes the scene past the far plane.
fn far_plane(radius: f32, clip: Option<f32>) -> f32 {
    clip.map_or(radius * 10.0, |clip| clip + radius)
}

/// Whether the view already reaches as far as it should; an orthographic
/// view, which looks down on the whole scene, follows only the orbit.
fn fitted(projection: &Projection, radius: f32, clip: Option<f32>) -> bool {
    match projection {
        Projection::Perspective(perspective) => {
            perspective.far.to_bits() == far_plane(radius, clip).to_bits()
        }
        Projection::Orthographic(orthographic) => {
            orthographic.scale.to_bits() == (radius / 400.0).to_bits()
        }
        Projection::Custom(_) => true,
    }
}

/// Keeps the view's depth, and an orthographic view's scale, in step with the
/// orbit distance and the clip distance.
fn fit_projection(projection: &mut Projection, radius: f32, clip: Option<f32>) {
    match projection {
        Projection::Perspective(perspective) => perspective.far = far_plane(radius, clip),
        Projection::Orthographic(orthographic) => {
            let far = far_plane(radius, None);
            orthographic.scale = radius / 400.0;
            orthographic.near = -far;
            orthographic.far = far;
        }
        Projection::Custom(_) => (),
    }
}

pub(super) fn orbit_transform(camera: &OrbitCamera) -> Transform {
    let rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);
    let position = camera.focus + rotation * Vec3::new(0.0, 0.0, camera.radius);
    Transform::from_translation(position).looking_at(camera.focus, Vec3::Y)
}

/// Keeps the camera on the player's side of roofs and walls without changing
/// the chosen zoom: it pulls in at once to just short of whatever stands
/// between it and the player, then eases back out as the way clears, never
/// past the chosen radius. Both the line from the focus (the body's middle)
/// and the line from near the top of the head, `head` above it, must reach
/// the camera clear, so a wall top never hides the head while the waist is in
/// view. Display only; inferred, item 6.233.
pub(super) fn unobstructed_orbit(
    camera: &mut OrbitCamera,
    collision: Option<&eq_client_core::movement::CollisionWorld>,
    head: f32,
    seconds: f32,
) -> Transform {
    let mut pose = orbit_transform(camera);
    let offset = pose.translation - camera.focus;
    let distance = offset.length();
    if distance <= 0.001 {
        camera.pulled = None;
        return pose;
    }
    let direction = offset / distance;
    let allowed = collision.map_or(distance, |world| {
        let along_focus = world
            .ray_distance(camera.focus, direction, distance)
            .map_or(distance, |hit| (hit - 0.5).max(0.01));
        clear_of_head(world, camera.focus, Vec3::Y * head, direction, along_focus)
    });
    camera.pulled = camera
        .pulled
        .map(|shown| eased_distance(shown, allowed, seconds))
        .or((allowed < distance).then_some(allowed))
        .filter(|shown| distance - shown > PULLED_DONE);
    if let Some(shown) = camera.pulled {
        pose.translation = camera.focus + direction * shown;
    }
    pose
}

/// Where the head line starts, as a share of the body's height above its
/// middle: near the top of the head.
pub(super) const HEAD_LINE: f32 = 0.4;

/// How many times the head line is cast before the distance it gives stands.
const HEAD_CASTS: usize = 3;

/// The distance along the orbit, at most `along_focus`, at which the line from
/// the head (`rise` above the focus) to the camera is clear: each hit pulls the
/// camera in by the share of the line that was clear, then the line is cast
/// again, a bounded number of times.
fn clear_of_head(
    world: &eq_client_core::movement::CollisionWorld,
    focus: Vec3,
    rise: Vec3,
    direction: Vec3,
    along_focus: f32,
) -> f32 {
    let head = focus + rise;
    let mut distance = along_focus;
    for _ in 0..HEAD_CASTS {
        let line = focus + direction * distance - head;
        let length = line.length();
        if length <= 0.001 {
            break;
        }
        let Some(hit) = world.ray_distance(head, line / length, length) else {
            break;
        };
        distance = (distance * (hit - 0.5).max(0.0) / length).max(0.01);
    }
    distance
}

/// How close to the chosen radius an easing camera counts as back out.
const PULLED_DONE: f32 = 0.05;

/// The camera's eased-out time constant: about 95% of the way back in 0.3 s.
const PULL_OUT_SECONDS: f32 = 0.1;

/// The shown distance one frame on: in at once to `allowed`, so geometry
/// never sits between the camera and the player; out toward it eased, so
/// it never passes `allowed` however long the frame.
pub(super) fn eased_distance(shown: f32, allowed: f32, seconds: f32) -> f32 {
    if allowed <= shown {
        allowed
    } else {
        shown + (allowed - shown) * (1.0 - (-seconds.max(0.0) / PULL_OUT_SECONDS).exp())
    }
}
