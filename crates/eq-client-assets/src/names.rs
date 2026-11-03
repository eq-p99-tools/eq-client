//! The colours an installed client draws names over heads in, from the
//! program. A client generation's rule is its [`NameColours`], from
//! [`crate::ui::InstalledClient::name_colours`].
//!
//! Both programs pick the supported base colours from constants in their
//! code (Titanium 0x47aa0b; Mac-era 0x4b114d). The Mac-era routine reaches
//! `s3dSetStringSpriteTint` at 0x4b1427. No guild comparison or pet-owner
//! lookup changes these base colours: guildmates are players, pets and
//! corpses are non-players. Unresolved flag branches stay unapplied.
//! Titanium program read N4 of 11-01 and its follow-up on the zone rule.
//! The byte order is
//! inferred from the default player colour reading blue, as players
//! remember it (recording checklist 6.354).

/// An RGB colour, a byte a channel.
pub type Rgb = [u8; 3];

/// What a name's colour depends on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Named {
    /// A player character, rather than an NPC, a corpse or another spawn.
    pub player: bool,
    /// Its race, by the server's number.
    pub race: u32,
    /// The roleplay bit of its anonymity is set: `/roleplay`, alone or with
    /// `/anonymous`.
    pub roleplaying: bool,
    /// The world's player-versus-player ruleset, by the server's number,
    /// from its rules at login.
    pub pvp_rule: u32,
}

/// How a client generation colours names over heads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameColours {
    generation: Generation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Generation {
    Titanium,
    EqMac,
}

/// The Titanium client's colours.
pub const TITANIUM: NameColours = NameColours {
    generation: Generation::Titanium,
};

/// The Mac-era client's directly read base colours (0x4b114d), resolving
/// the earlier white fallback recorded in checklist 6.355.
pub const EQMAC: NameColours = NameColours {
    generation: Generation::EqMac,
};

/// The first race number the Titanium program draws as an object rather
/// than a character.
const FIRST_OBJECT_RACE: u32 = 0x8cd;

impl NameColours {
    /// The colour a name is drawn in.
    ///
    /// Both generations: players blue, and purple while roleplaying where
    /// the world's ruleset is 0 or 4; NPCs, pets and corpses teal; objects green.
    /// Not applied, as their meanings are unread (recording checklist
    /// 6.353): a red and a cyan the program gives some players, a green it
    /// gives some spawns by a flag, ruleset 2's colours by a group value,
    /// and an object's second colour.
    #[must_use]
    pub const fn colour(self, named: Named) -> Rgb {
        match self.generation {
            Generation::Titanium | Generation::EqMac => {
                if named.race >= FIRST_OBJECT_RACE {
                    [102, 255, 0]
                } else if !named.player {
                    [23, 184, 173]
                } else if named.roleplaying && matches!(named.pvp_rule, 0 | 4) {
                    [133, 72, 156]
                } else {
                    [61, 107, 220]
                }
            }
        }
    }

    /// The colour of the player's target's name at `step` of the program's
    /// eleven (0 to 10): white at 0, and a tenth of the way to its own
    /// colour more each step, each channel 255 − ((255 − c) / 10) × step in
    /// whole numbers. Both programs step it each time they draw the name
    /// (Titanium 0x47aa0b, Mac-era 0x4b1380); how fast is this client's
    /// (recording checklist 6.354).
    #[must_use]
    pub const fn target(self, colour: Rgb, step: u8) -> Rgb {
        const fn channel(c: u8, step: u8) -> u8 {
            255 - (255 - c) / 10 * step
        }
        let step = if step > 10 { 10 } else { step };
        [
            channel(colour[0], step),
            channel(colour[1], step),
            channel(colour[2], step),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player() -> Named {
        Named {
            player: true,
            race: 1,
            ..Named::default()
        }
    }

    #[test]
    fn titanium_draws_players_blue_and_roleplayers_purple() {
        assert_eq!(TITANIUM.colour(player()), [61, 107, 220]);
        let roleplaying = Named {
            roleplaying: true,
            ..player()
        };
        assert_eq!(TITANIUM.colour(roleplaying), [133, 72, 156]);
        // Ruleset 4 allows it; ruleset 2's colours are unread, so blue.
        for (rule, colour) in [
            (4, [133, 72, 156]),
            (2, [61, 107, 220]),
            (1, [61, 107, 220]),
        ] {
            let named = Named {
                pvp_rule: rule,
                ..roleplaying
            };
            assert_eq!(TITANIUM.colour(named), colour, "rule {rule}");
        }
    }

    #[test]
    fn titanium_draws_other_spawns_teal_and_objects_green() {
        let npc = Named {
            player: false,
            roleplaying: true,
            ..player()
        };
        assert_eq!(TITANIUM.colour(npc), [23, 184, 173]);
        let object = Named { race: 0x8cd, ..npc };
        assert_eq!(TITANIUM.colour(object), [102, 255, 0]);
    }

    #[test]
    fn the_mac_era_program_defines_the_same_supported_base_colours() {
        assert_eq!(EQMAC.colour(player()), [61, 107, 220]);
        let npc = Named {
            player: false,
            ..player()
        };
        assert_eq!(EQMAC.colour(npc), [23, 184, 173]);
        assert_eq!(
            EQMAC.colour(Named {
                roleplaying: true,
                ..player()
            }),
            [133, 72, 156]
        );
        assert_eq!(EQMAC.colour(Named { race: 0x8cd, ..npc }), [102, 255, 0]);
        assert_eq!(EQMAC.target([61, 107, 220], 4), [179, 199, 243]);
    }

    #[test]
    fn a_targets_name_steps_from_white_to_its_colour() {
        let blue = [61, 107, 220];
        assert_eq!(TITANIUM.target(blue, 0), [255, 255, 255]);
        assert_eq!(TITANIUM.target(blue, 5), [160, 185, 240]);
        assert_eq!(TITANIUM.target(blue, 10), [65, 115, 225]);
        assert_eq!(TITANIUM.target(blue, 12), [65, 115, 225]);
    }
}
