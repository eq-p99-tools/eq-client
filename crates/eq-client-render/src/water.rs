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

use bevy::{
    asset::embedded_asset,
    core_pipeline::{
        fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin},
        prepass::DepthPrepass,
    },
    pbr::{DistanceFog, ExtendedMaterial, FogFalloff, MaterialExtension, MaterialPlugin},
    prelude::*,
    reflect::Reflect,
    render::extract_component::ExtractComponent,
    render::render_resource::{AsBindGroup, Face, ShaderType},
    shader::ShaderRef,
};
use eq_client_assets::{ZonePrimitive, regions::Liquid, regions::ZoneRegions};
use eq_client_core::qol::Fix;
use std::collections::HashSet;

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
    (surface >= 2 && surface * 2 >= water + lava + other).then_some(if lava > water {
        Liquid::Lava
    } else {
        Liquid::Water
    })
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
    embedded_asset!(app, "underwater.wgsl");
    app.add_plugins((
        MaterialPlugin::<WaterMaterial>::default(),
        FullscreenMaterialPlugin::<UnderwaterView>::default(),
    ))
    .add_systems(
        Update,
        (wear, depth, animate, weather)
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

/// A surface with whichever material it wears now.
type Wearers = (
    Entity,
    &'static mut WaterSurface,
    Option<&'static MeshMaterial3d<StandardMaterial>>,
    Option<&'static Parked>,
);

/// Puts the look on each surface while the setting is on, and the plain
/// material back when it is off.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn wear(
    mut commands: Commands,
    options: Res<crate::options::OptionsState>,
    mut surfaces: Query<Wearers>,
    plain: Res<Assets<StandardMaterial>>,
    mut looks: ResMut<Assets<WaterMaterial>>,
) {
    let on = better(&options);
    for (entity, mut surface, material, parked) in &mut surfaces {
        match (on, material, parked) {
            (true, Some(material), None) => {
                let look = surface.look.clone().unwrap_or_else(|| {
                    let look = looks.add(new_look(&surface, plain.get(&material.0)));
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
        double_sided: !lava,
        cull_mode: if lava {
            plain.and_then(|plain| plain.cull_mode).or(Some(Face::Back))
        } else {
            None
        },
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
    for (mut surface, material) in &mut surfaces {
        let count = surface.frames.len().max(1) as u64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let tick = (now / surface.frame_secs).floor().max(0.0) as u64;
        if surface.shown == Some(tick) {
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
    for material in &surfaces {
        if let Some(mut look) = looks.get_mut(&material.0) {
            let params = &mut look.extension.params;
            params.sky = sky;
            params.sun_direction = sun_direction;
            params.sun_color = sun_color;
        }
    }
}

/// The camera's own fog before the haze replaced it, if it had any.
struct Before(Option<DistanceFog>);

/// What the view under the surface adds once the scene is drawn
/// (`underwater.wgsl`): a gentle shimmer and light slanting down from the
/// surface. Mirrors `Underwater` there.
#[derive(Component, ExtractComponent, Clone, Copy, Default, ShaderType)]
pub(crate) struct UnderwaterView {
    /// The tint of the light shafts (rgb) and how strong they are (a).
    shafts: Vec4,
    /// Seconds on the wrapped clock (x), depth under the surface (y), 1
    /// for lava (z), and how far the camera looks up, -1 to 1 (w).
    state: Vec4,
}

impl FullscreenMaterial for UnderwaterView {
    fn fragment_shader() -> ShaderRef {
        "embedded://eq_client_render/underwater.wgsl".into()
    }
}

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
    suns: Query<&DirectionalLight, With<crate::daylight::Sun>>,
    mut cameras: Query<Divers, With<crate::camera::OrbitCamera>>,
    mut saved: Local<Option<Before>>,
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
    let sun = suns.iter().next().map_or(1.0, |light| {
        (light.illuminance / crate::daylight::DAY_SUN).clamp(0.0, 1.0)
    });
    let look_up = place.forward().y;
    let shafts = if lava {
        Vec4::ZERO
    } else {
        // Shafts fade with depth and at night.
        Vec4::new(0.55, 0.85, 0.8, 0.22 * (1.0 - down) * (0.25 + 0.75 * sun))
    };
    commands.entity(camera).insert(UnderwaterView {
        shafts,
        state: Vec4::new(
            time.elapsed_secs_wrapped(),
            depth,
            f32::from(u8::from(lava)),
            look_up,
        ),
    });
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
        let lava = ZoneRegions::test_liquid_surface(0.0, Liquid::Lava);
        assert_eq!(
            surface_liquid(&square(0.0, true), &lava, &none),
            Some(Liquid::Lava)
        );
    }
}
