//! Local-result transaction for clear surface motion. No wire policy.
use bevy::prelude::Vec3;
use eq_client_core::{
    WorldPosition,
    movement::{SurfaceClock, SurfaceDecline, SurfacePlan, SurfaceRegion, water::WetVelocity},
};
use std::time::Instant;

struct Pending {
    session: u64,
    created: Instant,
    expected: WorldPosition,
    feet_offset: f32,
    clock: SurfaceClock,
    selected: bool,
    resume: bool,
}

#[derive(Default)]
pub(super) struct Control {
    committed: Option<SurfaceClock>,
    pending: Option<Pending>,
    pub last_decline: Option<SurfaceDecline>,
    pub pieces: u32,
    pub tails: u32,
    pub checks: u32,
    pub boundary_delay: f32,
}
pub(super) struct Transfer {
    pub velocity: WetVelocity,
    pub medium: SurfaceRegion,
    pub feet: Vec3,
    pub dry_peak: Option<f32>,
}
impl Control {
    pub fn prior(&self) -> Option<&SurfaceClock> {
        self.committed.as_ref()
    }
    pub fn active(&self) -> bool {
        self.committed.as_ref().is_some_and(SurfaceClock::active)
            || self
                .pending
                .as_ref()
                .is_some_and(|p| p.selected && p.resume)
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    /// Eligibility/idle/timeout ends continuation ownership. An already
    /// queued move may still be accepted: transfer its raw velocity once,
    /// but never resume its old continuation or integrate the gap.
    pub fn pause(&mut self) {
        self.committed = None;
        if let Some(p) = self.pending.as_mut() {
            p.resume = false;
        }
    }
    pub fn declined(&mut self, reason: SurfaceDecline) {
        self.pause();
        self.last_decline = Some(reason);
    }
    pub fn sent(
        &mut self,
        session: u64,
        created: Instant,
        expected: WorldPosition,
        feet_offset: f32,
        plan: SurfacePlan,
        selected: bool,
    ) {
        self.pieces = plan.pieces;
        self.tails = plan.tails;
        self.checks = plan.checks;
        self.boundary_delay = plan.boundary_delay;
        self.last_decline = None;
        self.pending = Some(Pending {
            session,
            created,
            expected,
            feet_offset,
            clock: plan.clock,
            selected,
            resume: true,
        });
    }
    /// A reset/correction/zoning discards both epoch and pending prediction.
    /// The enclosing Controls default reset does this by replacing Control.
    pub fn refuse(&mut self, session: Option<u64>, created: Instant) {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| Some(p.session) == session && p.created == created)
        {
            self.pending = None;
            self.committed = None;
        }
    }
    pub fn resolved(
        &mut self,
        session: Option<u64>,
        created: Instant,
        position: WorldPosition,
        refused: bool,
    ) -> Option<Transfer> {
        if !self
            .pending
            .as_ref()
            .is_some_and(|p| Some(p.session) == session && p.created == created)
        {
            return None;
        }
        let mut pending = self.pending.take().expect("matching surface proposal");
        self.committed = None;
        if refused || pending.expected != position {
            return None;
        }
        let feet = Vec3::from_array(eq_client_core::render_position(position))
            - Vec3::Y * pending.feet_offset;
        if !pending.clock.accepted_feet(feet) {
            return None;
        }
        let transfer = pending.selected.then(|| Transfer {
            velocity: pending.clock.velocity(),
            medium: pending.clock.medium(),
            feet,
            dry_peak: pending.clock.dry_peak(),
        });
        if pending.resume {
            self.committed = Some(pending.clock);
        }
        transfer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::movement::{
        CollisionWorld, Liquid, Liquids, SurfaceStroke,
        water::{LIQUID_5, WetInput},
    };
    use std::time::Duration;
    struct Pool;
    impl Liquids for Pool {
        fn surface_water_identity(&self, point: Vec3) -> Option<usize> {
            (self.liquid_at(point) == Some(Liquid::Water)).then_some(0)
        }
        fn surface_water_below(
            &self,
            origin: Vec3,
            bottom: f32,
            water: SurfaceRegion,
            region: usize,
        ) -> Option<f32> {
            (origin.is_finite()
                && bottom.is_finite()
                && origin.y > 0.
                && bottom < 0.
                && water == SurfaceRegion::Water
                && region == 0)
                .then_some(0.)
        }
        fn surface_region(&self, p: Vec3) -> Option<SurfaceRegion> {
            Some(if p.y < 0. {
                SurfaceRegion::Water
            } else {
                SurfaceRegion::Air
            })
        }
        fn surface_segment(&self, a: Vec3, b: Vec3, k: SurfaceRegion) -> bool {
            a.is_finite() && b.is_finite() && k == SurfaceRegion::Water
        }
        fn liquid_at(&self, p: Vec3) -> Option<Liquid> {
            (p.y < 0.).then_some(Liquid::Water)
        }
        fn passes_through(&self, a: Vec3, b: Vec3, k: Liquid) -> bool {
            k == Liquid::Water && (a.y < 0. || b.y < 0.)
        }
        fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
            None
        }
    }
    fn plan() -> SurfacePlan {
        let world = CollisionWorld::new([
            [
                [-100., -1000., -100.],
                [1000., -1000., 100.],
                [1000., -1000., -100.],
            ],
            [
                [-100., -1000., -100.],
                [-100., -1000., 100.],
                [1000., -1000., 100.],
            ],
        ])
        .unwrap()
        .with_liquids(Pool)
        .with_swim_origin(3.75);
        SurfaceClock::plan(
            None,
            &world,
            Vec3::Y * -3.85,
            WetVelocity::default(),
            SurfaceStroke {
                set: LIQUID_5,
                input: WetInput {
                    horizontal: [1.7, 0.],
                    vertical: 1.7 * 15_f32.to_radians().sin(),
                    jump: false,
                },
                seconds: 0.1,
                height: 6.677_319_5,
            },
        )
        .unwrap()
    }
    fn staged() -> (Control, Instant, WorldPosition, WetVelocity) {
        let p = plan();
        let v = p.clock.velocity();
        let at = Instant::now();
        let expected = eq_client_core::world_position((p.feet + Vec3::Y * 3.75).to_array(), 0.);
        let mut c = Control::default();
        c.sent(11, at, expected, 3.75, p, true);
        (c, at, expected, v)
    }
    #[test]
    fn surface_transaction_accepts_only_exact_local_result_once() {
        let (mut c, at, p, velocity) = staged();
        assert!(c.prior().is_none());
        assert!(c.resolved(Some(12), at, p, false).is_none());
        assert!(
            c.resolved(Some(11), at + Duration::from_nanos(1), p, false)
                .is_none()
        );
        assert!(c.pending());
        let transfer = c.resolved(Some(11), at, p, false).unwrap();
        assert_eq!(transfer.velocity, velocity);
        assert!(c.active());
        assert!(!c.pending());
        assert!(c.resolved(Some(11), at, p, false).is_none());
        assert_eq!(c.prior().unwrap().velocity(), velocity);
    }
    #[test]
    fn surface_transaction_idle_or_delay_never_resumes_the_old_epoch() {
        let (mut c, at, p, velocity) = staged();
        c.pause();
        assert!(!c.active());
        assert!(c.pending());
        let transfer = c.resolved(Some(11), at, p, false).unwrap();
        assert_eq!(transfer.velocity, velocity);
        assert!(c.prior().is_none());
    }
    #[test]
    fn surface_transaction_refusal_correction_and_zoning_cannot_commit() {
        let (mut c, at, p, _) = staged();
        assert!(c.resolved(Some(11), at, p, true).is_none());
        assert!(!c.active());
        let (mut c, at, p, _) = staged();
        let mut changed = p;
        changed.z += 1.;
        assert!(c.resolved(Some(11), at, changed, false).is_none());
        assert!(c.prior().is_none());
        let (mut c, at, p, _) = staged();
        assert!(c.pending());
        c = Control::default();
        assert!(c.resolved(Some(11), at, p, false).is_none());
        assert!(c.prior().is_none());
    }
}
