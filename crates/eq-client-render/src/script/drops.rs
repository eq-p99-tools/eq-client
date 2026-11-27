//! Script consent is separate from the session's ability to simulate falls.
//! These disposable probes use the movement controller and landing cost;
//! they never advance accepted movement or authorize a network request.
use crate::{PlayerBody, motion};
use bevy::prelude::Vec3;
use eq_client_assets::ui::InstalledClient;
use eq_client_core::{
    movement::{AirborneController, CollisionWorld, VerticalPhysics},
    world::ClientWorld,
};

pub(super) const TOO_HIGH: &str =
    "drop too high; use with_drops(True) to allow known damaging drops";
const UNKNOWN_LANDING: &str = "drop has no verified landing";
const UNKNOWN_COST: &str = "drop damage is unavailable";
const SLICE: f32 = 0.05;
const FORECAST_STEPS: usize = 200;

/// The exact next input's displacement and physical body, or a short slice
/// of a route leg. A forecast owns a clone of the airborne state.
pub(crate) struct Probe {
    pub feet: Vec3,
    pub delta: Vec3,
    pub seconds: f32,
    pub height: f32,
    pub jump: bool,
    pub physics: VerticalPhysics,
}

struct Forecast {
    next: Vec3,
    landing: Option<Vec3>,
}

/// Check a proposal before it mutates the live controller. Even consent
/// cannot turn an unknown landing/cost into a traversable route.
pub(crate) fn input(
    collision: &CollisionWorld,
    world: &ClientWorld,
    client: InstalledClient,
    previous: Option<&AirborneController>,
    probe: Probe,
    allowed: bool,
) -> Result<Vec3, &'static str> {
    forecast(collision, world, client, previous, probe, allowed).map(|forecast| forecast.next)
}

fn forecast(
    collision: &CollisionWorld,
    world: &ClientWorld,
    client: InstalledClient,
    previous: Option<&AirborneController>,
    probe: Probe,
    allowed: bool,
) -> Result<Forecast, &'static str> {
    let Probe {
        feet,
        delta,
        seconds,
        height,
        jump,
        physics,
    } = probe;
    if !feet.is_finite() || !delta.is_finite() || !seconds.is_finite() || seconds <= 0.0 {
        return Err(UNKNOWN_LANDING);
    }
    let mut trial = previous.cloned().unwrap_or_default();
    let _ = trial.take_landing(); // A prior sample's report is not this forecast's contact.
    let step = |trial: &mut AirborneController, feet, delta, seconds, jump| {
        motion::fall_step_with_water(
            trial,
            collision,
            physics,
            client.falls().map(|_| physics.reach()),
            feet,
            delta,
            seconds,
            height,
            jump,
            true,
        )
    };
    let next = step(&mut trial, feet, delta, seconds, jump);
    let steering = if physics.steers_in_air {
        delta / seconds * SLICE
    } else {
        Vec3::ZERO
    };
    let mut at = next;
    for _ in 0..FORECAST_STEPS {
        if !at.is_finite() {
            return Err(UNKNOWN_LANDING);
        }
        // The existing landing-cost rule makes mapped liquid entry free.
        // It ends this fall-risk forecast, without claiming solid support.
        let landing = trial.take_landing().or_else(|| {
            collision
                .in_liquid(at)
                .then(|| eq_client_core::movement::Landing {
                    fall_distance: trial.peak().map_or(0.0, |peak| (peak - at.y).max(0.0)),
                    impact_speed: (-trial.velocity()).max(0.0),
                })
        });
        if let Some(landing) = landing {
            let cost =
                motion::landing_cost(client, collision, at, landing, world).ok_or(UNKNOWN_COST)?;
            return if cost.damage > 0 && !allowed {
                Err(TOO_HIGH)
            } else {
                Ok(Forecast {
                    next,
                    landing: Some(at),
                })
            };
        }
        if trial.peak().is_none() && collision.support(at).is_some() {
            return Ok(Forecast {
                next,
                landing: None,
            });
        }
        // Native movement keeps its drift without air input. The existing
        // provisional physics can steer, so it forecasts this held input.
        at = step(&mut trial, at, steering, SLICE, false);
    }
    Err(UNKNOWN_LANDING)
}

/// Check a leg's collision-resolved path, including its exit. The next
/// actual input is independently checked with its real speed and momentum.
pub(super) fn leg(
    collision: &CollisionWorld,
    world: &ClientWorld,
    client: InstalledClient,
    body: PlayerBody,
    from: Vec3,
    to: Vec3,
    allowed: bool,
) -> Result<Vec3, &'static str> {
    let delta = (to - from).with_y(0.0);
    if !delta.is_finite() {
        return Err(UNKNOWN_LANDING);
    }
    let walking_world =
        motion::carrier_walking_world(collision, client, body.feet_offset, false, false);
    let collision = walking_world.as_ref().unwrap_or(collision);
    let posture = world
        .player()
        .and_then(|player| world.posture(player.spawn_id));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // Bounded authored zone coordinates.
    let count = delta.length().ceil().max(1.0) as u32;
    let mut at = from;
    for _ in 0..count {
        let part = (to - at).with_y(0.0).clamp_length_max(1.0);
        if part.length_squared() <= 1e-12 {
            break;
        }
        let raised = motion::raised_walk(collision, at, body.height, posture);
        let physics = motion::walking(
            motion::physics(client, world.zone_gravity()),
            client,
            &body,
            world.player().map_or(0, |player| player.race),
            (raised, raised && collision.in_liquid(at)),
        );
        let next = forecast(
            collision,
            world,
            client,
            None,
            Probe {
                feet: at,
                delta: part,
                seconds: SLICE,
                height: body.height,
                jump: false,
                physics,
            },
            allowed,
        )?;
        let next = next.landing.unwrap_or(next.next);
        if (next - at).length_squared() <= 1e-12 {
            break;
        }
        at = next;
    }
    Ok(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor(x: [f32; 2], z: [f32; 2], y: f32) -> [[[f32; 3]; 3]; 2] {
        let (a, b, c, d) = (
            [x[0], y, z[0]],
            [x[1], y, z[0]],
            [x[1], y, z[1]],
            [x[0], y, z[1]],
        );
        [[a, c, b], [a, d, c]]
    }

    fn body() -> PlayerBody {
        PlayerBody {
            feet_offset: 3.75,
            height: 6.5,
            size: 6.0,
        }
    }

    fn physics() -> VerticalPhysics {
        motion::walking(
            motion::physics(InstalledClient::Titanium, None),
            InstalledClient::Titanium,
            &body(),
            1,
            (true, false),
        )
    }

    pub(crate) fn platform(with_floor: bool) -> CollisionWorld {
        let mut triangles = floor([-100.0, 100.0], [-100.0, 0.0], 53.0).to_vec();
        if with_floor {
            triangles.extend(floor([-1000.0, 1000.0], [-1000.0, 1000.0], 0.0));
        }
        CollisionWorld::new(triangles).unwrap()
    }

    fn leaving() -> Probe {
        Probe {
            feet: Vec3::new(0.0, 53.0, -0.1),
            delta: Vec3::Z * 1.5,
            seconds: SLICE,
            height: body().height,
            jump: false,
            physics: physics(),
        }
    }

    #[test]
    fn a_known_damaging_drop_requires_explicit_consent() {
        let collision = platform(true);
        let world = ClientWorld::default();
        let previous = AirborneController::default();
        assert_eq!(
            input(
                &collision,
                &world,
                InstalledClient::Titanium,
                Some(&previous),
                leaving(),
                false
            ),
            Err(TOO_HIGH)
        );
        let permitted = input(
            &collision,
            &world,
            InstalledClient::Titanium,
            Some(&previous),
            leaving(),
            true,
        )
        .unwrap();
        assert!(permitted.z > -0.1);
        assert_eq!(
            previous.peak(),
            None,
            "a forecast cannot advance accepted movement"
        );
        assert_eq!(previous.velocity(), 0.0);
        assert_eq!(previous.drift(), Vec3::ZERO);
    }

    #[test]
    fn consent_cannot_invent_a_landing_below_an_open_edge() {
        let collision = platform(false);
        assert_eq!(
            input(
                &collision,
                &ClientWorld::default(),
                InstalledClient::Titanium,
                None,
                leaving(),
                true
            ),
            Err(UNKNOWN_LANDING)
        );
    }

    #[test]
    fn ordinary_steps_and_slopes_remain_walkable_without_drop_consent() {
        let world = ClientWorld::default();
        let step = CollisionWorld::new(
            floor([-20.0, 0.0], [-20.0, 20.0], 0.0)
                .into_iter()
                .chain(floor([0.0, 20.0], [-20.0, 20.0], 1.5)),
        )
        .unwrap();
        leg(
            &step,
            &world,
            InstalledClient::Titanium,
            body(),
            Vec3::new(-5.0, 0.0, 0.0),
            Vec3::new(5.0, 1.5, 0.0),
            false,
        )
        .unwrap();
        let slope = CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 10.0, 20.0], [20.0, 10.0, -20.0]],
            [[-20.0, 0.0, -20.0], [-20.0, 0.0, 20.0], [20.0, 10.0, 20.0]],
        ])
        .unwrap();
        leg(
            &slope,
            &world,
            InstalledClient::Titanium,
            body(),
            Vec3::new(-10.0, 2.5, 0.0),
            Vec3::new(10.0, 7.5, 0.0),
            false,
        )
        .unwrap();
    }

    #[test]
    fn drop_consent_keeps_collision_walls_in_force() {
        let wall = [
            [
                [-100.0, -10.0, 0.0],
                [100.0, 100.0, 0.0],
                [100.0, -10.0, 0.0],
            ],
            [
                [-100.0, -10.0, 0.0],
                [-100.0, 100.0, 0.0],
                [100.0, 100.0, 0.0],
            ],
        ];
        let collision = CollisionWorld::new(
            floor([-100.0, 100.0], [-100.0, 100.0], 53.0)
                .into_iter()
                .chain(wall),
        )
        .unwrap();
        let mut probe = leaving();
        probe.feet.z = -2.0;
        probe.delta.z = 4.0;
        let end = input(
            &collision,
            &ClientWorld::default(),
            InstalledClient::Titanium,
            None,
            probe,
            true,
        )
        .unwrap();
        assert!(end.z < 0.0, "consent walked through a solid wall: {end:?}");
    }

    #[test]
    fn mapped_water_keeps_the_existing_zero_fall_damage_rule() {
        use eq_client_core::movement::{Liquid, Liquids};
        struct Water;
        impl Liquids for Water {
            fn liquid_at(&self, point: Vec3) -> Option<Liquid> {
                (point.y < 25.0).then_some(Liquid::Water)
            }
            fn passes_through(&self, from: Vec3, to: Vec3, liquid: Liquid) -> bool {
                liquid == Liquid::Water && from.y.min(to.y) < 25.0
            }
            fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
                Some(Box::new(Self))
            }
        }
        let collision = platform(true).with_liquids(Water);
        assert!(
            input(
                &collision,
                &ClientWorld::default(),
                InstalledClient::Titanium,
                None,
                leaving(),
                false
            )
            .is_ok()
        );
    }

    #[test]
    fn an_ordinary_jump_keeps_its_existing_harmless_landing() {
        let collision = CollisionWorld::new(floor([-100.0, 100.0], [-100.0, 100.0], 0.0)).unwrap();
        let mut probe = leaving();
        probe.feet = Vec3::ZERO;
        probe.delta = Vec3::X * 0.5;
        probe.jump = true;
        assert!(
            input(
                &collision,
                &ClientWorld::default(),
                InstalledClient::Titanium,
                None,
                probe,
                false
            )
            .is_ok()
        );
    }

    #[test]
    fn a_previous_landings_pending_report_is_not_costed_as_a_new_drop() {
        let collision = CollisionWorld::new(floor([-100.0, 100.0], [-100.0, 100.0], 0.0)).unwrap();
        let mut previous = AirborneController::default();
        let mut feet = Vec3::Y * 53.0;
        for _ in 0..FORECAST_STEPS {
            feet = previous.step(
                &collision,
                feet,
                physics(),
                eq_client_core::movement::MotionStep {
                    horizontal: Vec3::ZERO,
                    jump: false,
                    seconds: SLICE,
                    height: body().height,
                },
            );
            if feet.y.abs() < 1e-3 && previous.peak().is_none() {
                break;
            }
        }
        assert!(feet.y.abs() < 1e-3);
        let mut prove_contact = previous.clone();
        assert!(prove_contact.take_landing().is_some());
        let mut probe = leaving();
        probe.feet = feet;
        probe.delta = Vec3::X * 0.5;
        assert!(
            input(
                &collision,
                &ClientWorld::default(),
                InstalledClient::Titanium,
                Some(&previous),
                probe,
                false
            )
            .is_ok()
        );
        assert!(
            previous.take_landing().is_some(),
            "forecast consumed accepted landing state"
        );
    }
}
