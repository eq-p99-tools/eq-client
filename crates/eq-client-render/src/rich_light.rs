//! Rich Lighting ([`Fix::RichLighting`], off by default): this client's own
//! look for every zone, in place of the engine's (`lighting`, `zone_light`,
//! `docs/zone-lighting.md`). Nothing here is set per zone: it works from each
//! zone's own placed lights, baked light and ambient, on any installed
//! client.
//!
//! - Zone surfaces draw with a lit material: the vertex colour's light (the
//!   base) plus Bevy's point lights per pixel, with a soft specular
//!   highlight.
//! - Every placed light is a point light, not the first three of a mesh's
//!   list, with a smooth falloff, so a lamp's pool follows the floor rather
//!   than the edges of its triangles. The nearest are drawn, two of them
//!   casting shadows.
//! - The carried lights follow their bearers, the first casting shadows.
//! - Warm lights flicker a little, as a flame does.
//! - The camera draws in high dynamic range with a soft glow.
//!
//! Without a sky, the base is the engine's base with a fill and the wash
//! ([`base`]), and a tone curve rolls a bright pool off rather than clipping
//! it to flat yellow. With a sky, the base is the plain texture as before,
//! and the lights show only as the night darkens ([`outdoor_scale`]).
//!
//! None of this is the original's; it exists because Adam asked for better
//! lighting (11-24), not for parity.
use super::{
    OrbitCamera, Player, PlayerBody,
    entities::NearbyEntities,
    lighting::{self, SceneLights},
    online::OnlineState,
    options::OptionsState,
    zone_light::{self, BaseInputs, PassLight},
};
use bevy::{
    asset::embedded_asset,
    camera::{Exposure, Hdr},
    core_pipeline::tonemapping::Tonemapping,
    pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin},
    post_process::bloom::Bloom,
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use std::{
    collections::{HashMap, HashSet},
    ops::Neg,
};

/// Rich Lighting's look, in one place: the defaults are the shipped look;
/// the capture probe overrides them to compare.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(super) struct Tuning {
    /// How much of the zone's ambient every vertex takes on top of the
    /// engine's base, whatever its alpha: a fill, so corners the lamps miss
    /// keep a little of the room's colour.
    pub fill: f32,
    /// How strongly the placed lights near a vertex tint it ([`base`]).
    pub wash: f32,
    /// How many radii a light's wash reaches.
    pub wash_span: f32,
    /// A placed light's strength against the engine's slot 0 and 1 term
    /// (0.05 R² / d²).
    pub lamp: f32,
    /// Outdoors, the lights' strength against that, under the night.
    pub outdoor: f32,
    /// A carried light's strength against the same term.
    pub carried: f32,
    /// How far a placed light's colour moves toward a warm white, 0 to 1:
    /// the authored colours are strong, and many of them add up.
    pub whiten: f32,
    /// How far past its radius a light reaches before its smooth falloff
    /// ends.
    pub reach: f32,
    /// How many of the placed lights nearest the camera are drawn.
    pub lamps_drawn: usize,
    /// How many of the placed lights nearest the camera cast shadows,
    /// besides the first carried light.
    pub shadowed_lamps: usize,
    /// Nothing nearer a placed light than this casts its shadow: the
    /// brazier or sconce the flame sits in.
    pub lamp_shadow_near: f32,
    /// How far a warm light's strength wanders either way.
    pub flicker: f32,
    /// The surfaces' roughness and reflectance: a soft sheen from the lamps.
    pub roughness: f32,
    pub reflectance: f32,
    /// The soft ceiling on the lights' own brightness ([`BaseLight`]).
    pub light_cap: f32,
    /// How much of the first carried light casts shadows; the rest lights
    /// through, so its shadows are soft, never black.
    pub shadow_strength: f32,
    /// Whether the first carried light casts shadows at all.
    pub own_shadow: bool,
    /// Whether the camera glows (bloom, on a high dynamic range target).
    pub bloom: bool,
    /// The glow's strength (Bevy's natural bloom is 0.15).
    pub glow: f32,
    /// The camera's tone curve.
    pub tone: Tonemapping,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            fill: 0.12,
            wash: 0.6,
            wash_span: 2.0,
            lamp: 0.5,
            outdoor: 0.3,
            carried: 1.0,
            whiten: 0.3,
            reach: 1.2,
            lamps_drawn: 48,
            shadowed_lamps: 0,
            lamp_shadow_near: 4.0,
            flicker: 0.10,
            roughness: 0.65,
            reflectance: 0.3,
            glow: 0.10,
            light_cap: 1.5,
            shadow_strength: 0.6,
            own_shadow: true,
            bloom: true,
            tone: Tonemapping::ReinhardLuminance,
        }
    }
}

const WARM_WHITE: [f32; 3] = [1.0, 0.86, 0.66];

/// A zone surface lit per pixel (`rich_light.wgsl`).
pub(super) type RichMaterial = ExtendedMaterial<StandardMaterial, BaseLight>;

/// The rich material's own values. Bindless where the GPU allows, as
/// Bevy's standard material is, so the zone's thousands of surfaces draw
/// in batches rather than one by one (each surface keeps its own material).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[data(50, BaseLightUniform, binding_array(101))]
#[bindless(index_table(range(50..51), binding(100)))]
pub(super) struct BaseLight {
    /// The base light's tint.
    tint: LinearRgba,
    /// x: how far the lights' own brightness may go, softly: a light is
    /// drawn as L / (1 + L / x) by its luminance, so a torch against a wall
    /// brightens the wall without washing its texture out.
    limits: Vec4,
}

/// [`BaseLight`] as the shader reads it (`rich_light.wgsl`).
#[derive(Clone, Copy, Default, ShaderType)]
pub(super) struct BaseLightUniform {
    tint: Vec4,
    limits: Vec4,
}

impl From<&BaseLight> for BaseLightUniform {
    fn from(light: &BaseLight) -> Self {
        Self {
            tint: light.tint.to_vec4(),
            limits: light.limits,
        }
    }
}

impl MaterialExtension for BaseLight {
    fn fragment_shader() -> ShaderRef {
        "embedded://eq_client_render/rich_light.wgsl".into()
    }
}

/// A zone surface drawn by its rich material, keeping the plain material it
/// returns to.
#[derive(Component)]
pub(super) struct Parked(pub(super) Handle<StandardMaterial>);

/// A placed light drawn as a point light.
#[derive(Component)]
pub(super) struct Lamp {
    intensity: f32,
    phase: f32,
    flickers: bool,
}

/// One of the carried lights, by its place in the list.
#[derive(Component)]
pub(super) struct Carried(usize);

#[derive(Resource, Default)]
pub(super) struct RichState {
    active: bool,
    /// The zone has no sky: the base holds all its light.
    interior: bool,
    /// The zone the materials below belong to.
    zone: String,
    lit: HashSet<AssetId<Mesh>>,
    materials: HashMap<AssetId<StandardMaterial>, Handle<RichMaterial>>,
    carried: Vec<PassLight>,
    /// The lights' soft ceiling now, and as the materials were last given
    /// it: outdoors it rises as the night's grading darkens the frame, so
    /// the ceiling holds once graded.
    cap: f32,
    applied_cap: f32,
}

impl Tuning {
    /// The look with `name=value` pairs applied, comma separated: for
    /// comparing looks and their cost (`EQ_RICH_TUNE`, the capture probe).
    /// Unknown names and values are skipped with a warning.
    pub(super) fn with(mut self, pairs: &str) -> Self {
        for pair in pairs.split(',').filter(|pair| !pair.trim().is_empty()) {
            let Some((name, value)) = pair.split_once('=') else {
                warn!("Rich Lighting tuning: no value in {pair:?}");
                continue;
            };
            let (name, value) = (name.trim(), value.trim());
            let number = value.parse::<f32>().ok();
            let count = value.parse::<usize>().ok();
            let flag = match value {
                "1" | "on" | "true" => Some(true),
                "0" | "off" | "false" => Some(false),
                _ => None,
            };
            let set = match name {
                "fill" => number.map(|value| self.fill = value),
                "wash" => number.map(|value| self.wash = value),
                "span" => number.map(|value| self.wash_span = value),
                "lamp" => number.map(|value| self.lamp = value),
                "carried" => number.map(|value| self.carried = value),
                "outdoor" => number.map(|value| self.outdoor = value),
                "whiten" => number.map(|value| self.whiten = value),
                "reach" => number.map(|value| self.reach = value),
                "drawn" => count.map(|value| self.lamps_drawn = value),
                "shadowed" => count.map(|value| self.shadowed_lamps = value),
                "near" => number.map(|value| self.lamp_shadow_near = value),
                "flicker" => number.map(|value| self.flicker = value),
                "roughness" => number.map(|value| self.roughness = value),
                "reflectance" => number.map(|value| self.reflectance = value),
                "glow" => number.map(|value| self.glow = value),
                "cap" => number.map(|value| self.light_cap = value),
                "shadow" => number.map(|value| self.shadow_strength = value),
                "own_shadow" => flag.map(|value| self.own_shadow = value),
                "bloom" => flag.map(|value| self.bloom = value),
                "tone" => match value {
                    "none" => Some(Tonemapping::None),
                    "reinhard" => Some(Tonemapping::ReinhardLuminance),
                    "boring" => Some(Tonemapping::SomewhatBoringDisplayTransform),
                    "blender" => Some(Tonemapping::BlenderFilmic),
                    "aces" => Some(Tonemapping::AcesFitted),
                    _ => None,
                }
                .map(|tone| self.tone = tone),
                _ => None,
            };
            if set.is_none() {
                warn!("Rich Lighting tuning: can't use {name}={value}");
            }
        }
        self
    }
}

pub(super) fn register(app: &mut App) {
    embedded_asset!(app, "rich_light.wgsl");
    app.add_plugins(MaterialPlugin::<RichMaterial>::default())
        .init_resource::<RichState>()
        .insert_resource(std::env::var("EQ_RICH_TUNE").map_or_else(
            |_| Tuning::default(),
            |pairs| Tuning::default().with(&pairs),
        ))
        // Half Bevy's default: the shadows' edges blur over twice the angle,
        // and each costs a quarter of the drawing.
        .insert_resource(bevy::light::PointLightShadowMap { size: 512 })
        .add_systems(Startup, room_for_lamps);
    if std::env::var_os("EQ_FRAME_LOG").is_some() {
        app.add_systems(Last, frame_log);
    }
}

/// Starts Bevy's light cluster lists big enough for a zone's lamps, which
/// overflow the default list in Erudin Palace: Bevy grows it, but draws a
/// few frames with lights missing first.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn room_for_lamps(settings: Option<ResMut<bevy::light::cluster::GlobalClusterSettings>>) {
    const INDICES: usize = 1 << 18;
    if let Some(mut settings) = settings
        && let Some(gpu) = settings.gpu_clustering.as_mut()
    {
        gpu.initial_index_list_capacity = gpu.initial_index_list_capacity.max(INDICES);
    }
}

/// Logs the frame times every five seconds (`EQ_FRAME_LOG`): the mean, the
/// slowest frame, and what Rich Lighting draws, for measuring its cost.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn frame_log(
    time: Res<Time<Real>>,
    state: Res<RichState>,
    lamps: Query<&Visibility, With<Lamp>>,
    carried: Query<&PointLight, With<Carried>>,
    drawn: Query<(), With<MeshMaterial3d<RichMaterial>>>,
    mut window: Local<(f32, f32, u32)>,
) {
    let delta = time.delta_secs();
    let (spent, worst, frames) = &mut *window;
    *spent += delta;
    *worst = worst.max(delta);
    *frames += 1;
    if *spent < 5.0 {
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
    #[allow(clippy::cast_precision_loss)] // A frame count.
    let mean = *spent / *frames as f32;
    info!(
        "frame log: {frames} frames, {:.1} fps, mean {:.2} ms, worst {:.2} ms; rich {} interior {}, lamps {shown}/{}, carried {} ({shadowed} shadowed), rich surfaces {}",
        1.0 / mean,
        mean * 1000.0,
        *worst * 1000.0,
        state.active,
        state.interior,
        lamps.iter().count(),
        carried.iter().count(),
        drawn.iter().count(),
    );
    *window = (0.0, 0.0, 0);
}

/// The vertex colour's light with Rich Lighting: the engine's base plus
/// the fill. The point lights come per pixel.
///
/// A vertex at a known place also takes the wash: each placed light near
/// it tints it by the light's own colour, fading smoothly to nothing at
/// [`Tuning::wash_span`] radii, and overlapping lights give their mean. It keeps the rooms' colours (the palace's
/// blues and greens), which the engine gets from each mesh's third light
/// at full strength, without that light's hard edge where meshes meet.
pub(super) fn base(
    baked: [u8; 4],
    inputs: BaseInputs,
    at: Option<[f32; 3]>,
    near: &[PassLight],
    tuning: &Tuning,
) -> [f32; 3] {
    let base = zone_light::base_light(baked, inputs);
    // The lights' colours, each by its fade, averaged where they overlap
    // (a weight over 1) so a room of many lamps takes their mean hue
    // rather than white.
    let mut wash = [0.0_f32; 3];
    let mut weight = 0.0_f32;
    if let Some(at) = at {
        for light in near {
            let span = light.radius * tuning.wash_span;
            let squared: f32 = (0..3)
                .map(|axis| (light.position[axis] - at[axis]).powi(2))
                .sum();
            let fade = (1.0 - squared / (span * span)).max(0.0).powi(2);
            weight += fade;
            for channel in 0..3 {
                wash[channel] += light.rgb[channel] * fade;
            }
        }
    }
    let wash = wash.map(|channel| channel / weight.max(1.0));
    // The inputs hold the ambient halved.
    std::array::from_fn(|channel| {
        base[channel] + tuning.fill * inputs.ambient[channel] * 2.0 + tuning.wash * wash[channel]
    })
}

/// The placed lights whose wash can reach a box.
pub(super) fn near(
    placed: &[eq_client_assets::lighting::PlacedLight],
    bounds: ([f32; 3], [f32; 3]),
    tuning: &Tuning,
) -> Vec<PassLight> {
    placed
        .iter()
        .map(|light| PassLight {
            position: light.position,
            radius: light.radius,
            rgb: light.rgb.map(|channel| channel * light.level),
        })
        .filter(|light| {
            let span = light.radius * tuning.wash_span;
            light.distance_squared(bounds) <= span * span
        })
        .collect()
}

/// A point light's intensity for the engine's 0.05 R² / d² term: Bevy
/// lights a rough surface by colour × I / (4π) × 1/π × exposure / d².
fn intensity(radius: f32, strength: f32) -> f32 {
    let pi = std::f32::consts::PI;
    strength * 0.05 * radius * radius * 4.0 * pi * pi / Exposure::default().exposure()
}

fn warm(rgb: [f32; 3]) -> bool {
    rgb[0] > rgb[2] * 1.2
}

fn colour(rgb: [f32; 3]) -> Color {
    let [red, green, blue] = rgb.map(|channel| channel.clamp(0.0, 1.0));
    Color::srgb(red, green, blue)
}

/// Outdoors, how strong a light is drawn under the night's exposure
/// (`exposure`, in stops; 0 by day): nothing by day, when the sun drowns it,
/// and by night strong enough that, once the night's grading darkens the
/// frame, it keeps 1 - 2^exposure of its strength while the world around it
/// darkens.
fn outdoor_scale(exposure: f32) -> f32 {
    (exposure.min(0.0).neg().exp2() - 1.0).max(0.0)
}

/// A flame's wander, -1 to 1, smooth in time.
fn flicker(seconds: f32, phase: f32) -> f32 {
    0.5 * (seconds * 7.3 + phase).sin()
        + 0.3 * (seconds * 13.1 + phase * 2.0).sin()
        + 0.2 * (seconds * 23.7 + phase * 3.0).sin()
}

type Positions<'w, 's> = (
    Query<'w, 's, &'static Transform, (With<OrbitCamera>, Without<Player>)>,
    Query<'w, 's, (&'static Transform, &'static PlayerBody), (With<Player>, Without<OrbitCamera>)>,
    Query<'w, 's, &'static Transform, (Without<OrbitCamera>, Without<Player>)>,
);

/// Whether Rich Lighting draws the zone now, and the carried lights.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system parameters.
pub(super) fn gather(
    online: Res<OnlineState>,
    options: Res<OptionsState>,
    scene: Option<Res<SceneLights>>,
    nearby: Res<NearbyEntities>,
    (eye, player, transforms): Positions,
    grading: Query<&bevy::render::view::ColorGrading, With<OrbitCamera>>,
    tuning: Res<Tuning>,
    mut state: ResMut<RichState>,
) {
    let world = online.world();
    let qol = &options.options.qol;
    let clearly = qol.on(eq_client_core::qol::Fix::ClearView);
    // Every zone, with a sky or without, on every installed client.
    state.active = !clearly
        && qol.on(eq_client_core::qol::Fix::RichLighting)
        && lighting::in_scene(scene.as_deref(), world);
    state.interior = state.active && world.sky().is_some_and(|sky| sky.sky == 0);
    let night = grading
        .single()
        .map_or(0.0, |grading| grading.global.exposure.min(0.0));
    state.cap = if state.interior {
        tuning.light_cap
    } else {
        tuning.light_cap * night.neg().exp2()
    };
    let zone = scene.as_ref().map_or("", |scene| scene.zone.as_str());
    if state.zone != zone {
        zone.clone_into(&mut state.zone);
        state.materials.clear();
        state.lit = scene
            .iter()
            .flat_map(|scene| {
                let lit = scene
                    .meshes
                    .iter()
                    .filter(|lit| !lit.additive)
                    .map(|lit| lit.mesh.id());
                lit.chain(scene.plain.iter().copied())
            })
            .collect();
    }
    state.carried = match (state.active, player.single()) {
        (true, Ok((own, body))) => {
            let eye = eye
                .single()
                .map_or(own.translation, |camera| camera.translation);
            lighting::carried_lights(
                world,
                &nearby,
                eye,
                (own.translation, body.height),
                &transforms,
            )
        }
        _ => Vec::new(),
    };
}

/// Draws the zone's surfaces by their rich materials while Rich Lighting is
/// on, and by their plain ones otherwise.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)] // Bevy system parameters.
pub(super) fn swap(
    mut commands: Commands,
    mut state: ResMut<RichState>,
    tuning: Res<Tuning>,
    // Water surfaces keep their own draw: Better Water dresses them, and
    // the two swaps must not take the same surface.
    plain: Query<
        (Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>),
        (Without<Parked>, Without<crate::water::WaterSurface>),
    >,
    parked: Query<(Entity, &Parked)>,
    standard: Res<Assets<StandardMaterial>>,
    mut rich: ResMut<Assets<RichMaterial>>,
) {
    if !state.active {
        for (entity, Parked(material)) in &parked {
            commands
                .entity(entity)
                .insert(MeshMaterial3d(material.clone()))
                .remove::<(Parked, MeshMaterial3d<RichMaterial>)>();
        }
        return;
    }
    let state = &mut *state;
    if (state.cap - state.applied_cap).abs() > state.cap * 0.02 {
        state.applied_cap = state.cap;
        for handle in state.materials.values() {
            if let Some(mut material) = rich.get_mut(handle) {
                material.extension.limits.x = state.cap;
            }
        }
    }
    for (entity, mesh, material) in &plain {
        if !state.lit.contains(&mesh.id()) {
            continue;
        }
        let handle = if let Some(handle) = state.materials.get(&material.id()) {
            handle.clone()
        } else {
            {
                let Some(source) = standard.get(&material.0) else {
                    continue;
                };
                let handle = rich.add(RichMaterial {
                    base: StandardMaterial {
                        unlit: false,
                        perceptual_roughness: tuning.roughness,
                        reflectance: tuning.reflectance,
                        metallic: 0.0,
                        ..source.clone()
                    },
                    extension: BaseLight {
                        tint: LinearRgba::WHITE,
                        limits: Vec4::new(state.cap, 0.0, 0.0, 0.0),
                    },
                });
                state.materials.insert(material.id(), handle.clone());
                handle
            }
        };
        commands
            .entity(entity)
            .insert((MeshMaterial3d(handle), Parked(material.0.clone())))
            .remove::<MeshMaterial3d<StandardMaterial>>();
    }
}

/// Keeps an animated surface's rich material on its plain material's
/// current frame.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(super) fn follow_frames(
    surfaces: Query<
        (&Parked, &MeshMaterial3d<RichMaterial>),
        With<crate::gpu_assets::TerrainAnimation>,
    >,
    standard: Res<Assets<StandardMaterial>>,
    mut rich: ResMut<Assets<RichMaterial>>,
) {
    for (Parked(plain), drawn) in &surfaces {
        let Some(frame) = standard
            .get(plain)
            .map(|plain| plain.base_color_texture.clone())
        else {
            continue;
        };
        if rich
            .get(&drawn.0)
            .is_some_and(|drawn| drawn.base.base_color_texture != frame)
            && let Some(mut drawn) = rich.get_mut(&drawn.0)
        {
            drawn.base.base_color_texture = frame;
        }
    }
}

/// Places the zone's lamps and the carried lights while Rich Lighting is
/// on, flickering the warm ones, and takes them away otherwise.
#[allow(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::too_many_lines
)] // Bevy system parameters; the lamps, then the carried lights.
pub(super) fn lamps(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<RichState>,
    tuning: Res<Tuning>,
    scene: Option<Res<SceneLights>>,
    eye: Query<
        (&Transform, Option<&bevy::render::view::ColorGrading>),
        (With<OrbitCamera>, Without<Lamp>, Without<Carried>),
    >,
    mut placed: Query<
        (Entity, &Lamp, &Transform, &mut PointLight, &mut Visibility),
        Without<Carried>,
    >,
    mut carried: Query<(Entity, &Carried, &mut Transform, &mut PointLight), Without<Lamp>>,
    mut placed_for: Local<Option<String>>,
) {
    let scene = scene.filter(|_| state.active);
    let Some(scene) = scene else {
        *placed_for = None;
        for (entity, ..) in &placed {
            commands.entity(entity).despawn();
        }
        for (entity, ..) in &carried {
            commands.entity(entity).despawn();
        }
        return;
    };
    let seconds = time.elapsed_secs();
    if placed_for.as_deref() != Some(scene.zone.as_str()) {
        *placed_for = Some(scene.zone.clone());
        for (entity, ..) in &placed {
            commands.entity(entity).despawn();
        }
        for (index, light) in scene.placed.iter().enumerate() {
            let rgb: [f32; 3] = std::array::from_fn(|channel| {
                let authored = light.rgb[channel] * light.level;
                authored + (WARM_WHITE[channel] * light.level - authored) * tuning.whiten
            });
            let intensity = intensity(light.radius, tuning.lamp);
            #[allow(clippy::cast_precision_loss)] // A phase.
            let phase = index as f32 * 2.399;
            commands.spawn((
                Lamp {
                    intensity,
                    phase,
                    flickers: warm(rgb),
                },
                Transform::from_translation(Vec3::from_array(light.position)),
                PointLight {
                    color: colour(rgb),
                    // Set from the next frame on, with the flicker and the
                    // outdoor scale.
                    intensity: 0.0,
                    range: light.radius * tuning.reach,
                    radius: 0.5,
                    shadow_map_near_z: tuning.lamp_shadow_near,
                    ..default()
                },
            ));
        }
        return;
    }
    let (eye, grading) = eye.single().map_or((Vec3::ZERO, None), |(eye, grading)| {
        (
            eye.translation,
            grading.map(|grading| grading.global.exposure),
        )
    });
    let scale = if state.interior {
        1.0
    } else {
        tuning.outdoor * outdoor_scale(grading.unwrap_or_default())
    };
    // The lamps nearest the camera, by how far their light falls short of
    // it, are drawn; the rest wait. A zone holds hundreds, and a pixel
    // under too many of them loses some (Bevy's light clusters).
    let mut nearest: Vec<_> = placed
        .iter()
        .map(|(entity, _, at, light, _)| (at.translation.distance(eye) - light.range, entity))
        .collect();
    nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
    let shadowed: HashSet<Entity> = nearest
        .iter()
        .take(tuning.shadowed_lamps)
        .map(|(_, entity)| *entity)
        .collect();
    let drawn: HashSet<Entity> = nearest
        .iter()
        .take(tuning.lamps_drawn)
        .map(|(_, entity)| *entity)
        .collect();
    for (entity, lamp, _, mut light, mut shown) in &mut placed {
        let visibility = if drawn.contains(&entity) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *shown != visibility {
            *shown = visibility;
        }
        let wander = if lamp.flickers {
            1.0 + tuning.flicker * flicker(seconds, lamp.phase)
        } else {
            1.0
        };
        light.intensity = lamp.intensity * wander * scale;
        let shadows = shadowed.contains(&entity);
        if light.shadow_maps_enabled != shadows {
            light.shadow_maps_enabled = shadows;
        }
    }
    // The carried lights. The first casts shadows, softly: it is drawn
    // twice, a shadowed part and an unshadowed fill, so its shadows darken
    // by the shadowed part alone.
    let mut slots = Vec::with_capacity(state.carried.len() + 1);
    for (index, pass) in state.carried.iter().enumerate() {
        if index == 0 && tuning.own_shadow {
            slots.push((pass, index, tuning.shadow_strength, true));
            slots.push((pass, index, 1.0 - tuning.shadow_strength, false));
        } else {
            slots.push((pass, index, 1.0, false));
        }
    }
    for (slot, (pass, index, part, shadows)) in slots.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)] // A phase.
        let wander = if warm(pass.rgb) {
            1.0 + tuning.flicker * 1.5 * flicker(seconds * 1.3, 0.7 + *index as f32)
        } else {
            1.0
        };
        let at = Vec3::from_array(pass.position);
        let intensity = intensity(pass.radius, tuning.carried) * wander * scale * part;
        match carried
            .iter_mut()
            .find(|(_, carried, ..)| carried.0 == slot)
        {
            Some((_, _, mut transform, mut light)) => {
                transform.translation = at;
                light.intensity = intensity;
                light.color = colour(pass.rgb);
                light.range = pass.radius * tuning.reach;
                if light.shadow_maps_enabled != *shadows {
                    light.shadow_maps_enabled = *shadows;
                }
            }
            None => {
                commands.spawn((
                    Carried(slot),
                    Transform::from_translation(at),
                    PointLight {
                        color: colour(pass.rgb),
                        intensity,
                        range: pass.radius * tuning.reach,
                        radius: 0.3,
                        shadow_maps_enabled: *shadows,
                        ..default()
                    },
                ));
            }
        }
    }
    for (entity, carried, ..) in &carried {
        if carried.0 >= slots.len() {
            commands.entity(entity).despawn();
        }
    }
}

/// The camera's high dynamic range, tone curve and glow while Rich Lighting
/// is on; the original's plain colours otherwise.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(super) fn grade(
    mut commands: Commands,
    state: Res<RichState>,
    tuning: Res<Tuning>,
    mut camera: Query<(Entity, &mut Tonemapping, Has<Bloom>), With<OrbitCamera>>,
) {
    // The tone curve only where the base holds all the light: outdoors it
    // would darken the plain day.
    let curve = if state.active && state.interior {
        tuning.tone
    } else {
        Tonemapping::None
    };
    for (entity, mut tone, glowing) in &mut camera {
        if *tone != curve {
            *tone = curve;
        }
        let glow = state.active && tuning.bloom;
        if glow && !glowing {
            commands.entity(entity).insert(Bloom {
                intensity: tuning.glow,
                ..Bloom::NATURAL
            });
        } else if !glow && glowing {
            commands.entity(entity).remove::<(Bloom, Hdr)>();
        }
    }
}

#[cfg(test)]
mod capture;
#[cfg(test)]
mod tests;
