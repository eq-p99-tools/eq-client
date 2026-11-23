//! Read breath modifiers from the current inventory and active effects.
//!
//! Titanium `eqgame.exe` uses the profile and racial fire resistance at
//! 0x418756, and adds the ordinary Ranger and Monk bonuses and effects 46
//! and 111 at 0x4187e4. Effect 40 is checked at 0x41c905. Restrictions on
//! instrument, level and formula inputs are conservative limits of this
//! projection (recording checklist 6.393).
use super::Unavailable;
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
    water_breathing: i64,
    invulnerable: bool,
}

impl Effects {
    fn add(&mut self, other: Self) {
        self.stamina += other.stamina;
        self.breath_level += other.breath_level;
        self.fire_resistance += other.fire_resistance;
        self.water_breathing += other.water_breathing;
        self.invulnerable |= other.invulnerable;
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Need {
    /// Effect 14's signed aggregate, independently of reserve capacity.
    Breathing,
    Air,
    Heat,
}

fn project(effects: &[Effect], level: u16, instrument: u32, need: Need) -> Option<Effects> {
    let mut result = Effects::default();
    for effect in effects {
        if effect.id == 40 {
            // Divine Aura can suppress the native environmental send.
            result.invulnerable = need != Need::Breathing;
            continue;
        }
        let affects = match need {
            Need::Breathing => effect.id == WATER_BREATHING,
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
            WATER_BREATHING if value >= 0 => result.water_breathing += value,
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

/// Projects the supported selected AA effects without guessing capacity inputs.
fn aa_effects(
    world: &ClientWorld,
    class: u32,
    level: u8,
    need: Need,
) -> Result<Effects, Unavailable> {
    let effects = world
        .alternate_advancement()
        .effects(class, level)
        .ok_or(Unavailable::AlternateAdvancement)?;
    let mut combined = Effects::default();
    for effect in effects {
        match (need, effect.id) {
            (_, WATER_BREATHING) => combined.water_breathing += i64::from(effect.value),
            (Need::Air, SET_BREATH_LEVEL) => combined.breath_level += i64::from(effect.value),
            // The full effective-STA/cap and invulnerability paths need a
            // separate port. These never obstruct a resolved breathing
            // predicate, but cannot silently disappear from capacity/damage.
            (Need::Air, 7 | 159 | 262 | 40) if effect.value != 0 => {
                return Err(Unavailable::AlternateAdvancement);
            }
            _ => (),
        }
    }
    Ok(combined)
}

/// Evaluates a known spell using the supplied level and instrument provenance.
fn spell(
    spells: &SpellNames,
    id: u32,
    level: u16,
    instrument: u32,
    need: Need,
) -> Result<Effects, Unavailable> {
    let mechanics = spells.mechanics(id).ok_or(Unavailable::Effects)?;
    project(&mechanics.effects, level, instrument, need).ok_or(Unavailable::Effects)
}

/// Applies the native buff record type to the projected signed contributions.
fn read_buff(
    spells: &SpellNames,
    buff: &eq_client_core::Buff,
    need: Need,
    local_air: bool,
) -> Result<Effects, Unavailable> {
    let mut effect = spell(
        spells,
        buff.spell_id,
        u16::from(buff.caster_level),
        u32::from(buff.bard_modifier),
        need,
    )?;
    if local_air {
        if buff.effect_type == 0 {
            return Err(Unavailable::Effects);
        }
        if buff.effect_type == 4 {
            // Native 0x41002f reverses ordinary numeric contributions
            // from a type-4 record before adding them to the cache.
            effect.water_breathing = -effect.water_breathing;
            effect.stamina = -effect.stamina;
            effect.breath_level = -effect.breath_level;
            if effect.invulnerable {
                return Err(Unavailable::Effects);
            }
        }
    }
    Ok(effect)
}

pub(super) fn read(
    world: &ClientWorld,
    spells: &SpellNames,
    player: &PlayerState,
    need: Need,
) -> Result<Inputs, Unavailable> {
    let profile = player
        .titanium_environment
        .ok_or(Unavailable::MissingProfile)?;
    let local_air = need != Need::Heat && world.can(eq_client_core::Capability::Drowning);
    if !local_air && profile.has_alternate_advancement {
        return Err(Unavailable::AlternateAdvancement);
    }
    let inventory = world.worker_inventory();
    let level = u16::from(player.level);
    let class = player.class.ok_or(Unavailable::Equipment)?;
    let equipment = eqemu_equipped_modifiers(inventory, class, player.race, level)
        .map_err(|_| Unavailable::Equipment)?;
    let item_hp = eqemu_item_hit_points(&equipment);
    let (stamina, mut fire_resistance) =
        crate::speed::food_environment(inventory, level).ok_or(Unavailable::Food)?;
    let mut combined = Effects {
        stamina,
        ..Effects::default()
    };
    if local_air {
        combined.add(aa_effects(world, class, player.level, need)?);
    }
    for (_, item) in equipment {
        combined.stamina += i64::from(item.bonuses.stamina);
        fire_resistance = fire_resistance
            .zip(item.bonuses.fire_resistance)
            .map(|(total, item)| total + i64::from(item));
        if let Some(worn) = item.worn {
            if worn.effect_type != 2 {
                return Err(Unavailable::Effects);
            }
            let caster_level = if worn.level == 0 {
                level
            } else {
                u16::try_from(worn.level).map_err(|_| Unavailable::Effects)?
            };
            combined.add(spell(spells, worn.spell_id, caster_level, 10, need)?);
        }
    }
    let buffs = world.buffs();
    let slots = buffs.slots().ok_or(Unavailable::BuffSlots)?;
    for buff in slots.values() {
        combined.add(read_buff(spells, buff, need, local_air)?);
    }
    for effect in buffs.effects().values() {
        let spell_id = u32::from(effect.spell_id);
        if slots.values().any(|buff| buff.spell_id == spell_id) {
            continue;
        }
        if local_air {
            // A known native slot model preserves the old record on a refused
            // refresh and supplies the native constant instrument modifier.
            // An unresolved replacement cannot be added to the old total.
            let record = buffs
                .modelled_effect_record(effect.spell_id)
                .ok_or(Unavailable::UnslottedModifier)?;
            let projected = read_buff(spells, record, need, local_air)?;
            if projected.stamina != 0 || projected.breath_level != 0 {
                return Err(Unavailable::UnslottedModifier);
            }
            combined.add(projected);
            continue;
        }
        let effect = spell(
            spells,
            spell_id,
            effect.caster_level,
            effect.instrument_modifier,
            need,
        )?;
        // A landing without a slot may replace a contributor. Additive
        // capacity cannot be established until the slot report settles.
        if effect.stamina != 0 || effect.breath_level != 0 {
            return Err(Unavailable::UnslottedModifier);
        }
        if effect.fire_resistance != 0 {
            fire_resistance = None;
        }
        combined.add(effect);
    }
    let race = world
        .spawn(player.spawn_id)
        .map_or(player.race, |spawn| spawn.state.race);
    // The ordinary Titanium branch uses the received profile field and
    // these racial adjustments. The server's own resist calculation differs.
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
    Ok(Inputs {
        stamina: player
            .base_attributes
            .filter(|base| base.stamina > 0)
            .map(|base| i64::from(base.stamina) + combined.stamina),
        breath_level: combined.breath_level,
        water_breathing: combined.water_breathing > 0,
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
        testing::news(
            &mut online,
            [WorldEvent::AlternateAdvancement {
                session_id: 5,
                updates: vec![eq_client_core::alternate_advancement::Update::Selections(
                    Some(Vec::new()),
                )],
            }],
        );
        assert!(matches!(
            read(online.world(), &spells, &player, Need::Heat),
            Err(Unavailable::Equipment)
        ));
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![]));
        assert!(matches!(
            read(online.world(), &spells, &player, Need::Air),
            Err(Unavailable::BuffSlots)
        ));
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
        testing::news(
            &mut online,
            [WorldEvent::AlternateAdvancement {
                session_id: 5,
                updates: vec![eq_client_core::alternate_advancement::Update::Selections(
                    None,
                )],
            }],
        );
        assert!(matches!(
            read(online.world(), &spells, &player, Need::Air),
            Err(Unavailable::AlternateAdvancement)
        ));
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
        assert_eq!(read.water_breathing, 1);
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
        assert_eq!(heat.water_breathing, 0);
    }

    #[test]
    fn worn_stamina_requires_mechanics_and_an_unslotted_change_stays_unresolved() {
        use crate::online::{OnlineState, testing};
        use eq_client_core::{
            BaseAttributes, EquipmentRules, ItemBonuses, SpellEffect, WorldEvent,
            hazards::TitaniumEnvironment,
            inventory::{InventorySlot, InventoryUpdate},
        };
        use eq_network_game::items::WornEffect;

        let mut player = testing::player(7);
        player.base_attributes = Some(BaseAttributes {
            stamina: 40,
            ..BaseAttributes::default()
        });
        player.titanium_environment = Some(TitaniumEnvironment {
            zone_id: 27,
            base_fire_resistance: 0,
            has_alternate_advancement: false,
        });
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 5, player.clone());
        testing::news(
            &mut online,
            [WorldEvent::AlternateAdvancement {
                session_id: 5,
                updates: vec![eq_client_core::alternate_advancement::Update::Selections(
                    Some(Vec::new()),
                )],
            }],
        );
        testing::buffs(&mut online, std::collections::BTreeMap::new());
        let mut item = crate::preview::items().remove(0);
        item.slot = InventorySlot(13);
        item.bag_slots = 0;
        item.details.slots = u32::MAX;
        item.details.races = u32::MAX;
        item.details.classes = u32::MAX;
        item.details.bonuses = Some(ItemBonuses::default());
        item.details.equipment = Some(EquipmentRules {
            worn: Some(WornEffect {
                spell_id: 42,
                effect_type: 2,
                level: 0,
                level2: 0,
            }),
            ..EquipmentRules::default()
        });
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item]));
        assert!(matches!(
            read(online.world(), &SpellNames::default(), &player, Need::Air),
            Err(Unavailable::Effects)
        ));
        let mut fields = vec!["0"; 183];
        fields[0] = "42";
        fields[1] = "Synthetic stamina effect";
        fields[20] = "5";
        fields[70] = "100";
        fields[86] = "7";
        let spells = SpellNames::parse(&fields.join("^"));
        let read_air = |online: &OnlineState| read(online.world(), &spells, &player, Need::Air);
        assert_eq!(read_air(&online).unwrap().stamina, Some(45));

        testing::news(
            &mut online,
            [WorldEvent::SpellEffect(SpellEffect {
                spell_id: 42,
                caster_level: 1,
                effect_flag: 4,
                instrument_modifier: 10,
                target_id: 7,
                caster_id: 7,
                spell_level: 1,
                push: None,
            })],
        );
        assert!(matches!(
            read_air(&online),
            Err(Unavailable::UnslottedModifier)
        ));
    }
}
