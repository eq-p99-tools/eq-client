//! Actual applied ordinary water calls, sealed before the final pose is sent.
use super::*;
use eq_client_core::movement::{SwimBasis, SwimControl, SwimControls, SwimInput, water::*};

#[derive(Default)]
pub(super) struct Owner {
    basis: Option<(u64, eq_client_core::WorldPosition, SwimBasis)>,
    reset: Option<(u64, eq_client_core::WorldPosition)>,
    pending: Option<Pending>,
    result: Option<(u64, Instant, bool)>,
}

struct Pending {
    session: u64,
    request: MovementRequest,
    basis: eq_client_core::WorldPosition,
    seconds: f32,
    before: WetVelocity,
}

impl Owner {
    pub fn span(&self, now: Instant, span: f32) -> f32 {
        self.basis.map_or(span, |(_, _, basis)| {
            span.min(
                now.saturating_duration_since(basis.started_at())
                    .as_secs_f32(),
            )
        })
    }
    pub fn physical_frame(&mut self, swimming: bool) {
        // A reset not yet matched before physics cannot anchor later state.
        self.reset = None;
        if !swimming {
            self.basis = None;
        }
    }
    pub fn reset_observed(&mut self, session: u64, position: eq_client_core::WorldPosition) {
        *self = Self {
            reset: Some((session, position)),
            ..Self::default()
        };
    }

    pub fn receipt(
        &mut self,
        session: u64,
        position: eq_client_core::WorldPosition,
        created: Option<Instant>,
        basis: Option<SwimBasis>,
        wet: &mut WetVelocity,
    ) {
        if created.is_none()
            && basis.is_some_and(SwimBasis::is_reset)
            && self.reset == Some((session, position))
        {
            // This marker was recorded at the actual admission/correction,
            // not inferred from a blocked or unchanged endpoint.
            self.reset = None;
            *wet = WetVelocity::default();
            self.basis = basis.map(|basis| (session, position, basis));
        } else if let Some(created) = created {
            if let Some((seen, at, accepted)) = self.result
                && (seen, at) == (session, created)
            {
                self.result = None;
                trace!(target: "eq_client_render::motion::swim", ?position,
                    verdict = if accepted && basis.is_some() { "owned_replay_retained" } else { "continuity_unavailable" },
                    "Ordinary swim continuity after the matching local result");
                self.basis = basis
                    .filter(|_| accepted)
                    .map(|basis| (session, position, basis));
            }
        } else if basis.is_none() {
            self.basis = None;
        }
    }

    pub fn begin(
        &mut self,
        session: Option<u64>,
        position: eq_client_core::WorldPosition,
        input: Option<(SwimInput, f32)>,
    ) -> Option<Capture> {
        // Taking the receipt means an early return or an unsupported physical
        // operation cannot silently omit calls and later resume continuity.
        let (admission, origin, basis) = self.basis.take()?;
        self.reset = None;
        let (input, heading) = input?;
        (session == Some(admission) && origin == position).then_some(Capture {
            basis,
            input,
            heading,
            pieces: Vec::new(),
            until: Duration::ZERO,
            complete: true,
        })
    }

    pub fn submitted(
        &mut self,
        request: MovementRequest,
        basis: eq_client_core::WorldPosition,
        seconds: f32,
        before: WetVelocity,
    ) {
        self.pending = Some(Pending {
            session: request.session_id,
            request,
            basis,
            seconds,
            before,
        });
    }

    pub fn resolved(
        &mut self,
        session: Option<u64>,
        created: Instant,
        refused: bool,
        wet: &mut WetVelocity,
    ) {
        if self.pending.as_ref().is_some_and(|pending| {
            session == Some(pending.session) && pending.request.created == created
        }) {
            let pending = self.pending.take().expect("matching pending swim");
            trace!(target: "eq_client_render::motion::swim", basis = ?pending.basis,
                proposed = ?pending.request.position, dt = pending.seconds, mode = ?pending.request.mode,
                verdict = if refused { "local_refused" } else { "local_accepted" },
                "Ordinary swim local result; neither packet submission nor server acknowledgment");
            if refused {
                *wet = pending.before;
            }
            self.result = Some((pending.session, created, !refused));
        }
    }
}

pub(super) struct Capture {
    basis: SwimBasis,
    input: SwimInput,
    heading: f32,
    pieces: Vec<SwimControl>,
    until: Duration,
    complete: bool,
}

impl Capture {
    /// Called at the same site and with the same duration/kind as `advance`.
    pub fn applied(&mut self, kind: WetKind, seconds: f32) {
        self.until += Duration::from_secs_f32(seconds);
        match SwimControl::applied(self.until, self.heading, kind, self.input, seconds) {
            Ok(piece) if self.pieces.len() < 64 => self.pieces.push(piece),
            _ => self.complete = false,
        }
    }

    pub fn finish(self, pose: f32) -> Option<SwimControls> {
        self.complete
            .then(|| SwimControls::from_applied(self.basis, self.pieces, pose).ok())
            .flatten()
    }
}

/// Classifies resolved *input*, before water integration changes its direction
/// into travel. Camera-relative forward has the actual input vector's facing;
/// ordinary backward and strafe retain the basis that produced those controls.
pub(super) fn observed(
    mode: MovementMode,
    direction: Vec3,
    facing: f32,
) -> Option<(SwimInput, f32)> {
    if Vec2::new(direction.x, direction.z) == Vec2::ZERO {
        return Some((SwimInput::Idle, facing));
    }
    match mode {
        MovementMode::Backward => Some((SwimInput::Backward, facing)),
        MovementMode::Strafe(pace) => {
            let radians = eq_client_core::render_heading(facing);
            let left = Vec3::new(radians.cos(), 0., -radians.sin());
            let side = if direction.dot(left) > 0. {
                StrafeSide::Left
            } else {
                StrafeSide::Right
            };
            Some((SwimInput::Strafe(side, pace), facing))
        }
        MovementMode::Forward | MovementMode::Walk => Some((
            SwimInput::Forward(if mode == MovementMode::Walk {
                Pace::Walk
            } else {
                Pace::Run
            }),
            eq_client_core::world_heading(direction.x.atan2(direction.z)),
        )),
        _ => None,
    }
}
