//! Local resource contributions from spell effects, before server-specific modifiers.
//! Effect IDs follow `EQEmu` common/spdat.h; calculation follows zone/bonuses.cpp.
use super::{Effect, Mechanics};

/// Direct additive stat and pool changes; these are not current HP/mana changes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers {
    /// Strength contribution.
    pub strength: i64,
    /// Stamina attribute contribution.
    pub stamina: i64,
    /// Dexterity contribution.
    pub dexterity: i64,
    /// Agility contribution.
    pub agility: i64,
    /// Intelligence contribution.
    pub intelligence: i64,
    /// Wisdom contribution.
    pub wisdom: i64,
    /// Charisma contribution.
    pub charisma: i64,
    /// Flat maximum HP contribution.
    pub hit_points: i64,
    /// Maximum mana contribution.
    pub mana: i64,
    /// Maximum endurance contribution.
    pub endurance: i64,
}

/// One unsupported slot preventing a complete capacity calculation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnresolvedEffect {
    /// Zero-based position in the original twelve-slot spell definition.
    pub slot: usize,
    /// Original effect and formula, preserved for diagnostics and future support.
    pub effect: Effect,
}

/// Partial projection that keeps unsupported mechanics visible to callers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Projection {
    /// Contributions whose local magnitude can be evaluated.
    pub modifiers: Modifiers,
    /// Nonempty means modifiers alone cannot establish the spell's resource impact.
    pub unresolved: Vec<UnresolvedEffect>,
}

impl Mechanics {
    /// Whether every effect is known to leave mana/endurance capacity unchanged.
    /// This is independent of duration, caster level and instrument scaling. It
    /// does not establish stacking: callers must also check possibly replaced buffs.
    #[must_use]
    pub fn preserves_mana_and_endurance_capacity(&self) -> bool {
        self.effects
            .iter()
            .all(|effect| matches!(effect.id, 0..=3 | 10..=15 | 69 | 79 | 254))
    }

    /// Projects direct resource effects without turning unsupported mechanics into zero.
    /// Stacking, instrument scaling, focus and server-specific adjustments remain external.
    pub fn resource_projection(&self, caster_level: u16) -> Projection {
        let mut result = Projection::default();
        for (slot, &effect) in self.effects.iter().enumerate() {
            match effect.id {
                // Current HP, AC, ATK, movement, attack speed, invisibility,
                // see invisibility, water breathing, current mana, and empty slots
                // do not change maximum resources or governing attributes directly.
                // CurrentHPOnce (79) changes current HP once, not the pool size.
                0..=3 | 11..=15 | 79 | 254 => continue,
                4..=10 | 69 | 97 | 159 | 190 => (),
                _ => {
                    result.unresolved.push(UnresolvedEffect { slot, effect });
                    continue;
                }
            }
            let Some(value) = effect.base_magnitude(caster_level) else {
                result.unresolved.push(UnresolvedEffect { slot, effect });
                continue;
            };
            let stats = &mut result.modifiers;
            match effect.id {
                4 => stats.strength += value,
                5 => stats.dexterity += value,
                6 => stats.agility += value,
                7 => stats.stamina += value,
                8 => stats.intelligence += value,
                9 => stats.wisdom += value,
                10 => stats.charisma += value,
                69 => stats.hit_points += value,
                97 => stats.mana += value,
                190 => stats.endurance += value,
                159 => {
                    stats.strength += value;
                    stats.dexterity += value;
                    stats.agility += value;
                    stats.stamina += value;
                    stats.intelligence += value;
                    stats.wisdom += value;
                    stats.charisma += value;
                }
                _ => unreachable!("resource effect filtered above"),
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spell(effects: &[(u32, i32, u32)]) -> Mechanics {
        let mut spell = Mechanics::from_fields(&vec!["0"; 98]).unwrap();
        for ((id, base, formula), slot) in effects.iter().zip(&mut spell.effects) {
            *slot = Effect {
                id: *id,
                base: *base,
                formula: *formula,
                limit: 0,
                maximum: 0,
            };
        }
        spell
    }

    #[test]
    fn capacity_independence_excludes_resource_stats_and_unknowns() {
        for id in [0, 1, 2, 3, 10, 11, 12, 13, 14, 15, 69, 79, 254] {
            assert!(spell(&[(id, 99, 999)]).preserves_mana_and_endurance_capacity());
        }
        for id in [4, 5, 6, 7, 8, 9, 97, 159, 190, 9999] {
            assert!(!spell(&[(id, 0, 100)]).preserves_mana_and_endurance_capacity());
        }
    }

    #[test]
    fn distinct_attributes_and_capacity_fields_keep_penalties() {
        let projection = spell(&[
            (4, 1, 100),
            (5, 2, 100),
            (6, 3, 100),
            (7, 4, 100),
            (8, 5, 100),
            (9, 6, 100),
            (10, -7, 100),
            (69, 8, 100),
            (97, -9, 100),
            (190, 10, 100),
            (159, 20, 100),
        ])
        .resource_projection(1);
        assert!(projection.unresolved.is_empty());
        assert_eq!(
            projection.modifiers,
            Modifiers {
                strength: 21,
                dexterity: 22,
                agility: 23,
                stamina: 24,
                intelligence: 25,
                wisdom: 26,
                charisma: 13,
                hit_points: 8,
                mana: -9,
                endurance: 10,
            }
        );
    }

    #[test]
    fn healing_and_regeneration_are_not_capacity_and_unknowns_remain_visible() {
        let projection = spell(&[
            (0, 1000, 100),
            (15, 20, 100),
            (1, 15, 100),
            (97, 12, 999),
            (9999, 0, 100),
            (9, 10, 102),
        ])
        .resource_projection(3);
        assert_eq!(
            projection.modifiers,
            Modifiers {
                wisdom: 13,
                ..Default::default()
            }
        );
        assert_eq!(
            projection
                .unresolved
                .iter()
                .map(|entry| entry.slot)
                .collect::<Vec<_>>(),
            [3, 4]
        );
        assert_eq!(projection.unresolved[0].effect.formula, 999);
        assert_eq!(projection.unresolved[1].effect.id, 9999);
    }

    #[test]
    fn maximum_hp_and_one_time_heal_are_not_double_counted() {
        // Synthetic values for the documented buff pattern: AC, max HP, immediate HP.
        let projection = spell(&[(1, 3, 100), (69, 7, 100), (79, 7, 100)]).resource_projection(2);
        assert!(projection.unresolved.is_empty());
        assert_eq!(
            projection.modifiers,
            Modifiers {
                hit_points: 7,
                ..Default::default()
            }
        );
    }
}
