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

/// A detached view. Its transform is the camera's alone; the orbit keeps
/// following the real player so returning never changes the player's position.
#[derive(Component)]
pub(crate) struct FreeCamera {
    yaw: f32,
    pitch: f32,
    admission: Option<u64>,
}

/// Toggles before character input, and leaves free camera on an admission change.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(crate) fn toggle_free(
    mut commands: Commands,
    keys: crate::keys::Keys,
    online: Res<online::OnlineState>,
    options: Res<options::OptionsState>,
    players: Query<&Transform, (With<Player>, Without<OrbitCamera>)>,
    mut cameras: Query<
        (
            Entity,
            &mut OrbitCamera,
            &mut Transform,
            Option<&FreeCamera>,
        ),
        Without<Player>,
    >,
) {
    let admission = online.world().session_id();
    // A QoL setting ([`eq_client_core::qol::Fix::FreeCamera`]): off, the key
    // does nothing and a detached camera returns at once.
    let offered = options.options.qol.on(eq_client_core::qol::Fix::FreeCamera);
    let playing =
        offered && (!online.enabled || (admission.is_some() && online.world().connected()));
    let pressed = keys.pressed(crate::keys::Act::FreeCamera);
    for (entity, mut orbit, mut transform, free) in &mut cameras {
        let expired = free.is_some_and(|free| free.admission != admission || !playing);
        if free.is_some() && (pressed || expired) {
            commands.entity(entity).remove::<FreeCamera>();
            if let Ok(player) = players.single() {
                orbit.focus = player.translation;
            }
            orbit.pulled = None;
            *transform = orbit_transform(&orbit);
        } else if pressed && playing && players.single().is_ok() {
            let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
            commands.entity(entity).insert(FreeCamera {
                yaw,
                pitch,
                admission,
            });
        }
    }
}

/// The free camera's speed in world units per second, independent of the
/// character's movement grants, and its keyboard turning rate in radians.
const FLY_SPEED: f32 = 60.0;
const FLY_TURN: f32 = 2.0;

impl FreeCamera {
    fn fly(
        &mut self,
        transform: &mut Transform,
        drag: Vec2,
        movement: Vec3,
        turn: f32,
        seconds: f32,
    ) {
        self.yaw += turn * FLY_TURN * seconds - drag.x;
        self.pitch = (self.pitch - drag.y).clamp(-1.55, 1.55);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
        let direction = *transform.right() * movement.x
            + Vec3::Y * movement.y
            + *transform.forward() * movement.z;
        transform.translation += direction.normalize_or_zero() * FLY_SPEED * seconds;
    }
}

/// Flies only the detached view, with the same keyboard ownership and mouse
/// sensitivity as the character controls. No collision or network path runs here.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn fly_camera(
    keys: crate::keys::Keys,
    navigation: Res<crate::navigation::NavigationKeys>,
    time: Res<Time>,
    look: Res<MouseLook>,
    options: Res<options::OptionsState>,
    mut motion: MessageReader<MouseMotion>,
    mut cameras: Query<(&mut FreeCamera, &mut Transform)>,
) {
    use crate::keys::Act;
    let drag = if look.held && keys.focused() {
        motion.read().map(|event| event.delta).sum::<Vec2>()
    } else {
        motion.clear();
        Vec2::ZERO
    };
    let (mut movement, mut turning) = (Vec3::ZERO, 0.0);
    if keys.focused() {
        let input = navigation.sample(&keys.input);
        let held = |a, b| keys.map.held(&input, a) || keys.map.held(&input, b);
        movement.x = f32::from(held(Act::StrafeRight, Act::CameraRight))
            - f32::from(held(Act::StrafeLeft, Act::CameraLeft));
        movement.y = f32::from(held(Act::CameraUp, Act::Jump))
            - f32::from(keys.map.held(&input, Act::CameraDown));
        movement.z = f32::from(held(Act::Forward, Act::CameraForward))
            - f32::from(held(Act::Back, Act::CameraBack));
        turning = keys.map.axis(&input, Act::TurnRight, Act::TurnLeft);
    }
    let turn = options.options.turn_per_pixel();
    let rise = if options.options.invert_y { -1.0 } else { 1.0 };
    for (mut free, mut transform) in &mut cameras {
        free.fly(
            &mut transform,
            drag * Vec2::new(turn, turn * rise),
            movement,
            turning,
            time.delta_secs(),
        );
    }
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
    mut cameras: Query<(
        &mut OrbitCamera,
        &mut Transform,
        Option<&mut Projection>,
        Has<FreeCamera>,
    )>,
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
    let program = client.camera_limits();
    let limits = program.on_leash(leash(&options.options));
    let rise = if options.options.invert_y { -1.0 } else { 1.0 };
    let clip = clip_distance(client, &options.options, online.world());
    for (mut camera, mut transform, projection, free) in &mut cameras {
        let turn = options.options.turn_per_pixel();
        if free {
            let clip = clip.map(|clip| clip + past_program(camera.radius, program));
            if let Some(mut projection) = projection
                && !fitted(&projection, camera.radius, clip)
            {
                fit_projection(&mut projection, camera.radius, clip);
            }
            continue;
        }
        camera.yaw -= drag.x * turn;
        // The limits are the program's chase camera's, with the distance on
        // the player's leash. The top-down
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
        let clip = clip.map(|clip| clip + past_program(radius, program));
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

/// How far behind the player the player lets the chase camera go: this
/// client's long range unless they turned the quality-of-life setting off
/// ([`eq_client_core::qol::Fix::LongChaseCamera`]), else the program's.
pub(crate) fn leash(
    options: &eq_client_core::options::Options,
) -> eq_client_assets::camera::ChaseLeash {
    if options.qol.on(eq_client_core::qol::Fix::LongChaseCamera) {
        eq_client_assets::camera::ChaseLeash::Long
    } else {
        eq_client_assets::camera::ChaseLeash::Program
    }
}

/// How much farther out than the program lets its chase camera go a camera
/// `radius` from the player is, if at all. The far plane moves out by as
/// much, so on the long leash the scene around the player draws as far as
/// it would with the camera at the program's limit, and the player never
/// drops past the far plane. Within the program's limit it is nothing, and
/// the far plane is the program's.
fn past_program(radius: f32, program: eq_client_assets::camera::CameraLimits) -> f32 {
    (radius - program.chase_distance.1).max(0.0)
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

/// How far the view reaches from the camera: the clip distance, as the
/// program gives the camera itself its far clip (0x43c78f, program read
/// N7), never nearer than [`NEAREST_FAR`]; or, where the zone gives no clip,
/// ten times the orbit distance, so zooming out never pushes the scene past
/// the far plane.
///
/// The far plane here is a cull, not a cut: the view's projection has no
/// far side, so the engine leaves out a model whose whole box lies past the
/// plane and draws one that crosses it whole, where the program's camera
/// cuts it at the plane (recording checklist 6.342).
fn far_plane(radius: f32, clip: Option<f32>) -> f32 {
    clip.map_or(radius * 10.0, |clip| clip.max(NEAREST_FAR))
}

/// The nearest the far plane comes, twice the near plane's distance, so a
/// clip of nothing still leaves the view a depth.
const NEAREST_FAR: f32 = 2.0;

/// Whether `point` lies past the camera's far plane: deeper along the view
/// than its far distance, the test that leaves a model out of the view.
/// Only a perspective view has one; an orthographic view shows the whole
/// scene it orbits.
pub(crate) fn past_far(projection: &Projection, view: &Transform, point: Vec3) -> bool {
    match projection {
        Projection::Perspective(perspective) => {
            (point - view.translation).dot(*view.forward()) > perspective.far
        }
        Projection::Orthographic(_) | Projection::Custom(_) => false,
    }
}

/// How much farther than its depth a perspective view sees at its corners,
/// where the line of sight is longest: a point at the far plane's depth in
/// a corner of the view lies this many times the far distance from the
/// camera. None for other views.
pub(crate) fn corner_reach(projection: &Projection) -> Option<f32> {
    let Projection::Perspective(perspective) = projection else {
        return None;
    };
    let tall = (perspective.fov / 2.0).tan();
    let wide = tall * perspective.aspect_ratio;
    Some(tall.mul_add(tall, wide.mul_add(wide, 1.0)).sqrt())
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
    use crate::keys::Act;

    fn free_app() -> (App, Entity, Entity) {
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        // Detached-camera controls are explicit test assignments. Native
        // F9 and Page Up/Down belong to different camera commands.
        for (act, key) in [
            (Act::FreeCamera, KeyCode::F9),
            (Act::CameraUp, KeyCode::PageUp),
            (Act::CameraDown, KeyCode::PageDown),
            (Act::CameraForward, KeyCode::KeyW),
        ] {
            crate::keys::testing::bind(&mut app, act, crate::keys::Chord::key(key));
        }
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs(1));
        app.insert_resource(time)
            .init_resource::<crate::navigation::NavigationKeys>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<MouseLook>()
            .init_resource::<chat::ChatState>()
            .init_resource::<items::ItemState>()
            .init_resource::<inventory::InventoryState>()
            .init_resource::<options::OptionsState>()
            .init_resource::<windows::pointer::Wheel>()
            .add_message::<MouseMotion>()
            .insert_resource(online::OnlineState::new(false))
            .insert_resource(Collision(Some(
                eq_client_core::movement::CollisionWorld::new([
                    [
                        [-1000.0, 0.0, -1000.0],
                        [1000.0, 0.0, -1000.0],
                        [1000.0, 0.0, 1000.0],
                    ],
                    [
                        [-1000.0, 0.0, -1000.0],
                        [1000.0, 0.0, 1000.0],
                        [-1000.0, 0.0, 1000.0],
                    ],
                    [
                        [-100.0, -100.0, -2.0],
                        [100.0, -100.0, -2.0],
                        [100.0, 100.0, -2.0],
                    ],
                    [
                        [-100.0, -100.0, -2.0],
                        [100.0, 100.0, -2.0],
                        [-100.0, 100.0, -2.0],
                    ],
                ])
                .unwrap(),
            )))
            .add_systems(
                Update,
                (toggle_free, crate::move_player, fly_camera, orbit_camera).chain(),
            );
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        let player = app
            .world_mut()
            .spawn((
                Player,
                PlayerBody {
                    feet_offset: 3.0,
                    height: 6.0,
                    size: 6.0,
                },
                Transform::from_xyz(0.0, 3.0, 0.0),
            ))
            .id();
        let orbit = OrbitCamera {
            focus: Vec3::new(0.0, 3.0, 0.0),
            radius: 30.0,
            yaw: 0.0,
            pitch: -0.35,
            pulled: None,
        };
        let camera = app.world_mut().spawn((orbit_transform(&orbit), orbit)).id();
        (app, camera, player)
    }

    fn press(app: &mut App, key: KeyCode) {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.reset_all();
        input.press(key);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }

    #[test]
    fn the_free_camera_setting_off_leaves_the_key_dead_and_returns_a_detached_camera() {
        let (mut app, camera, _) = free_app();
        press(&mut app, KeyCode::F9);
        assert!(app.world().get::<FreeCamera>(camera).is_some());
        app.world_mut()
            .resource_mut::<options::OptionsState>()
            .options
            .qol
            .set(eq_client_core::qol::Fix::FreeCamera, false);
        app.update();
        assert!(app.world().get::<FreeCamera>(camera).is_none());
        press(&mut app, KeyCode::F9);
        assert!(app.world().get::<FreeCamera>(camera).is_none());
        // A fly key leaves the camera on its orbit around the player.
        press(&mut app, KeyCode::KeyW);
        assert!(app.world().get::<FreeCamera>(camera).is_none());
        let orbit = app.world().get::<OrbitCamera>(camera).unwrap();
        assert!(
            app.world()
                .get::<Transform>(camera)
                .unwrap()
                .translation
                .abs_diff_eq(orbit_transform(orbit).translation, 1e-5)
        );
    }

    #[test]
    fn free_camera_crosses_walls_and_floor_and_returns_to_the_current_player() {
        let (mut app, camera, player) = free_app();
        let before = *app.world().get::<Transform>(camera).unwrap();
        press(&mut app, KeyCode::F9);
        assert!(app.world().get::<FreeCamera>(camera).is_some());
        assert!(
            app.world()
                .get::<Transform>(camera)
                .unwrap()
                .translation
                .abs_diff_eq(before.translation, 1e-5)
        );
        // Holding the toggle does not switch again.
        app.update();
        assert!(app.world().get::<FreeCamera>(camera).is_some());
        press(&mut app, KeyCode::KeyW);
        let flying = *app.world().get::<Transform>(camera).unwrap();
        assert!(flying.translation.z < -2.0 && flying.translation.y < 0.0);
        assert!(
            app.world()
                .get::<Transform>(player)
                .unwrap()
                .translation
                .abs_diff_eq(Vec3::new(0.0, 3.0, 0.0), 1e-5)
        );
        // A server placement while detached never drags the camera along.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .get_mut::<Transform>(player)
            .unwrap()
            .translation
            .x = 80.0;
        app.update();
        assert!(
            app.world()
                .get::<Transform>(camera)
                .unwrap()
                .translation
                .abs_diff_eq(flying.translation, 1e-5)
        );
        press(&mut app, KeyCode::F9);
        assert!(app.world().get::<FreeCamera>(camera).is_none());
        let orbit = app.world().get::<OrbitCamera>(camera).unwrap();
        assert!(orbit.focus.abs_diff_eq(Vec3::new(80.0, 3.0, 0.0), 1e-5));
        assert!(
            app.world()
                .get::<Transform>(camera)
                .unwrap()
                .translation
                .abs_diff_eq(orbit_transform(orbit).translation, 1e-5)
        );
        assert!((orbit.radius - 30.0).abs() < 1e-5 && (orbit.pitch + 0.35).abs() < 1e-5);
    }

    #[test]
    fn free_camera_respects_typing_focus_and_vertical_keys() {
        let (mut app, camera, _) = free_app();
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        press(&mut app, KeyCode::F9);
        assert!(app.world().get::<FreeCamera>(camera).is_none());
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = false;
        press(&mut app, KeyCode::F9);
        let before = app.world().get::<Transform>(camera).unwrap().translation;
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .counting = true;
        press(&mut app, KeyCode::KeyW);
        assert!(
            app.world()
                .get::<Transform>(camera)
                .unwrap()
                .translation
                .abs_diff_eq(before, 1e-5)
        );
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .counting = false;
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        press(&mut app, KeyCode::F9);
        assert!(app.world().get::<FreeCamera>(camera).is_some());
        press(&mut app, KeyCode::KeyW);
        assert!(
            app.world()
                .get::<Transform>(camera)
                .unwrap()
                .translation
                .abs_diff_eq(before, 1e-5)
        );
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        press(&mut app, KeyCode::PageUp);
        let up = app.world().get::<Transform>(camera).unwrap().translation;
        assert!(up.y > before.y && (up.z - before.z).abs() < 1e-5);
        press(&mut app, KeyCode::PageDown);
        assert!(
            app.world()
                .get::<Transform>(camera)
                .unwrap()
                .translation
                .abs_diff_eq(before, 1e-5)
        );
        // Leaving a session also returns the view, without another key press.
        app.world_mut()
            .resource_mut::<online::OnlineState>()
            .enabled = true;
        app.update();
        assert!(app.world().get::<FreeCamera>(camera).is_none());
    }

    #[test]
    fn detaching_a_zoomed_out_camera_keeps_the_extended_view_distance() {
        let (mut app, camera, _) = free_app();
        let mut settings = crate::ViewerSettings::default();
        settings.0.installed_client = eq_client_assets::ui::InstalledClient::Titanium;
        app.insert_resource(settings);
        app.world_mut()
            .get_mut::<OrbitCamera>(camera)
            .unwrap()
            .radius = 1000.0;
        app.world_mut()
            .entity_mut(camera)
            .insert(Projection::Perspective(PerspectiveProjection::default()));
        app.update();
        let Projection::Perspective(before) = app.world().get::<Projection>(camera).unwrap() else {
            panic!("expected perspective view")
        };
        let distance = before.far;
        assert!(distance > eq_client_assets::camera::FarPlane::UNCLIPPED);
        press(&mut app, KeyCode::F9);
        let Projection::Perspective(after) = app.world().get::<Projection>(camera).unwrap() else {
            panic!("expected perspective view")
        };
        assert_eq!(after.far.to_bits(), distance.to_bits());
    }

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
    fn the_far_plane_sits_at_the_clip_from_the_camera() {
        // The program gives the camera itself the far clip (N7), so zooming
        // out never carries it farther.
        for radius in [2.0, 200.0] {
            let mut projection = Projection::Perspective(PerspectiveProjection::default());
            fit_projection(&mut projection, radius, Some(2400.0));
            let Projection::Perspective(perspective) = &projection else {
                unreachable!("fitting keeps the projection's kind");
            };
            assert_eq!(perspective.far.to_bits(), 2400.0_f32.to_bits());
            assert!(fitted(&projection, radius, Some(2400.0)));
        }
        // A clip of nothing still leaves a depth; no clip follows the orbit.
        assert_eq!(far_plane(50.0, Some(0.0)).to_bits(), NEAREST_FAR.to_bits());
        assert_eq!(far_plane(50.0, None).to_bits(), 500.0_f32.to_bits());
    }

    #[test]
    fn a_point_deeper_than_the_far_plane_is_past_it_wherever_it_lies() {
        let projection = Projection::Perspective(PerspectiveProjection {
            far: 2400.0,
            ..default()
        });
        let view = Transform::default().looking_to(Vec3::NEG_Z, Vec3::Y);
        assert!(!past_far(&projection, &view, Vec3::new(0.0, 0.0, -2399.0)));
        assert!(past_far(&projection, &view, Vec3::new(0.0, 0.0, -2401.0)));
        // Off to the side: farther than the far distance, but not as deep.
        assert!(!past_far(
            &projection,
            &view,
            Vec3::new(1500.0, 0.0, -2300.0)
        ));
        // Behind the camera is never past it, and a top-down view has none.
        assert!(!past_far(&projection, &view, Vec3::new(0.0, 0.0, 3000.0)));
        let top_down = Projection::Orthographic(OrthographicProjection::default_3d());
        assert!(!past_far(&top_down, &view, Vec3::new(0.0, 0.0, -9000.0)));
    }

    #[test]
    fn a_views_corners_see_farther_than_its_depth() {
        // A quarter-turn view as wide as it is tall: a corner's line of sight
        // runs one across and one up for each one deep.
        let projection = Projection::Perspective(PerspectiveProjection {
            fov: std::f32::consts::FRAC_PI_2,
            aspect_ratio: 1.0,
            ..default()
        });
        let corner = corner_reach(&projection).unwrap();
        assert!((corner - 3.0_f32.sqrt()).abs() < 1e-5, "{corner}");
        let top_down = Projection::Orthographic(OrthographicProjection::default_3d());
        assert_eq!(corner_reach(&top_down), None);
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
    fn the_long_leash_lets_the_chase_camera_go_far_and_keeps_its_height() {
        use eq_client_assets::camera::{ChaseLeash, TITANIUM};
        let mut options = eq_client_core::options::Options::default();
        // The long leash by default.
        assert_eq!(leash(&options), ChaseLeash::Long);
        let limits = TITANIUM.on_leash(leash(&options));
        let mut camera = OrbitCamera {
            focus: Vec3::ZERO,
            radius: 100.0,
            yaw: 0.0,
            pitch: -0.2,
            pulled: None,
        };
        let height = |camera: &OrbitCamera| camera.radius * (-camera.pitch).sin();
        let distance = |camera: &OrbitCamera| camera.radius * camera.pitch.cos();
        chase(&mut camera, limits, 10.0, 30.0);
        assert!((distance(&camera) - 100.0 * 0.2_f32.cos() * 30.0).abs() < 0.1);
        chase(&mut camera, limits, 0.0, 100.0);
        assert!((distance(&camera) - 20_000.0).abs() < 0.1);
        // The height keeps to the program's range.
        chase(&mut camera, limits, 1000.0, 1.0);
        assert!((height(&camera) - 30.0).abs() < 1e-2);
        chase(&mut camera, limits, -1000.0, 1.0);
        assert!(height(&camera).abs() < 1e-3);
        // Out past the program's 200, the far plane moves out with it.
        assert!((past_program(camera.radius, TITANIUM) - 19_800.0).abs() < 0.1);
        assert!(past_program(150.0, TITANIUM).abs() < f32::EPSILON);
        // Turned off, the program's limit.
        options
            .qol
            .set(eq_client_core::qol::Fix::LongChaseCamera, false);
        assert_eq!(leash(&options), ChaseLeash::Program);
        chase(&mut camera, TITANIUM.on_leash(leash(&options)), 0.0, 1.0);
        assert!((distance(&camera) - 200.0).abs() < 1e-3);
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
