//! How an installed client's player falls and what a landing costs, from
//! the program. A client generation's rules are its [`ClientFalls`], from
//! [`crate::ui::InstalledClient::falls`]; a generation whose rules are not
//! yet read has none, and keeps the provisional fall and inferred damage.
//!
//! The Mac-era rules are from the TAKP client's executable, read statically
//! on 10-30 (TAKP Batch 1 R3, recording checklist item 6.161), and the
//! Titanium ones from its program (sittings 6, 8g, 8h and 8i), except where
//! a rule says it is this client's reading or not yet known.

/// The length of the program's movement step, in seconds: it counts
/// elapsed milliseconds × 0.02 steps (the clock's unit is unsure, 6.161).
pub const STEP_SECONDS: f32 = 0.05;

/// How a client generation falls, and what its landings cost.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClientFalls {
    /// Downward acceleration, in units a second squared, until the zone's
    /// header says: the program adds the header's gravity (`OP_NewZone`
    /// +420) to the fall speed each step (TAKP sitting 2, M1; the Titanium
    /// program reads the same field, sitting 8g), and 0.4 a step is the
    /// default before a header comes.
    pub gravity: f32,
    /// The fastest fall, in units a second: where the air's slowing meets
    /// the default gravity at one frame a step. The program has no cap of
    /// its own; the session's motion guard holds falls to this.
    pub terminal_speed: f32,
    /// The upward speed a jump starts with, in units a second.
    pub jump_speed: f32,
    /// The share of the vertical speed the air leaves after each frame. The
    /// program applies it per rendered frame, so its effect depends on the
    /// frame rate; this client applies it per 50 ms step (unsure, 6.161).
    pub air_factor: f32,
    /// The fall speed a landing must pass to count as a fall, in units a
    /// step.
    pub gate: f32,
    /// Zones where a landing costs nothing, by number.
    pub harmless_zones: &'static [u16],
    /// The extra share of Safe Fall each rank of its raising ability gives.
    pub safe_fall_rank_share: f32,
    /// The line the client says a fall's damage in, by its id in the
    /// client's string table, with the damage.
    pub damage_line: Option<u32>,
    /// The line it says after that on a fall the player lives through.
    pub survived_line: Option<u32>,
    /// How far above the floor the feet may be, after this step's vertical
    /// speed, and still count as on the ground, in units: past it the
    /// player is in the air and falls.
    pub ground_tolerance: f32,
    /// How far above the spawn's origin the program's floor sweep starts, in
    /// units. Each step it sweeps a sphere straight down from there and
    /// stands the feet on the first surface it meets ([`Self::climb_per_step`]).
    pub sweep_start: f32,
    /// The floor sweep's sphere radius and the measurement it scales.
    pub sweep_radius: FloorSweepRadius,
    /// The landing rule counts the carried weight in whole units, cut
    /// toward zero, as an integer field holds it; else as the weight is.
    pub whole_weight: bool,
    /// Where a spawn other than the player's that falls under the zone with
    /// no floor beneath it is put back.
    pub underworld_return: UnderworldReturn,
}

/// The body measurement a client's floor sweep uses to size its sphere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloorSweepRadius {
    /// A fraction of the spawn's size.
    SpawnSize(f32),
    /// A fraction of the scaled model's `ROffset`.
    ModelOffset(f32),
}

/// How the program puts back a spawn other than the player's that falls with
/// no floor beneath it until its feet pass under the zone's lowest height:
/// it finds the zone's top surface at the spawn's spot, straight down from
/// [`Self::search_from`], and puts the spawn there if that surface lies more
/// than [`Self::under_top`] under the zone's top, else leaves it be. Read
/// for Titanium (R-s Titanium addendum, section 5), but for a zone flag
/// (0x923b88) it does so under, whose meaning is unread and which this
/// client takes as set in every zone (checklist 6.362).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnderworldReturn {
    /// The height the search for the top surface starts from.
    pub search_from: f32,
    /// How far under the zone's top that surface must lie.
    pub under_top: f32,
}

/// What a landing costs, before the server's own reductions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FallCost {
    /// The damage to report; none for a landing that costs nothing.
    pub damage: u32,
    /// Whether Safe Fall was used on it, which the program announces to the
    /// server before the damage (`OP_SafeFallSuccess`).
    pub safe_fall: bool,
}

/// What a landing on the player is made of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallLanding {
    /// The downward speed at touchdown, in units a second.
    pub impact_speed: f32,
    /// What the player carries, in the program's weight units.
    pub carried_weight: f32,
    /// The player's Safe Fall skill.
    pub safe_fall: u32,
    /// The ranks of the ability that raises Safe Fall (AA 122).
    pub safe_fall_ranks: u8,
    /// The zone it happens in, by number.
    pub zone: u16,
}

/// The damage at which the program stops working the amount out and
/// reports this flat amount, which Safe Fall does not lessen.
const DEADLY_FALL: u32 = 20_000;
/// The value of T from which a fall does [`DEADLY_FALL`].
const DEADLY_T: f32 = 9.0;

impl ClientFalls {
    /// Gravity in units a second squared for a zone header's gravity, the
    /// fall speed it adds each step; a header of 0 pulls nothing.
    #[must_use]
    pub fn zone_gravity(&self, per_step: f32) -> f32 {
        if per_step.is_finite() && per_step >= 0.0 {
            per_step / (STEP_SECONDS * STEP_SECONDS)
        } else {
            self.gravity
        }
    }

    /// The fastest fall under this gravity, in units a second, where the
    /// air's slowing meets it at one frame a step, held to
    /// [`ClientFalls::terminal_speed`].
    #[must_use]
    pub fn terminal_under(&self, gravity: f32) -> f32 {
        let per_step = gravity * STEP_SECONDS * STEP_SECONDS;
        let terminal = per_step * self.air_factor / (1.0 - self.air_factor) / STEP_SECONDS;
        if terminal.is_finite() && terminal > 0.0 {
            terminal.min(self.terminal_speed)
        } else {
            self.terminal_speed
        }
    }

    /// How far one step can lift the feet of a body whose origin stands
    /// `stand_height` above them, at `size`, in units: the floor sweep starts
    /// at the origin (raised by [`Self::sweep_start`]) and takes the first
    /// surface it meets, so any floor up to there less the sphere's radius is
    /// stood on in one step (read 6.344). What blocks a move short of it is
    /// the horizontal collision's, which this client still makes with its own
    /// capsule (6.344 remainder).
    /// Titanium uses this client's standing-height estimate for the scaled
    /// model reference. This does not determine the origin of a received placement.
    #[must_use]
    pub fn climb_per_step(&self, stand_height: f32, size: f32) -> f32 {
        let radius = match self.sweep_radius {
            FloorSweepRadius::SpawnSize(factor) => factor * size,
            FloorSweepRadius::ModelOffset(factor) => factor * stand_height,
        };
        let climb = stand_height + self.sweep_start - radius;
        if climb.is_finite() {
            climb.max(0.0)
        } else {
            0.0
        }
    }

    /// What a landing costs: with the speed past the gate in units a step,
    /// T = carried weight / 100 + (speed − 2.5) − 1.5, the weight whole
    /// where [`Self::whole_weight`] says so. None at T ≤ 0, a
    /// flat 20,000 at T ≥ 9, else trunc(10·T²) less Safe Fall's skill
    /// (raised for each rank of AA 122), at least 0. Safe Fall is used on
    /// every landing it applies to that the skill is above 0 for.
    #[must_use]
    pub fn landing_cost(&self, landing: FallLanding) -> FallCost {
        let speed = landing.impact_speed * STEP_SECONDS;
        if !self.can_hurt(speed, landing.zone) {
            return FallCost::default();
        }
        let weight = landing.carried_weight.max(0.0);
        let weight = if self.whole_weight {
            weight.trunc()
        } else {
            weight
        };
        let t = weight / 100.0 + (speed - 2.5) - 1.5;
        if t >= DEADLY_T {
            return FallCost {
                damage: DEADLY_FALL,
                safe_fall: false,
            };
        }
        // Below 9, 10·T² is below 810.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let raw = if t > 0.0 { (10.0 * t * t) as u32 } else { 0 };
        if raw == 0 {
            return FallCost::default();
        }
        let share = 1.0 + self.safe_fall_rank_share * f32::from(landing.safe_fall_ranks);
        // The skill is far below 2^24, where f32 stays exact.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let saved = (landing.safe_fall as f32 * share) as u32;
        FallCost {
            damage: raw.saturating_sub(saved),
            safe_fall: landing.safe_fall > 0,
        }
    }

    /// Returns a landing cost only when it is independent of the carried
    /// load. The impact gate and exempt zones are always harmless. An
    /// impact already deadly at zero load is deadly at any greater load.
    #[must_use]
    pub fn landing_cost_without_weight(&self, impact_speed: f32, zone: u16) -> Option<FallCost> {
        let speed = impact_speed * STEP_SECONDS;
        if !self.can_hurt(speed, zone) {
            return Some(FallCost::default());
        }
        ((speed - 2.5) - 1.5 >= DEADLY_T).then_some(FallCost {
            damage: DEADLY_FALL,
            safe_fall: false,
        })
    }

    fn can_hurt(&self, speed: f32, zone: u16) -> bool {
        speed.is_finite() && speed > self.gate && !self.harmless_zones.contains(&zone)
    }
}

/// The Mac-era client's falls (from the program, 6.161). A jump adds
/// 1.65 × 1.35 = 2.2275 units a step to the rising speed (TAKP sitting 2,
/// section M1; recording checklist item 1.3).
pub const EQMAC: ClientFalls = ClientFalls {
    gravity: 0.4 / (STEP_SECONDS * STEP_SECONDS),
    // 0.4 × 0.99 / (1 − 0.99) units a step.
    terminal_speed: 39.6 / STEP_SECONDS,
    jump_speed: 1.65 * 1.35 / STEP_SECONDS,
    air_factor: 0.99,
    gate: 2.5,
    harmless_zones: &[151],
    safe_fall_rank_share: 0.1,
    damage_line: Some(12481),
    survived_line: Some(12118),
    // 0.5 + 0.001, both programs (R-s: Titanium 0x48a2f4, Mac-era 0x50ba07).
    ground_tolerance: 0.5 + 0.001,
    // The floor finder 0x4b3a84 sweeps from the origin + 0.5, radius
    // size / 4; the origin stands +0x90 above the feet, 3.125 × the drawn
    // scale (0x51f79b; read 6.344).
    sweep_start: 0.5,
    sweep_radius: FloorSweepRadius::SpawnSize(0.25),
    // W is the weight routine's float (R3, 0x4bbb78).
    whole_weight: false,
    // Titanium's (below), taken as the Mac-era program's until read (6.362).
    underworld_return: TITANIUM_UNDERWORLD_RETURN,
};

/// The Titanium program's return from under the zone: the search starts at
/// 32,000 (0x64863c, through 0x439f26), and the surface must lie under the
/// zone's top (0x923bd8) less 10 (R-s Titanium addendum, section 5).
const TITANIUM_UNDERWORLD_RETURN: UnderworldReturn = UnderworldReturn {
    search_from: 32_000.0,
    under_top: 10.0,
};

/// The Titanium client's falls (from the program): the same gravity field,
/// 50 ms step, 0.99 air factor and 1.65 × 1.35 jump as the Mac-era client
/// (sittings 8g, 8h and 8i), and the same landing rule (sitting 6, T2):
/// a landing counts past 2.5 units a step, then T = the character's +0x48
/// field / 100 + (speed − 2.5) − 1.5, 20,000 from T = 9, else
/// trunc(10·T²), less Safe Fall's S + trunc(S × P / 100), S the raw skill
/// word (R-s Safe Fall follow-up). The same on-ground tolerance too (R-s,
/// 0x48a2f4). Its floor sweep starts at the origin itself (0x48a6f2 feeds
/// 0x48a0fc the origin). The radius is 0.1 times the scaled `ROffset`
/// at actor +0x238. Initialization at 0x47fa9e reads the model's reference
/// through 0x4b6d42 and applies its scale. The link between a received
/// position and this client's standing-height estimate remains inferred
/// (6.347). Read but not yet known,
/// so left out, each costing the player more than the program would, never
/// less: which zones zero the damage (its table was not read, 6.304), P
/// (spell effect 228's total; not decoded, so it counts as 0, 6.305 and
/// 6.338, which also notes that the program announces Safe Fall only on
/// that reduced path), the character flag that zeroes damage under 200 (6.306), and any
/// line the client says (none in the damage routine; not traced further).
/// That +0x48 is the carried weight the Mac-era rule names is inferred
/// (6.303).
pub const TITANIUM: ClientFalls = ClientFalls {
    harmless_zones: &[],
    safe_fall_rank_share: 0.0,
    damage_line: None,
    survived_line: None,
    sweep_start: 0.0,
    sweep_radius: FloorSweepRadius::ModelOffset(0.1),
    // The rule loads +0x48 as an integer (int(+0x48) × 0.01, R-s Safe Fall
    // read); that it holds the carried weight cut toward zero is inferred
    // with the field itself (6.303).
    whole_weight: true,
    underworld_return: TITANIUM_UNDERWORLD_RETURN,
    ..EQMAC
};

#[cfg(test)]
mod tests {
    use super::*;

    fn landing(speed_a_step: f32, carried_weight: f32, safe_fall: u32) -> FallLanding {
        FallLanding {
            impact_speed: speed_a_step / STEP_SECONDS,
            carried_weight,
            safe_fall,
            safe_fall_ranks: 0,
            zone: 1,
        }
    }

    #[test]
    fn the_program_counts_speed_past_the_gate_and_weight() {
        // Speed 4 a step with nothing carried is T = 0.
        assert_eq!(EQMAC.landing_cost(landing(4.0, 0.0, 0)).damage, 0);
        assert_eq!(EQMAC.landing_cost(landing(5.0, 0.0, 0)).damage, 10);
        assert_eq!(EQMAC.landing_cost(landing(6.0, 0.0, 0)).damage, 40);
        // 100 of carried weight is one more unit of speed.
        assert_eq!(EQMAC.landing_cost(landing(5.0, 100.0, 0)).damage, 40);
        // T = 8.5: trunc(722.5).
        assert_eq!(EQMAC.landing_cost(landing(12.5, 0.0, 0)).damage, 722);
        // From T = 9, a flat 20,000 that Safe Fall does not lessen.
        assert_eq!(
            EQMAC.landing_cost(landing(13.0, 0.0, 200)),
            FallCost {
                damage: 20_000,
                safe_fall: false
            }
        );
        // Titanium counts whole units of weight, the Mac-era client all of
        // it: at 7.8 a step, T = 0.17 or 0.179 + 3.8, so 157 or 158.
        assert_eq!(TITANIUM.landing_cost(landing(7.8, 17.9, 0)).damage, 157);
        assert_eq!(EQMAC.landing_cost(landing(7.8, 17.9, 0)).damage, 158);
        // At or under the gate is no fall, however much is carried.
        assert_eq!(EQMAC.landing_cost(landing(2.5, 900.0, 0)).damage, 0);
        assert_eq!(EQMAC.landing_cost(landing(f32::NAN, 0.0, 0)).damage, 0);
    }

    #[test]
    fn safe_fall_takes_off_its_skill_and_is_announced_when_it_had_damage_to_lessen() {
        assert_eq!(
            EQMAC.landing_cost(landing(6.0, 0.0, 25)),
            FallCost {
                damage: 15,
                safe_fall: true
            }
        );
        assert_eq!(
            EQMAC.landing_cost(landing(6.0, 0.0, 90)),
            FallCost {
                damage: 0,
                safe_fall: true
            }
        );
        // A landing that does no damage uses none.
        assert_eq!(
            EQMAC.landing_cost(landing(3.0, 0.0, 90)),
            FallCost::default()
        );
        // Each rank raises the skill by a tenth: 20 × 1.3 = 26.
        let ranked = FallLanding {
            safe_fall_ranks: 3,
            ..landing(6.0, 0.0, 20)
        };
        assert_eq!(EQMAC.landing_cost(ranked).damage, 14);
    }

    #[test]
    fn a_step_climbs_to_the_floor_sweeps_start_less_its_radius() {
        // At a standing height of 3.75, Titanium's radius is 0.375
        // regardless of the separate spawn size.
        assert!((TITANIUM.climb_per_step(3.75, 6.0) - 3.375).abs() < 1e-5);
        assert!((TITANIUM.climb_per_step(3.75, 8.0) - 3.375).abs() < 1e-5);
        assert!((TITANIUM.climb_per_step(2.5, 6.0) - 2.25).abs() < 1e-5);
        // EQMac retains its size-based radius.
        assert!((EQMAC.climb_per_step(3.75, 6.0) - 2.75).abs() < 1e-5);
        assert!((EQMAC.climb_per_step(3.75, 8.0) - 2.25).abs() < 1e-5);
        // A size 3 gnome: under a two-unit riser.
        assert!((TITANIUM.climb_per_step(1.875, 3.0) - 1.6875).abs() < 1e-5);
        assert!(TITANIUM.climb_per_step(f32::NAN, 6.0).abs() < f32::EPSILON);
        assert!(TITANIUM.climb_per_step(-0.1, 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn titanium_falls_like_the_mac_era_client_and_says_nothing() {
        for speed in [2.5, 4.0, 5.0, 12.5, 13.0] {
            assert_eq!(
                TITANIUM.landing_cost(landing(speed, 0.0, 0)),
                EQMAC.landing_cost(landing(speed, 0.0, 0)),
                "{speed}"
            );
        }
        // No zone table is read for it, so zone 151 hurts like any other.
        let anywhere = FallLanding {
            zone: 151,
            ..landing(6.0, 0.0, 0)
        };
        assert_eq!(TITANIUM.landing_cost(anywhere).damage, 40);
        assert_eq!((TITANIUM.damage_line, TITANIUM.survived_line), (None, None));
        assert!((TITANIUM.jump_speed - 44.55).abs() < 0.001);
        assert!((TITANIUM.gravity - 160.0).abs() < 0.001);
    }

    #[test]
    fn some_zones_take_nothing_for_a_fall() {
        let harmless = FallLanding {
            zone: 151,
            ..landing(12.0, 0.0, 0)
        };
        assert_eq!(EQMAC.landing_cost(harmless), FallCost::default());
    }

    #[test]
    fn the_terminal_speed_is_where_the_air_meets_gravity() {
        let step = EQMAC.terminal_speed * STEP_SECONDS;
        let next = (step + EQMAC.gravity * STEP_SECONDS * STEP_SECONDS) * EQMAC.air_factor;
        assert!((next - step).abs() < 0.001, "{next}");
        // A jump starts at 44.55 units a second.
        assert!(
            (EQMAC.jump_speed - 44.55).abs() < 0.001,
            "{}",
            EQMAC.jump_speed
        );
    }
}
