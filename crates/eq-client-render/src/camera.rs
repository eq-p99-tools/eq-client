//! Chase camera placement, input arbitration and collision-aware zoom.
mod attached;
use crate::{
    Collision, Player, PlayerBody, ProjectionStyle, chat, inventory, items, online, options,
    sounds, windows,
};
use bevy::input::mouse::{MouseButtonInput, MouseMotion};
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowFocused};
use eq_client_core::movement::CollisionWorld;
#[derive(Component)]
pub(super) struct OrbitCamera {
    /// Body center before the view chooses its pivot.
    pub(super) focus: Vec3,
    /// Requested distance along the sightline, before collision pulls it in.
    pub(super) radius: f32,
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    /// The distance shown while a roof or wall holds the camera short of
    /// `radius`; None once it is back out at the chosen zoom.
    pub(super) pulled: Option<f32>,
    pub(super) view: attached::View,
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

/// A right press over the world keeps the look until release, even if the
/// pointer moves over UI. Left presses during that look belong to movement.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MouseLook {
    /// The right button is held from a press over the world.
    pub(crate) held: bool,
    /// The left button is held from a press that belonged to this look.
    pub(crate) left: bool,
}

/// The horizontal input owned by an attached online look this frame.
#[derive(Resource, Default)]
pub(crate) struct MouseAim(pub(crate) Option<f32>);

#[derive(Default)]
pub(crate) struct LookOwnership {
    admission: Option<u64>,
    /// A captured left press stays hidden from UI and world clicks through
    /// release, even if the right button ends the look first.
    left: bool,
}

/// Reads native button edges before UI focus. The button resource can then
/// hide a captured press without also erasing the look's held state.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn look(
    mut look: ResMut<MouseLook>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut ownership: Local<LookOwnership>,
    mut events: MessageReader<MouseButtonInput>,
    mut focus: MessageReader<WindowFocused>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    ui: windows::pointer::PointerUi,
    online: Res<online::OnlineState>,
    script: Option<Res<crate::script::Script>>,
    (chat, items, inventory): (
        Res<chat::ChatState>,
        Res<items::ItemState>,
        Res<inventory::InventoryState>,
    ),
) {
    let window = windows.single().ok();
    let primary = window.map(|(entity, _)| entity);
    let mut focused = window.is_some_and(|(_, window)| window.focused);
    for event in focus.read() {
        focused &= Some(event.window) != primary || event.focused;
    }
    let admission = online.world().session_id();
    let playing = online.selection.is_none()
        && !online.world().is_dead()
        && (!online.enabled || online.world().connected());
    let available =
        focused && playing && !script.is_some_and(|script| script.background_owns_input());
    if !available || ownership.admission != admission {
        *look = MouseLook::default();
    }
    ownership.admission = admission;
    let over_ui = window
        .and_then(|(_, window)| window.physical_cursor_position())
        .is_some_and(|cursor| ui.contains(cursor));
    let free = !over_ui && !chat.hovered && !items.hovered && !inventory.hovered;
    let mut mask_left = ownership.left;
    for event in events.read().filter(|event| Some(event.window) == primary) {
        match (event.button, event.state.is_pressed()) {
            (MouseButton::Right, true) => look.held = available && free,
            (MouseButton::Right, false) => *look = MouseLook::default(),
            (MouseButton::Left, true) => {
                look.left = look.held;
                ownership.left = look.left;
                mask_left = ownership.left;
            }
            (MouseButton::Left, false) => {
                look.left = false;
                ownership.left = false;
            }
            _ => {}
        }
    }
    if !focused {
        ownership.left = false;
    }
    if mask_left {
        buttons.reset(MouseButton::Left);
    }
}

/// Samples horizontal look before locomotion. Online the attached view follows
/// the actor's rotation, and offline the same input turns the orbit directly.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn turn(
    look: Res<MouseLook>,
    online: Res<online::OnlineState>,
    options: Res<options::OptionsState>,
    mut aim: ResMut<MouseAim>,
    mut motion: MessageReader<MouseMotion>,
    mut cameras: Query<&mut OrbitCamera, Without<FreeCamera>>,
) {
    aim.0 = None;
    if !look.held {
        motion.clear();
        return;
    }
    let horizontal = motion.read().map(|event| event.delta.x).sum::<f32>();
    if let Ok(mut camera) = cameras.single_mut() {
        let yaw = -horizontal * options.options.turn_per_pixel();
        if online.enabled {
            aim.0 = Some(yaw);
        } else {
            camera.yaw += yaw;
        }
    }
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
        view: default(),
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

/// The head above the body center and the feet below it, in world units.
#[derive(Clone, Copy)]
struct CameraBody {
    head: f32,
    feet: Option<f32>,
}

impl CameraBody {
    /// Shared eye point for chase and first person, shortened by low ceilings.
    fn eye(self, center: Vec3, world: Option<&CollisionWorld>) -> Vec3 {
        let head = self.head.max(0.0);
        let eye = world
            .and_then(|world| world.ray_distance(center, Vec3::Y, head))
            .map_or(head, |hit| (hit - 0.1).max(0.0));
        center + Vec3::Y * eye
    }
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
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
    players: Query<(&PlayerBody, &Transform), (With<Player>, Without<OrbitCamera>)>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    ui: windows::pointer::PointerUi,
) {
    let player = players.single().ok();
    let body = player.map(|(body, _)| body);
    let body = CameraBody {
        head: body.map_or(0.0, |body| body.height * HEAD_LINE),
        feet: body.map(|body| body.feet_offset),
    };
    let playing = online.selection.is_none()
        && !online.world().is_dead()
        && (!online.enabled || online.world().connected());
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
    let scroll = if !playing
        || !accepts_input
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
            let radius = camera.radius;
            let clip = clip.map(|clip| clip + past_program(radius, program));
            if let Some(mut projection) = projection {
                fit_view_projection(&mut projection, radius, clip, false);
            }
            continue;
        }
        if playing
            && online.enabled
            && look.held
            && let Some((_, player)) = player
        {
            start_behind(&mut camera, player);
        }
        let world = collision.as_ref().and_then(|world| world.0.as_ref());
        if !playing {
            camera.reset_view();
        }
        // The orthographic view keeps its own pitch and zoom ranges.
        if matches!(projection.as_deref(), Some(Projection::Orthographic(_))) {
            if camera.is_first_person() {
                camera.reset_view();
            }
            top_down(&mut camera, drag.y * turn * rise, (-scroll * 0.12).exp());
            *transform = unobstructed_orbit(&mut camera, world, body.head, None, time.delta_secs());
        } else {
            *transform = camera.attached_view(
                limits,
                drag.y * turn * rise,
                scroll,
                world,
                body,
                time.delta_secs(),
            );
        }
        let radius = camera.radius;
        let clip = clip.map(|clip| clip + past_program(radius, program));
        if let Some(mut projection) = projection {
            let close_upward_orbit =
                camera.pitch > 0.0 && transform.translation.distance_squared(camera.focus) < 4.0;
            fit_view_projection(
                &mut projection,
                radius,
                clip,
                camera.is_first_person() || close_upward_orbit,
            );
        }
    }
}

/// Changes visibility on the existing local model/gear hierarchy in the
/// frame the camera enters or leaves first person.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn show_player(
    cameras: Query<(&OrbitCamera, Has<FreeCamera>)>,
    mut players: Query<&mut Visibility, With<Player>>,
) {
    let hidden = cameras
        .single()
        .is_ok_and(|(camera, free)| !free && camera.is_first_person());
    for mut visibility in &mut players {
        visibility.set_if_neq(if hidden {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

/// Stops short of either pole so the orbit retains its horizontal heading.
const MAX_CHASE_PITCH: f32 = 89.0_f32.to_radians();

/// Drag changes the angle and zoom changes the distance along that sightline.
/// The selected leash bounds that distance before collision adjusts the pose.
pub(super) fn chase(
    camera: &mut OrbitCamera,
    limits: eq_client_assets::camera::CameraLimits,
    tilt: f32,
    zoom: f32,
) {
    let (least_distance, most_distance) = limits.chase_distance;
    camera.pitch = (camera.pitch - tilt).clamp(-MAX_CHASE_PITCH, MAX_CHASE_PITCH);
    camera.radius = (camera.radius * zoom).clamp(least_distance, most_distance);
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

/// Fits scene reach and leaves nearby surfaces visible in a close view.
fn fit_view_projection(projection: &mut Projection, radius: f32, clip: Option<f32>, close: bool) {
    if !fitted(projection, radius, clip) {
        fit_projection(projection, radius, clip);
    }
    if let Projection::Perspective(perspective) = projection {
        perspective.near = if close { 0.1 } else { 1.0 };
    }
}

pub(super) fn orbit_transform(camera: &OrbitCamera) -> Transform {
    orbit_pose(camera, camera.focus)
}

fn orbit_pose(camera: &OrbitCamera, pivot: Vec3) -> Transform {
    let rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);
    let position = pivot + rotation * Vec3::new(0.0, 0.0, camera.radius);
    Transform::from_translation(position).with_rotation(rotation)
}

/// Body-centered framing for the top-down view, with a second ray at the head.
pub(super) fn unobstructed_orbit(
    camera: &mut OrbitCamera,
    collision: Option<&CollisionWorld>,
    head: f32,
    feet: Option<f32>,
    seconds: f32,
) -> Transform {
    let center = camera.focus;
    place_orbit(
        camera,
        collision,
        center,
        center + Vec3::Y * head,
        feet,
        seconds,
    )
}

/// Shortens the sightline to keep the camera on the player's side of geometry.
/// Also checks a ray from the other body point and preserves the chosen zoom
/// as the camera eases back out. The floor bound is relative to the pivot.
fn place_orbit(
    camera: &mut OrbitCamera,
    collision: Option<&CollisionWorld>,
    pivot: Vec3,
    other_origin: Vec3,
    clearance_below_pivot: Option<f32>,
    seconds: f32,
) -> Transform {
    let mut pose = orbit_pose(camera, pivot);
    let distance = camera.radius;
    if distance <= 0.001 {
        camera.pulled = None;
        return pose;
    }
    let direction = *pose.back();
    let requested = match clearance_below_pivot {
        Some(clearance) if direction.y < 0.0 => {
            distance.min((clearance - 0.1).max(0.0) / -direction.y)
        }
        _ => distance,
    };
    let allowed = collision.map_or(requested, |world| {
        let along_pivot = world
            .ray_distance(pivot, direction, requested)
            .map_or(requested, |hit| (hit - 0.5).max(0.0));
        clear_sightline(world, pivot, other_origin, direction, along_pivot).min(along_pivot)
    });
    camera.pulled = camera
        .pulled
        .map(|shown| eased_distance(shown, allowed, seconds))
        .or((allowed < requested).then_some(allowed))
        .filter(|shown| requested - shown > PULLED_DONE);
    let shown = camera.pulled.unwrap_or(allowed);
    if shown < distance {
        pose.translation = pivot + direction * shown;
    }
    pose
}

/// Where the head line starts, as a share of the body's height above its
/// middle: near the top of the head.
pub(super) const HEAD_LINE: f32 = 0.4;

/// Pulls the camera in when the line from the other body point is blocked,
/// recasting along the shortened line a bounded number of times.
fn clear_sightline(
    world: &CollisionWorld,
    pivot: Vec3,
    other_origin: Vec3,
    direction: Vec3,
    along_pivot: f32,
) -> f32 {
    let mut distance = along_pivot;
    for _ in 0..3 {
        let line = pivot + direction * distance - other_origin;
        let length = line.length();
        if length <= 0.001 {
            break;
        }
        let Some(hit) = world.ray_distance(other_origin, line / length, length) else {
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
pub(super) mod testing {
    use super::*;
    use bevy::input::{ButtonState, mouse::mouse_button_input_system};

    pub(crate) fn install_look(app: &mut App) {
        app.init_resource::<MouseLook>()
            .init_resource::<MouseAim>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<MouseButtonInput>()
            .add_message::<MouseMotion>()
            .add_message::<WindowFocused>()
            .add_systems(PreUpdate, (mouse_button_input_system, look).chain());
    }

    pub(crate) fn button(app: &mut App, button: MouseButton, pressed: bool) {
        let world = app.world_mut();
        let window = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .unwrap();
        world.write_message(MouseButtonInput {
            button,
            state: if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
            window,
        });
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
            view: default(),
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
                .abs_diff_eq(orbit_transform(orbit).translation + Vec3::Y * 2.4, 1e-5)
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
                .abs_diff_eq(orbit_transform(orbit).translation + Vec3::Y * 2.4, 1e-5)
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

    #[test]
    fn a_look_begun_over_the_world_lasts_over_windows_until_let_go() {
        let mut app = crate::testing::app();
        testing::install_look(&mut app);
        testing::button(&mut app, MouseButton::Right, true);
        app.update();
        assert!(app.world().resource::<MouseLook>().held);
        // The pointer passes over a window while the button stays down.
        app.world_mut().resource_mut::<chat::ChatState>().hovered = true;
        app.update();
        assert!(app.world().resource::<MouseLook>().held);
        testing::button(&mut app, MouseButton::Right, false);
        app.update();
        assert!(!app.world().resource::<MouseLook>().held);
    }

    #[test]
    fn a_press_begun_over_a_window_stays_the_windows() {
        let mut app = crate::testing::app();
        testing::install_look(&mut app);
        app.world_mut().resource_mut::<chat::ChatState>().hovered = true;
        testing::button(&mut app, MouseButton::Right, true);
        testing::button(&mut app, MouseButton::Left, true);
        app.update();
        assert!(!app.world().resource::<MouseLook>().held);
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .just_pressed(MouseButton::Left)
        );
        // Dragged off the window onto the world, it still turns nothing.
        app.world_mut().resource_mut::<chat::ChatState>().hovered = false;
        app.update();
        assert!(!app.world().resource::<MouseLook>().held);
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
    fn the_classic_chase_leash_bounds_distance_at_steep_pitch() {
        let limits = eq_client_assets::camera::TITANIUM;
        let mut camera = OrbitCamera {
            focus: Vec3::ZERO,
            radius: 100.0,
            yaw: 0.0,
            pitch: -0.2,
            pulled: None,
            view: default(),
        };
        // Pitch can be changed at the current distance.
        chase(&mut camera, limits, 1000.0, 1.0);
        assert!((camera.radius - 100.0).abs() < 1e-3);
        let before = orbit_transform(&camera);
        for (zoom, distance) in [(100.0, 200.0), (0.0, 2.0)] {
            chase(&mut camera, limits, 0.0, zoom);
            let pose = orbit_transform(&camera);
            assert!((pose.translation.distance(camera.focus) - distance).abs() < 1e-3);
            assert!(pose.forward().abs_diff_eq(*before.forward(), 1e-5));
        }
    }

    #[test]
    fn the_long_leash_extends_scene_reach_at_steep_pitch() {
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
            view: default(),
        };
        chase(&mut camera, limits, 0.0, 1_000.0);
        for tilt in [-1000.0, 1000.0] {
            chase(&mut camera, limits, tilt, 1.0);
            assert!((camera.radius - 20_000.0).abs() < 0.1);
            assert!((past_program(camera.radius, TITANIUM) - 19_800.0).abs() < 0.1);
        }
        assert!(past_program(150.0, TITANIUM).abs() < f32::EPSILON);
        // Turned off, the program's limit.
        options
            .qol
            .set(eq_client_core::qol::Fix::LongChaseCamera, false);
        assert_eq!(leash(&options), ChaseLeash::Program);
        chase(&mut camera, TITANIUM.on_leash(leash(&options)), 0.0, 1.0);
        assert!((camera.radius - 200.0).abs() < 1e-3);
    }

    #[test]
    fn captured_left_press_and_release_never_reach_click_consumers() {
        let mut app = crate::testing::app();
        testing::install_look(&mut app);
        testing::button(&mut app, MouseButton::Right, true);
        app.update();
        testing::button(&mut app, MouseButton::Left, true);
        for _ in 0..3 {
            app.update();
            assert!(app.world().resource::<MouseLook>().left);
            let buttons = app.world().resource::<ButtonInput<MouseButton>>();
            assert!(!buttons.pressed(MouseButton::Left));
            assert!(!buttons.just_pressed(MouseButton::Left));
        }
        // Release the right button first, then release left over a UI control.
        testing::button(&mut app, MouseButton::Right, false);
        app.update();
        assert!(!app.world().resource::<MouseLook>().left);
        testing::button(&mut app, MouseButton::Left, false);
        app.update();
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .just_released(MouseButton::Left)
        );
        // Two ordinary clicks remain available to UI and the door tracker.
        for _ in 0..2 {
            testing::button(&mut app, MouseButton::Left, true);
            app.update();
            assert!(
                app.world()
                    .resource::<ButtonInput<MouseButton>>()
                    .just_pressed(MouseButton::Left)
            );
            testing::button(&mut app, MouseButton::Left, false);
            app.update();
            assert!(
                app.world()
                    .resource::<ButtonInput<MouseButton>>()
                    .just_released(MouseButton::Left)
            );
        }
    }

    #[test]
    fn losing_the_focus_ends_the_look() {
        let mut app = crate::testing::app();
        testing::install_look(&mut app);
        testing::button(&mut app, MouseButton::Right, true);
        testing::button(&mut app, MouseButton::Left, true);
        app.update();
        assert!(app.world().resource::<MouseLook>().left);
        let world = app.world_mut();
        let window = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .unwrap();
        // Even a focus round trip in one frame ends the native hold.
        for focused in [false, true] {
            world.write_message(WindowFocused { window, focused });
        }
        for _ in 0..2 {
            app.update();
            assert_eq!(*app.world().resource::<MouseLook>(), MouseLook::default());
        }
    }
}
