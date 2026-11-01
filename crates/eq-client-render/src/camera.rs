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

/// The right button's look: from a press over the world (not over a
/// window) until the button is let go, the mouse belongs to the look
/// wherever the pointer goes, and the windows see none of its buttons, as
/// the Titanium program skips feeding the mouse to its windows while its
/// look is on (0x4a9cd2, cleared at 0x4a8ba2; program read N2 of 11-01). A
/// press that begins over a window stays the window's (inferred, recording
/// checklist 6.350).
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MouseLook {
    /// The right button is held from a press over the world.
    pub(crate) held: bool,
    /// The left button is held during the look. The program refuses a left
    /// click while its look is on (0x4a91a4), so the press goes to the look
    /// alone: it runs the player the way the camera faces (6.341).
    pub(crate) left: bool,
}

impl MouseLook {
    /// Follows the right button for one frame: a fresh press starts a look
    /// only where the world, not a window, is under the pointer (`free`);
    /// letting go, or the window losing the focus, ends it.
    pub(crate) fn follow(&mut self, buttons: &ButtonInput<MouseButton>, focused: bool, free: bool) {
        if !focused || !buttons.pressed(MouseButton::Right) {
            self.held = false;
        } else if buttons.just_pressed(MouseButton::Right) {
            self.held = free;
        }
    }

    /// Takes the left button for the look while it is on, so nothing else
    /// sees it pressed, clicked or let go: no window, target or door.
    fn take_left(&mut self, buttons: &mut ButtonInput<MouseButton>) {
        self.left = self.held && buttons.pressed(MouseButton::Left);
        if self.held {
            buttons.reset(MouseButton::Left);
        }
    }
}

/// Starts, keeps or ends the right button's look, before the windows see
/// the mouse, and takes the left button for it while it is on.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn look(
    mut look: ResMut<MouseLook>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    ui: windows::pointer::PointerUi,
    (chat, items, inventory): (
        Res<chat::ChatState>,
        Res<items::ItemState>,
        Res<inventory::InventoryState>,
    ),
) {
    let window = windows.single().ok();
    let focused = window.is_some_and(|window| window.focused);
    let over_ui = window
        .and_then(Window::physical_cursor_position)
        .is_some_and(|cursor| ui.contains(cursor));
    let free = !over_ui && !chat.hovered && !items.hovered && !inventory.hovered;
    look.follow(&buttons, focused, free);
    look.take_left(&mut buttons);
}

/// The chase view's pitch as it first opens: a little above the player,
/// looking down past them. Inferred, item 6.240.
pub(super) const CHASE_PITCH: f32 = -20.0_f32.to_radians();

/// The orbit yaw that puts the camera behind whatever faces the render yaw
/// `facing`, looking along it, so the camera-relative forward walks that way: the camera sits opposite
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
    look: Res<MouseLook>,
    mut motion: MessageReader<MouseMotion>,
    (wheel, options, settings): (
        Res<windows::pointer::Wheel>,
        Res<options::OptionsState>,
        Option<Res<crate::ViewerSettings>>,
    ),
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
    // The drag is the look's ([`look`]): once begun over the world it goes
    // on over the windows until the button is let go.
    let drag = if look.held {
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

    let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
    let limits = client.camera_limits();
    let rise = if options.options.invert_y { -1.0 } else { 1.0 };
    let clip = clip_distance(client, &options.options, online.world());
    for (mut camera, mut transform, projection) in &mut cameras {
        let turn = options.options.turn_per_pixel();
        camera.yaw -= drag.x * turn;
        // The limits are the program's chase camera's. The top-down
        // orthographic view is not in the program, so it keeps this
        // client's own pitch and zoom ranges.
        if matches!(projection.as_deref(), Some(Projection::Orthographic(_))) {
            top_down(&mut camera, drag.y * turn * rise, (-scroll * 0.12).exp());
        } else {
            chase(
                &mut camera,
                limits,
                drag.y * turn * rise * HEIGHT_PER_RADIAN,
                (-scroll * 0.12).exp(),
            );
        }
        let radius = camera.radius;
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

/// How many units the chase camera rises for each radian today's mouse
/// sensitivity used to tilt it: the whole 30-unit range for about a
/// quarter turn's movement. The program's own mapping of the mouse to the
/// height is not traced (inferred, recording checklist 6.348).
const HEIGHT_PER_RADIAN: f32 = 20.0;

/// Moves the chase camera as the installed client's does: it sits a height
/// above the player and a distance behind, each held within the client's
/// range ([`eq_client_assets::camera::CameraLimits`]), and looks down at
/// the player from there, with no angle limit of its own. `raise` raises
/// the height by that many units (the mouse moved down raises it, as it
/// tilted the view down before); `zoom` multiplies the distance. A zoom
/// keeps the height, so the view tilts as the distance changes.
pub(super) fn chase(
    camera: &mut OrbitCamera,
    limits: eq_client_assets::camera::CameraLimits,
    raise: f32,
    zoom: f32,
) {
    let (least_height, most_height) = limits.chase_height;
    let (least_distance, most_distance) = limits.chase_distance;
    let height = camera.radius * (-camera.pitch).sin() + raise;
    let distance = camera.radius * camera.pitch.cos() * zoom;
    let clamped = (
        height.clamp(least_height, most_height),
        distance.clamp(least_distance, most_distance),
    );
    // Left where it is when nothing moved it and it is within its ranges.
    #[allow(clippy::float_cmp)] // Exactly no movement asked.
    if raise == 0.0 && zoom == 1.0 && clamped == (height, distance) {
        return;
    }
    let (height, distance) = clamped;
    camera.radius = height.hypot(distance);
    camera.pitch = -height.atan2(distance);
}

/// How far the scene is drawn, terrain, objects and spawns alike, by the
/// installed client's far plane rule with the player's clip plane setting;
/// None while the zone has given no far clip.
pub(crate) fn clip_distance(
    client: eq_client_assets::ui::InstalledClient,
    options: &eq_client_core::options::Options,
    world: &eq_client_core::world::ClientWorld,
) -> Option<f32> {
    client
        .camera_limits()
        .far_plane
        .distance(options.clip_share(), (world.min_clip(), world.far_clip()))
}

/// Moves the top-down orthographic view, which only this client has: it
/// tilts by `tilt` radians within its own pitch range and zooms by `zoom`
/// within its own distances, as every camera did before the chase camera
/// took the program's limits.
pub(super) fn top_down(camera: &mut OrbitCamera, tilt: f32, zoom: f32) {
    camera.pitch = (camera.pitch - tilt).clamp(-1.45, -0.15);
    camera.radius = (camera.radius * zoom).clamp(20.0, 20_000.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The right button held since an earlier frame: pressed, not fresh.
    fn held() -> ButtonInput<MouseButton> {
        let mut buttons = ButtonInput::default();
        buttons.press(MouseButton::Right);
        buttons.clear_just_pressed(MouseButton::Right);
        buttons
    }

    #[test]
    fn a_look_begun_over_the_world_lasts_over_windows_until_let_go() {
        let mut look = MouseLook::default();
        let mut buttons = ButtonInput::default();
        buttons.press(MouseButton::Right);
        look.follow(&buttons, true, true);
        assert!(look.held);
        // The pointer passes over a window while the button stays down.
        look.follow(&held(), true, false);
        assert!(look.held);
        // Let go over the window: the look ends, and nothing starts again
        // until a fresh press.
        look.follow(&ButtonInput::default(), true, false);
        assert!(!look.held);
        look.follow(&held(), true, true);
        assert!(!look.held);
    }

    #[test]
    fn a_press_begun_over_a_window_stays_the_windows() {
        let mut look = MouseLook::default();
        let mut buttons = ButtonInput::default();
        buttons.press(MouseButton::Right);
        look.follow(&buttons, true, false);
        assert!(!look.held);
        // Dragged off the window onto the world, it still turns nothing.
        look.follow(&held(), true, true);
        assert!(!look.held);
    }

    #[test]
    fn the_chase_camera_stays_within_its_height_and_distance() {
        let limits = eq_client_assets::camera::TITANIUM;
        let mut camera = OrbitCamera {
            focus: Vec3::ZERO,
            radius: 100.0,
            yaw: 0.0,
            pitch: -0.2,
            pulled: None,
        };
        // Raised as far as it goes: 30 units over the player.
        chase(&mut camera, limits, 1000.0, 1.0);
        let height = |camera: &OrbitCamera| camera.radius * (-camera.pitch).sin();
        let distance = |camera: &OrbitCamera| camera.radius * camera.pitch.cos();
        assert!((height(&camera) - 30.0).abs() < 1e-3);
        // Lowered as far as it goes: level with the player, never under.
        chase(&mut camera, limits, -1000.0, 1.0);
        assert!(height(&camera).abs() < 1e-3);
        assert!(camera.pitch.abs() < 1e-6);
        // Zoomed far out and in, it keeps to 200 and 2 units.
        chase(&mut camera, limits, 10.0, 100.0);
        assert!((distance(&camera) - 200.0).abs() < 1e-3);
        assert!((height(&camera) - 10.0).abs() < 1e-3);
        chase(&mut camera, limits, 0.0, 0.0);
        assert!((distance(&camera) - 2.0).abs() < 1e-3);
        // Close in, the same height looks nearly straight down.
        assert!((height(&camera) - 10.0).abs() < 1e-3);
        assert!(camera.pitch < -1.3);
    }

    #[test]
    fn the_look_takes_the_left_button_from_everything_else() {
        let mut look = MouseLook::default();
        let mut buttons = ButtonInput::default();
        buttons.press(MouseButton::Right);
        buttons.press(MouseButton::Left);
        look.follow(&buttons, true, true);
        look.take_left(&mut buttons);
        assert!(look.left);
        assert!(!buttons.pressed(MouseButton::Left));
        assert!(!buttons.just_pressed(MouseButton::Left));
        // Without a look, the left button is the windows' and the world's.
        let mut look = MouseLook::default();
        let mut buttons = ButtonInput::default();
        buttons.press(MouseButton::Left);
        look.take_left(&mut buttons);
        assert!(!look.left);
        assert!(buttons.just_pressed(MouseButton::Left));
    }

    #[test]
    fn losing_the_focus_ends_the_look() {
        let mut look = MouseLook::default();
        let mut buttons = ButtonInput::default();
        buttons.press(MouseButton::Right);
        look.follow(&buttons, true, true);
        look.follow(&held(), false, true);
        assert!(!look.held);
        look.follow(&held(), true, true);
        assert!(!look.held);
    }
}
