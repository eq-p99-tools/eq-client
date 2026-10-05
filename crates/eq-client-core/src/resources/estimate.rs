//! Complete capacity calculations independent of rendering and spell-file formats.
use super::{EffectiveAttributes, eqemu_equipped_modifiers, eqemu_titanium_base};

/// Complete direct modifiers used by resource estimates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceBonuses {
    /// Direct strength modifier.
    pub strength: i64,
    /// Direct stamina modifier.
    pub stamina: i64,
    /// Direct dexterity modifier.
    pub dexterity: i64,
    /// Direct agility modifier.
    pub agility: i64,
    /// Direct intelligence modifier.
    pub intelligence: i64,
    /// Direct wisdom modifier.
    pub wisdom: i64,
    /// Direct mana modifier.
    pub mana: i64,
    /// Direct endurance modifier.
    pub endurance: i64,
}

/// The host's spell/effect information, without a dependency on an asset loader.
pub trait ResourceSpells {
    /// True only when all effects are known not to alter these capacities.
    fn preserves_capacities(&self, spell: u32) -> bool;
    /// Complete modifiers at a caster level. Unknown/unsupported effects return None.
    fn resource_bonuses(&self, spell: u32, level: u16) -> Option<ResourceBonuses>;
}

/// Estimated maxima under the explicitly selected pre-SoF rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EstimatedCapacities {
    /// Maximum mana, zero for a class without mana.
    pub mana: u32,
    /// Maximum endurance.
    pub endurance: u32,
}

/// The input category that prevented a complete estimate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstimateUnavailable {
    /// Missing or unsupported class/attributes.
    Attributes,
    /// Unmodeled level or AA rules.
    Level,
    /// Unresolved buff membership or effects.
    Buffs,
    /// Unresolved equipment data or predictions.
    Equipment,
    /// Unresolved food/drink modifiers.
    Food,
    /// Unsupported worn effect, caster level or instrument.
    Effect,
    /// Missing or incomplete mechanics for this spell ID.
    Spell(u32),
    /// Checked arithmetic exceeded the representable range.
    Arithmetic,
    /// An observed resource exceeds the computed maximum.
    Contradicted,
}

impl std::fmt::Display for EstimateUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Resource estimate unavailable: {self:?}")
    }
}
impl std::error::Error for EstimateUnavailable {}

/// Combines supported direct modifiers under pre-SoF assumptions, not server confirmation.
///
/// # Errors
/// Returns the unresolved input category when a complete estimate is unavailable.
pub fn estimate_titanium(
    player: &crate::PlayerState,
    inventory: &crate::inventory::Inventory,
    world: &crate::world::ClientWorld,
    names: &dyn ResourceSpells,
) -> Result<EstimatedCapacities, EstimateUnavailable> {
    use EstimateUnavailable as Missing;
    let base = player.base_attributes.ok_or(Missing::Attributes)?;
    let class = player.class.ok_or(Missing::Attributes)?;
    // Higher-level stat caps/AA contributions remain unresolved.
    if player.level > 60 {
        return Err(Missing::Level);
    }
    let buffs = world.buffs().slots().ok_or(Missing::Buffs)?;
    // Unknown slots may replace existing buffs. Preserve an estimate only when
    // every possible participant leaves both capacities unchanged, regardless
    // of stacking, level, duration or instrument scaling.
    let uncertain_slots = !world.buffs().effects().is_empty();
    if uncertain_slots && !capacity_independent_buffs(world, names) {
        return Err(Missing::Buffs);
    }
    let equipment =
        eqemu_equipped_modifiers(inventory, class, player.race, u16::from(player.level))
            .map_err(|_| Missing::Equipment)?;
    let mut totals = [
        i64::from(base.strength),
        i64::from(base.stamina),
        i64::from(base.dexterity),
        i64::from(base.agility),
        i64::from(base.intelligence),
        i64::from(base.wisdom),
        0,
        0,
    ];
    validate_food(inventory)?;
    for (_, item) in equipment {
        let b = item.bonuses;
        add(
            &mut totals,
            [
                b.strength,
                b.stamina,
                b.dexterity,
                b.agility,
                b.intelligence,
                b.wisdom,
                b.mana,
                b.endurance,
            ]
            .map(i64::from),
        )?;
        if let Some(worn) = item.worn {
            if worn.effect_type != 2 {
                return Err(Missing::Effect);
            }
            add_spell(
                &mut totals,
                names,
                worn.spell_id,
                u16::try_from(worn.level).map_err(|_| Missing::Effect)?,
            )?;
        }
    }
    if !uncertain_slots {
        add_buffs(&mut totals, names, buffs.values())?;
    }
    let stat =
        |index: usize| u32::try_from(totals[index].clamp(1, 255)).map_err(|_| Missing::Arithmetic);
    let base = eqemu_titanium_base(
        class,
        u16::from(player.level),
        EffectiveAttributes {
            strength: stat(0)?,
            stamina: stat(1)?,
            dexterity: stat(2)?,
            agility: stat(3)?,
            intelligence: stat(4)?,
            wisdom: stat(5)?,
        },
    )
    .ok_or(Missing::Attributes)?;
    let maximum = |base: u64, bonus: i64| {
        u32::try_from(
            i64::try_from(base)
                .map_err(|_| Missing::Arithmetic)?
                .checked_add(bonus)
                .ok_or(Missing::Arithmetic)?
                .max(0),
        )
        .map_err(|_| Missing::Arithmetic)
    };
    let mana = if crate::resources::ManaAttribute::for_class(class).ok_or(Missing::Attributes)?
        == crate::resources::ManaAttribute::None
    {
        0
    } else {
        maximum(base.mana, totals[6])?
    };
    let endurance = maximum(base.endurance, totals[7])?;
    if world.vitals().mana.is_some_and(|value| value > mana)
        || world
            .vitals()
            .endurance
            .is_some_and(|value| value > endurance)
    {
        return Err(Missing::Contradicted);
    }
    Ok(EstimatedCapacities { mana, endurance })
}

/// Rejects food whose unresolved selection could affect the result.
fn validate_food(inventory: &crate::inventory::Inventory) -> Result<(), EstimateUnavailable> {
    // Stat-bearing food/drink has separate selection/consumption rules not yet evaluated.
    // Include pre-prediction contents: picking up stat food must not hide its
    // unresolved contribution merely because it has disappeared from carried slots.
    for item in inventory
        .items()
        .values()
        .chain(inventory.prediction_origins().filter_map(|(_, item)| item))
        .filter(|item| item.slot.is_carried() && matches!(item.rules.item_type, 14 | 15))
    {
        if item.details.bonuses.ok_or(EstimateUnavailable::Food)? != crate::ItemBonuses::default()
            || item
                .details
                .equipment
                .ok_or(EstimateUnavailable::Food)?
                .worn
                .is_some()
        {
            return Err(EstimateUnavailable::Food);
        }
    }
    Ok(())
}

/// Checks both incoming effects and the buffs they might replace.
fn capacity_independent_buffs(
    world: &crate::world::ClientWorld,
    names: &dyn ResourceSpells,
) -> bool {
    world
        .buffs()
        .slots()
        .iter()
        .flat_map(|buffs| buffs.values())
        .map(|buff| buff.spell_id)
        .chain(world.buffs().effects().keys().map(|id| u32::from(*id)))
        .all(|id| names.preserves_capacities(id))
}

/// Applies confirmed buff modifiers when slot membership is resolved.
fn add_buffs<'a>(
    totals: &mut [i64; 8],
    names: &dyn ResourceSpells,
    buffs: impl Iterator<Item = &'a crate::Buff>,
) -> Result<(), EstimateUnavailable> {
    for buff in buffs {
        // Instrument modifiers require effect-specific handling; ten is unmodified.
        if buff.bard_modifier != 10 {
            return Err(EstimateUnavailable::Effect);
        }
        add_spell(totals, names, buff.spell_id, u16::from(buff.caster_level))?;
    }
    Ok(())
}

fn add(totals: &mut [i64; 8], values: [i64; 8]) -> Result<(), EstimateUnavailable> {
    for (total, value) in totals.iter_mut().zip(values) {
        *total = total
            .checked_add(value)
            .ok_or(EstimateUnavailable::Arithmetic)?;
    }
    Ok(())
}

fn add_spell(
    totals: &mut [i64; 8],
    names: &dyn ResourceSpells,
    id: u32,
    level: u16,
) -> Result<(), EstimateUnavailable> {
    if level == 0 {
        return Err(EstimateUnavailable::Effect);
    }
    let b = names
        .resource_bonuses(id, level)
        .ok_or(EstimateUnavailable::Spell(id))?;
    add(
        totals,
        [
            b.strength,
            b.stamina,
            b.dexterity,
            b.agility,
            b.intelligence,
            b.wisdom,
            b.mana,
            b.endurance,
        ],
    )
}
