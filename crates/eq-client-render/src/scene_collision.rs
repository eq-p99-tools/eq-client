//! Static zone geometry and region adapters shared by movement and navigation.
use bevy::prelude::*;
use eq_client_assets::{ZoneAsset, ZonePrimitive};
#[derive(Resource)]
pub(super) struct TerrainSurface(pub(super) Vec<[Vec3; 3]>);

/// Where a zone object sits; the assets crate already placed it in the
/// renderer's frame.
pub(super) fn object_transform(object: &eq_client_assets::ZoneObject) -> Transform {
    Transform {
        translation: Vec3::from_array(object.translation),
        rotation: Quat::from_array(object.rotation),
        scale: Vec3::from_array(object.scale),
    }
}

/// The zone's water and lava as its regions mark them, which every walk keeps
/// out of.
pub(super) struct ZoneLiquids(pub(super) eq_client_assets::regions::ZoneRegions);

impl eq_client_core::movement::Liquids for ZoneLiquids {
    fn cache_fingerprint(&self) -> Option<u64> {
        Some(self.0.liquid_fingerprint())
    }

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

    fn within(&self, low: Vec3, high: Vec3) -> Option<Box<dyn eq_client_core::movement::Liquids>> {
        let regions = self.0.within(low.to_array(), high.to_array())?;
        Some(Box::new(Self(regions)))
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
/// A zone's solid geometry as movement and the camera meet it: its own
/// collision with its placed objects', and its water and lava; None where
/// it has none that can be built. For tools that weigh a zone's shape, such
/// as the character select spot proposer.
#[must_use]
pub fn zone_collision(zone: &ZoneAsset) -> Option<eq_client_core::movement::CollisionWorld> {
    build_collision(zone)
}

pub(super) fn build_collision(
    zone: &ZoneAsset,
) -> Option<eq_client_core::movement::CollisionWorld> {
    collision_world(zone, collision_triangles(zone))
}

/// The zone's collision triangles with every placed object's: the solids the
/// walker meets, doors aside.
pub(super) fn collision_triangles(zone: &ZoneAsset) -> Vec<[[f32; 3]; 3]> {
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
    triangles
}

pub(super) fn collision_world(
    zone: &ZoneAsset,
    triangles: Vec<[[f32; 3]; 3]>,
) -> Option<eq_client_core::movement::CollisionWorld> {
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
    pub(super) fn from_primitives(primitives: &[ZonePrimitive]) -> Self {
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

    pub(super) fn height_below(&self, x: f32, z: f32, ceiling: f32) -> Option<f32> {
        self.0
            .iter()
            .filter_map(|triangle| triangle_height_at(*triangle, x, z))
            .filter(|height| *height <= ceiling)
            .max_by(f32::total_cmp)
    }

    pub(super) fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        self.0
            .iter()
            .filter_map(|triangle| triangle_height_at(*triangle, x, z))
            .max_by(f32::total_cmp)
    }
}

pub(super) fn triangle_height_at(
    triangle: [Vec3; 3],
    position_x: f32,
    position_z: f32,
) -> Option<f32> {
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
