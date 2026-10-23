#![doc = "Bevy scene and camera support for renderer-independent EQ zone assets."]

use std::path::PathBuf;
use theme::Size;

mod camera;
mod gpu_assets;
mod scene_collision;
mod systems;

use camera::{
    OrbitCamera, behind, camera_relative_direction, orbit_camera, spawn_camera, start_behind,
};
use gpu_assets::{create_material, create_render_primitives, create_texture_images};
pub use scene_collision::zone_collision;
use scene_collision::{TerrainSurface, collision_triangles, collision_world, object_transform};
pub(crate) use systems::Stage;
use systems::{init_presentation, install_overlays, schedule};

#[cfg(test)]
use bevy::input::mouse::MouseMotion;
#[cfg(test)]
use camera::{CHASE_PITCH, HEAD_LINE, eased_distance, orbit_transform, unobstructed_orbit};
#[cfg(test)]
use gpu_assets::{bevy_texture_coordinates, create_image, generate_mip_chain};
#[cfg(test)]
use scene_collision::{ZoneLiquids, build_collision, triangle_height_at};

mod abilities;
mod asset_loads;
mod attention;
mod background;
mod book_delete;
mod buffs;
mod character;
mod character_select;
mod chat;
mod coins;
mod combat;
mod confirm;
mod daylight;
mod device_pixels;
mod doors;
mod entities;
mod escape;
mod exit_log;
mod frame_limit;
mod give;
mod ground;
mod group;
mod hud;
mod interact;
mod inventory;
mod item_models;
mod items;
mod keys;
mod loading;
mod login;
mod logs;
mod map;
mod motion;
mod names;
mod navigation;
mod navmesh_prebuild;
mod notices;
mod online;
mod options;
mod outbox;
mod outfit;
mod paperdoll;
mod pet;
mod preview;
#[cfg(test)]
mod probes;
mod profile_files;
mod raid;
mod reading;
mod resources;
mod resurrection;
pub mod script;
mod select_preview;
mod sheets;
mod skills;
mod skin;
mod skinned;
mod sounds;
mod spell_effects;
mod spell_icons;
mod spellbook;
mod target;
mod theme;
mod tooltip;
mod trade;
mod tradeskills;
mod training;
mod whereabouts;
mod who;
mod window_size;
mod windows;
mod zone;
mod zone_navmesh;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Captured, Screenshot, save_to_disk};
use eq_client_assets::ZoneAsset;
use eq_client_assets::characters::CharacterAsset;
use eq_client_core::{WorldPosition, render_position, world_position};

/// The projection used by the top-down camera.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProjectionStyle {
    /// A RuneScape-like perspective view.
    #[default]
    Perspective,
    /// A fixed-scale Diablo-like view.
    Orthographic,
}

/// Explicit one-shot validation using the normal typed command path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationAction {
    /// Select the nearest visible rendered player.
    TargetNearestPlayer,
    /// Inspect the first genuine item link received in chat.
    InspectFirstItem,
}

pub use login::{Availability, Connection, LoginServer, Logins, Session, Worker};
pub use preview::Preview;

/// Where the viewer's world comes from.
pub enum Source {
    /// Servers: the login screen and the sessions it starts, beginning with
    /// the session the launch began, if any.
    Online {
        /// The login servers the login screen offers, and how to log in.
        logins: Box<dyn Logins>,
        /// The session the launch began, which skips the login screen.
        session: Option<Session>,
    },
    /// No server. The offline preview, if it shows anything, stands in for one.
    Offline(Preview),
}

/// Settings for a viewer window.
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Independent display switches.
pub struct ViewerConfig {
    /// Permit explicitly labeled pre-SoF resource estimates for the Titanium session.
    pub estimate_titanium_resources: bool,
    /// Initial camera projection.
    pub projection: ProjectionStyle,
    /// Optional PNG destination captured after the scene has settled.
    pub screenshot: Option<PathBuf>,
    /// Seconds to wait after the scene is ready before capturing.
    pub screenshot_after: Option<f32>,
    /// Optional initial position in EQ world coordinates.
    pub start_position: Option<WorldPosition>,
    /// Whether the start position's height was given; the viewer then stands
    /// on the floor below it rather than on the highest surface.
    pub start_height_known: bool,
    /// Optional initial camera distance from the character.
    pub camera_distance: Option<f32>,
    /// Whether to omit placed static objects for terrain inspection.
    pub terrain_only: bool,
    /// User-owned installation used to load server-selected zones and models.
    pub eq_directory: Option<PathBuf>,
    /// Which official client that installation holds, which decides how its
    /// own settings files are read.
    pub installed_client: eq_client_assets::ui::InstalledClient,
    /// Nearby-entity radius in EQ units; None uses 200.
    pub entity_distance: Option<f32>,
    /// Optional read-only live validation action.
    pub validation: Option<ValidationAction>,
    /// Optional attended key script driven through the normal input paths.
    pub script: Option<Vec<script::Step>>,
    /// Script file and byte offset to keep reading appended steps from.
    pub script_follow: Option<(PathBuf, usize)>,
    /// Leave the script pointer's arrow undrawn while a script runs; it is
    /// drawn by default, so a recording shows where each click lands.
    pub hide_script_pointer: bool,
    /// The network session refuses servers outside this machine's network (a local
    /// `EQEmu` test server), so scripts may send `gm` steps and run unwatched.
    pub local_session: bool,
    /// UI skin to use instead of the one the character chose in the official client.
    pub ui_skin: Option<String>,
    /// What a character with no options of their own starts with, the
    /// frame rate cap among them.
    pub option_defaults: eq_client_core::options::Options,
    /// The frame rate cap the command line sets, which wins over
    /// `eqclient.ini`'s for a character with no choice of their own.
    pub max_fps: Option<u16>,
    /// Where this client keeps its own settings; None keeps nothing between runs.
    pub settings_directory: Option<PathBuf>,
    /// Optional top-left window corner in physical desktop pixels.
    pub window_position: Option<(i32, i32)>,
    /// Optional size of the window's drawing area in physical pixels.
    pub window_size: Option<(u32, u32)>,
    /// Add the developer's readings to the status box: the zone's short
    /// name, coordinates, the movement mode with its keys, and the count of
    /// nearby entities.
    pub debug_overlay: bool,
    /// Build each zone's navigation mesh at entry, for probes, scripts and
    /// looks; never a player setting. Nothing plans over it yet.
    pub navmesh: bool,
}

impl ViewerConfig {
    /// The installed official client's own settings, read by the rules of its
    /// generation; None without an installation.
    fn official_settings(&self) -> Option<Box<dyn eq_client_assets::ui::OfficialSettings + '_>> {
        self.eq_directory
            .as_deref()
            .map(|directory| self.installed_client.settings(directory))
    }
}

#[derive(Resource)]
struct PendingZone {
    zone: Option<ZoneAsset>,
    character: Option<CharacterAsset>,
}

#[derive(Resource, Default)]
struct ViewerSettings(ViewerConfig);

#[derive(Resource)]
struct CaptureRequest {
    path: PathBuf,
    settle_timer: Timer,
}

#[derive(Resource)]
struct SceneInfo {
    zone_name: String,
}

#[derive(Resource)]
struct Collision(Option<eq_client_core::movement::CollisionWorld>);

#[derive(Component)]
struct Player;

#[derive(Component, Clone, Copy)]
struct PlayerBody {
    feet_offset: f32,
    height: f32,
    /// The size the player is drawn at, which picks its walker size on the
    /// navigation mesh. Its collision height is still the model's own.
    size: f32,
}

impl PlayerBody {
    /// The walker size the player plans on the navigation mesh by: its
    /// drawn size, but never shorter than its collision height, so the mesh
    /// offers no way its body's own checks would refuse. Its size alone
    /// once collision follows the size.
    fn mesh_size(&self) -> f32 {
        self.size.max(self.height)
    }
}

#[derive(Component)]
struct SceneEntity;

#[derive(Component)]
struct HudText;

/// Opens a window and renders a loaded zone until the user closes it.
pub fn run(
    zone: ZoneAsset,
    character: Option<CharacterAsset>,
    config: ViewerConfig,
    source: Source,
) -> i32 {
    let screenshot = config.screenshot.clone();
    let steps = config.script.clone();
    let arrow = !config.hide_script_pointer;
    let follow = config.script_follow.clone();
    let local_session = config.local_session;
    let installed_client = config.installed_client;
    let mut option_defaults = config.option_defaults;
    // The official client's own settings, for characters with no choice of
    // their own here; the client's defaults where it says nothing.
    if let Some(settings) = config.official_settings() {
        use eq_client_core::options::Toggle;
        let official = settings.options();
        for (toggle, setting) in [
            (Toggle::Log, official.log),
            (Toggle::Sound, official.sound),
            (Toggle::PcNames, official.pc_names),
            (Toggle::NpcNames, official.npc_names),
        ] {
            if let Some(on) = setting {
                option_defaults.set(toggle, on);
            }
        }
        if let Some(level) = official
            .show_names_level
            .and_then(eq_client_core::names::ShowNames::from_level)
        {
            option_defaults.show_names = level;
        }
        seed_levels(&mut option_defaults, &official);
    }
    if let Some(cap) = config.max_fps {
        option_defaults.max_fps = cap;
    }
    let online = matches!(source, Source::Online { .. });
    let screenshot_after = config.screenshot_after.unwrap_or(2.0).max(0.1);
    let window_size = config.window_size;
    let window = primary_window(
        online,
        screenshot.is_none(),
        config.window_position,
        window_size,
    );
    let mut app = App::new();
    let (front, updates, commands) = match source {
        Source::Online { logins, session } => login::FrontEnd::launched(logins, session),
        Source::Offline(preview) => (
            login::FrontEnd::default(),
            preview::install(&mut app, preview),
            None,
        ),
    };
    app.insert_resource(front);
    app.insert_resource(login::LoginStrings::load(config.eq_directory.as_deref()));
    app.insert_resource(spellbook::SpellNames::load(config.eq_directory.as_deref()));
    app.insert_resource(hud::messages::Messages::load(
        config.eq_directory.as_deref(),
    ));
    app.insert_resource(PendingZone {
        zone: Some(zone),
        character,
    })
    .insert_resource(ViewerSettings(config))
    .insert_resource(online::OnlineState::new(online))
    .insert_resource(online::Updates(std::sync::Mutex::new(updates)))
    .insert_resource(outbox::Outbox::new(commands));
    init_presentation(&mut app);
    app.insert_resource(options::OptionsState::new(option_defaults));
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(window),
                ..default()
            })
            // What the client logs also goes to a file in the settings
            // folder, so an ending leaves its reason behind.
            .set(bevy::log::LogPlugin {
                custom_layer: exit_log::file_layer,
                ..default()
            }),
    );
    theme::install_font(&mut app, installed_client);
    app.add_systems(Startup, (setup_scene, navmesh_prebuild::offer))
        .add_systems(Update, exit_log::close_requests);
    schedule(&mut app);
    navigation::install(&mut app);
    zone_navmesh::install(&mut app);
    frame_limit::install(&mut app);
    attention::install(&mut app);
    if let Some(size) = window_size {
        window_size::install(&mut app, size);
    }
    loading::install(&mut app);
    device_pixels::install(&mut app);
    install_overlays(&mut app);
    if let Some(steps) = steps {
        install_script(&mut app, (steps, follow), (local_session, online), arrow);
    }
    if let Some(path) = screenshot {
        app.insert_resource(CaptureRequest {
            path,
            settle_timer: Timer::from_seconds(screenshot_after, TimerMode::Once),
        });
    }
    exit_status(&app.run())
}

/// What the tests of the windows start from.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// An app with every presentation resource the viewer starts with, empty
    /// and offline, plus input and a focused primary window: a test adds the
    /// systems it exercises, and a system that comes to need another
    /// presentation resource breaks no test's setup.
    pub(crate) fn app() -> App {
        let mut app = App::new();
        init_presentation(&mut app);
        app.init_resource::<ViewerSettings>()
            .init_resource::<spellbook::SpellNames>()
            .init_resource::<hud::messages::Messages>()
            .init_resource::<Assets<Image>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .insert_resource(online::OnlineState::new(false))
            .insert_resource(outbox::Outbox::new(None));
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        app
    }
}

/// The sliders' settings from `eqclient.ini`, on the scales assumed until a
/// recording of the official client says otherwise: `ClipPlane` 0 to 20 and
/// `MouseSensitivity` 0 to 10 as fractions of the slider, `MaxFPS` in frames.
fn seed_levels(
    options: &mut eq_client_core::options::Options,
    official: &eq_client_assets::ui::OfficialOptions,
) {
    use eq_client_core::options::Level;
    let share = |value: u32, top: u32| u16::try_from(value.min(top) * 100 / top).unwrap_or(100);
    if let Some(clip) = official.clip_plane {
        options.set_level(Level::ClipPlane, share(clip, 20));
    }
    if let Some(fps) = official.max_fps {
        options.set_level(Level::MaxFps, u16::try_from(fps.min(1000)).unwrap_or(1000));
    }
    if let Some(sensitivity) = official.mouse_sensitivity {
        options.set_level(Level::MouseSensitivity, share(sensitivity, 10));
    }
}

/// The process exit status for how the viewer ended.
fn exit_status(exit: &AppExit) -> i32 {
    match exit {
        AppExit::Success => 0,
        AppExit::Error(code) => i32::from(code.get()),
    }
}

/// Drives a script after UI focus, optionally following its file; it is
/// attended unless the session is local-only or the preview is offline.
/// The script pointer's arrow is drawn while it runs, unless `arrow` is false.
fn install_script(
    app: &mut App,
    (steps, follow): (Vec<script::Step>, Option<(PathBuf, usize)>),
    (local_session, online): (bool, bool),
    arrow: bool,
) {
    let script = match follow {
        Some((path, offset)) => script::Script::following(steps, path, offset),
        None => script::Script::new(steps),
    };
    app.insert_resource(script.local_session(local_session).offline_preview(!online));
    app.add_systems(PreUpdate, script::drive.after(bevy::ui::UiSystems::Focus))
        .add_systems(
            Update,
            (
                script::pointer::draw,
                // A scripted notch of the wheel replaces the real wheel's
                // turn, read just before, and reaches the lists as one does.
                script::pointer::turn
                    .after(windows::pointer::wheel)
                    .after(skinned::scrollbar::scroll)
                    .in_set(Stage::Typing),
            ),
        );
    if arrow {
        app.init_resource::<script::pointer::SkinArrow>()
            .add_systems(Startup, script::pointer::spawn)
            .add_systems(
                Update,
                script::pointer::read_arrow.before(script::pointer::draw),
            );
    }
}

/// Configures the live or offline window, hiding one-shot screenshot previews.
fn primary_window(
    online: bool,
    visible: bool,
    position: Option<(i32, i32)>,
    size: Option<(u32, u32)>,
) -> Window {
    Window {
        title: if online {
            "eq-client"
        } else {
            "eq-client offline viewer"
        }
        .to_owned(),
        visible,
        position: position.map_or(WindowPosition::Automatic, |(x, y)| {
            WindowPosition::At(IVec2::new(x, y))
        }),
        resolution: size.map(window_size::resolution).unwrap_or_default(),
        ..default()
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn setup_scene(
    mut commands: Commands,
    mut ambient_light: ResMut<GlobalAmbientLight>,
    mut pending: ResMut<PendingZone>,
    settings: Res<ViewerSettings>,
    mut online: ResMut<online::OnlineState>,
    mut scene: zone::Scene,
) {
    let zone = pending
        .zone
        .take()
        .expect("startup only consumes the zone once");
    let (bounds_min, bounds_max) = zone.bounds().unwrap_or(([-100.0; 3], [100.0; 3]));
    let min = Vec3::from_array(bounds_min);
    let max = Vec3::from_array(bounds_max);
    let zone_center = (min + max) * 0.5;
    let terrain_surface = TerrainSurface::from_primitives(&zone.primitives);
    let requested_position = settings
        .0
        .start_position
        .map(render_position)
        .map_or(Vec3::ZERO, Vec3::from_array);
    let start_x = requested_position.x.clamp(min.x, max.x);
    let start_z = requested_position.z.clamp(min.z, max.z);
    // A given height finds the floor under it, so a start inside a building
    // is not put on its roof; without one, the highest surface is the guess.
    let start_y = settings
        .0
        .start_height_known
        .then(|| terrain_surface.height_below(start_x, start_z, requested_position.y + 5.0))
        .flatten()
        .or_else(|| terrain_surface.height_at(start_x, start_z))
        .unwrap_or(zone_center.y);
    let character = pending.character.take();
    let height = character.as_ref().map_or(6.0, CharacterAsset::height);
    let body = PlayerBody {
        feet_offset: height * 0.5,
        height,
        size: 6.0,
    };
    let player_position = Vec3::new(start_x, start_y + body.feet_offset, start_z);
    let default_radius = 120.0;
    let radius = settings
        .0
        .camera_distance
        .unwrap_or(default_radius)
        .clamp(20.0, 20_000.0);
    let placed = Transform::from_translation(player_position);
    scene.enter(
        &mut commands,
        zone::Entry {
            zone,
            character,
            placed,
            body,
        }
        .prepare(settings.0.navmesh),
        settings.0.terrain_only,
        &mut online.regions,
    );
    spawn_lighting(&mut commands, &mut ambient_light);
    spawn_camera(&mut commands, placed, radius, settings.0.projection);
}

fn spawn_static_zone(
    commands: &mut Commands,
    zone: ZoneAsset,
    terrain_only: bool,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let texture_images = create_texture_images(zone.textures, images);
    let terrain = create_render_primitives(
        zone.primitives,
        &texture_images,
        meshes,
        materials,
        true,
        RenderAssetUsages::RENDER_WORLD,
    );
    commands.spawn_batch(
        terrain
            .into_iter()
            .map(|(mesh, material)| (SceneEntity, Mesh3d(mesh), MeshMaterial3d(material))),
    );

    if terrain_only {
        commands.insert_resource(doors::Models::default());
        return;
    }

    let mut door_models = doors::Models::default();
    let models: Vec<_> = zone
        .models
        .into_iter()
        .map(|model| {
            let primitives = create_render_primitives(
                model.primitives,
                &texture_images,
                meshes,
                materials,
                true,
                RenderAssetUsages::RENDER_WORLD,
            );
            door_models.0.insert(
                doors::model_key(&model.name),
                doors::Model {
                    primitives: primitives.clone(),
                    collision: model.collision,
                },
            );
            primitives
        })
        .collect();
    commands.insert_resource(door_models);
    for object in zone.objects {
        let Some(model) = models.get(object.model) else {
            continue;
        };
        let transform = object_transform(&object);
        commands
            .spawn((SceneEntity, transform, Visibility::Inherited))
            .with_children(|parent| {
                for (mesh, material) in model {
                    parent.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone())));
                }
            });
    }
}

fn spawn_player_and_hud(
    commands: &mut Commands,
    player_position: Vec3,
    placeholder: bool,
    body: PlayerBody,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let player_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.45, 0.95),
        metallic: 0.15,
        perceptual_roughness: 0.7,
        ..default()
    });
    let player = commands
        .spawn((
            Player,
            body,
            SceneEntity,
            Transform::from_translation(player_position),
            Visibility::Inherited,
        ))
        .id();
    if placeholder {
        commands.entity(player).insert((
            Mesh3d(meshes.add(Capsule3d::new(0.5, body.height - 1.0))),
            MeshMaterial3d(player_material),
            names::Overhead(body.height / 2.0),
        ));
    }

    let status_panel = commands
        .spawn((
            HudText,
            Text::new(""),
            theme::font(Size::Label),
            TextColor(theme::INK),
            windows::placed(
                windows::WindowId::Status,
                Node {
                    padding: UiRect::all(px(10)),
                    ..default()
                },
            ),
            BackgroundColor(theme::SCRIM),
        ))
        .id();
    windows::passive(commands, status_panel);
    windows::identify(commands, status_panel, windows::WindowId::Status);
    hud::spawn(commands);
    player
}

fn spawn_lighting(commands: &mut Commands, ambient_light: &mut GlobalAmbientLight) {
    // The day's light; online, the time of day changes it.
    commands.spawn((
        DirectionalLight {
            illuminance: daylight::DAY_SUN,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, -0.8, 0.0)),
        daylight::Sun,
    ));
    *ambient_light = daylight::launch_ambient();
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn schedule_screenshot(
    mut commands: Commands,
    time: Res<Time>,
    request: Option<ResMut<CaptureRequest>>,
    (online, front): (Res<online::OnlineState>, Res<login::FrontEnd>),
    loading: Option<Res<loading::Loading>>,
) {
    let covered = loading.is_some_and(|loading| loading.covered());
    if !screenshot_ready(&front, &online, covered) {
        return;
    }
    let Some(mut request) = request else {
        return;
    };
    if !request.settle_timer.tick(time.delta()).just_finished() {
        return;
    }

    let path = request.path.clone();
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    commands.remove_resource::<CaptureRequest>();
}

/// Whether a screenshot may count down: offline at once; online once a
/// login screen, the list of worlds or the character list has settled, or
/// once the player is in a zone with the loading screen lifted, as a
/// script's wait for the zone has it. The screen stays up while the zone's
/// scene loads, so the picture is the zone, not the cover.
fn screenshot_ready(front: &login::FrontEnd, online: &online::OnlineState, covered: bool) -> bool {
    !online.enabled || (front.settled(online) && !covered)
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn exit_after_screenshot(
    captured: RemovedComponents<Captured>,
    mut app_exit: MessageWriter<AppExit>,
    online: Res<online::OnlineState>,
    settings: Res<ViewerSettings>,
) {
    if online.world().ended() && settings.0.screenshot.is_some() {
        info!("The session ended before the screenshot, so the client is ending");
        app_exit.write(AppExit::error());
    }
    // Scripted screenshots keep the session running; only `--screenshot` is one-shot.
    if !captured.is_empty() && settings.0.screenshot.is_some() {
        info!("The screenshot is taken, so the client is ending");
        app_exit.write(AppExit::Success);
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system parameters are value wrappers.
fn move_player(
    keys: keys::Keys,
    online: Res<online::OnlineState>,
    time: Res<Time>,
    collision: Res<Collision>,
    mut airborne: Local<eq_client_core::movement::AirborneController>,
    mut players: Query<(&mut Transform, &PlayerBody), With<Player>>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    if online.enabled {
        airborne.reset();
        return;
    }
    let Ok((mut player, body)) = players.single_mut() else {
        return;
    };
    let accepts_input = keys.focused();
    let (horizontal, vertical) = if accepts_input {
        (
            keys.map
                .axis(&keys.input, keys::Act::CameraLeft, keys::Act::CameraRight),
            keys.map
                .axis(&keys.input, keys::Act::CameraBack, keys::Act::CameraForward),
        )
    } else {
        (0.0, 0.0)
    };
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    let direction = camera_relative_direction(horizontal, vertical, camera.yaw);
    if direction != Vec3::ZERO {
        player.rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
    }
    let Some(world) = &collision.0 else {
        return;
    };
    let feet = player.translation - Vec3::Y * body.feet_offset;
    let delta = eq_client_core::movement::displacement(direction, 6.0, time.delta_secs());
    // Preview tuning only: not a claim about EQ gravity, jump impulse or wire velocity.
    let physics = eq_client_core::movement::VerticalPhysics {
        gravity: 32.0,
        terminal_speed: 40.0,
        jump_speed: 10.0,
    };
    player.translation = airborne.step(
        world,
        feet,
        physics,
        eq_client_core::movement::MotionStep {
            horizontal: delta,
            jump: keys.pressed(keys::Act::Jump),
            seconds: time.delta_secs(),
            height: body.height,
        },
    ) + Vec3::Y * body.feet_offset;
    if let Some(landing) = airborne.take_landing() {
        debug!(
            fall_distance = landing.fall_distance,
            impact_speed = landing.impact_speed,
            damage =
                world.landing_damage(player.translation - Vec3::Y * body.feet_offset, landing, 0),
            "Offline landing; nothing is sent"
        );
    }
    camera.focus = player.translation;
}

/// Shows the status line and what the player can use here, and the
/// developer's readings (the zone's short name among them) only when the
/// debug overlay is on. With nothing to show, the box hides.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn update_hud(
    motion: Res<motion::Controls>,
    (scene, settings, notices, map): (
        Res<SceneInfo>,
        Res<ViewerSettings>,
        Res<notices::Lines>,
        Res<keys::KeyMap>,
    ),
    nearby: Res<entities::NearbyEntities>,
    online: Res<online::OnlineState>,
    players: Query<&Transform, With<Player>>,
    mut labels: Query<(&mut Text, &mut Visibility), With<HudText>>,
) {
    let (Ok(player), Ok((mut label, mut visibility))) = (players.single(), labels.single_mut())
    else {
        return;
    };
    let mut lines = vec![notices.status.text().to_owned()];
    if online.enabled && online.in_world() && motion.speed.is_none() {
        lines.push("Movement unavailable: start with --movement-calibration".into());
    }
    let nearest = interact::nearest(&online);
    lines.push(match nearest {
        Some(interact::Use::Door(..)) => map.help(&[(keys::Act::Use, "use the door")]),
        Some(interact::Use::Object(_)) => {
            map.help(&[(keys::Act::Use, "pick up or open what is here")])
        }
        None => String::new(),
    });
    if settings.0.debug_overlay {
        lines.push(scene.zone_name.clone());
        let position = world_position(player.translation.to_array(), 0.0);
        lines.push(format!(
            "{:.0}, {:.0}, {:.0}",
            position.x, position.y, position.z
        ));
        lines.push(movement_help(&online, &motion, &map));
        lines.push(format!("Nearby entities: {}", nearby.rendered.len()));
        if let Some(interact::Use::Door(id, model)) = nearest {
            lines.push(format!("Door {id} ({model})"));
        }
    }
    let text = lines
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    // The official client has no such box, so it shows only while it has
    // something to say.
    let wanted = if text.is_empty() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    if *visibility != wanted {
        *visibility = wanted;
    }
    if label.0 != text {
        label.0 = text;
    }
}

/// The movement mode and its keys, for the debug overlay.
fn movement_help(
    online: &online::OnlineState,
    motion: &motion::Controls,
    map: &keys::KeyMap,
) -> String {
    use keys::Act;
    let moving = !online.enabled || motion.speed.is_some();
    let mode = if !online.enabled {
        "Offline"
    } else if motion.speed.is_none() && !online.in_world() {
        // Zoning or dead: the session takes movement away until it is over.
        "Movement paused"
    } else if motion.speed.is_none() {
        "Movement disabled (start with --movement-calibration)"
    } else if motion.walking {
        "Walking"
    } else {
        "Running"
    };
    let mut acts = Vec::new();
    if moving {
        acts.push((Act::Jump, "jump"));
    }
    if online.enabled && motion.walk_speed.is_some() {
        acts.push((Act::Walk, if motion.walking { "run" } else { "walk" }));
    }
    let keys = map.help(&acts);
    [mode, &keys, "Right-drag: orbit", "Wheel: zoom"]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_player_plans_on_a_mesh_no_shorter_than_its_collision() {
        let body = |height, size| super::PlayerBody {
            feet_offset: 0.0,
            height,
            size,
        };
        // A gnome drawn 3 tall with a 6.8 collision plans as 6.8.
        assert!((body(6.8, 3.0).mesh_size() - 6.8).abs() < 1e-6);
        // An ogre drawn 9 tall plans as 9.
        assert!((body(6.4, 9.0).mesh_size() - 9.0).abs() < 1e-6);
    }

    #[test]
    fn a_screenshot_waits_for_the_loading_screen_to_lift() {
        use super::*;
        // A session logging in, as `--online` starts one.
        let (_updates, receive) = std::sync::mpsc::sync_channel(1);
        let (commands, _queue) = std::sync::mpsc::sync_channel(1);
        let (front, ..) = login::FrontEnd::launched(
            Box::new(login::testing::Fake {
                servers: vec![login::testing::server("Example", "someone")],
                heard: std::sync::Arc::default(),
            }),
            Some(login::Session {
                updates: receive,
                commands,
                worker: Box::new(login::testing::Thread::default()),
            }),
        );
        let mut online = online::OnlineState::new(true);
        assert!(!screenshot_ready(&front, &online, false));
        online::testing::admit(&mut online, 1, online::testing::player(7));
        // In the zone, but the cover is still up while its scene loads.
        assert!(!screenshot_ready(&front, &online, true));
        assert!(screenshot_ready(&front, &online, false));
        // Offline there is nothing to wait for.
        let offline = online::OnlineState::new(false);
        assert!(screenshot_ready(&front, &offline, true));
    }

    #[test]
    fn the_status_box_shows_only_while_it_has_something_to_say() {
        use super::*;
        let mut app = testing::app();
        app.insert_resource(SceneInfo {
            zone_name: "qeytoqrg".into(),
        })
        .add_systems(Update, update_hud);
        app.world_mut().spawn((Player, Transform::default()));
        let status = app
            .world_mut()
            .spawn((HudText, Text::new(""), Visibility::Inherited))
            .id();
        let shown = |app: &mut App| {
            app.update();
            let text = app.world().get::<Text>(status).unwrap().0.clone();
            let visibility = *app.world().get::<Visibility>(status).unwrap();
            (text, visibility)
        };
        // Nothing to say: no zone name, and no box.
        assert_eq!(shown(&mut app), (String::new(), Visibility::Hidden));
        app.world_mut()
            .resource_mut::<notices::Lines>()
            .status
            .set("Camped - choose a character");
        assert_eq!(
            shown(&mut app),
            ("Camped - choose a character".into(), Visibility::Inherited)
        );
        // The developer's readings name the zone.
        app.world_mut()
            .resource_mut::<notices::Lines>()
            .status
            .set("");
        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .0
            .debug_overlay = true;
        let (text, visibility) = shown(&mut app);
        assert!(text.starts_with("qeytoqrg\n"), "{text}");
        assert_eq!(visibility, Visibility::Inherited);
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies unchanged or explicitly assigned state"
    )]
    fn camera_stays_below_roof_and_restores_requested_zoom_in_open_space() {
        use super::*;
        let roof = eq_client_core::movement::CollisionWorld::new([
            [
                [-100.0, 8.0, -100.0],
                [100.0, 8.0, -100.0],
                [100.0, 8.0, 100.0],
            ],
            [
                [-100.0, 8.0, -100.0],
                [100.0, 8.0, 100.0],
                [-100.0, 8.0, 100.0],
            ],
        ])
        .unwrap();
        let camera = OrbitCamera {
            focus: Vec3::ZERO,
            radius: 40.0,
            yaw: 0.75,
            pitch: -60.0_f32.to_radians(),
            pulled: None,
        };
        let requested = orbit_transform(&camera);
        let mut under = OrbitCamera { ..camera };
        let inside = unobstructed_orbit(&mut under, Some(&roof), 0.0, 1.0 / 60.0);
        assert!(inside.translation.y < 8.0);
        assert!(
            (inside.translation.length() - (8.0 / 60.0_f32.to_radians().sin() - 0.5)).abs() < 0.001
        );
        assert_eq!(inside.rotation, requested.rotation);
        assert_eq!(under.radius, 40.0);
        let mut open = OrbitCamera { ..camera };
        assert_eq!(
            unobstructed_orbit(&mut open, None, 0.0, 1.0 / 60.0),
            requested
        );
        let mut outside = OrbitCamera {
            focus: Vec3::new(200.0, 0.0, 200.0),
            ..camera
        };
        let expected = orbit_transform(&outside);
        assert_eq!(
            unobstructed_orbit(&mut outside, Some(&roof), 0.0, 1.0 / 60.0),
            expected
        );
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies the camera is back at the chosen zoom"
    )]
    fn camera_pulls_in_at_once_and_eases_back_out_to_the_chosen_zoom() {
        use super::*;
        let roof = eq_client_core::movement::CollisionWorld::new([
            [
                [-100.0, 8.0, -100.0],
                [100.0, 8.0, -100.0],
                [100.0, 8.0, 100.0],
            ],
            [
                [-100.0, 8.0, -100.0],
                [100.0, 8.0, 100.0],
                [-100.0, 8.0, 100.0],
            ],
        ])
        .unwrap();
        let mut camera = OrbitCamera {
            focus: Vec3::new(200.0, 0.0, 0.0),
            radius: 40.0,
            yaw: 0.75,
            pitch: -60.0_f32.to_radians(),
            pulled: None,
        };
        // Walking under the roof's edge and back out, a frame at a time, with
        // the head's line as well as the middle's.
        let frame = 1.0 / 60.0;
        let rise = 6.0 * HEAD_LINE;
        let mut shown = Vec::new();
        for step in 0..240_u16 {
            let x = if step < 120 {
                140.0 - f32::from(step)
            } else {
                20.0 + f32::from(step - 120) * 2.0
            };
            camera.focus.x = x;
            let unobstructed = unobstructed_orbit(
                &mut OrbitCamera {
                    pulled: None,
                    ..camera
                },
                Some(&roof),
                rise,
                frame,
            );
            let pose = unobstructed_orbit(&mut camera, Some(&roof), rise, frame);
            let distance = (pose.translation - camera.focus).length();
            let limit = (unobstructed.translation - camera.focus).length();
            assert!(
                distance <= limit + 0.001,
                "step {step}: {distance} past {limit}"
            );
            assert!(distance <= camera.radius + 0.001);
            shown.push(distance);
        }
        // It went in under the roof and was easing out, not jumping, as it cleared.
        assert!(shown.iter().any(|distance| *distance < 20.0));
        assert!(
            shown
                .windows(2)
                .any(|pair| pair[0] < pair[1] && pair[1] < 39.0)
        );
        // Out in the open again, a few frames later it is back at the radius.
        for _ in 0..60 {
            unobstructed_orbit(&mut camera, Some(&roof), rise, frame);
        }
        assert_eq!(camera.pulled, None);
        assert_eq!(
            unobstructed_orbit(&mut camera, Some(&roof), rise, frame),
            orbit_transform(&camera)
        );
    }

    #[test]
    fn a_wall_top_across_the_head_line_pulls_the_camera_in() {
        use super::*;
        // A lintel hanging from 2 to 4 units up, 10 units behind the player:
        // under the line from the body's middle, across the line from the head.
        let lintel = eq_client_core::movement::CollisionWorld::new([
            [[-10.0, 2.0, 10.0], [10.0, 2.0, 10.0], [10.0, 4.0, 10.0]],
            [[-10.0, 2.0, 10.0], [10.0, 4.0, 10.0], [-10.0, 4.0, 10.0]],
        ])
        .unwrap();
        let camera = OrbitCamera {
            focus: Vec3::ZERO,
            radius: 40.0,
            yaw: 0.0,
            pitch: -0.15,
            pulled: None,
        };
        let requested = orbit_transform(&camera);
        let rise = 6.0 * HEAD_LINE;
        // The middle's line alone is clear, so it stays out at the radius.
        let mut waist_only = OrbitCamera { ..camera };
        assert_eq!(
            unobstructed_orbit(&mut waist_only, Some(&lintel), 0.0, 1.0 / 60.0),
            requested
        );
        // With the head's line, it comes in to this side of the lintel.
        let mut both = OrbitCamera { ..camera };
        let pose = unobstructed_orbit(&mut both, Some(&lintel), rise, 1.0 / 60.0);
        assert!(pose.translation.z < 10.0, "{:?}", pose.translation);
        assert_eq!(pose.rotation, requested.rotation);
        let head = Vec3::Y * rise;
        let line = pose.translation - head;
        assert!(
            lintel
                .ray_distance(head, line.normalize(), line.length())
                .is_none()
        );
        assert!(both.pulled.is_some_and(|shown| shown < 40.0));
    }

    /// The EQ heading W walks along with the camera at `yaw`, as the script's
    /// `camera` step test reads it.
    fn walked_heading(yaw: f32) -> f32 {
        let walk = super::camera_relative_direction(0.0, 1.0, yaw);
        eq_client_core::world_heading(walk.x.atan2(walk.z))
    }

    fn assert_walks_along(yaw: f32, heading: f32) {
        let walked = walked_heading(yaw);
        let error = (walked - heading).rem_euclid(512.0);
        assert!(error.min(512.0 - error) < 0.01, "{heading} -> {walked}");
    }

    /// The player placed at `heading`, as the server's position places them.
    fn placed_facing(heading: f32) -> bevy::prelude::Transform {
        use super::*;
        Transform::from_xyz(10.0, 2.0, -5.0).with_rotation(Quat::from_rotation_y(
            eq_client_core::render_heading(heading),
        ))
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies the pitch and zoom are left as chosen"
    )]
    fn the_camera_starts_behind_the_player_and_turns_behind_them_on_each_zone_entry() {
        use super::*;
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>();
        let camera = |app: &mut App| {
            let world = app.world_mut();
            let mut cameras = world.query::<&OrbitCamera>();
            let camera = cameras.single(world).unwrap();
            (camera.yaw, camera.pitch, camera.radius)
        };
        // Entering the world: the first zone, then the camera, as `setup_scene`
        // does them.
        app.world_mut()
            .run_system_once(|mut commands: Commands| {
                spawn_camera(
                    &mut commands,
                    placed_facing(100.0),
                    120.0,
                    ProjectionStyle::Perspective,
                );
            })
            .unwrap();
        let (yaw, pitch, radius) = camera(&mut app);
        assert_walks_along(yaw, 100.0);
        assert_eq!((pitch, radius), (CHASE_PITCH, 120.0));
        assert!((CHASE_PITCH - (-20.0_f32).to_radians()).abs() < f32::EPSILON);
        // The player has turned the camera and zoomed; a zone entry facing 400
        // turns it behind them again and keeps the rest.
        app.world_mut()
            .run_system_once(|mut cameras: Query<&mut OrbitCamera>| {
                let mut camera = cameras.single_mut().unwrap();
                camera.pitch = -0.9;
                camera.radius = 60.0;
            })
            .unwrap();
        app.world_mut()
            .run_system_once(|mut commands: Commands, mut scene: zone::Scene| {
                let zone = eq_client_assets::ZoneAsset {
                    regions: eq_client_assets::regions::ZoneRegions::default(),
                    short_name: "test".to_owned(),
                    primitives: Vec::new(),
                    textures: Vec::new(),
                    models: Vec::new(),
                    objects: Vec::new(),
                    collision: Vec::new(),
                };
                scene.enter(
                    &mut commands,
                    zone::Entry {
                        zone,
                        character: None,
                        placed: placed_facing(400.0),
                        body: PlayerBody {
                            feet_offset: 3.0,
                            height: 6.0,
                            size: 6.0,
                        },
                    }
                    .prepare(false),
                    true,
                    &mut eq_client_assets::regions::ZoneRegions::default(),
                );
            })
            .unwrap();
        app.update();
        let (yaw, pitch, radius) = camera(&mut app);
        assert_walks_along(yaw, 400.0);
        assert_eq!((pitch, radius), (-0.9, 60.0));
        // A server correction mid-zone moves the player, never the camera's turn.
        app.world_mut()
            .run_system_once(|mut commands: Commands, mut scene: zone::Scene| {
                scene.place(&mut commands, placed_facing(256.0), None);
            })
            .unwrap();
        app.update();
        assert_eq!(camera(&mut app), (yaw, -0.9, 60.0));
    }

    #[test]
    fn a_long_frame_never_eases_the_camera_past_the_way_clear() {
        use super::*;
        assert!((eased_distance(30.0, 12.0, 1.0 / 60.0) - 12.0).abs() < f32::EPSILON);
        for seconds in [0.0, 1.0 / 60.0, 0.1, 0.3, 5.0, 1.0e6] {
            let next = eased_distance(10.0, 40.0, seconds);
            assert!((10.0..=40.0).contains(&next), "{seconds}: {next}");
        }
        // About 95% of the way out in 0.3 s.
        let three_tenths = eased_distance(10.0, 40.0, 0.3);
        assert!((38.0..40.0).contains(&three_tenths), "{three_tenths}");
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies unchanged or explicitly assigned state"
    )]
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered integration scenario and its assertions together"
    )]
    fn camera_input_is_consumed_over_panels_and_while_unfocused() {
        use super::*;
        use bevy::input::mouse::MouseWheel;
        let mut app = App::new();
        app.init_resource::<chat::ChatState>()
            .init_resource::<items::ItemState>()
            .init_resource::<inventory::InventoryState>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<MouseMotion>()
            .add_message::<MouseWheel>()
            .init_resource::<windows::pointer::Wheel>()
            .init_resource::<options::OptionsState>()
            .init_resource::<Time>()
            .insert_resource(online::OnlineState::new(false))
            .add_systems(Update, (windows::pointer::wheel, orbit_camera).chain());
        let mut window = Window {
            focused: true,
            ..default()
        };
        window.set_physical_cursor_position(Some(bevy::math::DVec2::new(50.0, 50.0)));
        let window = app
            .world_mut()
            .spawn((window, bevy::window::PrimaryWindow))
            .id();
        // Layout-only HUD roots span the viewport but must not capture empty space.
        app.world_mut().spawn((
            hud::HudRoot,
            ComputedNode {
                size: Vec2::splat(2000.0),
                ..default()
            },
            UiGlobalTransform::default(),
        ));
        let panel = app
            .world_mut()
            .spawn((
                windows::Frame::default(),
                ComputedNode {
                    size: Vec2::splat(200.0),
                    ..default()
                },
                UiGlobalTransform::default(),
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                OrbitCamera {
                    focus: Vec3::ZERO,
                    yaw: 0.0,
                    pitch: -1.0,
                    radius: 100.0,
                    pulled: None,
                },
                Transform::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        let send_input = |app: &mut App| {
            app.world_mut().write_message(MouseMotion {
                delta: Vec2::new(20.0, 10.0),
            });
            app.world_mut().write_message(MouseWheel {
                unit: bevy::input::mouse::MouseScrollUnit::Line,
                phase: bevy::input::touch::TouchPhase::Moved,
                x: 0.0,
                y: 1.0,
                window,
            });
        };
        send_input(&mut app);
        app.update();
        let state = app.world().get::<OrbitCamera>(camera).unwrap();
        assert_eq!((state.yaw, state.pitch, state.radius), (0.0, -1.0, 100.0));
        // A hidden panel has zero layout size and no longer captures the pointer.
        app.world_mut().get_mut::<ComputedNode>(panel).unwrap().size = Vec2::ZERO;
        app.update();
        assert_eq!(app.world().get::<OrbitCamera>(camera).unwrap().yaw, 0.0);
        send_input(&mut app);
        app.update();
        let state = app.world().get::<OrbitCamera>(camera).unwrap();
        assert!(state.yaw < 0.0 && state.pitch < -1.0 && state.radius < 100.0);
        let previous = (state.yaw, state.pitch, state.radius);
        // The inventory toggle is a standalone button, not a window frame.
        let button = app
            .world_mut()
            .spawn((
                Button,
                InheritedVisibility::VISIBLE,
                ComputedNode {
                    size: Vec2::splat(200.0),
                    ..default()
                },
                UiGlobalTransform::default(),
            ))
            .id();
        send_input(&mut app);
        app.update();
        let state = app.world().get::<OrbitCamera>(camera).unwrap();
        assert_eq!((state.yaw, state.pitch, state.radius), previous);
        app.world_mut().despawn(button);
        app.update();
        let state = app.world().get::<OrbitCamera>(camera).unwrap();
        assert_eq!((state.yaw, state.pitch, state.radius), previous);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        send_input(&mut app);
        app.update();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        let state = app.world().get::<OrbitCamera>(camera).unwrap();
        assert_eq!((state.yaw, state.pitch, state.radius), previous);
        // The Mouse page's options: moving up looks the other way, and the
        // wheel no longer zooms.
        {
            let mut options = app.world_mut().resource_mut::<options::OptionsState>();
            options.options.invert_y = true;
            options.options.wheel_zoom = false;
        }
        send_input(&mut app);
        app.update();
        let state = app.world().get::<OrbitCamera>(camera).unwrap();
        assert!(state.pitch > previous.1);
        assert_eq!(state.radius, previous.2);
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies unchanged or explicitly assigned state"
    )]
    fn offline_falling_continues_without_input_but_never_runs_for_online_players() {
        use super::*;
        let geometry = [
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
        ];
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_millis(50));
        app.insert_resource(time)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<chat::ChatState>()
            .insert_resource(online::OnlineState::new(false))
            .insert_resource(Collision(Some(
                eq_client_core::movement::CollisionWorld::new(geometry).unwrap(),
            )))
            .add_systems(Update, move_player);
        app.world_mut().spawn((
            Window {
                focused: false,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        app.world_mut().spawn(OrbitCamera {
            focus: Vec3::ZERO,
            radius: 10.0,
            yaw: 0.0,
            pitch: 0.0,
            pulled: None,
        });
        let player = app
            .world_mut()
            .spawn((
                Player,
                PlayerBody {
                    feet_offset: 0.0,
                    height: 6.0,
                    size: 6.0,
                },
                Transform::from_xyz(0.0, 10.0, 0.0),
            ))
            .id();
        app.update();
        let first = app.world().get::<Transform>(player).unwrap().translation.y;
        app.update();
        let second = app.world().get::<Transform>(player).unwrap().translation.y;
        assert!(first < 10.0 && second < first);
        app.world_mut()
            .resource_mut::<online::OnlineState>()
            .enabled = true;
        app.update();
        assert_eq!(
            app.world().get::<Transform>(player).unwrap().translation.y,
            second
        );
    }

    use bevy::math::Vec3;

    use super::{
        bevy_texture_coordinates, camera_relative_direction, generate_mip_chain, triangle_height_at,
    };

    #[test]
    fn interpolates_height_inside_a_terrain_triangle() {
        let triangle = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 10.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
        ];

        assert_eq!(triangle_height_at(triangle, 5.0, 0.0), Some(5.0));
        assert_eq!(triangle_height_at(triangle, 11.0, 0.0), None);
    }

    #[test]
    fn movement_tracks_camera_yaw() {
        assert!(camera_relative_direction(0.0, 1.0, 0.0).abs_diff_eq(Vec3::NEG_Z, 0.000_001));
        assert!(
            camera_relative_direction(0.0, 1.0, std::f32::consts::FRAC_PI_2)
                .abs_diff_eq(Vec3::NEG_X, 0.000_001)
        );
        assert!(
            camera_relative_direction(1.0, 1.0, 0.0)
                .abs_diff_eq(Vec3::new(1.0, 0.0, -1.0).normalize(), 0.000_001)
        );
    }

    #[test]
    fn generates_complete_non_square_mip_chain() {
        let (pixels, levels) = generate_mip_chain(4, 2, vec![255; 4 * 2 * 4]);
        assert_eq!(levels, 3);
        assert_eq!(pixels.len(), (4 * 2 + 2 + 1) * 4);
    }

    #[test]
    fn converts_classic_wld_v_coordinates_for_bevy() {
        assert_eq!(
            bevy_texture_coordinates(vec![[0.25, 0.75], [-2.0, 3.0]]),
            vec![[0.25, -0.75], [-2.0, -3.0]]
        );
    }
}
