use super::{CollisionWorld, Liquid, liquids::SOLES};
use glam::{Vec2, Vec3};

/// How far past the surface a rising wet stroke may carry the origin in
/// one sample, in world units: inside one program frame at the fastest rise.
const SURFACE_CROSSING: f64 = 0.05;

/// Which strokes may cross the water's surface into known air.
#[derive(Clone, Copy)]
enum Exit {
    None,
    Level,
    Any,
}

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

    /// Whether this body's origin is in water or lava: the program's deep
    /// liquid, the origin's region byte +0xad of 5 or more (0x489b0a;
    /// reads R40 and R49-26). Wading feet with the origin dry are not.
    #[must_use]
    pub fn origin_in_liquid(&self, feet: Vec3, height: f32) -> bool {
        self.liquids
            .as_ref()
            .is_some_and(|liquids| liquids.liquid_at(feet + self.swim_probe(height)).is_some())
    }

    /// Which water set the liquid at the origin takes, where the program's
    /// motion step reads it (+0xad, R33): Normal with no liquid there.
    #[must_use]
    pub fn wet_kind(&self, feet: Vec3, height: f32) -> super::water::WetKind {
        self.liquids
            .as_ref()
            .map_or_else(super::water::WetKind::default, |liquids| {
                liquids.wet_kind_at(feet + self.swim_probe(height))
            })
    }

    pub(super) fn swim_probe(&self, height: f32) -> Vec3 {
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
    /// The result stays in water, reaches a supported bank, or crosses into
    /// known air while the soles remain continuously in water. That last
    /// case hands off to ordinary gravity on the next sample.
    #[must_use]
    pub fn swim_step(&self, feet: Vec3, delta: Vec3, height: f32) -> Vec3 {
        self.stroke(
            feet,
            delta,
            height,
            if delta.y == 0.0 {
                Exit::Level
            } else {
                Exit::None
            },
        )
    }

    /// Sweeps one sample of the program's wet step (0x4bfe0f). As
    /// [`Self::swim_step`], except that any stroke may cross into known air
    /// above the same water while the soles stay continuously wet, rising
    /// level or sinking: when the origin's region is dry the program's next step is
    /// ordinary falling (reads R33 and R41), which the caller carries on.
    #[must_use]
    pub fn wet_step(&self, feet: Vec3, delta: Vec3, height: f32) -> Vec3 {
        self.stroke(feet, delta, height, Exit::Any)
    }

    /// The supported query used by independent replay. Unknown sweeps are not
    /// certified as blocked movement; the ordinary API retains its old fallback.
    pub(super) fn wet_step_checked(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
    ) -> Result<Vec3, ()> {
        self.stroke_query(feet, delta, height, Exit::Any, true)
            .map_err(|_| ())
    }

    /// First conservative sweep plus ordinary water-gap clipping, without
    /// projecting gravity along a solid slope into unearned horizontal travel.
    pub(super) fn wet_vertical_step_checked(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
    ) -> Result<Vec3, ()> {
        self.stroke_query(feet, delta, height, Exit::Any, false)
            .map_err(|_| ())
    }

    fn stroke(&self, feet: Vec3, delta: Vec3, height: f32, level: Exit) -> Vec3 {
        match self.stroke_query(feet, delta, height, level, true) {
            Ok(position) | Err(position) => position,
        }
    }

    // The error retains the exact conservative endpoint the legacy API used
    // before an unsupported first or second sweep. It is not replay evidence.
    fn stroke_query(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
        level: Exit,
        slide: bool,
    ) -> Result<Vec3, Vec3> {
        if !self.in_swimmable_water(feet, height) || !delta.is_finite() {
            return Err(feet);
        }
        if delta.length_squared() <= 1e-10 {
            return Ok(feet);
        }
        let (contact, normal) = self.swim_sweep(feet, delta, height).ok_or(feet)?;
        let position = self.limit_swim(feet, contact, height, level);
        if !slide || !self.in_swimmable_water(position, height) {
            return Ok(position);
        }
        let remaining = delta - (position - feet);
        let slide = if position.distance_squared(contact) > 1e-10 {
            Vec3::new(remaining.x, 0.0, remaining.z)
        } else if let Some(normal) = normal {
            remaining - normal * remaining.dot(normal)
        } else {
            return Ok(position);
        };
        // A stroke's slide redirects what the stroke had left; it never adds
        // ground travel. Projected along a slope, a blocked downward share
        // would otherwise come out as sideways speed the stroke never had,
        // past the swimmer's horizontal cap. A body with no ground travel of
        // its own, only sinking, still slides down the slope it settles on.
        let left = Vec2::new(remaining.x, remaining.z).length();
        let across = Vec2::new(slide.x, slide.z).length();
        // Scale the whole slide, so it stays along the surface it follows.
        let slide = if left > 1e-6 && across > left {
            slide * (left / across)
        } else {
            slide
        };
        if slide.length_squared() <= 1e-10 {
            return Ok(position);
        }
        let (end, _) = self.swim_sweep(position, slide, height).ok_or(position)?;
        Ok(self.limit_swim(position, end, height, level))
    }

    pub(super) fn swim_sweep(
        &self,
        from: Vec3,
        delta: Vec3,
        height: f32,
    ) -> Option<(Vec3, Option<Vec3>)> {
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
    fn limit_swim(&self, from: Vec3, to: Vec3, height: f32, level: Exit) -> Vec3 {
        let Some(liquids) = &self.liquids else {
            return from;
        };
        let soles = Vec3::Y * SOLES;
        let lava = |at| liquids.passes_through(from + soles, at + soles, Liquid::Lava);
        let probe = self.swim_probe(height);
        // The native frame integrates from the previous region, resolves
        // collision, then refreshes the origin's region (Titanium 0x445243,
        // 0x48ba76, 0x489b0a). Known air above the same water can therefore
        // take ordinary gravity next frame. The kinematic stroke keeps a
        // conservative subset, a level stroke only; the wet step crosses
        // in any direction. Both need continuously wet soles, never a dry
        // gap or unknown medium. No surface height is invented.
        let exits = match level {
            Exit::None => false,
            Exit::Level => to.y - from.y == 0.0,
            Exit::Any => true,
        };
        let surfaces = |at: Vec3| {
            liquids.water_surface_exit(from + probe, at + probe)
                && liquids.liquid_at(from + soles) == Some(Liquid::Water)
                && liquids.liquid_at(at + soles) == Some(Liquid::Water)
                && liquids.water_surface_exit(at + soles, at + probe)
                && liquids
                    .first_outside(from + soles, at + soles, Liquid::Water)
                    .is_none()
        };
        let at = |fraction| from.as_dvec3().lerp(to.as_dvec3(), fraction).as_vec3();
        let gap = liquids.first_outside(from + probe, to + probe, Liquid::Water);
        // A rising wet stroke stops just past the surface: the program moves
        // a frame's worth (about half a unit at the fastest rise) before its
        // next step finds the origin dry and falls (0x4bfe0f; reads R33 and
        // R41). A sample spans several frames, so the stroke goes no further
        // than the program's first dry frame starts from, and the caller's
        // fall carries the velocity on.
        let crossing = gap.as_ref().and_then(|gap| {
            let length = f64::from(from.distance(to));
            (matches!(level, Exit::Any) && to.y > from.y && length > 0.0 && !gap.is_empty())
                .then(|| at((gap.start + SURFACE_CROSSING / length).min(1.0)))
                .filter(|&past| surfaces(past))
        });
        if exits && surfaces(to) {
            return crossing.unwrap_or(to);
        }
        let to = if let Some(gap) = gap {
            if gap.is_empty() {
                return from;
            }
            let edge = at(gap.start);
            let horizontal = (to.x - from.x).hypot(to.z - from.z);
            if horizontal > 1e-5
                && self.ground(edge, 0.01, 0.15).is_some()
                && let Some(shore) = self.shore_point(at, &gap, height)
                && !lava(shore)
            {
                return shore;
            }
            if let Some(past) = crossing {
                return past;
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
            [[-20.0, y, -20.0], [20.0, y, 20.0], [20.0, y, -20.0]],
            [[-20.0, y, -20.0], [-20.0, y, 20.0], [20.0, y, 20.0]],
        ]
    }

    fn water() -> (Vec3, Vec3, Liquid) {
        (
            Vec3::new(0.0, -20.0, -20.0),
            Vec3::new(20.0, 0.0, 20.0),
            Liquid::Water,
        )
    }

    fn sloping_bottom(slope: f32, offset: Vec3) -> [[[f32; 3]; 3]; 2] {
        let at = |x: f32, z: f32| (offset + Vec3::new(x, -12.0 + x * slope, z)).to_array();
        [
            [at(-100.0, -100.0), at(100.0, 100.0), at(100.0, -100.0)],
            [at(-100.0, -100.0), at(-100.0, 100.0), at(100.0, 100.0)],
        ]
    }

    fn submerged_bottom(
        triangles: impl IntoIterator<Item = [[f32; 3]; 3]>,
        offset: Vec3,
    ) -> CollisionWorld {
        CollisionWorld::new(triangles)
            .unwrap()
            .with_swim_origin(3.75)
            .with_liquids(Boxes(vec![(
                offset + Vec3::splat(-100.0),
                offset + Vec3::new(100.0, 0.0, 100.0),
                Liquid::Water,
            )]))
    }

    #[test]
    fn submerged_forward_strokes_slide_up_a_rising_bottom() {
        let height = 6.677_319_5;
        for offset in [Vec3::ZERO, Vec3::new(7_118.0, 0.0, -3_372.0)] {
            for slope in [0.05_f32, 0.2] {
                let world = submerged_bottom(sloping_bottom(slope, offset), offset);
                for clearance in [0.0_f32, 0.03, 0.15] {
                    for down in [0.0_f32, -0.25, -1.0] {
                        let mut feet = offset + Vec3::new(0.0, -12.0 + clearance, 0.0);
                        let delta = Vec3::new(2.2, down, 0.0);
                        // The walking diagnostic is raised by LIFT. Its empty
                        // result does not mean the actual wet capsule is clear.
                        if slope.to_bits() == 0.2_f32.to_bits()
                            && clearance.to_bits() == 0.03_f32.to_bits()
                            && down >= -0.25
                        {
                            assert!(world.first_contact(feet, delta, height).is_none());
                        }
                        for _ in 0..12 {
                            assert!(world.in_swimmable_water(feet, height));
                            let next = world.wet_step(feet, delta, height);
                            assert!(
                                next.x - feet.x > 1.5,
                                "uphill stall: {slope} {clearance} {down} {feet:?} -> {next:?}"
                            );
                            assert!(
                                next.distance(feet) <= delta.length() + 0.002,
                                "slide added distance"
                            );
                            let floor = -12.0 + (next.x - offset.x) * slope;
                            assert!(
                                next.y >= floor - 0.002,
                                "sank into bottom: {next:?}, floor {floor}"
                            );
                            assert!(
                                (next.z - offset.z).abs() < 0.002,
                                "sideways drift: {next:?}"
                            );
                            feet = next;
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn submerged_level_and_downhill_strokes_keep_forward_progress() {
        for slope in [0.0_f32, -0.2] {
            let world = submerged_bottom(sloping_bottom(slope, Vec3::ZERO), Vec3::ZERO);
            for down in [0.0_f32, -0.25] {
                let mut feet = Vec3::new(0.0, -11.97, 0.0);
                let delta = Vec3::new(2.2, down, 0.0);
                for _ in 0..12 {
                    let next = world.wet_step(feet, delta, 6.677_319_5);
                    assert!(
                        (next.x - feet.x - delta.x).abs() < 0.002,
                        "{feet:?} -> {next:?}"
                    );
                    assert!(next.y >= -12.0 + next.x * slope - 0.002, "{next:?}");
                    if slope < 0.0 || down == 0.0 {
                        assert!(
                            (next - feet - delta).length() < 0.002,
                            "clear stroke changed"
                        );
                    }
                    feet = next;
                }
            }
        }
    }

    #[test]
    fn a_stroke_pitched_into_the_bottom_gains_no_ground_travel_from_the_slide() {
        // Swimming forward while looking down into the floor: the blocked
        // downward share must not slide along the slope into extra speed.
        let height = 6.677_319_5;
        for slope in [-0.2_f32, -0.05, 0.0, 0.05, 0.2] {
            let world = submerged_bottom(sloping_bottom(slope, Vec3::ZERO), Vec3::ZERO);
            for delta in [
                Vec3::new(2.4, -2.3, 0.4),
                Vec3::new(1.0, -2.0, 0.0),
                Vec3::new(-0.6, -3.2, 2.2),
            ] {
                let mut feet = Vec3::new(0.0, -11.97, 0.0);
                for _ in 0..8 {
                    let next = world.wet_step(feet, delta, height);
                    let ground = Vec2::new(next.x - feet.x, next.z - feet.z).length();
                    let asked = Vec2::new(delta.x, delta.z).length();
                    assert!(
                        ground <= asked + 0.002,
                        "slope {slope} {delta:?}: {ground} of ground for {asked} asked"
                    );
                    assert!(next.y >= -12.0 + next.x * slope - 0.002, "{next:?}");
                    feet = next;
                }
            }
        }
    }

    #[test]
    fn a_body_only_sinking_still_slides_down_a_sloped_bottom() {
        // No ground travel of its own: the slide is not scaled away, so a
        // sinking swimmer settles down the slope, downhill.
        let height = 6.677_319_5;
        for slope in [-0.2_f32, 0.2] {
            let world = submerged_bottom(sloping_bottom(slope, Vec3::ZERO), Vec3::ZERO);
            let feet = Vec3::new(0.0, -11.97, 0.0);
            let next = world.wet_step(feet, Vec3::new(0.0, -1.0, 0.0), height);
            let downhill = -(next.x - feet.x) * slope.signum();
            assert!(downhill > 0.01, "slope {slope}: {next:?}");
            assert!(next.y >= -12.0 + next.x * slope - 0.002, "{next:?}");
        }
    }

    #[test]
    fn submerged_bottom_slide_keeps_walls_and_headroom_solid() {
        let height = 6.677_319_5;
        for ceiling in [false, true] {
            let obstacles = if ceiling {
                flat(-5.0)
            } else {
                [
                    [[4.0, -20.0, -20.0], [4.0, 0.0, -20.0], [4.0, 0.0, 20.0]],
                    [[4.0, -20.0, -20.0], [4.0, 0.0, 20.0], [4.0, -20.0, 20.0]],
                ]
            };
            let world = submerged_bottom(
                sloping_bottom(0.2, Vec3::ZERO).into_iter().chain(obstacles),
                Vec3::ZERO,
            );
            let mut feet = Vec3::new(0.0, -11.97, 0.0);
            for _ in 0..12 {
                feet = world.wet_step(feet, Vec3::new(2.2, -0.25, 0.0), height);
                assert!(feet.y >= -12.0 + feet.x * 0.2 - 0.002, "{feet:?}");
                if ceiling {
                    assert!(feet.y + height <= -5.0 + 0.002, "ceiling crossed: {feet:?}");
                } else {
                    assert!(feet.x <= 3.6 + 0.002, "wall crossed: {feet:?}");
                }
            }
            assert!(
                feet.x > 0.5,
                "initial floor contact blocked the approach: {feet:?}"
            );
        }
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
    fn the_players_own_moves_fall_into_lava_and_swim_where_the_guarded_world_holds() {
        // The same box is lava to the guarded world and swims as water to
        // the player's own moves (R49-26).
        let lava = (water().0, water().1, Liquid::Lava);
        let world = CollisionWorld::new(flat(-12.0))
            .unwrap()
            .with_liquids(Boxes(vec![lava]))
            .with_own_liquids(Boxes(vec![water()]));
        let own = world.own_moves().unwrap();
        assert!(own.own_moves().is_none());
        // The view shares the world's geometry and obstacles (review L1).
        assert!(std::sync::Arc::ptr_eq(&own.terrain, &world.terrain));
        assert!(std::sync::Arc::ptr_eq(&own.obstacles, &world.obstacles));
        assert!(std::sync::Arc::ptr_eq(
            &own.spawn_obstacles,
            &world.spawn_obstacles
        ));
        assert!(
            CollisionWorld::new(flat(-12.0))
                .unwrap()
                .own_moves()
                .is_none()
        );
        let fall = |world: &CollisionWorld| {
            let mut airborne = AirborneController::default();
            let mut feet = Vec3::new(5.0, 4.0, 0.0);
            for _ in 0..40 {
                feet = airborne.step_into_water(
                    world,
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
            feet
        };
        let swum = fall(&own);
        assert_eq!(own.swim_depth(swum, HEIGHT), Some(SwimDepth::Surface));
        assert!((-3.01..=-3.0).contains(&swum.y), "{swum:?}");
        let deeper = own.swim_step(swum, Vec3::new(1.0, -1.0, 0.0), HEIGHT);
        assert!(
            deeper.x > swum.x + 0.9 && deeper.y < swum.y - 0.9,
            "{deeper:?}"
        );
        // The guarded world never swims it: a fall the server started over
        // it goes through to the floor, as before.
        let fallen = fall(&world);
        assert!(fallen.y < -11.9, "{fallen:?}");
        assert!(!world.in_swimmable_water(swum, HEIGHT));
        assert_eq!(world.liquid_contact(swum, HEIGHT), LiquidContact::Lava);
    }

    #[test]
    fn a_swimmer_can_leave_on_a_supported_bank_but_not_into_air() {
        let ramp = [
            [[-10.0, 2.0, -20.0], [10.0, -8.0, 20.0], [10.0, -8.0, -20.0]],
            [[-10.0, 2.0, -20.0], [-10.0, 2.0, 20.0], [10.0, -8.0, 20.0]],
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

    fn falling_water_surface() -> CollisionWorld {
        let pools = Boxes(vec![
            (
                Vec3::new(-20.0, -20.0, -20.0),
                Vec3::new(0.5, 0.0, 20.0),
                Liquid::Water,
            ),
            (
                Vec3::new(0.5, -20.0, -20.0),
                Vec3::new(20.0, -0.02, 20.0),
                Liquid::Water,
            ),
        ]);
        CollisionWorld::new(flat(-12.0))
            .unwrap()
            .with_liquids(pools)
            .with_swim_origin(3.0)
    }

    #[test]
    fn a_level_surface_exit_uses_ordinary_falling_to_reenter_water() {
        let world = falling_water_surface();
        let feet = Vec3::new(-0.1, -3.01, 0.0);
        let delta = Vec3::X * 0.8;
        let exited = world.swim_step(feet, delta, HEIGHT);
        assert!(exited.abs_diff_eq(feet + delta, 1e-6), "{exited:?}");
        assert!(!world.in_swimmable_water(exited, HEIGHT));
        assert_eq!(world.liquid_contact(exited, HEIGHT), LiquidContact::Wading);
        assert!(world.support(exited).is_none());

        let mut fall = AirborneController::default();
        let entered = fall.step_into_water(
            &world,
            exited,
            PROVISIONAL_PHYSICS,
            MotionStep {
                horizontal: Vec3::ZERO,
                height: HEIGHT,
                jump: false,
                seconds: 0.05,
            },
        );
        assert!(entered.y < exited.y, "{entered:?}");
        assert!((entered.x - exited.x).abs() < 1e-6, "{entered:?}");
        assert!(world.in_swimmable_water(entered, HEIGHT));
        assert!(
            fall.take_landing().is_none(),
            "water is not a solid landing"
        );
        assert!(world.swim_step(entered, delta, HEIGHT).x > entered.x + 0.79);
    }

    #[test]
    fn a_surface_exit_still_sweeps_solids_and_does_not_launch_upward() {
        let mut world = falling_water_surface();
        let wall = [
            [[0.0, -12.0, -10.0], [0.0, 10.0, -10.0], [0.0, 10.0, 10.0]],
            [[0.0, -12.0, -10.0], [0.0, 10.0, 10.0], [0.0, -12.0, 10.0]],
        ];
        world.set_obstacles(vec![std::sync::Arc::new(
            super::super::CollisionMesh::new(wall).unwrap(),
        )]);
        let feet = Vec3::new(-0.8, -3.01, 0.0);
        assert!(world.swim_step(feet, Vec3::X * 1.6, HEIGHT).x < -0.4);

        let world = falling_water_surface();
        let feet = Vec3::new(-0.1, -3.01, 0.0);
        let raised = world.swim_step(feet, Vec3::new(0.8, 0.1, 0.0), HEIGHT);
        assert!(world.in_swimmable_water(raised, HEIGHT), "{raised:?}");
    }

    #[test]
    fn a_supported_endpoint_does_not_allow_swimming_across_an_unsupported_exit() {
        let bank = [
            [
                [1.05, -3.19, -20.0],
                [20.0, -3.19, 20.0],
                [20.0, -3.19, -20.0],
            ],
            [
                [1.05, -3.19, -20.0],
                [1.05, -3.19, 20.0],
                [20.0, -3.19, 20.0],
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
            [[-20.0, 3.0, -20.0], [0.0, 3.0, 20.0], [0.0, 3.0, -20.0]],
            [[-20.0, 3.0, -20.0], [-20.0, 3.0, 20.0], [0.0, 3.0, 20.0]],
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
