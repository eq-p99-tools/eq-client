//! Read breath modifiers from the current inventory and active effects.
use crate::spellbook::SpellNames;
use eq_client_assets::spells::Effect;
use eq_client_core::{
    ItemHitPoints, PlayerState,
    breath::{SET_BREATH_LEVEL, WATER_BREATHING},
    resources::{eqemu_equipped_modifiers, eqemu_item_hit_points},
    world::ClientWorld,
};

#[derive(Clone, Copy, Default)]
struct Effects {
    stamina: i64,
    breath_level: i64,
    fire_resistance: i64,
    water_breathing: bool,
    invulnerable: bool,
}

impl Effects {
    fn add(&mut self, other: Self) {
        self.stamina += other.stamina;
        self.breath_level += other.breath_level;
        self.fire_resistance += other.fire_resistance;
        self.water_breathing |= other.water_breathing;
        self.invulnerable |= other.invulnerable;
    }
}

#[derive(Clone, Copy)]
pub(super) enum Need {
    Air,
    Heat,
}

fn project(effects: &[Effect], level: u16, instrument: u32, need: Need) -> Option<Effects> {
    let mut result = Effects::default();
    for effect in effects {
        if effect.id == 40 {
            // Divine Aura can suppress the native environmental send.
            result.invulnerable = true;
            continue;
        }
        let affects = match need {
            Need::Air => matches!(effect.id, 7 | 159 | WATER_BREATHING | SET_BREATH_LEVEL),
            Need::Heat => matches!(effect.id, 46 | 111),
        };
        if !affects {
            continue;
        }
        if !matches!(instrument, 0 | 10)
            || (level == 0 && !matches!(effect.formula, 0 | 100))
            || matches!(effect.formula, 60 | 70)
        {
            return None;
        }
        let value = effect.base_magnitude(level)?;
        match effect.id {
            7 | 159 => result.stamina += value,
            46 | 111 => result.fire_resistance += value,
            WATER_BREATHING if value >= 0 => result.water_breathing |= value > 0,
            SET_BREATH_LEVEL => result.breath_level += value,
            _ => return None,
        }
    }
    Some(result)
}

pub(super) struct Inputs {
    pub(super) stamina: Option<i64>,
    pub(super) breath_level: i64,
    pub(super) water_breathing: bool,
    pub(super) invulnerable: bool,
    pub(super) fire_resistance: Option<i64>,
    item_hp: i64,
}

impl Inputs {
    /// Use received maximum HP and add known direct item HP when the
    /// report leaves it out. A display estimate is insufficient for damage.
    pub(super) fn maximum_hp(&self, world: &ClientWorld) -> Option<u32> {
        let report = world.vitals().reported_hp?;
        if report.maximum <= 0 {
            return None;
        }
        let maximum = i64::from(report.maximum)
            + if report.items == ItemHitPoints::LeftOut {
                self.item_hp
            } else {
                0
            };
        u32::try_from(maximum.min(10_000_000)).ok()
    }
}

pub(super) fn read(
    world: &ClientWorld,
    spells: &SpellNames,
    player: &PlayerState,
    need: Need,
) -> Option<Inputs> {
    let profile = player.titanium_environment?;
    if profile.has_alternate_advancement {
        return None;
    }
    let inventory = world.worker_inventory();
    let level = u16::from(player.level);
    let equipment = eqemu_equipped_modifiers(inventory, player.class?, player.race, level).ok()?;
    let item_hp = eqemu_item_hit_points(&equipment);
    let (stamina, mut fire_resistance) = crate::speed::food_environment(inventory, level)?;
    let mut combined = Effects {
        stamina,
        ..Effects::default()
    };
    let evaluated =
        |id, level, instrument| project(&spells.mechanics(id)?.effects, level, instrument, need);
    for (_, item) in equipment {
        combined.stamina += i64::from(item.bonuses.stamina);
        fire_resistance = fire_resistance
            .zip(item.bonuses.fire_resistance)
            .map(|(total, item)| total + i64::from(item));
        if let Some(worn) = item.worn {
            if worn.effect_type != 2 {
                return None;
            }
            let caster_level = if worn.level == 0 {
                level
            } else {
                u16::try_from(worn.level).ok()?
            };
            combined.add(evaluated(worn.spell_id, caster_level, 10)?);
        }
    }
    let buffs = world.buffs();
    let slots = buffs.slots()?;
    for buff in slots.values() {
        combined.add(evaluated(
            buff.spell_id,
            u16::from(buff.caster_level),
            u32::from(buff.bard_modifier),
        )?);
    }
    for effect in buffs.effects().values() {
        let spell_id = u32::from(effect.spell_id);
        if slots.values().any(|buff| buff.spell_id == spell_id) {
            continue;
        }
        let effect = evaluated(spell_id, effect.caster_level, effect.instrument_modifier)?;
        // A landing without a slot may replace a contributor. Additive
        // capacity cannot be established until the slot report settles.
        if effect.stamina != 0 || effect.breath_level != 0 {
            return None;
        }
        if effect.fire_resistance != 0 {
            fire_resistance = None;
        }
        combined.add(effect);
    }
    let race = world
        .spawn(player.spawn_id)
        .map_or(player.race, |spawn| spawn.state.race);
    let base = match race {
        9 => 5,
        128 => i64::from(profile.base_fire_resistance) + 5,
        _ => i64::from(profile.base_fire_resistance),
    };
    let class_bonus = match player.class {
        Some(4) => 4 + i64::from(player.level.saturating_sub(49)),
        Some(7) => 8 + i64::from(player.level.saturating_sub(49)),
        _ => 0,
    };
    Some(Inputs {
        stamina: player
            .base_attributes
            .filter(|base| base.stamina > 0)
            .map(|base| i64::from(base.stamina) + combined.stamina),
        breath_level: combined.breath_level,
        water_breathing: combined.water_breathing,
        invulnerable: combined.invulnerable,
        fire_resistance: fire_resistance
            .map(|items| base + class_bonus + items + combined.fire_resistance),
        item_hp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_resist_and_maximum_hp_require_received_inputs() {
        use crate::online::{OnlineState, testing};
        use eq_client_core::{
            BaseAttributes, WorldEvent, hazards::TitaniumEnvironment, inventory::InventoryUpdate,
        };

        let spells = SpellNames::default();
        let mut player = testing::player(7);
        player.base_attributes = Some(BaseAttributes {
            stamina: 75,
            ..BaseAttributes::default()
        });
        player.titanium_environment = Some(TitaniumEnvironment {
            zone_id: 27,
            base_fire_resistance: 25,
            has_alternate_advancement: false,
        });
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 5, player.clone());
        assert!(read(online.world(), &spells, &player, Need::Heat).is_none());
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![]));
        testing::buffs(&mut online, std::collections::BTreeMap::default());
        let inputs = read(online.world(), &spells, &player, Need::Heat).unwrap();
        assert_eq!(inputs.fire_resistance, Some(25));
        assert_eq!(inputs.stamina, Some(75));
        assert_eq!(inputs.maximum_hp(online.world()), None);
        testing::news(
            &mut online,
            [WorldEvent::HitPoints {
                spawn_id: 7,
                current: 45,
                maximum: 80,
                items: ItemHitPoints::LeftOutOfCurrent,
            }],
        );
        assert_eq!(inputs.maximum_hp(online.world()), Some(80));
        player
            .titanium_environment
            .as_mut()
            .unwrap()
            .has_alternate_advancement = true;
        assert!(read(online.world(), &spells, &player, Need::Heat).is_none());
    }

    #[test]
    fn spell_projection_distinguishes_breathing_and_capacity_effects() {
        let effect = |id, base| Effect {
            id,
            base,
            limit: 0,
            maximum: 0,
            formula: 100,
        };
        let read = project(
            &[effect(7, 5), effect(14, 1), effect(246, 150)],
            0,
            10,
            Need::Air,
        )
        .unwrap();
        assert_eq!(read.stamina, 5);
        assert_eq!(read.breath_level, 150);
        assert!(read.water_breathing);
        assert!(project(&[effect(14, -1)], 0, 10, Need::Air).is_none());
        assert!(project(&[effect(246, 150)], 60, 12, Need::Air).is_none());
        let heat = project(
            &[effect(46, 15), effect(111, 20), effect(14, -1)],
            0,
            10,
            Need::Heat,
        )
        .unwrap();
        assert_eq!(heat.fire_resistance, 35);
        assert!(!heat.water_breathing);
    }
}
