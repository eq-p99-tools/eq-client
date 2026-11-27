//! Actual immutable collision/body provider for the bounded wet-frame owner.
use super::{CollisionWorld, LiquidContact, Obstruction, Surface, SwimDepth};
use anyhow::{Result, ensure};
use eq_network_game::movement::{SwimScene, water::WetKind};
use glam::Vec3;
use std::sync::Arc;

// This bounds scene continuity, not allowed movement distance or report timing.
const REACH: f32 = 256.0;

/// One actual local scene/body snapshot used by producer and independent replay.
/// The established conservative capsule and solid-floor ray are retained. This
/// is not a claim to reproduce the native engine's unread body/slide internals.
#[derive(Clone)]
pub struct SwimCollision {
    world: CollisionWorld,
    center: Vec3,
    height: f32,
    offset: f32,
}

impl SwimCollision {
    /// Captures the actual collision world and supported standing body.
    /// Moving geometry is retained as immutable placements; a changed relevant
    /// placement later retires continuity. No reset or velocity is asserted.
    #[must_use]
    pub fn new(world: &CollisionWorld, origin: Vec3, height: f32, offset: f32) -> Option<Self> {
        if !origin.is_finite()
            || !height.is_finite()
            || height < 1.0
            || !offset.is_finite()
            || offset <= 0.0
            || world.liquid_support
        {
            return None;
        }
        let snapshot = nearby(world, origin);
        (snapshot.liquid_contact(origin - Vec3::Y * offset, height)
            == LiquidContact::Swimming(SwimDepth::Submerged))
        .then_some(Self {
            world: snapshot,
            center: origin,
            height,
            offset,
        })
    }

    /// Whether actual body/geometry still matches this retained snapshot. Remote
    /// objects outside its bounded column cannot affect any supported query.
    #[must_use]
    #[allow(clippy::float_cmp, reason = "Exact immutable body/scene identity")]
    pub fn matches(&self, world: &CollisionWorld, height: f32, offset: f32) -> bool {
        let next = nearby(world, self.center);
        let same_liquids = match (&self.world.liquids, &next.liquids) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        height == self.height
            && offset == self.offset
            && !world.liquid_support
            && world.collision_height == self.world.collision_height
            && world.swim_origin == self.world.swim_origin
            && Arc::ptr_eq(&self.world.terrain, &next.terrain)
            && same_liquids
            && self.world.obstacles.len() == next.obstacles.len()
            && self
                .world
                .obstacles
                .iter()
                .zip(next.obstacles.iter())
                .all(|(a, b)| a.0 == b.0 && Arc::ptr_eq(&a.1, &b.1) && a.2 == b.2)
            && self.world.spawn_obstacles.len() == next.spawn_obstacles.len()
            && self
                .world
                .spawn_obstacles
                .iter()
                .zip(next.spawn_obstacles.iter())
                .all(|(a, b)| a.0 == b.0 && a.1.same_placement(&b.1))
    }
    fn render(&self, origin: [f32; 3]) -> Result<Vec3> {
        let at = Vec3::new(origin[1], origin[2], -origin[0]);
        ensure!(
            at.is_finite()
                && (at.x - self.center.x).abs() < REACH - 8.0
                && (at.z - self.center.z).abs() < REACH - 8.0,
            "swim scene coverage ended"
        );
        Ok(at)
    }
}
fn eq(render: Vec3) -> [f32; 3] {
    [-render.z, render.x, render.y]
}
fn within(surface: Surface<'_>, center: Vec3) -> bool {
    let bounds = surface.mesh.local_aabb().transform_by(&surface.pose);
    bounds.mins.x <= center.x + REACH
        && bounds.maxs.x >= center.x - REACH
        && bounds.mins.z <= center.z + REACH
        && bounds.maxs.z >= center.z - REACH
}
fn nearby(world: &CollisionWorld, center: Vec3) -> CollisionWorld {
    let obstacles: Vec<Obstruction> = world
        .obstacles
        .iter()
        .filter(|(_, mesh, _)| within(Surface::identity(mesh), center))
        .cloned()
        .collect();
    let spawns: Vec<_> = world
        .spawn_obstacles
        .iter()
        .filter(|(_, instance)| within(instance.surface(), center))
        .cloned()
        .collect();
    CollisionWorld {
        obstacles: obstacles.into(),
        spawn_obstacles: spawns.into(),
        observation: None,
        ..world.clone()
    }
}
impl SwimScene for SwimCollision {
    fn step_height(&self) -> f32 {
        self.offset
    }
    fn kind(&self, origin: [f32; 3]) -> Result<WetKind> {
        let at = self.render(origin)?;
        ensure!(
            self.world
                .liquid_contact(at - Vec3::Y * self.offset, self.height)
                == LiquidContact::Swimming(SwimDepth::Submerged),
            "unsupported swim region"
        );
        Ok(self.world.wet_kind(at - Vec3::Y * self.offset, self.height))
    }
    fn floor(&self, origin: [f32; 3]) -> Result<Option<f32>> {
        Ok(self.world.solid_ground(self.render(origin)?, 0.0, 32767.0))
    }
    fn sweep(&self, from: [f32; 3], target: [f32; 3]) -> Result<[f32; 3]> {
        let start = self.render(from)?;
        let target = self.render(target)?;
        let feet = start - Vec3::Y * self.offset;
        let delta = target - start;
        // Pure vertical idle/gravity cannot invent sideways travel from the
        // fallback projection. Retain the same capsule's first obstruction.
        let moved = if delta.x == 0.0 && delta.z == 0.0 {
            self.world
                .wet_vertical_step_checked(feet, delta, self.height)
                .map_err(|()| anyhow::anyhow!("unknown vertical swim sweep"))?
        } else {
            self.world
                .wet_step_checked(feet, delta, self.height)
                .map_err(|()| anyhow::anyhow!("unsupported swim collision query"))?
        };
        Ok(eq(moved + Vec3::Y * self.offset))
    }
    fn clear_lift(&self, from: [f32; 3], target: [f32; 3]) -> Result<bool> {
        let from = self.render(from)? - Vec3::Y * self.offset;
        let to = self.render(target)? - Vec3::Y * self.offset;
        Ok(self
            .world
            .flight_sweep(from, to - from, self.height)
            .map_err(|_| anyhow::anyhow!("unknown swim floor lift"))?
            .is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movement::{
        Liquid, SwimSceneHandle, SwimSceneMotion,
        liquids::Boxes,
        water::{LIQUID_5, WetInput},
    };
    fn bottom(slope: f32) -> (CollisionWorld, SwimCollision) {
        let at = |x: f32, z: f32| [x, -12.0 + slope * x, z];
        let world = CollisionWorld::new([
            [at(-100.0, -100.0), at(100.0, 100.0), at(100.0, -100.0)],
            [at(-100.0, -100.0), at(-100.0, 100.0), at(100.0, 100.0)],
        ])
        .unwrap()
        .with_swim_origin(3.75)
        .with_liquids(Boxes(vec![(
            Vec3::splat(-100.0),
            Vec3::new(100.0, 0.0, 100.0),
            Liquid::Water,
        )]));
        let scene =
            SwimCollision::new(&world, Vec3::new(0.0, -8.25, 0.0), 6.677_319_5, 3.75).unwrap();
        (world, scene)
    }
    #[test]
    fn real_conservative_bottom_supports_both_directions() {
        for slope in [-0.08_f32, 0.08] {
            for sign in [-1.0_f32, 1.0] {
                let (_, scene) = bottom(slope);
                let mut model =
                    SwimSceneMotion::at_reset(SwimSceneHandle::new(scene), [0.0, 0.0, -8.25])
                        .unwrap();
                let mut path = 0.0;
                for tick in 0..120 {
                    let before = model.origin();
                    let input = WetInput {
                        horizontal: [0.0, sign * 0.6],
                        ..WetInput::default()
                    };
                    let next = model.frame(LIQUID_5, input, 0.016).unwrap_or_else(|error| {
                        panic!("slope={slope} sign={sign} tick={tick}: {error}")
                    });
                    path += (next.1.origin[0] - before[0]).hypot(next.1.origin[1] - before[1]);
                    model = next.0;
                }
                assert!(sign * model.origin()[1] > 8.0);
                assert!(model.origin()[0].hypot(model.origin()[1]) <= path + 0.001);
            }
        }
    }
    #[test]
    fn real_bottom_idle_has_no_horizontal_projection_and_body_change_retires_scene() {
        let (world, scene) = bottom(0.08);
        assert!(scene.matches(&world, 6.677_319_5, 3.75));
        assert!(!scene.matches(&world, 6.677_319_5, 3.0));
        let mut model =
            SwimSceneMotion::at_reset(SwimSceneHandle::new(scene), [0.0, 0.0, -8.25]).unwrap();
        for _ in 0..600 {
            model = model
                .frame(LIQUID_5, WetInput::default(), 0.016_667)
                .unwrap()
                .0;
        }
        assert!(model.origin()[0].hypot(model.origin()[1]) < 0.001);
        assert!((model.origin()[2] + 8.25).abs() < 0.001);
    }
    #[test]
    fn vertical_gravity_sweep_cannot_cross_a_dry_gap_between_known_water() {
        let at = |x: f32, z: f32| [x, -90.0, z];
        let world = CollisionWorld::new([
            [at(-100.0, -100.0), at(100.0, 100.0), at(100.0, -100.0)],
            [at(-100.0, -100.0), at(-100.0, 100.0), at(100.0, 100.0)],
        ])
        .unwrap()
        .with_swim_origin(3.75)
        .with_liquids(Boxes(vec![
            (
                Vec3::new(-100.0, -12.0, -100.0),
                Vec3::new(100.0, 0.0, 100.0),
                Liquid::Water,
            ),
            (
                Vec3::splat(-100.0),
                Vec3::new(100.0, -13.0, 100.0),
                Liquid::Water,
            ),
        ]));
        let scene =
            SwimCollision::new(&world, Vec3::new(0.0, -8.0, 0.0), 6.677_319_5, 3.75).unwrap();
        assert!(
            scene.kind([0.0, 0.0, -17.0]).is_ok(),
            "endpoint alone is insufficient"
        );
        let end = scene.sweep([0.0, 0.0, -8.0], [0.0, 0.0, -17.0]).unwrap();
        assert_eq!([end[0], end[1]], [0.0; 2]);
        assert!(
            end[2] > -13.0,
            "ordinary liquid-gap clipping was bypassed: {end:?}"
        );
    }

    #[test]
    fn actual_scene_identity_and_coverage_are_required() {
        let (world, scene) = bottom(0.08);
        let (other, _) = bottom(0.08);
        assert!(!scene.matches(&other, 6.677_319_5, 3.75));
        assert!(scene.kind([0.0, 1000.0, -8.25]).is_err());
        assert!(SwimCollision::new(&world, Vec3::new(0.0, 10.0, 0.0), 6.677_319_5, 3.75).is_none());
    }
}
