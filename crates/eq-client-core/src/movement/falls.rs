//! What a fall does to the player, as the client works it out for the
//! servers that take a fall's damage from it. The official client's rule is
//! unrecorded, so this one is inferred until falls are measured against it:
//! players say a longer fall hurts more and a long one kills, and Safe Fall
//! lessens it. The server takes the amount as it is, then applies its own
//! reductions from spells, items and AAs, so they are left out here.
use super::{CollisionWorld, Landing};
use glam::Vec3;

/// The highest drop that does no damage (inferred), which is also the
/// deepest a route drops, so that routes never hurt.
pub const HARMLESS_DROP: f32 = 20.0;
/// Damage per square unit of drop past the harmless one (inferred): 160 at
/// 100 units, about 810 at 200 and 11,500 at 700.
const PER_SQUARE_UNIT: f32 = 0.025;
/// The Safe Fall skill past which it saves no more (inferred).
const SAFE_FALL_CAP: u8 = 200;
/// The share of a fall each point of Safe Fall saves (inferred): four fifths
/// at the cap.
const SAFE_FALL_SHARE: f32 = 1.0 / 250.0;

/// The damage a fall from this height does to a player who learned this
/// much Safe Fall (zero where they have not), before the server's own
/// reductions: none up to [`HARMLESS_DROP`], then growing with the square of
/// the drop past it, less Safe Fall's share. All of it is inferred.
#[must_use]
pub fn fall_damage(fall_distance: f32, safe_fall: u32) -> u32 {
    if !fall_distance.is_finite() || fall_distance <= HARMLESS_DROP {
        return 0;
    }
    let past = fall_distance - HARMLESS_DROP;
    let skill = u8::try_from(safe_fall).map_or(SAFE_FALL_CAP, |skill| skill.min(SAFE_FALL_CAP));
    let saved = f32::from(skill) * SAFE_FALL_SHARE;
    let damage = (PER_SQUARE_UNIT * past * past * (1.0 - saved)).round();
    // The servers read more than `i32::MAX` as negative; `2^31` is the first
    // float past it, so anything below converts within range.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Bounded above.
    if damage < 2_147_483_648.0 {
        damage as u32
    } else {
        i32::MAX.unsigned_abs()
    }
}

impl CollisionWorld {
    /// The damage a landing with the feet here does: its fall's
    /// [`fall_damage`], or none where the feet land in water or lava. `EQEmu`
    /// ignores a fall reported there (`Client::Handle_OP_EnvDamage`), and
    /// the official client is taken not to report one (inferred).
    #[must_use]
    pub fn landing_damage(&self, feet: Vec3, landing: Landing, safe_fall: u32) -> u32 {
        if self.in_liquid(feet) {
            return 0;
        }
        fall_damage(landing.fall_distance, safe_fall)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Liquid, liquids::Boxes};
    use super::*;

    #[test]
    fn short_drops_are_free_and_longer_ones_hurt_more() {
        for drop in [0.0, 1.5, 19.9, HARMLESS_DROP, f32::NAN, f32::NEG_INFINITY] {
            assert_eq!(fall_damage(drop, 0), 0, "{drop}");
        }
        assert_eq!(fall_damage(30.0, 0), 3);
        assert_eq!(fall_damage(100.0, 0), 160);
        assert_eq!(fall_damage(200.0, 0), 810);
        assert_eq!(fall_damage(700.0, 0), 11_560);
        // Never more than the servers read as it is.
        assert_eq!(fall_damage(1.0e9, 0), i32::MAX.unsigned_abs());
        assert_eq!(fall_damage(f32::INFINITY, 0), 0);
    }

    #[test]
    fn safe_fall_saves_its_share_up_to_four_fifths() {
        assert_eq!(fall_damage(100.0, 100), 96);
        assert_eq!(fall_damage(100.0, 200), 32);
        // Past the cap, it saves no more.
        for skill in [201, 250, 9999] {
            assert_eq!(fall_damage(100.0, skill), 32, "{skill}");
        }
    }

    /// Water up to y 5 for x from 10, and lava from 20.
    fn pools() -> Boxes {
        Boxes(vec![
            (
                Vec3::new(10.0, -50.0, -50.0),
                Vec3::new(20.0, 5.0, 50.0),
                Liquid::Water,
            ),
            (
                Vec3::new(20.0, -50.0, -50.0),
                Vec3::new(50.0, 5.0, 50.0),
                Liquid::Lava,
            ),
        ])
    }

    #[test]
    fn a_fall_that_ends_in_water_or_lava_does_no_damage() {
        let world = CollisionWorld::new([
            [[-30.0, 0.0, -30.0], [30.0, 0.0, -30.0], [30.0, 0.0, 30.0]],
            [[-30.0, 0.0, -30.0], [30.0, 0.0, 30.0], [-30.0, 0.0, 30.0]],
        ])
        .unwrap()
        .with_liquids(pools());
        let landing = Landing {
            fall_distance: 100.0,
            impact_speed: 40.0,
        };
        assert_eq!(world.landing_damage(Vec3::ZERO, landing, 0), 160);
        assert_eq!(world.landing_damage(Vec3::ZERO, landing, 200), 32);
        for x in [15.0, 25.0] {
            assert_eq!(world.landing_damage(Vec3::X * x, landing, 0), 0, "{x}");
        }
    }
}
