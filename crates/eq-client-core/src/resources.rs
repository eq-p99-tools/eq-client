//! Resource capacity rules, separate from wire values and renderer state.
//!
//! Reference: `EQEmu` `zone/client_mods.cpp`, pre-SoF branches of
//! `CalcBaseMana` and `CalcBaseEndurance`. These rules are not yet verified
//! as P99/Quarm rules. Callers must supply effective, capped stats rather than
//! unmodified profile attributes. Equipment, buffs and AA bonuses are not inferred.

/// Governing attribute for classes that have a mana pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManaAttribute {
    /// Cleric, paladin, ranger, druid, shaman or beastlord.
    Wisdom,
    /// Shadow knight, bard, necromancer, wizard, magician or enchanter.
    Intelligence,
    /// Warrior, monk, rogue or berserker.
    None,
}

impl ManaAttribute {
    /// Resolves playable class IDs; unknown IDs stay unavailable rather than becoming warriors.
    pub const fn for_class(class: u32) -> Option<Self> {
        match class {
            2 | 3 | 4 | 6 | 10 | 15 => Some(Self::Wisdom),
            5 | 8 | 11..=14 => Some(Self::Intelligence),
            1 | 7 | 9 | 16 => Some(Self::None),
            _ => None,
        }
    }
}

/// Final stat values after the chosen server's bonuses and caps have been applied.
/// Deliberately distinct from `BaseAttributes` to prevent accidental profile-only maxima.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectiveAttributes {
    /// Effective strength.
    pub strength: u32,
    /// Effective stamina attribute, not the remaining endurance amount.
    pub stamina: u32,
    /// Effective dexterity.
    pub dexterity: u32,
    /// Effective agility.
    pub agility: u32,
    /// Effective intelligence.
    pub intelligence: u32,
    /// Effective wisdom.
    pub wisdom: u32,
}

/// A calculated capacity under explicitly selected rules, not a network observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaseCapacities {
    /// Base mana, before direct resource bonuses.
    pub mana: u64,
    /// Base endurance, before direct resource bonuses.
    pub endurance: u64,
}

/// Level-adjusted item modifiers and the passive spell still requiring evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScaledEquipment {
    /// Direct attribute and capacity modifiers only, excluding the worn spell.
    pub bonuses: eq_network_game::items::ItemBonuses,
    /// Retained separately so callers cannot mistake direct modifiers for complete bonuses.
    pub worn: Option<eq_network_game::items::WornEffect>,
}

/// Why equipment contributions cannot currently be calculated from authoritative data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EquipmentUnavailable {
    /// No complete server inventory has arrived.
    MissingSnapshot,
    /// Inventory needs correction or equipped slots include unconfirmed local moves.
    UnconfirmedInventory,
    /// Class, base race or level cannot be interpreted by this rule set.
    UnknownCharacter,
    /// One equipped item has missing or contradictory metadata.
    Item(crate::inventory::InventorySlot),
}

/// Selects direct stat-bearing equipment using Titanium slot and eligibility masks.
/// Ammo, cursor, carried and bank items are excluded. Food/drink contributions are separate.
/// Predictions in storage slots do not invalidate unchanged equipment.
/// Returned worn effects still require evaluation; this is not a complete capacity result.
///
/// # Errors
/// Returns a typed reason when authoritative inventory or required metadata is unavailable.
pub fn eqemu_equipped_modifiers(
    inventory: &crate::inventory::Inventory,
    class: u32,
    base_race: u32,
    level: u16,
) -> Result<Vec<(crate::inventory::InventorySlot, ScaledEquipment)>, EquipmentUnavailable> {
    if !inventory.received() {
        return Err(EquipmentUnavailable::MissingSnapshot);
    }
    if inventory.stale()
        || inventory
            .prediction_origins()
            .any(|(slot, _)| (0..=20).contains(&slot.0))
    {
        return Err(EquipmentUnavailable::UnconfirmedInventory);
    }
    if !(1..=16).contains(&class) || level == 0 {
        return Err(EquipmentUnavailable::UnknownCharacter);
    }
    let race_bit = match base_race {
        1..=12 => base_race - 1,
        128 => 12,
        _ => return Err(EquipmentUnavailable::UnknownCharacter),
    };
    let mut result = Vec::new();
    for (&slot, item) in inventory
        .items()
        .range(crate::inventory::InventorySlot(0)..=crate::inventory::InventorySlot(20))
    {
        let bit = u32::try_from(slot.0).map_err(|_| EquipmentUnavailable::Item(slot))?;
        if item.slot != slot || item.bag_slots != 0 || item.details.slots & (1 << bit) == 0 {
            return Err(EquipmentUnavailable::Item(slot));
        }
        if item.details.classes & (1 << (class - 1)) == 0
            || item.details.races & (1 << race_bit) == 0
        {
            continue;
        }
        result.push((
            slot,
            eqemu_equipment_by_level(&item.details, level)
                .ok_or(EquipmentUnavailable::Item(slot))?,
        ));
    }
    Ok(result)
}

/// Hit points equipped items add, as `EQEmu` totals them in `itembonuses.HP`
/// (zone/bonuses.cpp `Mob::AddItemBonuses`): each eligible item's own HP, scaled
/// below its recommended level. Worn +HP effects count toward a different bonus
/// (`FlatMaxHPChange`), and augments are not modelled.
pub fn eqemu_item_hit_points(
    equipment: &[(crate::inventory::InventorySlot, ScaledEquipment)],
) -> i64 {
    equipment
        .iter()
        .map(|(_, item)| i64::from(item.bonuses.hit_points))
        .sum()
}

/// Applies the `EQEmu` required/recommended-level rules to one eligible equipped item.
/// Callers must first check slot, class, race and item category; carried items do not qualify.
/// Missing metadata or level zero returns None. Below-required items yield zero modifiers.
pub fn eqemu_equipment_by_level(
    item: &eq_network_game::items::ItemDetails,
    level: u16,
) -> Option<ScaledEquipment> {
    use eq_network_game::items::ItemBonuses;
    if level == 0 {
        return None;
    }
    let rules = item.equipment?;
    if u32::from(level) < rules.required_level {
        return Some(ScaledEquipment {
            bonuses: ItemBonuses::default(),
            worn: None,
        });
    }
    let raw = item.bonuses?;
    let scale = |value| recommended_bonus(value, level, rules.recommended_level);
    Some(ScaledEquipment {
        bonuses: ItemBonuses {
            strength: scale(raw.strength),
            stamina: scale(raw.stamina),
            agility: scale(raw.agility),
            dexterity: scale(raw.dexterity),
            charisma: scale(raw.charisma),
            intelligence: scale(raw.intelligence),
            wisdom: scale(raw.wisdom),
            hit_points: scale(raw.hit_points),
            mana: scale(raw.mana),
            endurance: scale(raw.endurance),
        },
        worn: rules.worn,
    })
}

/// Scales with a truncated 1/10000 multiplier, then rounds ties away from zero.
fn recommended_bonus(value: i32, level: u16, recommended: u32) -> i32 {
    if recommended == 0 || u32::from(level) >= recommended {
        return value;
    }
    let scaled = (i64::from(level) * 10000 / i64::from(recommended)) * i64::from(value);
    let rounded = (scaled + if scaled < 0 { -5000 } else { 5000 }) / 10000;
    // The multiplier is strictly below one, so rounded remains within the original i32 range.
    i32::try_from(rounded).expect("a reduced signed item bonus fits i32")
}

/// Evaluates pre-SoF `EQEmu` rules with integer truncation at the documented steps.
/// Zero level and unknown classes return None; no current resource value is consulted.
pub fn eqemu_titanium_base(
    class: u32,
    level: u16,
    stats: EffectiveAttributes,
) -> Option<BaseCapacities> {
    if level == 0 {
        return None;
    }
    let attribute = ManaAttribute::for_class(class)?;
    let level = u64::from(level);
    let mana = match attribute {
        ManaAttribute::None => 0,
        ManaAttribute::Intelligence | ManaAttribute::Wisdom => {
            let raw = u64::from(match attribute {
                ManaAttribute::Intelligence => stats.intelligence,
                _ => stats.wisdom,
            });
            let adjusted = raw - raw.saturating_sub(199) / 2;
            if raw > 100 {
                (5 * (adjusted + 20) / 2) * 3 * level / 40
            } else {
                (5 * (adjusted + 200) / 2) * 3 * level / 100
            }
        }
    };
    let total = [
        stats.strength,
        stats.stamina,
        stats.dexterity,
        stats.agility,
    ]
    .into_iter()
    .map(u64::from)
    .sum::<u64>();
    let below_800 = total.min(800);
    let middle = below_800.saturating_sub(400);
    let high = total.saturating_sub(800);
    let bonus = below_800 / 4 + middle / 4 + middle / 8 + (high / 8) * 2 + high / 16;
    Some(BaseCapacities {
        mana,
        endurance: 15 * level + bonus * 3 * level / 40,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_excludes_storage_and_rejects_unknown_or_predicted_equipment() {
        use crate::inventory::{Inventory, InventoryItem, InventorySlot, InventoryUpdate};
        use eq_network_game::items::{EquipmentRules, ItemBonuses, ItemDetails};
        let item = |slot| InventoryItem {
            activation: eq_network_game::inventory::ItemActivation::default(),
            scroll_spell: None,
            rules: eq_network_game::inventory::ItemPlacement::default(),
            slot: InventorySlot(slot),
            icon: 0,
            stack_count: None,
            charges: 0,
            bag_slots: 0,
            details: ItemDetails {
                equipment: Some(EquipmentRules::default()),
                bonuses: Some(ItemBonuses {
                    mana: 15,
                    ..Default::default()
                }),
                id: 42,
                name: "Synthetic equipment".into(),
                lore: String::new(),
                weight_tenths: 0,
                slots: u32::MAX,
                classes: 2,
                races: 1 << 12,
                flags: Vec::new(),
                stats: Vec::new(),
            },
        };
        let mut inventory = Inventory::default();
        assert_eq!(
            eqemu_equipped_modifiers(&inventory, 2, 128, 1),
            Err(EquipmentUnavailable::MissingSnapshot)
        );
        inventory.apply(InventoryUpdate::Snapshot(vec![]));
        assert!(
            eqemu_equipped_modifiers(&inventory, 2, 128, 1)
                .unwrap()
                .is_empty()
        );
        inventory.apply(InventoryUpdate::Snapshot(
            [0, 20, 21, 22, 30, 251, 2000].map(item).to_vec(),
        ));
        let selected = eqemu_equipped_modifiers(&inventory, 2, 128, 1).unwrap();
        assert_eq!(
            selected.iter().map(|(slot, _)| slot.0).collect::<Vec<_>>(),
            [0, 20]
        );
        assert!(
            selected
                .iter()
                .all(|(_, contribution)| contribution.bonuses.mana == 15)
        );
        assert!(
            eqemu_equipped_modifiers(&inventory, 1, 128, 1)
                .unwrap()
                .is_empty()
        );
        assert!(
            eqemu_equipped_modifiers(&inventory, 2, 1, 1)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            eqemu_equipped_modifiers(&inventory, 2, 999, 1),
            Err(EquipmentUnavailable::UnknownCharacter)
        );
        let mut incomplete = item(0);
        incomplete.details.bonuses = None;
        inventory.apply(InventoryUpdate::Set(vec![incomplete]));
        assert_eq!(
            eqemu_equipped_modifiers(&inventory, 2, 128, 1),
            Err(EquipmentUnavailable::Item(InventorySlot(0)))
        );
        inventory.apply(InventoryUpdate::Snapshot(vec![item(0)]));
        inventory.apply(InventoryUpdate::Prediction(vec![item(20)]));
        assert_eq!(
            eqemu_equipped_modifiers(&inventory, 2, 128, 1),
            Err(EquipmentUnavailable::UnconfirmedInventory)
        );
        inventory.apply(InventoryUpdate::Snapshot(vec![]));
        inventory.apply(InventoryUpdate::Invalidated);
        assert_eq!(
            eqemu_equipped_modifiers(&inventory, 2, 128, 1),
            Err(EquipmentUnavailable::UnconfirmedInventory)
        );
    }

    #[test]
    fn recommended_scaling_keeps_negative_penalties_and_reference_rounding() {
        for (raw, level, recommended, expected) in [
            (3, 1, 2, 2),
            (-3, 1, 2, -2),
            (10000, 1, 3, 3333),
            (-10000, 1, 3, -3333),
            (9, 1, 0, 9),
            (9, 20, 20, 9),
            (9, 21, 20, 9),
            (i32::MIN, 1, 2, -1_073_741_824),
            (i32::MAX, 1, 2, 1_073_741_824),
            (i32::MAX, 1, u32::MAX, 0),
        ] {
            assert_eq!(recommended_bonus(raw, level, recommended), expected);
        }
    }

    #[test]
    fn equipment_level_gate_keeps_unresolved_passive_effects_explicit() {
        use eq_network_game::items::{EquipmentRules, ItemBonuses, ItemDetails, WornEffect};
        let worn = WornEffect {
            spell_id: 42,
            effect_type: 2,
            level: 30,
            level2: 20,
        };
        let mut item = ItemDetails {
            equipment: Some(EquipmentRules {
                required_level: 10,
                recommended_level: 20,
                worn: Some(worn),
            }),
            bonuses: Some(ItemBonuses {
                mana: 15,
                wisdom: -3,
                ..Default::default()
            }),
            id: 1,
            name: "Synthetic equipment".into(),
            lore: String::new(),
            weight_tenths: 0,
            slots: 1,
            classes: 1,
            races: 1,
            flags: Vec::new(),
            stats: Vec::new(),
        };
        assert!(eqemu_equipment_by_level(&item, 0).is_none());
        assert_eq!(
            eqemu_equipment_by_level(&item, 9),
            Some(ScaledEquipment {
                bonuses: ItemBonuses::default(),
                worn: None,
            })
        );
        let scaled = eqemu_equipment_by_level(&item, 10).unwrap();
        assert_eq!(scaled.bonuses.mana, 8);
        assert_eq!(scaled.bonuses.wisdom, -2);
        assert_eq!(scaled.worn, Some(worn));
        assert_eq!(
            eqemu_equipment_by_level(&item, 20).unwrap().bonuses,
            item.bonuses.unwrap()
        );
        item.bonuses = None;
        assert!(eqemu_equipment_by_level(&item, 20).is_none());
        item.equipment = None;
        assert!(eqemu_equipment_by_level(&item, 9).is_none());
    }

    fn attributes(value: u32) -> EffectiveAttributes {
        EffectiveAttributes {
            strength: value,
            stamina: value,
            dexterity: value,
            agility: value,
            intelligence: value,
            wisdom: value,
        }
    }

    #[test]
    fn mana_uses_class_attribute_and_preserves_rounding_thresholds() {
        let mut stats = attributes(75);
        stats.wisdom = 115;
        stats.intelligence = 100;
        assert_eq!(eqemu_titanium_base(2, 1, stats).unwrap().mana, 25);
        assert_eq!(eqemu_titanium_base(12, 1, stats).unwrap().mana, 22);
        // Exercise both sides of the 100-stat branch and post-199 diminishing returns.
        for (wisdom, expected) in [
            (100, 1350),
            (101, 1359),
            (199, 2461),
            (200, 2475),
            (201, 2475),
            (255, 2776),
        ] {
            stats.wisdom = wisdom;
            assert_eq!(eqemu_titanium_base(2, 60, stats).unwrap().mana, expected);
        }
        assert_eq!(eqemu_titanium_base(1, 60, stats).unwrap().mana, 0);
        assert_eq!(eqemu_titanium_base(8, 60, stats).unwrap().mana, 1350);
        assert_eq!(ManaAttribute::for_class(15), Some(ManaAttribute::Wisdom));
        assert!(eqemu_titanium_base(0, 1, stats).is_none());
        assert!(eqemu_titanium_base(17, 1, stats).is_none());
        assert!(eqemu_titanium_base(2, 0, stats).is_none());
    }

    #[test]
    fn endurance_preserves_piecewise_bonuses_and_truncation() {
        for (stat, expected) in [
            (75, 1237),
            (100, 1350),
            (101, 1359),
            (200, 2475),
            (201, 2475),
            (204, 2497),
        ] {
            assert_eq!(
                eqemu_titanium_base(1, 60, attributes(stat))
                    .unwrap()
                    .endurance,
                expected
            );
        }
        assert_eq!(
            eqemu_titanium_base(2, 1, attributes(75)).unwrap().endurance,
            20
        );
    }

    #[test]
    fn wide_intermediates_do_not_overflow_on_untrusted_extremes() {
        let result = eqemu_titanium_base(2, u16::MAX, attributes(u32::MAX)).unwrap();
        assert!(result.mana > u64::from(u32::MAX));
        assert!(result.endurance > u64::from(u32::MAX));
    }
}
