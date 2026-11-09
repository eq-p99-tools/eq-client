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

/// What the ordinary collision capsule could do on one shoreline frame.
pub struct ShorelineStep {
    /// Swept feet position.
    pub feet: Vec3,
    /// The rise or descent struck a solid face.
    pub stop_vertical: bool,
    /// The feet reached ground while outside swimming water.
    pub supported: bool,
}

impl CollisionWorld {
    /// The native shoreline probe, with the dry region required to resolve.
    #[must_use]
    pub fn shoreline_ahead(&self, feet: Vec3, heading: Vec3, feet_offset: f32) -> bool {
        self.liquids.as_ref().is_some_and(|liquids| {
            let origin = feet + Vec3::Y * feet_offset;
            let ahead = origin + heading * feet_offset - Vec3::Y;
            liquids.region_known(ahead) && liquids.liquid_at(ahead).is_none()
        })
    }

    /// Sweep one frame of an already bounded shoreline impulse. Use separate
    /// axes so a wall can consume forward motion while a clear ascent continues.
    #[must_use]
    pub fn shoreline_step(
        &self,
        feet: Vec3,
        horizontal: Vec3,
        vertical: f32,
        height: f32,
    ) -> Option<ShorelineStep> {
        let liquids = self.liquids.as_ref()?;
        let sweep = |from: Vec3, delta: Vec3| -> Option<(Vec3, bool)> {
            if delta.length_squared() <= 1e-10 {
                return Some((from, false));
            }
            let (to, hit) = self.swim_sweep(from, delta, height)?;
            let soles = Vec3::Y * SOLES;
            let origin = self.swim_probe(height);
            (liquids.segment_known(from + origin, to + origin)
                && liquids.segment_known(from + soles, to + soles)
                && !liquids.passes_through(from + soles, to + soles, Liquid::Lava)
                && !liquids.passes_through(from + origin, to + origin, Liquid::Lava))
            .then_some((to, hit.is_some()))
        };
        let rise = Vec3::Y * vertical;
        let (feet, stop_vertical) = if vertical > 0.0 {
            let (raised, stop_vertical) = sweep(feet, rise)?;
            let (moved, _) = sweep(raised, horizontal)?;
            (moved, stop_vertical)
        } else {
            let (moved, _) = sweep(feet, horizontal)?;
            let (lowered, stop_vertical) = sweep(moved, rise)?;
            (lowered, stop_vertical)
        };
        let supported = vertical <= 0.0
            && !self.in_swimmable_water(feet, height)
            && self.ground(feet, 0.01, 0.15).is_some();
        Some(ShorelineStep {
            feet,
            stop_vertical,
            supported,
        })
    }

    /// Uses the admitted Titanium actor origin for wet movement. The
    /// program probes the transmitted origin at 0x489b0a and stores the
    /// body region through 0x47cae6. The offset is measured from our soles.
    #[must_use]
    pub fn with_swim_origin(mut self, offset: f32) -> Self {
        self.swim_origin = Some(offset);
        self
    }

    fn swim_probe(&self, height: f32) -> Vec3 {
        Vec3::Y * self.swim_origin.unwrap_or(height * 0.5)
    }

    /// Classifies the local collision body's sampled points. The
    /// separately resolved native head reference determines breath state.
    #[must_use]
    pub fn liquid_contact(&self, feet: Vec3, height: f32) -> LiquidContact {
        let Some(liquids) = &self.liquids else {
            return LiquidContact::Unknown;
        };
        if !feet.is_finite() || !height.is_finite() || height < 1.0 {
            return LiquidContact::Unknown;
        }
        let soles = feet + Vec3::Y * SOLES;
        let origin = feet + self.swim_probe(height);
        let head = feet + Vec3::Y * height;
        match (liquids.liquid_at(soles), liquids.liquid_at(origin)) {
            (Some(Liquid::Lava), _) => LiquidContact::Lava,
            (_, Some(Liquid::Water)) => match liquids.liquid_at(head) {
                Some(Liquid::Water) => LiquidContact::Swimming(SwimDepth::Submerged),
                _ if liquids.region_known(head) => LiquidContact::Swimming(SwimDepth::Surface),
                _ => LiquidContact::Unknown,
            },
            _ if !liquids.region_known(soles) || !liquids.region_known(origin) => {
                LiquidContact::Unknown
            }
            (Some(Liquid::Water), _) => LiquidContact::Wading,
            _ => LiquidContact::Unmarked,
        }
    }

    /// The collision body's water depth when both wet origin and head
    /// region resolve. This is independent of the native breath probe.
    #[must_use]
    pub fn swim_depth(&self, feet: Vec3, height: f32) -> Option<SwimDepth> {
        match self.liquid_contact(feet, height) {
            LiquidContact::Swimming(depth) => Some(depth),
            _ => None,
        }
    }

    /// Whether a local position can use swim strokes, including after a
    /// correction. A missing head sample does not change a known wet origin.
    #[must_use]
    pub fn in_swimmable_water(&self, feet: Vec3, height: f32) -> bool {
        feet.is_finite()
            && height.is_finite()
            && height >= 1.0
            && self.liquid_at(feet + self.swim_probe(height)) == Some(Liquid::Water)
            && self.liquid_at(feet + Vec3::Y * SOLES) != Some(Liquid::Lava)
    }

    /// Shortens an ordinary move to the first swimmable part of its path.
    /// The prefix test sees BSP leaves that an endpoint-only test could skip.
    pub(super) fn water_entry(&self, from: Vec3, to: Vec3, height: f32) -> Option<Vec3> {
        let water = self.liquids.as_ref()?;
        if self.in_swimmable_water(from, height) {
            return Some(from);
        }
        let probe = self.swim_probe(height);
        let crosses = |at| water.passes_through(from + probe, at + probe, Liquid::Water);
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
            .find(|&point| {
                self.in_swimmable_water(point, height)
                    && !water.passes_through(
                        from + Vec3::Y * SOLES,
                        point + Vec3::Y * SOLES,
                        Liquid::Lava,
                    )
                    && self
                        .clearance(point, height, 0.0)
                        .is_none_or(|distance| distance >= 0.0)
            })
    }

    /// Sweeps a bounded stroke from water. A collision can spend the rest of
    /// the stroke along the solid; surfacing can spend it horizontally.
    /// The result stays in water or reaches a supported bank under its feet.
    #[must_use]
    pub fn swim_step(&self, feet: Vec3, delta: Vec3, height: f32) -> Vec3 {
        if !self.in_swimmable_water(feet, height)
            || !delta.is_finite()
            || delta.length_squared() <= 1e-10
        {
            return feet;
        }
        let Some((contact, normal)) = self.swim_sweep(feet, delta, height) else {
            return feet;
        };
        let position = self.limit_swim(feet, contact, height);
        if !self.in_swimmable_water(position, height) {
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

    /// Clips before the first break in water or lava, even if the stroke
    /// would end in water. A bank hands off at the water boundary.
    fn limit_swim(&self, from: Vec3, to: Vec3, height: f32) -> Vec3 {
        let Some(liquids) = &self.liquids else {
            return from;
        };
        let soles = Vec3::Y * SOLES;
        let lava = |at| liquids.passes_through(from + soles, at + soles, Liquid::Lava);
        let probe = self.swim_probe(height);
        let to = if let Some(gap) = liquids.first_outside(from + probe, to + probe, Liquid::Water) {
            if gap.is_empty() {
                return from;
            }
            let at = |fraction| from.as_dvec3().lerp(to.as_dvec3(), fraction).as_vec3();
            let edge = at(gap.start);
            let horizontal = (to.x - from.x).hypot(to.z - from.z);
            if horizontal > 1e-5
                && self.ground(edge, 0.01, 0.15).is_some()
                && let Some(shore) = self.shore_point(at, &gap, height)
                && !lava(shore)
            {
                return shore;
            }
            edge
        } else if self.in_swimmable_water(to, height) && !lava(to) {
            return to;
        } else {
            to
        };
        // Interpolation back to f32 can round the edge onto the far side
        // of a very thin gap. Validate the actual proposed point as well.
        let clear = |point| {
            self.in_swimmable_water(point, height)
                && !lava(point)
                && liquids
                    .first_outside(from + probe, point + probe, Liquid::Water)
                    .is_none()
        };
        if clear(to) {
            return to;
        }
        let (mut safe, mut blocked) = (0.0, 1.0);
        for _ in 0..16 {
            let t = f32::midpoint(safe, blocked);
            let point = from.lerp(to, t);
            if clear(point) {
                safe = t;
            } else {
                blocked = t;
            }
        }
        from.lerp(to, safe)
    }

    /// Finds the nearest representable position on the bank's side of the
    /// split. Leave the remainder of the dry path to ordinary movement.
    fn shore_point(
        &self,
        at: impl Fn(f64) -> Vec3,
        gap: &std::ops::Range<f64>,
        height: f32,
    ) -> Option<Vec3> {
        let (mut wet, mut dry) = (gap.start, f64::midpoint(gap.start, gap.end));
        let edge = at(wet);
        let wade_or_walk = |point| {
            matches!(
                self.liquid_contact(point, height),
                LiquidContact::Wading | LiquidContact::Unmarked
            )
        };
        if wade_or_walk(edge) {
            return Some(edge);
        }
        if !wade_or_walk(at(dry)) {
            return None;
        }
        for _ in 0..32 {
            let middle = f64::midpoint(wet, dry);
            if wade_or_walk(at(middle)) {
                dry = middle;
            } else {
                wet = middle;
            }
        }
        let shore = at(dry);
        self.ground(shore, 0.01, 0.15).is_some().then_some(shore)
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
    fn titanium_water_entry_and_surfacing_use_the_origin_above_the_soles() {
        let world = CollisionWorld::new(flat(-12.0))
            .unwrap()
            .with_liquids(Boxes(vec![water()]));
        let start = Vec3::new(5.0, -3.5, 0.0);
        let admitted = world.clone().with_swim_origin(4.0);
        assert!(world.in_swimmable_water(start, HEIGHT));
        assert_eq!(
            admitted.liquid_contact(start, HEIGHT),
            LiquidContact::Wading
        );
        let entry = admitted
            .water_entry(start, start + Vec3::NEG_Y, HEIGHT)
            .unwrap();
        assert!((-4.001..=-4.0).contains(&entry.y));
        assert_eq!(admitted.swim_depth(entry, HEIGHT), Some(SwimDepth::Surface));
        let below = entry + Vec3::NEG_Y * 3.0;
        assert_eq!(
            admitted.swim_depth(below, HEIGHT),
            Some(SwimDepth::Submerged)
        );
        let surface = admitted.swim_step(below, Vec3::Y * 4.0, HEIGHT);
        assert!((-4.001..=-4.0).contains(&surface.y));
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
    fn a_swim_stroke_cannot_cross_a_dry_gap_between_water_volumes() {
        let (low, high, kind) = water();
        let pools = Boxes(vec![
            (low, Vec3::new(1.0, high.y, high.z), kind),
            (Vec3::new(1.05, low.y, low.z), high, kind),
        ]);
        let world = CollisionWorld::new(flat(-12.0))
            .unwrap()
            .with_liquids(pools);
        let feet = Vec3::new(0.9, -3.1, 0.0);
        let delta = Vec3::X * 0.3;
        assert_eq!(
            world.swim_depth(feet + delta, HEIGHT),
            Some(SwimDepth::Surface)
        );
        let edge = world.swim_step(feet, delta, HEIGHT);
        assert!((0.99..=1.0).contains(&edge.x), "{edge:?}");
        let next = world.swim_step(edge, delta, HEIGHT);
        assert!(next.x <= 1.0, "{next:?}");
    }

    #[test]
    fn a_supported_endpoint_does_not_allow_swimming_across_an_unsupported_exit() {
        let bank = [
            [
                [1.05, -3.19, -20.0],
                [20.0, -3.19, -20.0],
                [20.0, -3.19, 20.0],
            ],
            [
                [1.05, -3.19, -20.0],
                [20.0, -3.19, 20.0],
                [1.05, -3.19, 20.0],
            ],
        ];
        let (low, high, kind) = water();
        let pool = (low, Vec3::new(1.0, high.y, high.z), kind);
        let world = CollisionWorld::new(flat(-12.0).into_iter().chain(bank))
            .unwrap()
            .with_liquids(Boxes(vec![pool]));
        let feet = Vec3::new(0.9, -3.1, 0.0);
        let delta = Vec3::X * 0.3;
        assert!(world.ground(feet + delta, 0.01, 0.15).is_some());
        assert!(
            world
                .ground(Vec3::new(1.0, -3.1, 0.0), 0.01, 0.15)
                .is_none()
        );
        let edge = world.swim_step(feet, delta, HEIGHT);
        assert!((0.99..=1.0).contains(&edge.x), "{edge:?}");
    }

    #[test]
    fn stepping_down_checks_water_along_the_swept_legs() {
        let upper = [
            [[-20.0, 3.0, -20.0], [0.0, 3.0, -20.0], [0.0, 3.0, 20.0]],
            [[-20.0, 3.0, -20.0], [0.0, 3.0, 20.0], [-20.0, 3.0, 20.0]],
            [[0.0, 0.0, -20.0], [0.0, 3.0, -20.0], [0.0, 3.0, 20.0]],
            [[0.0, 0.0, -20.0], [0.0, 3.0, 20.0], [0.0, 0.0, 20.0]],
        ];
        let world = CollisionWorld::new(flat(0.0).into_iter().chain(upper)).unwrap();
        let feet = Vec3::new(-0.1, 3.0, 0.0);
        let step = |world: &CollisionWorld| {
            AirborneController::default().step_into_water(
                world,
                feet,
                crate::movement::VerticalPhysics {
                    ground_tolerance: 3.05,
                    ..PROVISIONAL_PHYSICS
                },
                MotionStep {
                    horizontal: Vec3::X * 0.7,
                    height: HEIGHT,
                    jump: false,
                    seconds: 0.05,
                },
            )
        };
        // At waist height the chord cuts this pocket beside the riser.
        // The walk crosses at deck height and settles further out.
        let chord_water = (
            Vec3::new(0.22, 4.35, -1.0),
            Vec3::new(0.31, 4.65, 1.0),
            Liquid::Water,
        );
        let world = world.with_liquids(Boxes(vec![chord_water]));
        let landed = step(&world);
        assert!(landed.x > 0.55 && landed.y.abs() < 0.01, "{landed:?}");
        assert!(world.swim_depth(landed, HEIGHT).is_none());
        assert!(
            world
                .sweep(feet, landed - feet, HEIGHT)
                .is_ok_and(|hit| hit.is_some())
        );

        let below = (
            Vec3::new(0.55, -2.0, -1.0),
            Vec3::new(0.7, 4.0, 1.0),
            Liquid::Water,
        );
        let world = world.with_liquids(Boxes(vec![chord_water, below]));
        let entry = step(&world);
        assert!(
            entry.x > 0.55 && (0.99..=1.0).contains(&entry.y),
            "{entry:?}"
        );
        assert_eq!(world.swim_depth(entry, HEIGHT), Some(SwimDepth::Surface));
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
