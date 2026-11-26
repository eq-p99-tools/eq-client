//! Better Water ([`Fix::BetterWater`]): an opt-in look for the zone's water
//! and lava surfaces, departing from the original client on purpose.
//!
//! The official client draws a liquid surface as a flat texture flipping
//! through its frames a few times a second, unlit, sometimes opaque or cut
//! out by its colour key, and changes nothing when the camera goes under it.
//! With the setting on, each surface the zone's regions show to be the top
//! of water or lava wears [`WaterMaterial`] instead (`water.wgsl`): frames
//! blend into each other and flow in two layers, the surface is see-through
//! looking down and reflects the sky at a glance, ripples catch the sun,
//! deeper water darkens, foam gathers along the shore, and the view under
//! the surface is hazed. Turning it off puts the plain material back.
//!
//! Only the drawn scene changes: no movement, region, collision or swim
//! rule reads anything here.

use crate::underwater::UnderwaterView;
use bevy::{
    asset::embedded_asset,
    core_pipeline::prepass::DepthPrepass,
    pbr::{DistanceFog, ExtendedMaterial, FogFalloff, MaterialExtension, MaterialPlugin},
    prelude::*,
    reflect::Reflect,
    render::render_resource::{AsBindGroup, Face, ShaderType},
    shader::ShaderRef,
};
use eq_client_assets::{ZonePrimitive, regions::Liquid, regions::ZoneRegions};
use eq_client_core::qol::Fix;
use std::collections::{HashMap, HashSet};

/// A zone material wearing the water look.
pub(crate) type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterLook>;

/// The water look's own inputs, beside the plain material it extends.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(crate) struct WaterLook {
    #[uniform(100)]
    params: WaterParams,
    /// The frame showing now.
    #[texture(101)]
    #[sampler(102)]
    frame: Option<Handle<Image>>,
    /// The frame it blends into.
    #[texture(103)]
    #[sampler(104)]
    next: Option<Handle<Image>>,
}

/// Mirrors `Water` in `water.wgsl`.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType, Reflect)]
struct WaterParams {
    /// The sky's colour at the horizon (rgb).
    sky: Vec4,
    /// Toward the sun (xyz) and how strongly it shines, 0 to 1 (w).
    sun_direction: Vec4,
    /// The sun's colour (rgb).
    sun_color: Vec4,
    /// What the texture is tinted by where the liquid is shallow (rgb).
    shallow: Vec4,
    /// The colour of deep liquid (rgb) and how fast depth reaches it (a).
    deep: Vec4,
    /// When the frame began (x), seconds each frame shows (y), 1 for lava
    /// (z), and 1 when the scene's depth is drawn first (w).
    timing: Vec4,
    /// Rich Lighting's lights on the water: 1 while it is on (x), their
    /// soft ceiling (y), and 1 outdoors (z), as its own surfaces take them
    /// ([`crate::rich_light::RichState::water_light`]).
    rich: Vec4,
}

impl MaterialExtension for WaterLook {
    fn fragment_shader() -> ShaderRef {
        "embedded://eq_client_render/water.wgsl".into()
    }

    fn enable_shadows() -> bool {
        false
    }

    // Water reads the scene's depth behind it, so it must stay out of that
    // depth itself.
    fn enable_prepass() -> bool {
        false
    }
}

/// A terrain draw that is the top of a liquid, and what its look draws from.
#[derive(Component)]
pub(super) struct WaterSurface {
    liquid: Liquid,
    /// Full-colour frames in animation order; one for a still surface.
    frames: Vec<Handle<Image>>,
    /// Seconds each frame shows.
    frame_secs: f32,
    /// The look, made the first time the setting is on.
    look: Option<Handle<WaterMaterial>>,
    /// The frame the look shows, once it has shown one.
    shown: Option<u64>,
}

impl WaterSurface {
    pub(super) fn new(
        liquid: Liquid,
        frames: Vec<Handle<Image>>,
        frame_millis: Option<std::num::NonZeroU32>,
    ) -> Self {
        Self {
            liquid,
            frames,
            frame_secs: frame_millis.map_or(1.0, |millis| {
                std::time::Duration::from_millis(u64::from(millis.get())).as_secs_f32()
            }),
            look: None,
            shown: None,
        }
    }
}

/// Which kind of terrain a liquid's surface merges as, never 0 (ordinary
/// terrain): water only with water, lava only with lava.
pub(super) fn merge_kind(liquid: Liquid) -> u8 {
    match liquid {
        Liquid::Water => 1,
        Liquid::Lava => 2,
    }
}

/// The plain material a surface wears while the look is off.
#[derive(Component)]
struct Parked(Handle<StandardMaterial>);

/// Triangles flatter than this (the cosine of their tilt) count as level.
const LEVEL: f32 = 0.95;
/// How far above and below a level triangle its sides are probed.
const PROBE: f32 = 0.5;

/// Which liquid a terrain primitive is the top of, if any: most of its level,
/// passable triangles have the liquid just below and none just above. Walls,
/// falls, floors and underwater ground aren't surfaces, and a zone filled
/// with water throughout has solid floors, which the `solid` centroids
/// (from [`solid_centroids`]) rule out.
pub(super) fn surface_liquid(
    primitive: &ZonePrimitive,
    regions: &ZoneRegions,
    solid: &HashSet<[i64; 3]>,
) -> Option<Liquid> {
    let (mut water, mut lava, mut other) = (0_usize, 0_usize, 0_usize);
    for triangle in primitive.indices.as_chunks::<3>().0 {
        let Some([a, b, c]) = triangle
            .iter()
            .map(|&index| {
                primitive
                    .positions
                    .get(index as usize)
                    .copied()
                    .map(Vec3::from)
            })
            .collect::<Option<Vec<_>>>()
            .and_then(|corners| <[Vec3; 3]>::try_from(corners).ok())
        else {
            continue;
        };
        let normal = (b - a).cross(c - a).normalize_or_zero();
        if normal.y.abs() < LEVEL {
            continue;
        }
        let centre = (a + b + c) / 3.0;
        let below = regions.liquid_at((centre - Vec3::Y * PROBE).to_array());
        let above = regions.liquid_at((centre + Vec3::Y * PROBE).to_array());
        match (below, above) {
            _ if solid.contains(&centroid_key(centre)) => other += 1,
            (Some(Liquid::Water), None) => water += 1,
            (Some(Liquid::Lava), None) => lava += 1,
            _ => other += 1,
        }
    }
    let surface = water.max(lava);
    (surface >= 1 && surface * 2 >= water + lava + other).then_some(if lava > water {
        Liquid::Lava
    } else {
        Liquid::Water
    })
}

/// Which liquid each primitive is the top of, by [`surface_liquid`], and
/// then by its sheet: zones draw some of a surface with no liquid region
/// under it (South Qeynos's canals at -20), so a level piece that is not
/// solid, wears the same texture as a found surface and lies at that
/// surface's height is the same water.
pub(super) fn surface_liquids(
    primitives: &[ZonePrimitive],
    regions: &ZoneRegions,
    solid: &HashSet<[i64; 3]>,
) -> Vec<Option<Liquid>> {
    let mut found: Vec<_> = primitives
        .iter()
        .map(|primitive| surface_liquid(primitive, regions, solid))
        .collect();
    let mut sheets = std::collections::HashMap::new();
    for (primitive, liquid) in primitives.iter().zip(&found) {
        if let (Some(texture), Some(liquid)) = (primitive.texture, liquid) {
            for (_, centre) in level_triangles(primitive) {
                sheets.insert((texture, height_key(centre.y)), *liquid);
            }
        }
    }
    for (primitive, liquid) in primitives.iter().zip(&mut found) {
        let Some(texture) = primitive.texture else {
            continue;
        };
        if liquid.is_some() {
            continue;
        }
        let level: Vec<_> = level_triangles(primitive).collect();
        let triangles = primitive.indices.len() / 3;
        if level.is_empty() || level.len() * 2 < triangles {
            continue;
        }
        let mut same = level.iter().map(|(_, centre)| {
            (!solid.contains(&centroid_key(*centre)))
                .then(|| sheets.get(&(texture, height_key(centre.y))).copied())
                .flatten()
        });
        let first = same.next().flatten();
        if first.is_some() && same.all(|other| other == first) {
            *liquid = first;
        }
    }
    found
}

/// A primitive's level triangles: their normals and centres.
fn level_triangles(primitive: &ZonePrimitive) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
    primitive
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|triangle| {
            let [a, b, c] = triangle.map(|index| primitive.positions.get(index as usize).copied());
            let [a, b, c] = [Vec3::from(a?), Vec3::from(b?), Vec3::from(c?)];
            let normal = (b - a).cross(c - a).normalize_or_zero();
            (normal.y.abs() >= LEVEL).then_some((normal, (a + b + c) / 3.0))
        })
}

#[allow(clippy::cast_possible_truncation)] // Zone heights are small.
fn height_key(height: f32) -> i64 {
    (height * 4.0).round() as i64
}

/// The zone's solid triangles by centroid, for [`surface_liquid`].
pub(super) fn solid_centroids(collision: &[[[f32; 3]; 3]]) -> HashSet<[i64; 3]> {
    collision
        .iter()
        .map(|[a, b, c]| centroid_key((Vec3::from(*a) + Vec3::from(*b) + Vec3::from(*c)) / 3.0))
        .collect()
}

#[allow(clippy::cast_possible_truncation)] // Zone coordinates are small.
fn centroid_key(point: Vec3) -> [i64; 3] {
    (point * 4.0).round().to_array().map(|value| value as i64)
}

pub(super) fn register(app: &mut App) {
    embedded_asset!(app, "water.wgsl");
    crate::underwater::register(app);
    app.add_plugins(MaterialPlugin::<WaterMaterial>::default())
        .add_systems(
            Update,
            (wear, even_light, depth, animate, weather, rich_lit)
                .chain()
                .in_set(crate::Stage::Present),
        )
        .add_systems(
            Update,
            underwater
                .after(crate::daylight::update)
                .in_set(crate::Stage::Present),
        );
}

fn better(options: &crate::options::OptionsState) -> bool {
    options.options.qol.on(Fix::BetterWater)
}

/// A liquid mesh's positions, normals and vertex colours.
type Evened<'a> = (&'a [[f32; 3]], &'a [[f32; 3]], &'a [[f32; 4]]);

/// The same, copied out of the mesh.
type Taken = (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 4]>);

/// How far apart the points are that a liquid's light is evened over: wide,
/// since the zone's baked light can climb from dim to full within a single
/// piece, and the bend where it turns shows as a line across the water.
const EVEN_CELL: f32 = 128.0;

/// Light, in a vertex colour, as one plane of liquid takes it: the zone
/// bakes light into each piece of a surface on its own, so neighbouring
/// pieces can disagree along their shared edge. The plain look's half
/// transparency hides that; Better Water's deeper colour shows it as a
/// hard line across the water. So each level of each liquid is treated as
/// one plane (each side of it apart: the sheet facing down bakes its own
/// light for the view from below): every vertex spreads its colour over the nearest points of
/// a grid, and every vertex then takes the blend of those points, which
/// changes smoothly across piece edges and mesh cells alike.
fn even_colours(meshes: &[Evened<'_>]) -> Vec<Vec<[f32; 4]>> {
    // The four grid points around a level point, and how much each counts.
    let around = |position: &[f32; 3], normal: &[f32; 3]| {
        let x = position[0] / EVEN_CELL - 0.5;
        let z = position[2] / EVEN_CELL - 0.5;
        let (ix, iz) = (x.floor(), z.floor());
        let (fx, fz) = (x - ix, z - iz);
        let level = height_key(position[1]) * 2 + i64::from(normal[1] < 0.0);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "zone coordinates are far inside i64"
        )]
        let (ix, iz) = (ix as i64, iz as i64);
        [
            ((ix, iz, level), (1.0 - fx) * (1.0 - fz)),
            ((ix + 1, iz, level), fx * (1.0 - fz)),
            ((ix, iz + 1, level), (1.0 - fx) * fz),
            ((ix + 1, iz + 1, level), fx * fz),
        ]
    };
    let mut points: HashMap<(i64, i64, i64), ([f32; 4], f32)> = HashMap::new();
    for (positions, normals, colours) in meshes {
        for ((position, normal), colour) in positions.iter().zip(normals.iter()).zip(colours.iter())
        {
            for (key, weight) in around(position, normal) {
                let point = points.entry(key).or_insert(([0.0; 4], 0.0));
                for (sum, channel) in point.0.iter_mut().zip(colour) {
                    *sum += channel * weight;
                }
                point.1 += weight;
            }
        }
    }
    meshes
        .iter()
        .map(|(positions, normals, colours)| {
            positions
                .iter()
                .zip(normals.iter())
                .zip(colours.iter())
                .map(|((position, normal), own)| {
                    let mut sum = [0.0; 4];
                    let mut total = 0.0;
                    for (key, weight) in around(position, normal) {
                        if let Some((point, count)) = points.get(&key)
                            && *count > 0.0
                        {
                            for (out, channel) in sum.iter_mut().zip(point) {
                                *out += channel / count * weight;
                            }
                            total += weight;
                        }
                    }
                    if total > 0.0 {
                        sum.map(|channel| channel / total)
                    } else {
                        *own
                    }
                })
                .collect()
        })
        .collect()
}

/// A mesh's positions, normals and colours, copied out; no colours when it
/// has none for each position.
fn taken(mesh: Option<&Mesh>) -> Taken {
    let positions = match mesh.and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_POSITION)) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(all)) => all.clone(),
        _ => Vec::new(),
    };
    let normals = match mesh.and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_NORMAL)) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(all)) if all.len() == positions.len() => {
            all.clone()
        }
        _ => vec![[0.0, 1.0, 0.0]; positions.len()],
    };
    let colours = match mesh.and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_COLOR)) {
        Some(bevy::mesh::VertexAttributeValues::Float32x4(all)) if all.len() == positions.len() => {
            all.clone()
        }
        _ => Vec::new(),
    };
    (positions, normals, colours)
}

/// What evening the light last changed in each water mesh: the change
/// to each vertex's colour, and how many meshes there were.
struct Evening {
    changes: HashMap<AssetId<Mesh>, Vec<[f32; 4]>>,
    meshes: usize,
}

/// While Better Water is on, evens the zone light across each liquid
/// surface ([`even_colours`]), and has the light write the zone's own
/// colours afresh when it is turned off.
///
/// The whole of it is worked out when the zone light colours every mesh
/// afresh (a new zone, the hour, the setting turned on). The light a torch
/// carries rewrites only the parts near it, often as its bearer walks, and
/// changes smoothly from one piece to the next, so those parts take the
/// same change as before rather than the whole sea being worked again.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn even_light(
    options: Res<crate::options::OptionsState>,
    scene: Option<ResMut<crate::lighting::SceneLights>>,
    surfaces: Query<&Mesh3d, With<WaterSurface>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut evening: Local<Option<Evening>>,
) {
    let Some(mut scene) = scene else {
        *evening = None;
        return;
    };
    let fresh = std::mem::take(&mut scene.fresh);
    let written = std::mem::take(&mut scene.written);
    if !better(&options) {
        if evening.take().is_some() {
            scene.recolour();
        }
        return;
    }
    let handles: Vec<_> = {
        let mut seen = HashSet::new();
        surfaces
            .iter()
            .filter(|mesh| seen.insert(mesh.0.id()))
            .map(|mesh| mesh.0.id())
            .collect()
    };
    let whole = fresh
        || evening
            .as_ref()
            .is_none_or(|evening| evening.meshes != handles.len());
    if !whole {
        let Some(evening) = evening.as_ref() else {
            return;
        };
        for (id, start, count) in written {
            let Some(change) = evening
                .changes
                .get(&id)
                .and_then(|change| change.get(start..start + count))
            else {
                continue;
            };
            if let Some(mut mesh) = meshes.get_mut(id)
                && let Some(bevy::mesh::VertexAttributeValues::Float32x4(all)) =
                    mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
                && let Some(part) = all.get_mut(start..start + count)
            {
                for (colour, by) in part.iter_mut().zip(change) {
                    for (channel, by) in colour.iter_mut().zip(by) {
                        *channel += by;
                    }
                }
            }
        }
        return;
    }
    let taken: Vec<Taken> = handles.iter().map(|id| taken(meshes.get(*id))).collect();
    let views: Vec<Evened<'_>> = taken
        .iter()
        .filter(|(_, _, colours)| !colours.is_empty())
        .map(|(positions, normals, colours)| {
            (positions.as_slice(), normals.as_slice(), colours.as_slice())
        })
        .collect();
    let mut even = even_colours(&views).into_iter();
    let mut changes = HashMap::new();
    for (id, (_, _, colours)) in handles.iter().zip(&taken) {
        if colours.is_empty() {
            continue;
        }
        let Some(evened) = even.next() else {
            break;
        };
        let change = evened
            .iter()
            .zip(colours)
            .map(|(evened, own)| std::array::from_fn(|channel| evened[channel] - own[channel]))
            .collect();
        changes.insert(*id, change);
        if let Some(mut mesh) = meshes.get_mut(*id) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, evened);
        }
    }
    *evening = Some(Evening {
        changes,
        meshes: handles.len(),
    });
}

/// A surface with whichever material it wears now.
type Wearers = (
    Entity,
    &'static mut WaterSurface,
    Option<&'static MeshMaterial3d<StandardMaterial>>,
    Option<&'static Parked>,
);

/// What makes two surfaces' looks the same: the liquid, the frames it
/// flips through and how fast, and which side it hides. Surfaces alike
/// wear one look, so they draw together and the look changes once a frame
/// tick for all of them.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Alike {
    lava: bool,
    frames: Vec<AssetId<Image>>,
    frame_secs: u32,
    cull: Option<Face>,
}

impl Alike {
    fn of(surface: &WaterSurface, plain: Option<&StandardMaterial>) -> Self {
        let lava = surface.liquid == Liquid::Lava;
        Self {
            lava,
            frames: surface.frames.iter().map(Handle::id).collect(),
            frame_secs: surface.frame_secs.to_bits(),
            cull: plain.and_then(|plain| plain.cull_mode),
        }
    }
}

/// Puts the look on each surface while the setting is on, and the plain
/// material back when it is off. The looks made are remembered weakly, so
/// a look goes once no surface wears it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn wear(
    mut commands: Commands,
    options: Res<crate::options::OptionsState>,
    mut surfaces: Query<Wearers>,
    plain: Res<Assets<StandardMaterial>>,
    mut looks: ResMut<Assets<WaterMaterial>>,
    mut made: Local<std::collections::HashMap<Alike, AssetId<WaterMaterial>>>,
) {
    let on = better(&options);
    for (entity, mut surface, material, parked) in &mut surfaces {
        match (on, material, parked) {
            (true, Some(material), None) => {
                let look = surface.look.clone().unwrap_or_else(|| {
                    let plain = plain.get(&material.0);
                    let alike = Alike::of(&surface, plain);
                    let look = made
                        .get(&alike)
                        .and_then(|id| looks.get_strong_handle(*id))
                        .unwrap_or_else(|| {
                            let look = looks.add(new_look(&surface, plain));
                            made.insert(alike, look.id());
                            look
                        });
                    surface.look = Some(look.clone());
                    look
                });
                surface.shown = None;
                commands
                    .entity(entity)
                    .remove::<MeshMaterial3d<StandardMaterial>>()
                    .insert((Parked(material.0.clone()), MeshMaterial3d(look)));
            }
            (false, _, Some(parked)) => {
                commands
                    .entity(entity)
                    .remove::<(Parked, MeshMaterial3d<WaterMaterial>)>()
                    .insert(MeshMaterial3d(parked.0.clone()));
            }
            _ => {}
        }
    }
    made.retain(|_, id| looks.contains(*id));
}

fn new_look(surface: &WaterSurface, plain: Option<&StandardMaterial>) -> WaterMaterial {
    let lava = surface.liquid == Liquid::Lava;
    let base = StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: None,
        // Water is drawn over a copy of the scene behind it (the
        // transmissive pass), which it bends, tints and reflects itself;
        // lava is plain and opaque.
        alpha_mode: AlphaMode::Opaque,
        specular_transmission: if lava { 0.0 } else { 1.0 },
        unlit: true,
        // Zones build water two-sided from two sheets in the same plane,
        // one facing up and one facing down: each keeps the side its plain
        // material shows, or the two would fight for the same pixels.
        double_sided: plain.is_some_and(|plain| plain.double_sided),
        cull_mode: plain.map_or(Some(Face::Back), |plain| plain.cull_mode),
        fog_enabled: true,
        ..default()
    };
    let (shallow, deep) = if lava {
        (
            Vec4::new(1.2, 0.9, 0.7, 0.0),
            Vec4::new(0.35, 0.05, 0.0, 0.5),
        )
    } else {
        (
            Vec4::new(0.85, 1.05, 1.0, 0.0),
            Vec4::new(0.015, 0.055, 0.07, 0.04),
        )
    };
    let frame = surface.frames.first().cloned();
    ExtendedMaterial {
        base,
        extension: WaterLook {
            params: WaterParams {
                sky: Vec4::new(0.55, 0.65, 0.75, 1.0),
                sun_direction: Vec4::new(0.3, 0.8, 0.5, 1.0).normalize(),
                sun_color: Vec4::ONE,
                shallow,
                deep,
                timing: Vec4::new(0.0, surface.frame_secs, f32::from(u8::from(lava)), 0.0),
                rich: Vec4::ZERO,
            },
            next: surface.frames.get(1).cloned().or_else(|| frame.clone()),
            frame,
        },
    }
}

/// Draws the scene's depth first while the setting is on, so water knows
/// how deep it is where it is seen.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn depth(
    mut commands: Commands,
    options: Res<crate::options::OptionsState>,
    cameras: Query<(Entity, Has<DepthPrepass>), With<crate::camera::OrbitCamera>>,
) {
    let on = better(&options);
    for (camera, has) in &cameras {
        if on && !has {
            commands.entity(camera).insert(DepthPrepass);
        } else if !on && has {
            commands.entity(camera).remove::<DepthPrepass>();
        }
    }
}

/// Moves each look to the frame the clock shows; the shader blends toward
/// the next one in between.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn animate(
    time: Res<Time>,
    cameras: Query<Has<DepthPrepass>, With<crate::camera::OrbitCamera>>,
    mut surfaces: Query<(&mut WaterSurface, &MeshMaterial3d<WaterMaterial>)>,
    mut looks: ResMut<Assets<WaterMaterial>>,
) {
    // The shader's clock is the same wrapped time.
    let now = time.elapsed_secs_wrapped();
    let deep = f32::from(u8::from(cameras.iter().any(|has| has)));
    // Surfaces alike share a look: the first of them moves it.
    let mut moved = HashSet::new();
    for (mut surface, material) in &mut surfaces {
        let count = surface.frames.len().max(1) as u64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let tick = (now / surface.frame_secs).floor().max(0.0) as u64;
        if surface.shown == Some(tick) {
            continue;
        }
        if !moved.insert(material.0.id()) {
            surface.shown = Some(tick);
            continue;
        }
        let Some(mut look) = looks.get_mut(&material.0) else {
            continue;
        };
        let at = |step: u64| {
            usize::try_from((tick + step) % count)
                .ok()
                .and_then(|index| surface.frames.get(index).cloned())
        };
        look.extension.frame = at(0);
        look.extension.next = at(1);
        // The wrapped clock stays within an hour, well inside f32's range.
        #[allow(clippy::cast_precision_loss)]
        let began = tick as f32 * surface.frame_secs;
        look.extension.params.timing.x = began;
        look.extension.params.timing.w = deep;
        surface.shown = Some(tick);
    }
}

/// Gives the looks the sky they reflect and the sun they glint in, when
/// either changes.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn weather(
    online: Res<crate::online::OnlineState>,
    clear: Option<Res<ClearColor>>,
    suns: Query<(&DirectionalLight, &GlobalTransform), With<crate::daylight::Sun>>,
    fogs: Query<&DistanceFog, With<crate::camera::OrbitCamera>>,
    surfaces: Query<&MeshMaterial3d<WaterMaterial>>,
    mut looks: ResMut<Assets<WaterMaterial>>,
    mut last: Local<Option<(Vec4, Vec4, Vec4)>>,
) {
    // The zone's fog is the colour of its horizon, else its sky; before a
    // zone has said (offline), a clear day's.
    let sky = fogs
        .iter()
        .next()
        .map(|fog| fog.color)
        .or_else(|| {
            online
                .world()
                .sky()
                .and_then(|_| clear.map(|clear| clear.0))
        })
        .unwrap_or(Color::srgb(0.52, 0.68, 0.86))
        .to_linear();
    let sky = Vec4::new(sky.red, sky.green, sky.blue, 1.0);
    let (sun_direction, sun_color) = suns.iter().next().map_or(
        (Vec4::new(0.3, 0.8, 0.5, 0.0), Vec4::ONE),
        |(light, place)| {
            let toward = place.back().as_vec3();
            let strength = (light.illuminance / crate::daylight::DAY_SUN).clamp(0.0, 1.0);
            let color = light.color.to_linear();
            (
                toward.extend(strength),
                Vec4::new(color.red, color.green, color.blue, 1.0),
            )
        },
    );
    let now = (sky, sun_direction, sun_color);
    let fresh = surfaces.iter().any(|material| {
        looks
            .get(&material.0)
            .is_some_and(|look| look.extension.params.sky != sky)
    });
    if *last == Some(now) && !fresh {
        return;
    }
    *last = Some(now);
    let looks_worn: HashSet<_> = surfaces.iter().map(|material| material.0.id()).collect();
    for id in looks_worn {
        if let Some(mut look) = looks.get_mut(id) {
            let params = &mut look.extension.params;
            params.sky = sky;
            params.sun_direction = sun_direction;
            params.sun_color = sun_color;
        }
    }
}

/// Gives the looks Rich Lighting's state, so its lights fall on the water
/// while it is on, when it changes or a look is new.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn rich_lit(
    rich: Option<Res<crate::rich_light::RichState>>,
    surfaces: Query<&MeshMaterial3d<WaterMaterial>>,
    mut looks: ResMut<Assets<WaterMaterial>>,
) {
    let now = rich.map_or(Vec4::ZERO, |rich| rich.water_light());
    let stale: HashSet<_> = surfaces
        .iter()
        .map(|material| material.0.id())
        .filter(|id| {
            looks
                .get(*id)
                .is_some_and(|look| look.extension.params.rich != now)
        })
        .collect();
    for id in stale {
        if let Some(mut look) = looks.get_mut(id) {
            look.extension.params.rich = now;
        }
    }
}

/// The camera's own fog before the haze replaced it, if it had any.
struct Before(Option<DistanceFog>);

/// How far above a point under the surface the surface is, found by
/// stepping up until the liquid ends; none past `limit`.
fn depth_below_surface(
    world: &eq_client_core::movement::CollisionWorld,
    at: Vec3,
    limit: f32,
) -> Option<f32> {
    const STEP: f32 = 2.0;
    let mut above = 0.0;
    while above < limit {
        above += STEP;
        if world.liquid_at(at + Vec3::Y * above).is_none() {
            // Halve back to the surface.
            let (mut wet, mut dry) = (above - STEP, above);
            for _ in 0..4 {
                let middle = f32::midpoint(wet, dry);
                if world.liquid_at(at + Vec3::Y * middle).is_some() {
                    wet = middle;
                } else {
                    dry = middle;
                }
            }
            return Some(f32::midpoint(wet, dry));
        }
    }
    None
}

/// The camera's parts that [`underwater`] reads and changes.
type Divers = (
    Entity,
    &'static GlobalTransform,
    Option<&'static mut DistanceFog>,
    Has<UnderwaterView>,
);

/// The haze over the view while the camera is under water or lava, deeper
/// and darker the further down it is, with the shimmer and light shafts of
/// [`UnderwaterView`]; the zone's own fog is put back when it comes out.
/// Water stays easy to see through: the haze only closes in far off.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system parameters.
fn underwater(
    mut commands: Commands,
    options: Res<crate::options::OptionsState>,
    collision: Res<crate::Collision>,
    time: Res<Time>,
    online: Res<crate::online::OnlineState>,
    suns: Query<(&DirectionalLight, &GlobalTransform), With<crate::daylight::Sun>>,
    mut cameras: Query<Divers, With<crate::camera::OrbitCamera>>,
    mut saved: Local<Option<Before>>,
    mut sunlit: Local<Option<SunlitGrid>>,
) {
    let Ok((camera, place, fog, viewed)) = cameras.single_mut() else {
        return;
    };
    let at = place.translation();
    let world = collision.0.as_ref();
    let under = better(&options).then(|| world?.liquid_at(at)).flatten();
    let Some(liquid) = under else {
        if viewed {
            commands.entity(camera).remove::<UnderwaterView>();
        }
        if let Some(Before(before)) = saved.take() {
            match before {
                Some(before) => {
                    commands.entity(camera).insert(before);
                }
                None => {
                    commands.entity(camera).remove::<DistanceFog>();
                }
            }
        }
        return;
    };
    if saved.is_none() {
        *saved = Some(Before(fog.as_deref().cloned()));
    }
    let lava = liquid == eq_client_core::movement::Liquid::Lava;
    let depth = world
        .and_then(|world| depth_below_surface(world, at, 400.0))
        .unwrap_or(400.0);
    // 0 just under the surface, 1 two hundred units down.
    let down = (depth / 200.0).clamp(0.0, 1.0);
    let haze = if lava {
        DistanceFog {
            color: Color::srgb(0.55, 0.12, 0.02),
            directional_light_color: Color::NONE,
            directional_light_exponent: 8.0,
            falloff: FogFalloff::Linear {
                start: 0.0,
                end: 18.0,
            },
        }
    } else {
        let shallow = Vec3::new(0.08, 0.26, 0.3);
        let deep = Vec3::new(0.02, 0.07, 0.11);
        let color = shallow.lerp(deep, down);
        DistanceFog {
            color: Color::srgb(color.x, color.y, color.z),
            directional_light_color: Color::NONE,
            directional_light_exponent: 8.0,
            falloff: FogFalloff::Linear {
                start: 8.0 - 6.0 * down,
                end: 170.0 - 70.0 * down,
            },
        }
    };
    match fog {
        Some(mut fog) => *fog = haze,
        None => {
            commands.entity(camera).insert(haze);
        }
    }
    // Sunlight bends toward straight down as it enters the water; a zone
    // without a sky (a dungeon) has no sun to send down.
    let outdoors = online.world().sky().is_none_or(|sky| sky.sky != 0);
    let (sun, toward) = suns.iter().next().map_or((1.0, Vec3::Y), |(light, place)| {
        (
            (light.illuminance / crate::daylight::DAY_SUN).clamp(0.0, 1.0),
            place.back().as_vec3(),
        )
    });
    let shafts = if lava || !outdoors {
        Vec4::ZERO
    } else {
        // Shafts fade with depth, and go with the sun at night.
        Vec4::new(0.55, 0.85, 0.8, 0.15 * (1.0 - down) * sun)
    };
    // Where the surface overhead can see the sun: shafts fall only there.
    let surface = at.y + depth;
    let grid = match (world, shafts.w > 0.0) {
        (Some(world), true) => {
            let wanted = SunlitGrid::around(at, surface, toward);
            if sunlit.as_ref().is_none_or(|grid| !grid.covers(&wanted)) {
                *sunlit = Some(wanted.cast(world));
            }
            sunlit.as_ref()
        }
        _ => None,
    };
    commands.entity(camera).insert(UnderwaterView {
        shafts,
        light: into_water(toward).extend(depth),
        state: Vec4::new(
            time.elapsed_secs_wrapped(),
            depth,
            f32::from(u8::from(lava)),
            0.0,
        ),
        grid: grid.map_or(Vec4::ZERO, SunlitGrid::placement),
        open: grid.map_or([UVec4::ZERO; 32], |grid| grid.open),
    });
}

/// Which spots of the water's surface around the camera can see the sun,
/// one bit for each [`SunlitGrid::CELL`]-wide square of a
/// [`SunlitGrid::SIDE`]-square grid: a ray from each spot toward the sun
/// that hits nothing solid lets light down there. Cast again when the
/// camera has moved a good way, the sun has turned, or the surface changed.
struct SunlitGrid {
    /// The grid's corner (x, z), snapped so small moves keep it.
    corner: Vec2,
    surface: f32,
    toward: Vec3,
    open: [UVec4; 32],
}

impl SunlitGrid {
    const SIDE: usize = 64;
    const CELL: f32 = 4.0;
    /// How far the corner snaps: the camera stays at least a quarter of the
    /// grid from its edge.
    const SNAP: f32 = 32.0;
    /// How far toward the sun a spot looks for cover: far enough for roofs,
    /// bridges, docks and tunnels over the water, short of the invisible
    /// walls and ceilings zones close themselves in with.
    const REACH: f32 = 50.0;

    fn around(at: Vec3, surface: f32, toward: Vec3) -> Self {
        #[allow(clippy::cast_precision_loss)] // A small constant.
        let half = Self::SIDE as f32 * Self::CELL / 2.0;
        let corner = (Vec2::new(at.x, at.z) / Self::SNAP).round() * Self::SNAP - Vec2::splat(half);
        Self {
            corner,
            surface,
            toward: Vec3::new(toward.x, toward.y.max(0.25), toward.z).normalize(),
            open: [UVec4::ZERO; 32],
        }
    }

    fn covers(&self, wanted: &Self) -> bool {
        self.corner == wanted.corner
            && (self.surface - wanted.surface).abs() < 1.0
            && self.toward.dot(wanted.toward) > 0.9995
    }

    fn cast(mut self, world: &eq_client_core::movement::CollisionWorld) -> Self {
        for index in 0..Self::SIDE * Self::SIDE {
            #[allow(clippy::cast_precision_loss)] // Small grid indices.
            let (x, z) = (
                (index % Self::SIDE) as f32 + 0.5,
                (index / Self::SIDE) as f32 + 0.5,
            );
            let spot = Vec3::new(
                self.corner.x + x * Self::CELL,
                self.surface + 0.3,
                self.corner.y + z * Self::CELL,
            );
            if world.ray_distance(spot, self.toward, Self::REACH).is_none() {
                let packed = index / 32;
                self.open[packed / 4][packed % 4] |= 1 << (index % 32);
            }
        }
        self
    }

    /// The grid's corner (xy) and cell size (z) for the shader, with w set
    /// to say it is there.
    fn placement(&self) -> Vec4 {
        Vec4::new(self.corner.x, self.corner.y, Self::CELL, 1.0)
    }
}

/// The way sunlight travels once it is in the water, for a sun in
/// direction `toward`: bent toward straight down at the surface (water's
/// index 1.33). A sun at or under the horizon still lights from high up,
/// as the sky's glow does.
fn into_water(toward: Vec3) -> Vec3 {
    let toward = Vec3::new(toward.x, toward.y.max(0.25), toward.z).normalize();
    let incoming = -toward;
    let ratio = 1.0 / 1.33;
    let cos_in = toward.y;
    let cos_out = (1.0 - ratio * ratio * (1.0 - cos_in * cos_in)).sqrt();
    (incoming * ratio + Vec3::Y * (ratio * cos_in - cos_out)).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_assets::MaterialMode;

    fn square(height: f32, up: bool) -> ZonePrimitive {
        let positions = vec![
            [0.0, height, 0.0],
            [10.0, height, 0.0],
            [0.0, height, 10.0],
            [10.0, height, 10.0],
        ];
        let indices = if up {
            vec![0, 2, 1, 1, 2, 3]
        } else {
            vec![0, 1, 2, 1, 3, 2]
        };
        ZonePrimitive {
            positions,
            normals: vec![[0.0, 1.0, 0.0]; 4],
            texture_coordinates: vec![[0.0, 0.0]; 4],
            indices,
            texture: None,
            material_mode: MaterialMode::Opaque,
            light: Vec::new(),
        }
    }

    #[test]
    fn a_level_passable_sheet_over_water_is_its_surface() {
        // Water fills everything below 0.
        let regions = ZoneRegions::test_liquid_surface(0.0, Liquid::Water);
        let none = HashSet::new();
        for up in [true, false] {
            assert_eq!(
                surface_liquid(&square(0.0, up), &regions, &none),
                Some(Liquid::Water)
            );
        }
        // The pool's floor is under the water, and dry ground above it.
        assert_eq!(surface_liquid(&square(-5.0, true), &regions, &none), None);
        assert_eq!(surface_liquid(&square(5.0, true), &regions, &none), None);
        // A solid sheet at the waterline is ground, not a surface.
        let floor = square(0.0, true);
        let solid = solid_centroids(
            &floor
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    t.iter()
                        .map(|&i| floor.positions[i as usize])
                        .collect::<Vec<_>>()
                })
                .map(|t| [t[0], t[1], t[2]])
                .collect::<Vec<_>>(),
        );
        assert_eq!(surface_liquid(&floor, &regions, &solid), None);
        // A piece of the sheet stored as a single triangle is still it.
        let mut single = square(0.0, true);
        single.indices.truncate(3);
        assert_eq!(
            surface_liquid(&single, &regions, &none),
            Some(Liquid::Water)
        );
        // Pieces in the surface's texture count at its height, not at the
        // floor's.
        let textured = |height| ZonePrimitive {
            texture: Some(3),
            ..square(height, true)
        };
        assert_eq!(
            surface_liquids(&[textured(0.0), textured(-5.0)], &regions, &none),
            vec![Some(Liquid::Water), None]
        );
        let lava = ZoneRegions::test_liquid_surface(0.0, Liquid::Lava);
        assert_eq!(
            surface_liquid(&square(0.0, true), &lava, &none),
            Some(Liquid::Lava)
        );
    }

    #[test]
    fn surfaces_alike_wear_one_look_and_it_moves_once_a_tick() {
        let mut app = App::new();
        app.init_resource::<crate::options::OptionsState>()
            .init_resource::<Time>()
            .insert_resource(Assets::<StandardMaterial>::default())
            .insert_resource(Assets::<WaterMaterial>::default())
            .insert_resource(Assets::<Image>::default())
            .add_systems(Update, (wear, animate).chain());
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(Fix::BetterWater, true);
        let (ripple, swell) = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            (images.add(Image::default()), images.add(Image::default()))
        };
        let plain = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let frames = vec![ripple.clone(), swell.clone()];
        let millis = std::num::NonZeroU32::new(250);
        let surfaces = [
            WaterSurface::new(Liquid::Water, frames.clone(), millis),
            WaterSurface::new(Liquid::Water, frames.clone(), millis),
            WaterSurface::new(Liquid::Water, frames.clone(), millis),
            WaterSurface::new(Liquid::Lava, frames, millis),
            WaterSurface::new(Liquid::Water, vec![swell], millis),
        ]
        .map(|surface| {
            app.world_mut()
                .spawn((surface, MeshMaterial3d(plain.clone())))
                .id()
        });
        app.update();
        app.update();
        let worn: Vec<_> = surfaces
            .iter()
            .map(|&surface| {
                app.world()
                    .get::<MeshMaterial3d<WaterMaterial>>(surface)
                    .expect("wears the look")
                    .0
                    .id()
            })
            .collect();
        // The three alike share one; lava and other frames have their own.
        assert_eq!(worn[0], worn[1]);
        assert_eq!(worn[0], worn[2]);
        assert_ne!(worn[0], worn[3]);
        assert_ne!(worn[0], worn[4]);
        assert_ne!(worn[3], worn[4]);
        assert_eq!(app.world().resource::<Assets<WaterMaterial>>().len(), 3);
        // Each surface has the tick shown, though only the first moved it.
        for &surface in &surfaces {
            assert!(
                app.world()
                    .get::<WaterSurface>(surface)
                    .unwrap()
                    .shown
                    .is_some()
            );
        }
        // Off, every surface wears the plain material again.
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(Fix::BetterWater, false);
        app.update();
        for &surface in &surfaces {
            assert!(
                app.world()
                    .get::<MeshMaterial3d<StandardMaterial>>(surface)
                    .is_some()
            );
        }
    }

    #[test]
    fn neighbouring_pieces_meet_in_one_light() {
        // Two pieces of one sheet at -2 share the edge x = 24, one baked
        // bright and one dim, the way South Qeynos's canal pieces are.
        let up = [0.0, 1.0, 0.0];
        let left: Vec<[f32; 3]> = vec![[0.0, -2.0, 0.0], [24.0, -2.0, 0.0], [24.0, -2.0, 24.0]];
        let right: Vec<[f32; 3]> = vec![[24.0, -2.0, 0.0], [48.0, -2.0, 0.0], [24.0, -2.0, 24.0]];
        let normals = vec![up; 3];
        let bright = vec![[0.9, 0.9, 0.6, 1.0]; 3];
        let dim = vec![[0.4, 0.5, 0.4, 1.0]; 3];
        // The sheet facing down under them bakes no light of its own.
        let below: Vec<[f32; 3]> = left.clone();
        let down = vec![[0.0, -1.0, 0.0]; 3];
        let dark = vec![[0.0, 0.0, 0.0, 1.0]; 3];
        let even = even_colours(&[
            (&left, &normals, &bright),
            (&right, &normals, &dim),
            (&below, &down, &dark),
        ]);
        // Where the pieces meet, both take the same light, between the two.
        for (a, b) in [(1, 0), (2, 2)] {
            assert_eq!(even[0][a], even[1][b]);
            assert!(
                even[0][a][0] < 0.9 && even[0][a][0] > 0.4,
                "{:?}",
                even[0][a]
            );
        }
        // The side facing down keeps its own.
        assert_eq!(even[2], dark);
    }
}
