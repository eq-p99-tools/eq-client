//! How far an installed client's camera may move, from the program. A
//! client generation's limits are its [`CameraLimits`], from
//! [`crate::ui::InstalledClient::camera_limits`].
//!
//! The Titanium program's third-person camera has no angle limit of its
//! own: it sits a height above the player's eyes and a distance behind
//! them, each held within its range on every update (0x57081f, 0x56fd78,
//! 0x570d81, 0x570fca, 0x5709db; the ranges at 0x65fb08-0x65fb14), and the
//! view tilts down to the player from wherever that puts it. So it never
//! drops below eye level: looking up is for the first-person view, whose
//! pitch is held within a quarter turn either way (0x4bfe0f). Program read
//! N2 of 11-01.
//!
//! How far behind the player the chase camera may go is the player's
//! choice of [`ChaseLeash`]: this client's own long range by default, a
//! quality-of-life setting, or the program's limit.
//!
//! How far it draws is its [`FarPlane`]: the Titanium program draws the
//! zone, and every spawn in it, out to its camera's far plane and to no
//! other distance (program read N7 of 11-01).

/// How far a client generation's camera may move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraLimits {
    /// The first-person view's pitch either side of level, in degrees:
    /// straight up to straight down.
    pub look_pitch_degrees: f32,
    /// The third-person camera's height above the player's eyes, least
    /// and most, in units.
    pub chase_height: (f32, f32),
    /// The third-person camera's distance behind the player, least and
    /// most, in units.
    pub chase_distance: (f32, f32),
    /// How far behind the player [`Self::chase_height`] holds as read: the
    /// program's own farthest distance, which [`Self::on_leash`] keeps.
    pub chase_height_reach: f32,
    /// How far the camera draws.
    pub far_plane: FarPlane,
}

/// How far behind the player the chase camera may go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChaseLeash {
    /// This client's own range, [`LONG_CHASE_DISTANCE`]: the player's
    /// quality-of-life choice, on unless they turn it off.
    Long,
    /// The installed client's program's own limit, the generation's
    /// [`CameraLimits::chase_distance`] as read.
    Program,
}

/// The distance behind the player, least and most, this client's chase
/// camera may go on a [`ChaseLeash::Long`]: the range every camera had
/// before the chase camera took the program's limits.
pub const LONG_CHASE_DISTANCE: (f32, f32) = (2.0, 20_000.0);

impl CameraLimits {
    /// The highest the chase camera may sit above the player's eyes at
    /// `distance` behind them: the program's height limit, and past the
    /// program's farthest distance, which only this client's long leash
    /// reaches, as high as keeps the view no steeper than the program's
    /// steepest view from its farthest point (this client's own rule for
    /// its own range).
    #[must_use]
    pub fn most_chase_height(self, distance: f32) -> f32 {
        let (_, most) = self.chase_height;
        if distance > self.chase_height_reach && self.chase_height_reach > 0.0 {
            most * distance / self.chase_height_reach
        } else {
            most
        }
    }

    /// The limits with the chase camera's distance on `leash`: the program's
    /// as read, or this client's long range. The height and everything else
    /// stay the generation's.
    #[must_use]
    pub const fn on_leash(self, leash: ChaseLeash) -> Self {
        match leash {
            ChaseLeash::Program => self,
            ChaseLeash::Long => Self {
                chase_distance: LONG_CHASE_DISTANCE,
                ..self
            },
        }
    }
}

/// How a client generation's camera sets how far it draws, from the zone
/// header's clip range and the player's clip plane setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarPlane {
    /// The Titanium program's (0x44104c): the zone's minimum clip plus the
    /// setting's share of the way to its maximum, never nearer than the
    /// minimum.
    ZoneRange,
    /// This client's earlier rule, kept where the program is not read: the
    /// setting's share of the zone's maximum clip, never nearer than 100
    /// units (recording checklist 6.342).
    ShareOfMax,
}

impl FarPlane {
    /// How far the Titanium program draws in a zone whose header turns its
    /// fog and clip off (0x44104c). Which header field turns them off is
    /// unread (recording checklist 6.342); this client draws spawns this far
    /// while the zone has given no clip.
    pub const UNCLIPPED: f32 = 600.0;

    /// How far the camera draws with the clip plane setting at `share` of
    /// its range (0 to 1), in a zone whose header gave this minimum and
    /// maximum clip; None where it gave no maximum.
    #[must_use]
    pub fn distance(
        self,
        share: f32,
        (min_clip, max_clip): (Option<f32>, Option<f32>),
    ) -> Option<f32> {
        let max = max_clip.filter(|max| max.is_finite() && *max > 0.0)?;
        let share = if share.is_finite() {
            share.clamp(0.0, 1.0)
        } else {
            1.0
        };
        Some(match self {
            Self::ZoneRange => {
                let min = min_clip
                    .filter(|min| min.is_finite() && *min >= 0.0)
                    .map_or(0.0, |min| min.min(max));
                (min + (max - min) * share).max(min)
            }
            Self::ShareOfMax => (max * share).max(max.min(100.0)),
        })
    }
}

/// The Titanium client's limits.
pub const TITANIUM: CameraLimits = CameraLimits {
    look_pitch_degrees: 90.0,
    chase_height: (0.0, 30.0),
    chase_distance: (2.0, 200.0),
    chase_height_reach: 200.0,
    far_plane: FarPlane::ZoneRange,
};

/// The Mac-era client's limits: not yet read, so Titanium's (recording
/// checklist 6.349), and this client's earlier far plane (6.342). They
/// change only what the player sees.
pub const EQMAC: CameraLimits = CameraLimits {
    far_plane: FarPlane::ShareOfMax,
    ..TITANIUM
};

#[cfg(test)]
mod tests {
    use super::{ChaseLeash, EQMAC, FarPlane, LONG_CHASE_DISTANCE, TITANIUM};

    #[test]
    fn the_leash_chooses_only_the_chase_distance() {
        for program in [TITANIUM, EQMAC] {
            assert_eq!(program.on_leash(ChaseLeash::Program), program);
            let long = program.on_leash(ChaseLeash::Long);
            assert_eq!(long.chase_distance, LONG_CHASE_DISTANCE);
            assert_eq!(long.chase_height, program.chase_height);
            assert_eq!(long.look_pitch_degrees, program.look_pitch_degrees);
            assert_eq!(long.far_plane, program.far_plane);
        }
        assert_eq!(TITANIUM.chase_distance, (2.0, 200.0));
    }

    #[test]
    fn the_height_limit_holds_to_the_programs_farthest_then_keeps_its_slope() {
        for leash in [ChaseLeash::Program, ChaseLeash::Long] {
            let limits = TITANIUM.on_leash(leash);
            assert!((limits.most_chase_height(2.0) - 30.0).abs() < f32::EPSILON);
            assert!((limits.most_chase_height(200.0) - 30.0).abs() < f32::EPSILON);
            // Past 200, only on the long leash, 30 in every 200.
            assert!((limits.most_chase_height(2_000.0) - 300.0).abs() < 1e-3);
        }
    }

    #[test]
    fn titanium_draws_from_the_zones_minimum_clip_to_its_maximum() {
        let range = (Some(200.0), Some(1000.0));
        assert_eq!(FarPlane::ZoneRange.distance(1.0, range), Some(1000.0));
        assert_eq!(FarPlane::ZoneRange.distance(0.5, range), Some(600.0));
        assert_eq!(FarPlane::ZoneRange.distance(0.0, range), Some(200.0));
        // A header that gave no minimum, or one past its maximum.
        assert_eq!(
            FarPlane::ZoneRange.distance(0.5, (None, Some(1000.0))),
            Some(500.0)
        );
        assert_eq!(
            FarPlane::ZoneRange.distance(0.5, (Some(2000.0), Some(1000.0))),
            Some(1000.0)
        );
        assert_eq!(FarPlane::ZoneRange.distance(0.5, (Some(200.0), None)), None);
    }

    #[test]
    fn the_earlier_rule_draws_a_share_of_the_maximum() {
        let range = (Some(200.0), Some(2400.0));
        assert_eq!(FarPlane::ShareOfMax.distance(0.05, range), Some(120.0));
        assert_eq!(
            FarPlane::ShareOfMax.distance(0.05, (None, Some(1000.0))),
            Some(100.0)
        );
        assert_eq!(
            FarPlane::ShareOfMax.distance(0.05, (None, Some(80.0))),
            Some(80.0)
        );
        assert_eq!(FarPlane::ShareOfMax.distance(0.5, range), Some(1200.0));
        assert_eq!(FarPlane::ShareOfMax.distance(0.5, (None, None)), None);
    }
}
