use super::{CollisionWorld, Liquid, liquids::SOLES};
use glam::Vec3;

/// The local collision body's relationship to known water.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwimDepth {
    /// The body is supported by water while its head is clear.
    Surface,
    /// The body's head is also inside the water region.
    Submerged,
}

/// What the local collision world knows of the body at its probe points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiquidContact {
    /// No usable geometry or position for the query.
    Unknown,
    /// No water or lava at the body's sampled points.
    Unmarked,
    /// Water at the feet, below the swimming threshold.
    Wading,
    /// Water at or above the swimming threshold.
    Swimming(SwimDepth),
    /// Lava at the feet.
    Lava,
}

impl CollisionWorld {
    /// Classifies the body from local region data. The midpoint is the
    /// movement threshold; head contact does not establish breath state.
    #[must_use]
    pub fn liquid_contact(&self, feet: Vec3, height: f32) -> LiquidContact {
        if self.liquids.is_none() || !feet.is_finite() || !height.is_finite() || height < 1.0 {
            return LiquidContact::Unknown;
        }
        let soles = self.liquid_at(feet + Vec3::Y * SOLES);
        if soles == Some(Liquid::Lava) {
            LiquidContact::Lava
        } else if self.liquid_at(feet + Vec3::Y * (height * 0.5)) == Some(Liquid::Water) {
            let depth = if self.liquid_at(feet + Vec3::Y * height) == Some(Liquid::Water) {
                SwimDepth::Submerged
            } else {
                SwimDepth::Surface
            };
            LiquidContact::Swimming(depth)
        } else if soles == Some(Liquid::Water) {
            LiquidContact::Wading
        } else {
            LiquidContact::Unmarked
        }
    }

    /// Whether a local position can use swim strokes, including after a correction.
    #[must_use]
    pub fn swim_depth(&self, feet: Vec3, height: f32) -> Option<SwimDepth> {
        match self.liquid_contact(feet, height) {
            LiquidContact::Swimming(depth) => Some(depth),
            _ => None,
        }
    }

    /// Shortens an ordinary move to the first swimmable part of its path.
    /// The prefix test sees BSP leaves that an endpoint-only test could skip.
    pub(super) fn water_entry(&self, from: Vec3, to: Vec3, height: f32) -> Option<Vec3> {
        let water = self.liquids.as_ref()?;
        if self.swim_depth(from, height).is_some() {
            return Some(from);
        }
        let middle = Vec3::Y * (height * 0.5);
        let crosses = |at| water.passes_through(from + middle, at + middle, Liquid::Water);
        if !crosses(to) {
            return None;
        }
        let (mut dry, mut wet) = (0.0, 1.0);
        for _ in 0..16 {
            let t = f32::midpoint(dry, wet);
            if crosses(from.lerp(to, t)) {
                wet = t;
            } else {
                dry = t;
            }
        }
        // The point on a BSP split itself can be neither region.
        [wet, (wet + 0.000_02).min(1.0), 1.0]
            .into_iter()
            .map(|t| from.lerp(to, t))
            .find(|&point| self.swim_depth(point, height).is_some())
    }

    /// Sweeps a bounded stroke from water. A collision can spend the rest of
    /// the stroke along the solid; surfacing can spend it horizontally.
    /// The result stays in water or reaches a supported bank under its feet.
    #[must_use]
    pub fn swim_step(&self, feet: Vec3, delta: Vec3, height: f32) -> Vec3 {
        if self.swim_depth(feet, height).is_none()
            || !delta.is_finite()
            || delta.length_squared() <= 1e-10
        {
            return feet;
        }
        let Some((contact, normal)) = self.swim_sweep(feet, delta, height) else {
            return feet;
        };
        let position = self.limit_swim(feet, contact, height);
        if self.swim_depth(position, height).is_none() {
            return position;
        }
        let remaining = delta - (position - feet);
        let slide = if position.distance_squared(contact) > 1e-10 {
            Vec3::new(remaining.x, 0.0, remaining.z)
        } else if let Some(normal) = normal {
            remaining - normal * remaining.dot(normal)
        } else {
            return position;
        };
        if slide.length_squared() <= 1e-10 {
            return position;
        }
        self.swim_sweep(position, slide, height)
            .map_or(position, |(end, _)| self.limit_swim(position, end, height))
    }

    fn swim_sweep(&self, from: Vec3, delta: Vec3, height: f32) -> Option<(Vec3, Option<Vec3>)> {
        match self.flight_sweep(from, delta, height).ok()? {
            None => Some((from + delta, None)),
            Some(hit) => {
                let fraction = (hit.time_of_impact - 0.005 / delta.length()).clamp(0.0, 1.0);
                let normal = Vec3::new(hit.normal1.x, hit.normal1.y, hit.normal1.z);
                Some((from + delta * fraction, Some(normal)))
            }
        }
    }

    /// Clips the stroke before lava or an unsupported exit. The bank must
    /// support the capsule at its actual destination, not just be nearby.
    fn limit_swim(&self, from: Vec3, to: Vec3, height: f32) -> Vec3 {
        let Some(liquids) = &self.liquids else {
            return from;
        };
        let soles = Vec3::Y * SOLES;
        let lava = |at| liquids.passes_through(from + soles, at + soles, Liquid::Lava);
        if !lava(to)
            && (self.swim_depth(to, height).is_some()
                || (Vec3::new(to.x - from.x, 0.0, to.z - from.z).length_squared() > 1e-10
                    && self.ground(to, 0.01, 0.15).is_some()))
        {
            return to;
        }
        let (mut safe, mut blocked) = (0.0, 1.0);
        for _ in 0..16 {
            let t = f32::midpoint(safe, blocked);
            let point = from.lerp(to, t);
            if self.swim_depth(point, height).is_some() && !lava(point) {
                safe = t;
            } else {
                blocked = t;
            }
        }
        from.lerp(to, safe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movement::{AirborneController, MotionStep, PROVISIONAL_PHYSICS, liquids::Boxes};

    const HEIGHT: f32 = 6.0;

    fn flat(y: f32) -> [[[f32; 3]; 3]; 2] {
        [
            [[-20.0, y, -20.0], [20.0, y, -20.0], [20.0, y, 20.0]],
            [[-20.0, y, -20.0], [20.0, y, 20.0], [-20.0, y, 20.0]],
        ]
    }

    fn water() -> (Vec3, Vec3, Liquid) {
        (
            Vec3::new(0.0, -20.0, -20.0),
            Vec3::new(20.0, 0.0, 20.0),
            Liquid::Water,
        )
    }

    #[test]
    fn a_fall_stops_at_water_entry_and_later_strokes_dive_and_surface() {
        let world = CollisionWorld::new(flat(-12.0))
            .unwrap()
            .with_liquids(Boxes(vec![water()]));
        let mut airborne = AirborneController::default();
        let mut feet = Vec3::new(5.0, 4.0, 0.0);
        for _ in 0..40 {
            feet = airborne.step_into_water(
                &world,
                feet,
                PROVISIONAL_PHYSICS,
                MotionStep {
                    horizontal: Vec3::ZERO,
                    height: HEIGHT,
                    jump: false,
                    seconds: 0.05,
                },
            );
            if world.swim_depth(feet, HEIGHT).is_some() {
                break;
            }
        }
        assert_eq!(world.swim_depth(feet, HEIGHT), Some(SwimDepth::Surface));
        assert!((-3.01..=-3.0).contains(&feet.y), "{feet:?}");
        assert!(airborne.take_landing().is_none());
        let mut deep = feet;
        for _ in 0..8 {
            deep = world.swim_step(deep, Vec3::NEG_Y * 0.5, HEIGHT);
        }
        assert_eq!(world.swim_depth(deep, HEIGHT), Some(SwimDepth::Submerged));
        for _ in 0..12 {
            deep = world.swim_step(deep, Vec3::new(0.1, 0.5, 0.0), HEIGHT);
        }
        assert_eq!(world.swim_depth(deep, HEIGHT), Some(SwimDepth::Surface));
        assert!((-3.01..=-3.0).contains(&deep.y), "{deep:?}");
        assert!(
            deep.x > feet.x + 1.0,
            "surfacing should keep the horizontal stroke"
        );
    }

    #[test]
    fn a_swimmer_can_leave_on_a_supported_bank_but_not_into_air() {
        let ramp = [
            [[-10.0, 2.0, -20.0], [10.0, -8.0, -20.0], [10.0, -8.0, 20.0]],
            [[-10.0, 2.0, -20.0], [10.0, -8.0, 20.0], [-10.0, 2.0, 20.0]],
        ];
        let pool = (Vec3::new(-10.0, -20.0, -20.0), water().1, Liquid::Water);
        let world = CollisionWorld::new(ramp)
            .unwrap()
            .with_liquids(Boxes(vec![pool]));
        let mut feet = Vec3::new(1.0, -3.4, 0.0);
        assert_eq!(world.swim_depth(feet, HEIGHT), Some(SwimDepth::Surface));
        for _ in 0..10 {
            feet = world.swim_step(feet, Vec3::NEG_X * 0.4, HEIGHT);
            if world.swim_depth(feet, HEIGHT).is_none() {
                break;
            }
        }
        assert_eq!(
            world.liquid_contact(feet, HEIGHT),
            LiquidContact::Wading,
            "{feet:?}"
        );
        assert!(world.ground(feet, 0.01, 0.15).is_some(), "{feet:?}");
        assert!(world.step(feet, Vec3::NEG_X * 0.2, HEIGHT).x < feet.x);

        let bottom = CollisionWorld::new(flat(-12.0))
            .unwrap()
            .with_liquids(Boxes(vec![water()]));
        let feet = Vec3::new(0.3, -3.1, 0.0);
        let held = bottom.swim_step(feet, Vec3::NEG_X, HEIGHT);
        assert!(held.x >= 0.0 && held.x < 0.01, "{held:?}");
        assert_eq!(bottom.swim_depth(held, HEIGHT), Some(SwimDepth::Surface));
    }

    #[test]
    fn swim_collision_keeps_walls_floors_and_thin_lava_solid() {
        let wall = [
            [[8.0, -12.0, -20.0], [8.0, 8.0, -20.0], [8.0, 8.0, 20.0]],
            [[8.0, -12.0, -20.0], [8.0, 8.0, 20.0], [8.0, -12.0, 20.0]],
        ];
        let lava = (
            Vec3::new(5.1, -7.0, -20.0),
            Vec3::new(5.11, -5.0, 20.0),
            Liquid::Lava,
        );
        let world = CollisionWorld::new(flat(-12.0).into_iter().chain(wall))
            .unwrap()
            .with_liquids(Boxes(vec![lava, water()]));
        let feet = Vec3::new(5.0, -6.0, 0.0);
        let held = world.swim_step(feet, Vec3::X, HEIGHT);
        assert!(held.x >= 5.0 && held.x < 5.1, "{held:?}");
        let opposite = Vec3::new(7.0, -6.0, 0.0);
        let held = world.swim_step(opposite, Vec3::new(2.0, 0.0, 0.4), HEIGHT);
        assert!(held.x < 7.6 && held.z > 0.05, "{held:?}");
        let floor = world.swim_step(feet, Vec3::NEG_Y * 10.0, HEIGHT);
        assert!(floor.y >= -12.0 && floor.y < -11.9, "{floor:?}");
    }
}
