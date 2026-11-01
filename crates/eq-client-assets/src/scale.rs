//! How an installed client draws a character model at a spawn's size, from
//! the program. A client generation's rule is its [`ModelScale`], from
//! [`crate::ui::InstalledClient::model_scale`].
//!
//! Both programs draw a model at size × 0.2 of its mesh as authored, so a
//! spawn of size 5 draws the mesh as it is: a short mesh, such as a wolf's,
//! draws short at any size, and a spawn's size is not its drawn height.
//! Players and other spawns follow the same rule. Read from the Mac-era
//! program's height and scale routine (0x51f79b) and the Titanium program's
//! (0x47fa9e, its scale 0x47a0e9, its default sizes 0x47a147 and its
//! per-race overrides 0x47a239) in program reads N1 and N1b of 11-01.

/// How a client generation draws its models at a spawn's size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelScale {
    generation: Generation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Generation {
    Titanium,
    EqMac,
}

/// The Titanium client's rule.
pub const TITANIUM: ModelScale = ModelScale {
    generation: Generation::Titanium,
};

/// The Mac-era client's rule.
pub const EQMAC: ModelScale = ModelScale {
    generation: Generation::EqMac,
};

/// What a zone tells the Titanium client about drawing its models.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ZoneScale {
    /// The zone header's classic model scale flag is set: the classic
    /// models draw at 1/1.3 of their usual scale (0x47a0e9).
    pub classic_reduced: bool,
}

impl ModelScale {
    /// The size a spawn sent without one (size 0, or not a number) takes:
    /// the program writes its race's default in its place. `gender` is the
    /// spawn's, as the server sent it.
    #[must_use]
    pub fn default_size(self, race: u32, gender: u32) -> f32 {
        match (race, gender) {
            (0x2a | 0x78, 2) | (0xc | 0x58, _) => 3.0,
            (2 | 0x82, _) => 7.0,
            (4 | 6 | 0x4d | 0x70, _) => 5.0,
            (7, _) => 5.5,
            (8 | 0x5e, _) => 4.0,
            (9 | 0x5c, _) => 8.0,
            (10 | 0x5d, _) => 9.0,
            (0xb | 0x51, _) => 3.5,
            (0x2b, _) => 4.7,
            // Only the Titanium program's table lists these.
            (0x5a, _) if self.generation == Generation::Titanium => 7.0,
            (0x14a, _) if self.generation == Generation::Titanium => 5.0,
            _ => 6.0,
        }
    }

    /// The size the program draws a spawn by: the size sent, or the race's
    /// default where none was sent. The Mac-era program draws race 63, and
    /// races 42 and 120 sent with gender 2 (the wolf forms), at 45% of the
    /// size sent.
    #[must_use]
    pub fn size(self, race: u32, gender: u32, size: f32) -> f32 {
        if !(size.is_finite() && size > 0.0) {
            return self.default_size(race, gender);
        }
        let wolf_form = race == 0x3f || (matches!(race, 0x2a | 0x78) && gender == 2);
        if self.generation == Generation::EqMac && wolf_form {
            size * 0.45
        } else {
            size
        }
    }

    /// How many times its authored size the program draws a spawn's model.
    /// `zone` is what the zone's header said; the Mac-era rule ignores it.
    ///
    /// Titanium: classic models (races below 405 other than 403) draw at
    /// size × 0.2, × 1/1.3 while the zone's flag is set; the newer ones at
    /// size / 6. The program draws those × 1.3 when the flag and a second
    /// setting ([0x923798], not yet read) are both clear: that is not
    /// applied (recording checklist 6.346). The per-race overrides' heights
    /// are not applied either (6.346).
    #[must_use]
    pub fn drawn_scale(self, race: u32, gender: u32, size: f32, zone: ZoneScale) -> f32 {
        let size = self.size(race, gender, size);
        match self.generation {
            Generation::EqMac => match race {
                0x48 => 2.0,
                0x6c => 0.05,
                0x31 | 0x49 | 0x72 | 0x8d | 0x8e | 0x8f | 0x9e | 0xab | 0xc4 | 0xfc => 1.0,
                _ => size * 0.2,
            },
            Generation::Titanium => match race {
                0x48 => 2.0,
                0x6c => 0.05,
                0x31 | 0x49 | 0x72 | 0x8d | 0x8e | 0x8f | 0x9e | 0xc4 | 0x194 => 1.0,
                0x193 | 0x195.. => size / 6.0,
                _ if zone.classic_reduced => size * 0.2 / 1.3,
                _ => size * 0.2,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn a_human_of_size_six_draws_at_six_fifths_of_its_mesh() {
        let zone = ZoneScale::default();
        assert!(close(TITANIUM.drawn_scale(1, 0, 6.0, zone), 1.2));
        assert!(close(EQMAC.drawn_scale(1, 0, 6.0, zone), 1.2));
    }

    #[test]
    fn the_mac_era_client_draws_wolf_forms_at_under_half_their_size() {
        let zone = ZoneScale::default();
        assert!(close(EQMAC.drawn_scale(42, 2, 6.0, zone), 0.54));
        assert!(close(EQMAC.drawn_scale(63, 0, 6.0, zone), 0.54));
        // Another gender of race 42 draws at its size, as Titanium's do.
        assert!(close(EQMAC.drawn_scale(42, 0, 6.0, zone), 1.2));
        assert!(close(TITANIUM.drawn_scale(42, 2, 6.0, zone), 1.2));
    }

    #[test]
    fn titaniums_newer_models_draw_at_a_sixth_of_their_size() {
        let zone = ZoneScale::default();
        assert!(close(TITANIUM.drawn_scale(0x195, 0, 6.0, zone), 1.0));
        assert!(close(TITANIUM.drawn_scale(0x193, 0, 3.0, zone), 0.5));
        // The zone's flag cuts only the classic models.
        let reduced = ZoneScale {
            classic_reduced: true,
        };
        assert!(close(TITANIUM.drawn_scale(1, 0, 6.5, reduced), 1.0));
        assert!(close(TITANIUM.drawn_scale(0x195, 0, 6.0, reduced), 1.0));
        assert!(close(EQMAC.drawn_scale(1, 0, 6.0, reduced), 1.2));
    }

    #[test]
    fn a_spawn_sent_without_a_size_takes_its_races_default() {
        let zone = ZoneScale::default();
        assert!(close(TITANIUM.drawn_scale(8, 0, 0.0, zone), 0.8));
        assert!(close(EQMAC.drawn_scale(10, 1, 0.0, zone), 1.8));
        assert!(close(EQMAC.size(42, 2, 0.0), 3.0));
        assert!(close(TITANIUM.size(0x14a, 0, f32::NAN), 5.0));
        assert!(close(EQMAC.size(0x14a, 0, 0.0), 6.0));
    }

    #[test]
    fn listed_races_draw_at_a_fixed_scale() {
        let zone = ZoneScale::default();
        for client in [TITANIUM, EQMAC] {
            assert!(close(client.drawn_scale(0x48, 0, 30.0, zone), 2.0));
            assert!(close(client.drawn_scale(0x6c, 0, 30.0, zone), 0.05));
            assert!(close(client.drawn_scale(0x49, 0, 30.0, zone), 1.0));
        }
        assert!(close(TITANIUM.drawn_scale(0x194, 0, 30.0, zone), 1.0));
    }
}
