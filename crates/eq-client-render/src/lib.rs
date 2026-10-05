#![doc = "Bevy scene and camera support for renderer-independent EQ zone assets."]

use std::{path::PathBuf, sync::mpsc::Receiver};
use theme::Size;

mod abilities;
mod attention;
mod book_delete;
mod buffs;
mod character;
mod character_select;
mod chat;
mod coins;
mod combat;
mod confirm;
mod daylight;
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
mod logs;
mod map;
mod motion;
mod names;
mod navigation;
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
mod sheets;
mod skills;
mod skin;
mod skinned;
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

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::MouseMotion;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{Captured, Screenshot, save_to_disk};
use eq_client_assets::characters::CharacterAsset;
use eq_client_assets::{BlendOpacity, MaterialMode, ZoneAsset, ZonePrimitive};
use eq_client_core::{WorldPosition, WorldUpdate, render_position, world_position};
use image::{RgbaImage, imageops::FilterType};

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

pub use preview::Preview;

/// Where the viewer's world comes from.
pub enum Source {
    /// A server session: its news, and the channel for the player's requests.
    Online {
        /// What the session tells the client.
        updates: Receiver<WorldUpdate>,
        /// What the player asks the session to send.
        commands: std::sync::mpsc::SyncSender<eq_client_core::ClientCommand>,
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
struct TerrainSurface(Vec<[Vec3; 3]>);

#[derive(Resource)]
struct Collision(Option<eq_client_core::movement::CollisionWorld>);

#[derive(Component)]
struct Player;

#[derive(Component, Clone, Copy)]
struct PlayerBody {
    feet_offset: f32,
    height: f32,
}

#[derive(Component)]
struct SceneEntity;

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct OrbitCamera {
    focus: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
}

/// Opens a window and renders a loaded zone until the user closes it.
pub fn run(
    zone: ZoneAsset,
    character: Option<CharacterAsset>,
    config: ViewerConfig,
    source: Source,
) -> i32 {
    let screenshot = config.screenshot.clone();
    let steps = config.script.clone();
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
    let (updates, commands) = match source {
        Source::Online { updates, commands } => (Some(updates), Some(commands)),
        Source::Offline(preview) => (preview::install(&mut app, preview), None),
    };
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
    app.add_systems(Startup, setup_scene)
        .add_systems(Update, exit_log::close_requests);
    schedule(&mut app);
    navigation::install(&mut app);
    frame_limit::install(&mut app);
    attention::install(&mut app);
    if let Some(size) = window_size {
        window_size::install(&mut app, size);
    }
    loading::install(&mut app);
    install_overlays(&mut app);
    if let Some(steps) = steps {
        install_script(&mut app, steps, follow, (local_session, online));
    }
    if let Some(path) = screenshot {
        app.insert_resource(CaptureRequest {
            path,
            settle_timer: Timer::from_seconds(screenshot_after, TimerMode::Once),
        });
    }
    exit_status(&app.run())
}

/// Starts the presentation state (HUD, windows, chat, targeting, inventory,
/// trade, combat and motion) empty: every resource the windows keep, in one
/// list that the tests' app starts from too.
fn init_presentation(app: &mut App) {
    app.init_resource::<options::OptionsState>()
        .init_resource::<logs::ChatLog>()
        .init_resource::<hud::HudState>()
        .init_resource::<hud::action_bar::ActionRequests>()
        .init_resource::<combat::CombatState>()
        .init_resource::<trade::TradeState>()
        .init_resource::<escape::Escape>()
        .init_resource::<windows::pointer::Wheel>()
        .init_resource::<skin::UiSkin>()
        .init_resource::<hud::hotbar::Bindings>()
        .init_resource::<spellbook::BookView>()
        .init_resource::<spellbook::BookSelection>()
        .init_resource::<spellbook::BookHand>()
        .init_resource::<sheets::Sheets>()
        .init_resource::<skinned::Screens>()
        .init_resource::<skinned::CursorLook>()
        .init_resource::<skinned::Skinned>()
        .init_resource::<skinned::Tabs>()
        .init_resource::<skinned::looks::Looks>()
        .init_resource::<skinned::KeyFilter>()
        .init_resource::<chat::ChatState>()
        .init_resource::<chat::ChatLook>()
        .init_resource::<profile_files::Profile>()
        .init_resource::<notices::Lines>()
        .init_resource::<items::ItemState>()
        .init_resource::<inventory::InventoryState>()
        .init_resource::<motion::Controls>()
        .init_resource::<entities::NearbyEntities>()
        .init_resource::<names::NameTags>()
        .init_resource::<item_models::ItemLibrary>()
        .init_resource::<outfit::Wardrobe>()
        .init_resource::<windows::DragState>()
        .init_resource::<windows::Layouts>()
        .init_resource::<windows::Shown>()
        .init_resource::<windows::Stack>()
        .init_resource::<keys::KeyMap>()
        .init_resource::<keys::Typing>()
        .init_resource::<training::Chosen>()
        .init_resource::<raid::RaidChoice>()
        .init_resource::<confirm::Asked>()
        .init_resource::<reading::Page>()
        .init_resource::<map::MapView>();
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
fn install_script(
    app: &mut App,
    steps: Vec<script::Step>,
    follow: Option<(PathBuf, usize)>,
    (local_session, online): (bool, bool),
) {
    let script = match follow {
        Some((path, offset)) => script::Script::following(steps, path, offset),
        None => script::Script::new(steps),
    };
    app.insert_resource(script.local_session(local_session).offline_preview(!online));
    app.add_systems(PreUpdate, script::drive.after(bevy::ui::UiSystems::Focus));
}

/// Puts every frame's work in its stage, and orders within a stage the few
/// systems that depend on one another.
#[allow(
    clippy::too_many_lines,
    reason = "one declarative listing of every system in its stage"
)]
fn schedule(app: &mut App) {
    app.configure_sets(
        Update,
        (
            Stage::Receive,
            Stage::Scene,
            Stage::Typing,
            Stage::Route,
            Stage::Input,
            Stage::Present,
        )
            .chain(),
    )
    .add_systems(
        Update,
        (
            (online::receive, online::tick, daylight::update)
                .chain()
                .in_set(Stage::Receive),
            (
                entities::reconcile,
                entities::interpolate,
                doors::reconcile,
                ground::reconcile,
            )
                .chain()
                .in_set(Stage::Scene),
            (
                windows::pointer::wheel,
                // A scrollbar's press scrolls its box as the wheel does.
                skinned::scrollbar::scroll,
                navigation::update,
                chat::input,
                skinned::type_amount,
            )
                .chain()
                .in_set(Stage::Typing),
            escape::route.in_set(Stage::Route),
        ),
    )
    .add_systems(
        Update,
        (
            items::link_input,
            items::input,
            inventory::input,
            coins::input,
            inventory::colors::input,
            interact::input,
            target::input,
            (
                who::zone_list,
                whereabouts::answers,
                pet::window,
                group::window,
                raid::window,
            ),
            combat::input,
            trade::input,
            (
                give::buttons,
                give::inspect_theirs,
                training::buttons,
                confirm::buttons,
                reading::buttons,
                tradeskills::buttons,
                map::buttons,
                raid::buttons,
            ),
            (abilities::input, skinned::slash),
            hud::actions,
            (hud::hotbar::update, hud::hotbar::persist).chain(),
            hud::hotbar::item_actions,
            (
                spellbook::update,
                spellbook::book_clicks,
                spellbook::say_book_lines,
            ),
            (character_select::update, character_select::names),
            windows::input,
            move_player,
            motion::input,
        )
            .chain()
            .in_set(Stage::Input),
    )
    .add_systems(
        Update,
        (
            (
                inventory::update,
                inventory::feedback,
                inventory::scroll,
                (skinned::cursor_look, inventory::cursor::update).chain(),
                (chat::follow_options, chat::look, chat::refresh).chain(),
                items::update,
                items::scroll,
                target::update,
                combat::target_color,
                trade::present,
                trade::scroll,
                skinned::scroll_lists,
                motion::interpolate,
                orbit_camera,
                update_hud,
                // Opens the give, quantity and training windows before their
                // frames are drawn.
                (
                    give::window,
                    skinned::quantity,
                    training::window,
                    confirm::window,
                    reading::window,
                    tradeskills::world_window,
                    map::load,
                ),
            )
                .chain(),
            (
                resources::update,
                hud::update,
                hud::spell_details,
                hud::key_help,
                target::key_help,
                hud::hotbar::item_artwork,
                hud::hotbar::presentation,
                spellbook::scribe_presentation,
                buffs::update,
                (buffs::hover, buffs::skinned_details, buffs::short_window),
                (
                    spell_icons::update,
                    logs::write,
                    (
                        options::toggle,
                        options::persist,
                        options::choose,
                        options::tell_session,
                    )
                        .chain(),
                ),
                (outbox::show, inventory::say_refusals),
                (hud::action_bar::cast_window, hud::action_bar::update),
                skinned::frames,
                (skinned::apply, skinned::looks::load, skinned::looks::dress).chain(),
                skinned::show,
                skinned::buttons,
                (
                    skinned::contents,
                    skinned::theirs,
                    skinned::loot,
                    (
                        trade::fill_wares,
                        trade::picture,
                        items::icon,
                        spellbook::book_present,
                    ),
                    skinned::tabs,
                    abilities::present,
                    (
                        skinned::slide,
                        skinned::drop_downs,
                        skinned::light_choices,
                        skinned::show_sliders,
                        skinned::show_amount,
                        skinned::show_choices,
                        skinned::fill_lists,
                        training::fill,
                        skills::fill,
                        raid::fill,
                        confirm::show,
                        reading::show,
                        tradeskills::show,
                        map::draw,
                        skinned::scrollbar::place,
                        skinned::rewind,
                    ),
                ),
                skinned::close,
            )
                .chain(),
            (
                hud::hotbar::needs,
                abilities::needs,
                outbox::veil,
                outbox::grey_out,
                tooltip::show,
                outfit::dress,
                character::animate,
                target::marker::update,
                (names::request, names::update).chain(),
                schedule_screenshot,
                exit_after_screenshot,
            )
                .chain(),
        )
            .chain()
            .in_set(Stage::Present),
    );
}

/// The order of every frame's work. A system joins the stage for what it
/// does instead of naming the systems it must follow; within a stage, the
/// listing above orders the few that depend on one another.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Stage {
    /// The session's news reaches the world, and the world's clocks run.
    Receive,
    /// The world reaches the scene: spawns, doors and items on the ground.
    Scene,
    /// The chat takes the keyboard first, and the wheel goes to one surface.
    Typing,
    /// Escape and the window stack decide what a press belongs to.
    Route,
    /// Keys and clicks become requests and moves.
    Input,
    /// Windows, the HUD, the camera and the models show the world; refusals
    /// show once every input has had its say.
    Present,
}

/// Registers the startup work of the overlays and the windows' shared systems.
fn install_overlays(app: &mut App) {
    app.add_systems(Startup, tooltip::spawn);
    windows::register_layout(app);
    paperdoll::register(app);
    app.add_systems(
        Update,
        profile_files::follow
            .after(Stage::Receive)
            .before(Stage::Scene),
    );
    skin::register(app);
    app.add_systems(PostUpdate, chat::scroll.after(bevy::ui::UiSystems::Layout));
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
    };
    let player_position = Vec3::new(start_x, start_y + body.feet_offset, start_z);
    let default_radius = 120.0;
    let radius = settings
        .0
        .camera_distance
        .unwrap_or(default_radius)
        .clamp(20.0, 20_000.0);
    scene.enter(
        &mut commands,
        zone::Entry {
            zone,
            character,
            placed: Transform::from_translation(player_position),
            body,
        },
        settings.0.terrain_only,
        &mut online.regions,
    );
    spawn_lighting(&mut commands, &mut ambient_light);
    spawn_camera(
        &mut commands,
        player_position,
        radius,
        settings.0.projection,
    );
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
    let terrain =
        create_render_primitives(zone.primitives, &texture_images, meshes, materials, true);
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
    let [red, green, blue] = daylight::DAY_AMBIENT_COLOR;
    *ambient_light = GlobalAmbientLight {
        color: Color::srgb(red, green, blue),
        brightness: daylight::DAY_AMBIENT,
        affects_lightmapped_meshes: true,
    };
}

fn spawn_camera(
    commands: &mut Commands,
    player_position: Vec3,
    radius: f32,
    projection_style: ProjectionStyle,
) {
    let orbit = OrbitCamera {
        focus: player_position,
        radius,
        yaw: 0.75,
        pitch: -60.0_f32.to_radians(),
    };
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
    ));
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn schedule_screenshot(
    mut commands: Commands,
    time: Res<Time>,
    request: Option<ResMut<CaptureRequest>>,
    online: Res<online::OnlineState>,
) {
    // Online, the scene is ready once the player is in the world, or once the
    // character list shows.
    let admitted = online.world().connected() && online.world().player().is_some();
    if online.enabled && !admitted && online.selection.is_none() {
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

struct TextureImages {
    opaque: Handle<Image>,
    masked: Handle<Image>,
}

fn create_texture_images(
    textures: Vec<eq_client_assets::ZoneTexture>,
    images: &mut Assets<Image>,
) -> Vec<TextureImages> {
    textures
        .into_iter()
        .map(|texture| {
            let mut masked_pixels = texture.rgba8.clone();
            let color_key = texture.color_key.unwrap_or([0, 0, 0]);
            for pixel in masked_pixels.as_chunks_mut::<4>().0 {
                if pixel[..3] == color_key {
                    pixel[3] = 0;
                }
            }
            TextureImages {
                opaque: images.add(create_image(texture.width, texture.height, texture.rgba8)),
                masked: images.add(create_image(texture.width, texture.height, masked_pixels)),
            }
        })
        .collect()
}

fn create_image(width: u32, height: u32, pixels: Vec<u8>) -> Image {
    let (pixels, mip_level_count) = generate_mip_chain(width, height, pixels);
    let base_level_len = width as usize * height as usize * 4;
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels[..base_level_len].to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(pixels);
    image.texture_descriptor.mip_level_count = mip_level_count;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

fn generate_mip_chain(width: u32, height: u32, pixels: Vec<u8>) -> (Vec<u8>, u32) {
    let mut level = RgbaImage::from_raw(width, height, pixels)
        .expect("decoded RGBA texture dimensions match its pixel buffer");
    let mut chain = level.as_raw().clone();
    let mut level_count = 1;
    while level.width() > 1 || level.height() > 1 {
        let next_width = (level.width() / 2).max(1);
        let next_height = (level.height() / 2).max(1);
        level = image::imageops::resize(&level, next_width, next_height, FilterType::Triangle);
        chain.extend_from_slice(level.as_raw());
        level_count += 1;
    }
    (chain, level_count)
}

fn create_render_primitives(
    primitives: Vec<ZonePrimitive>,
    texture_images: &[TextureImages],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    unlit: bool,
) -> Vec<(Handle<Mesh>, Handle<StandardMaterial>)> {
    primitives
        .into_iter()
        .filter(|primitive| !primitive.indices.is_empty() && !primitive.positions.is_empty())
        .map(|primitive| {
            let texture = primitive
                .texture
                .and_then(|index| texture_images.get(index));
            let material = create_material(primitive.material_mode, texture, materials, unlit);
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            );
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, primitive.positions);
            if !primitive.normals.is_empty() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, primitive.normals);
            }
            if !primitive.texture_coordinates.is_empty() {
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_UV_0,
                    bevy_texture_coordinates(primitive.texture_coordinates),
                );
            }
            mesh.insert_indices(Indices::U32(primitive.indices));
            (meshes.add(mesh), material)
        })
        .collect()
}

/// Converts classic WLD's downward-growing V axis to Bevy's texture orientation.
fn bevy_texture_coordinates(coordinates: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    coordinates.into_iter().map(|[u, v]| [u, -v]).collect()
}

fn create_material(
    mode: MaterialMode,
    texture: Option<&TextureImages>,
    materials: &mut Assets<StandardMaterial>,
    unlit: bool,
) -> Handle<StandardMaterial> {
    let (image, color, alpha_mode) = match mode {
        MaterialMode::Opaque => (
            texture.map(|images| images.opaque.clone()),
            Color::WHITE,
            AlphaMode::Opaque,
        ),
        MaterialMode::Masked => (
            texture.map(|images| images.masked.clone()),
            Color::WHITE,
            AlphaMode::Mask(0.5),
        ),
        MaterialMode::Blended(opacity) => {
            let alpha = match opacity {
                BlendOpacity::Quarter => 0.25,
                BlendOpacity::Half => 0.5,
                BlendOpacity::ThreeQuarters => 0.75,
            };
            (
                texture.map(|images| images.opaque.clone()),
                Color::srgba(1.0, 1.0, 1.0, alpha),
                AlphaMode::Blend,
            )
        }
        MaterialMode::Additive => (
            texture.map(|images| images.masked.clone()),
            Color::WHITE,
            AlphaMode::Add,
        ),
    };
    materials.add(StandardMaterial {
        base_color: color,
        base_color_texture: image,
        alpha_mode,
        unlit,
        perceptual_roughness: 0.95,
        double_sided: true,
        cull_mode: None,
        ..default()
    })
}

/// Where a zone object sits; the assets crate already placed it in the
/// renderer's frame.
fn object_transform(object: &eq_client_assets::ZoneObject) -> Transform {
    Transform {
        translation: Vec3::from_array(object.translation),
        rotation: Quat::from_array(object.rotation),
        scale: Vec3::from_array(object.scale),
    }
}

/// The zone's water and lava as its regions mark them, which every walk keeps
/// out of.
struct ZoneLiquids(eq_client_assets::regions::ZoneRegions);

impl eq_client_core::movement::Liquids for ZoneLiquids {
    fn liquid_at(&self, point: Vec3) -> Option<eq_client_core::movement::Liquid> {
        self.0.liquid_at(point.to_array()).map(movement_liquid)
    }

    fn passes_through(
        &self,
        from: Vec3,
        to: Vec3,
        liquid: eq_client_core::movement::Liquid,
    ) -> bool {
        use eq_client_assets::regions::Liquid;
        let liquid = match liquid {
            eq_client_core::movement::Liquid::Water => Liquid::Water,
            eq_client_core::movement::Liquid::Lava => Liquid::Lava,
        };
        self.0
            .passes_through(from.to_array(), to.to_array(), liquid)
    }
}

/// A liquid the zone's regions mark, as movement knows it.
fn movement_liquid(liquid: eq_client_assets::regions::Liquid) -> eq_client_core::movement::Liquid {
    use eq_client_assets::regions::Liquid;
    match liquid {
        Liquid::Water => eq_client_core::movement::Liquid::Water,
        Liquid::Lava => eq_client_core::movement::Liquid::Lava,
    }
}

/// Includes solid terrain and transformed object geometry, including hidden boundaries,
/// and the zone's water and lava.
fn build_collision(zone: &ZoneAsset) -> Option<eq_client_core::movement::CollisionWorld> {
    let mut triangles = zone.collision.clone();
    for object in &zone.objects {
        let Some(model) = zone.models.get(object.model) else {
            continue;
        };
        let transform = object_transform(object).to_matrix();
        triangles.extend(model.collision.iter().map(|triangle| {
            triangle.map(|p| transform.transform_point3(Vec3::from_array(p)).to_array())
        }));
    }
    match eq_client_core::movement::CollisionWorld::new(triangles) {
        Ok(world) if zone.regions.liquid_region_count() == 0 => Some(world),
        Ok(world) => Some(world.with_liquids(ZoneLiquids(zone.regions.clone()))),
        Err(error) => {
            warn!("Collision unavailable: {error}");
            None
        }
    }
}

impl TerrainSurface {
    fn from_primitives(primitives: &[ZonePrimitive]) -> Self {
        let triangles = primitives
            .iter()
            .flat_map(|primitive| {
                primitive
                    .indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .filter_map(|indices| {
                        Some([
                            Vec3::from_array(*primitive.positions.get(indices[0] as usize)?),
                            Vec3::from_array(*primitive.positions.get(indices[1] as usize)?),
                            Vec3::from_array(*primitive.positions.get(indices[2] as usize)?),
                        ])
                    })
            })
            .collect();
        Self(triangles)
    }

    fn height_below(&self, x: f32, z: f32, ceiling: f32) -> Option<f32> {
        self.0
            .iter()
            .filter_map(|triangle| triangle_height_at(*triangle, x, z))
            .filter(|height| *height <= ceiling)
            .max_by(f32::total_cmp)
    }

    fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        self.0
            .iter()
            .filter_map(|triangle| triangle_height_at(*triangle, x, z))
            .max_by(f32::total_cmp)
    }
}

fn triangle_height_at(triangle: [Vec3; 3], position_x: f32, position_z: f32) -> Option<f32> {
    let [vertex_a, vertex_b, vertex_c] = triangle;
    let denominator = (vertex_b.z - vertex_c.z) * (vertex_a.x - vertex_c.x)
        + (vertex_c.x - vertex_b.x) * (vertex_a.z - vertex_c.z);
    if denominator.abs() < 0.001 {
        return None;
    }
    let first = ((vertex_b.z - vertex_c.z) * (position_x - vertex_c.x)
        + (vertex_c.x - vertex_b.x) * (position_z - vertex_c.z))
        / denominator;
    let second = ((vertex_c.z - vertex_a.z) * (position_x - vertex_c.x)
        + (vertex_a.x - vertex_c.x) * (position_z - vertex_c.z))
        / denominator;
    let third = 1.0 - first - second;
    if first < -0.001 || second < -0.001 || third < -0.001 {
        return None;
    }
    Some(first * vertex_a.y + second * vertex_b.y + third * vertex_c.y)
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

fn camera_relative_direction(horizontal: f32, vertical: f32, yaw: f32) -> Vec3 {
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(-forward.z, 0.0, forward.x);
    (right * horizontal + forward * vertical).normalize_or_zero()
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

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn orbit_camera(
    collision: Option<Res<Collision>>,
    chat: Res<chat::ChatState>,
    items: Res<items::ItemState>,
    inventory: Res<inventory::InventoryState>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    (wheel, options): (Res<windows::pointer::Wheel>, Res<options::OptionsState>),
    online: Res<online::OnlineState>,
    mut cameras: Query<(&mut OrbitCamera, &mut Transform, Option<&mut Projection>)>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    ui: windows::pointer::PointerUi,
) {
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
            &camera,
            collision.as_ref().and_then(|world| world.0.as_ref()),
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

fn orbit_transform(camera: &OrbitCamera) -> Transform {
    let rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);
    let position = camera.focus + rotation * Vec3::new(0.0, 0.0, camera.radius);
    Transform::from_translation(position).looking_at(camera.focus, Vec3::Y)
}

/// Keeps the camera on the player's side of roofs and walls without changing chosen zoom.
fn unobstructed_orbit(
    camera: &OrbitCamera,
    collision: Option<&eq_client_core::movement::CollisionWorld>,
) -> Transform {
    let mut pose = orbit_transform(camera);
    let offset = pose.translation - camera.focus;
    let distance = offset.length();
    if distance > 0.001
        && let Some(hit) = collision
            .and_then(|world| world.ray_distance(camera.focus, offset / distance, distance))
    {
        pose.translation = camera.focus + offset / distance * (hit - 0.5).max(0.01);
    }
    pose
}

#[cfg(test)]
mod tests {
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
        };
        let requested = orbit_transform(&camera);
        let inside = unobstructed_orbit(&camera, Some(&roof));
        assert!(inside.translation.y < 8.0);
        assert!(
            (inside.translation.length() - (8.0 / 60.0_f32.to_radians().sin() - 0.5)).abs() < 0.001
        );
        assert_eq!(inside.rotation, requested.rotation);
        assert_eq!(camera.radius, 40.0);
        assert_eq!(unobstructed_orbit(&camera, None), requested);
        let outside = OrbitCamera {
            focus: Vec3::new(200.0, 0.0, 200.0),
            ..camera
        };
        assert_eq!(
            unobstructed_orbit(&outside, Some(&roof)),
            orbit_transform(&outside)
        );
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
        });
        let player = app
            .world_mut()
            .spawn((
                Player,
                PlayerBody {
                    feet_offset: 0.0,
                    height: 6.0,
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
