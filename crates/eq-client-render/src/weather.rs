//! Procedural precipitation for the gameplay camera. Zone authority lives in
//! [`ClientWorld`]. This module only chooses how to draw it.
use super::{OrbitCamera, SceneInfo, online::OnlineState, options::OptionsState};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    picking::Pickable,
    prelude::*,
};
use eq_client_core::{clock::Weather, qol::Fix, world::ClientWorld};

const VOLUME: Vec3 = Vec3::new(96.0, 76.0, 96.0);
const LOOK_AHEAD: f32 = 20.0;
const FADE_SECONDS: f32 = 0.5;

/// Presentation tuning in world units. Density uses `EQEmu`'s ordinary 1–10
/// intensity range; the speed, size, color and budget are inferred.
struct Style {
    limit: u16,
    velocity: Vec3,
    half_size: Vec2,
    color: [f32; 4],
    seed: u32,
}

const RAIN: Style = Style {
    limit: 480,
    velocity: Vec3::new(13.0, -110.0, 4.0),
    half_size: Vec2::new(0.04, 0.9),
    color: [0.53, 0.66, 0.80, 0.57],
    seed: 1,
};
const SNOW: Style = Style {
    limit: 320,
    velocity: Vec3::new(4.0, -12.0, 2.0),
    half_size: Vec2::new(0.13, 0.11),
    color: [0.90, 0.93, 1.0, 0.78],
    seed: 2,
};

/// Only draw weather in the scene for this admission. A sky of zero is the
/// header's explicit no-sky marker. A missing sky defers drawing.
fn visible_weather(
    world: &ClientWorld,
    scene: &SceneInfo,
    clearly: bool,
) -> Option<(u64, [f32; 2])> {
    if clearly
        || !world.connected()
        || scene.zone_name.is_empty()
        || !scene.zone_name.eq_ignore_ascii_case(world.zone())
        || world.sky().is_none_or(|sky| sky.sky == 0)
    {
        return None;
    }
    #[allow(clippy::cast_precision_loss, reason = "intensity is capped at 10")]
    let strength = |value: u32| value.min(10) as f32 / 10.0;
    let density = match world.weather()? {
        Weather::Rain { intensity } => [strength(intensity), 0.0],
        Weather::Snow { intensity } => [0.0, strength(intensity)],
        Weather::Clear => [0.0, 0.0],
        Weather::Unknown { .. } => return None,
    };
    Some((world.session_id()?, density))
}

#[derive(Default)]
struct Layer {
    density: f32,
    travel: Vec3,
}

impl Layer {
    fn step(&mut self, target: f32, velocity: Vec3, dt: f32) {
        let change = dt / FADE_SECONDS;
        let difference = target - self.density;
        self.density = if difference.abs() <= change {
            target
        } else {
            self.density + difference.signum() * change
        };
        if self.density > 0.0 {
            self.travel = (self.travel + velocity * dt).rem_euclid(VOLUME);
        }
    }
}

/// The two independently fading layers within the same bounded mesh.
#[derive(Default, Resource)]
pub(super) struct Field {
    admission: Option<u64>,
    rain: Layer,
    snow: Layer,
}

impl Field {
    fn step(&mut self, admission: u64, [rain, snow]: [f32; 2], dt: f32) {
        if self.admission != Some(admission) {
            *self = Self {
                admission: Some(admission),
                ..default()
            };
        }
        self.rain.step(rain, RAIN.velocity, dt);
        self.snow.step(snow, SNOW.velocity, dt);
    }

    fn geometry(&self, camera: &Transform) -> (Vec3, Quads) {
        let forward = camera.rotation * Vec3::NEG_Z;
        let right = camera.rotation * Vec3::X;
        let up = camera.rotation * Vec3::Y;
        let lead = forward * LOOK_AHEAD;
        let anchor = camera.translation + lead;
        let rain_count = self.rain.density * f32::from(RAIN.limit);
        let snow_count = self.snow.density * f32::from(SNOW.limit);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // Both counts are bounded by their layer's limit.
        let capacity = (rain_count.ceil() + snow_count.ceil()) as usize;
        let mut quads = Quads {
            positions: Vec::with_capacity(capacity * 4),
            colors: Vec::with_capacity(capacity * 4),
            indices: Vec::with_capacity(capacity * 6),
        };
        let rain_axis = -RAIN.velocity.normalize();
        let rain_side = forward.cross(rain_axis).normalize_or(right);
        quads.add_layer(
            &RAIN,
            (rain_count, self.rain.travel),
            (anchor, lead),
            (rain_side, rain_axis),
        );
        // Tilt the short flakes in their billboard plane.
        let diagonal = std::f32::consts::FRAC_1_SQRT_2;
        quads.add_layer(
            &SNOW,
            (snow_count, self.snow.travel),
            (anchor, lead),
            ((right + up) * diagonal, (up - right) * diagonal),
        );
        (anchor, quads)
    }
}

struct Quads {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Quads {
    /// Sample a repeatable field in world space and wrap it around the eye.
    /// Edge fades hide each wrap. Only the visible density's slots are built.
    fn add_layer(
        &mut self,
        style: &Style,
        (count, travel): (f32, Vec3),
        (anchor, lead): (Vec3, Vec3),
        (right, up): (Vec3, Vec3),
    ) {
        let mut seed = style.seed;
        let offset = travel - anchor.rem_euclid(VOLUME);
        let half = VOLUME * 0.5;
        for slot in 0..style.limit {
            let weight = (count - f32::from(slot)).min(1.0);
            if weight <= 0.0 {
                break;
            }
            let initial = Vec3::new(sample(&mut seed), sample(&mut seed), sample(&mut seed));
            let center = (initial * VOLUME + offset + half).rem_euclid(VOLUME) - half;
            let scale = 0.7 + 0.6 * sample(&mut seed);
            let edge = ((half - center.abs()).min_element() / 8.0).clamp(0.0, 1.0);
            let near = (((center + lead).length() - 4.0) / 8.0).clamp(0.0, 1.0);
            let [red, green, blue, alpha] = style.color;
            self.push(
                center,
                right * style.half_size.x * scale,
                up * style.half_size.y * scale,
                [red, green, blue, alpha * weight * edge * near],
            );
        }
    }

    fn push(&mut self, center: Vec3, right: Vec3, up: Vec3, color: [f32; 4]) {
        let first = u32::try_from(self.positions.len()).expect("bounded weather mesh");
        self.positions.extend(
            [-right - up, right - up, right + up, -right + up]
                .map(|corner| (center + corner).to_array()),
        );
        self.colors.extend([color; 4]);
        self.indices
            .extend([0, 1, 2, 0, 2, 3].map(|index| first + index));
    }

    fn write(self, mesh: &mut Mesh) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
    }
}

/// A deterministic sample in [0, 1), with no global random resource.
fn sample(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    f32::from_bits(0x3f80_0000 | (*seed >> 9)) - 1.0
}

pub(super) struct Drawn {
    entity: Entity,
    mesh: Handle<Mesh>,
}

/// Draw in a single depth-tested mesh, using the scene camera's grading and
/// distance fog. Release the mesh when there is no visible precipitation.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    time: Res<Time>,
    online: Res<OnlineState>,
    scene: Res<SceneInfo>,
    options: Res<OptionsState>,
    cameras: Query<(&Camera, &Transform), With<OrbitCamera>>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    (mut field, mut drawn): (ResMut<Field>, Local<Option<Drawn>>),
) {
    let weather = visible_weather(
        online.world(),
        &scene,
        options.options.qol.on(Fix::ClearView),
    );
    let camera = cameras.single().ok().filter(|(camera, transform)| {
        camera.is_active && transform.translation.is_finite() && transform.rotation.is_finite()
    });
    if let (Some((admission, density)), Some((_, camera))) = (weather, camera) {
        field.step(admission, density, time.delta_secs());
        if field.rain.density > 0.0 || field.snow.density > 0.0 {
            let (anchor, quads) = field.geometry(camera);
            let transform = Transform::from_translation(anchor);
            if let Some(existing) = &*drawn {
                if let Some(mut mesh) = meshes.get_mut(&existing.mesh) {
                    quads.write(&mut mesh);
                }
                commands.entity(existing.entity).insert(transform);
            } else {
                let mut mesh = Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::default(),
                );
                quads.write(&mut mesh);
                let mesh = meshes.add(mesh);
                let material = materials.add(StandardMaterial {
                    alpha_mode: AlphaMode::Blend,
                    cull_mode: None,
                    unlit: true,
                    ..default()
                });
                let entity = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material),
                        transform,
                        NoFrustumCulling,
                        NotShadowCaster,
                        Pickable::IGNORE,
                    ))
                    .id();
                *drawn = Some(Drawn { entity, mesh });
            }
            return;
        }
    } else {
        *field = Field::default();
    }
    if let Some(old) = drawn.take() {
        commands.entity(old.entity).despawn();
    }
}

#[cfg(test)]
mod tests;
