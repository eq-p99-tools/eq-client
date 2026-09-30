//! Explicitly approximate Titanium capacities, invalidated when inputs are incomplete.
use super::{
    ViewerSettings, hud::HudState, inventory::InventoryState, online::OnlineState,
    spellbook::SpellNames,
};
use bevy::prelude::*;
use eq_client_core::resources::{
    EffectiveAttributes, eqemu_equipped_modifiers, eqemu_titanium_base,
};

/// Recomputes from current admission data; never carries a maximum across a disconnect.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    settings: Res<ViewerSettings>,
    online: Res<OnlineState>,
    inventory: Res<InventoryState>,
    names: Res<SpellNames>,
    mut hud: ResMut<HudState>,
) {
    hud.resource_estimate = if settings.0.estimate_titanium_resources
        && online.connected
        && online.death.is_none()
        && online.session_id.is_some()
    {
        online
            .player
            .as_ref()
            .and_then(|player| estimate(player, &inventory.data, &hud, &names))
    } else {
        None
    };
}

/// Combines supported direct modifiers under pre-SoF assumptions, not server confirmation.
fn estimate(
    player: &eq_client_core::PlayerState,
    inventory: &eq_client_core::inventory::Inventory,
    hud: &HudState,
    names: &SpellNames,
) -> Option<(u32, u32)> {
    let base = player.base_attributes?;
    let class = player.class?;
    // Higher-level stat caps/AA contributions remain unresolved.
    if player.level > 60 {
        return None;
    }
    let buffs = hud.buff_state.slots()?;
    // Unknown slots may replace existing buffs. Preserve an estimate only when
    // every possible participant leaves both capacities unchanged, regardless
    // of stacking, level, duration or instrument scaling.
    let uncertain_slots = !hud.buff_state.effects().is_empty();
    if uncertain_slots && !capacity_independent_buffs(hud, names) {
        return None;
    }
    let equipment =
        eqemu_equipped_modifiers(inventory, class, player.race, u16::from(player.level)).ok()?;
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
    // Stat-bearing food/drink has separate selection/consumption rules not yet evaluated.
    // Include pre-prediction contents: picking up stat food must not hide its
    // unresolved contribution merely because it has disappeared from carried slots.
    for item in inventory
        .items()
        .values()
        .chain(inventory.prediction_origins().filter_map(|(_, item)| item))
        .filter(|item| {
            matches!(item.slot.0, 22..=29 | 251..=330) && matches!(item.rules.item_type, 14 | 15)
        })
    {
        if item.details.bonuses? != eq_client_core::ItemBonuses::default()
            || item.details.equipment?.worn.is_some()
        {
            return None;
        }
    }
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
                return None;
            }
            add_spell(
                &mut totals,
                names,
                worn.spell_id,
                u16::try_from(worn.level).ok()?,
            )?;
        }
    }
    if !uncertain_slots {
        add_buffs(&mut totals, names, buffs.values())?;
    }
    let stat = |index: usize| u32::try_from(totals[index].clamp(1, 255)).ok();
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
    )?;
    let maximum = |base: u64, bonus: i64| {
        u32::try_from(i64::try_from(base).ok()?.checked_add(bonus)?.max(0)).ok()
    };
    let mana = if eq_client_core::resources::ManaAttribute::for_class(class)?
        == eq_client_core::resources::ManaAttribute::None
    {
        0
    } else {
        maximum(base.mana, totals[6])?
    };
    let endurance = maximum(base.endurance, totals[7])?;
    if hud.mana.is_some_and(|value| value > mana)
        || hud.endurance.is_some_and(|value| value > endurance)
    {
        return None;
    }
    Some((mana, endurance))
}

/// Checks both incoming effects and the buffs they might replace.
fn capacity_independent_buffs(hud: &HudState, names: &SpellNames) -> bool {
    hud.buff_state
        .slots()
        .iter()
        .flat_map(|buffs| buffs.values())
        .map(|buff| buff.spell_id)
        .chain(hud.buff_state.effects().keys().map(|id| u32::from(*id)))
        .all(|id| {
            names.mechanics(id).is_some_and(
                eq_client_assets::spells::Mechanics::preserves_mana_and_endurance_capacity,
            )
        })
}

/// Applies confirmed buff modifiers when slot membership is resolved.
fn add_buffs<'a>(
    totals: &mut [i64; 8],
    names: &SpellNames,
    buffs: impl Iterator<Item = &'a eq_client_core::Buff>,
) -> Option<()> {
    for buff in buffs {
        // Instrument modifiers require effect-specific handling; ten is unmodified.
        if buff.bard_modifier != 10 {
            return None;
        }
        add_spell(totals, names, buff.spell_id, u16::from(buff.caster_level))?;
    }
    Some(())
}

fn add(totals: &mut [i64; 8], values: [i64; 8]) -> Option<()> {
    for (total, value) in totals.iter_mut().zip(values) {
        *total = total.checked_add(value)?;
    }
    Some(())
}

fn add_spell(totals: &mut [i64; 8], names: &SpellNames, id: u32, level: u16) -> Option<()> {
    if level == 0 {
        return None;
    }
    let projection = names.mechanics(id)?.resource_projection(level);
    if !projection.unresolved.is_empty() {
        return None;
    }
    let b = projection.modifiers;
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

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::{BaseAttributes, PlayerState, inventory::InventoryUpdate};

    fn player() -> PlayerState {
        PlayerState {
            name: "Example".into(),
            base_attributes: Some(BaseAttributes {
                strength: 75,
                stamina: 75,
                dexterity: 75,
                agility: 75,
                intelligence: 75,
                wisdom: 115,
                charisma: 75,
            }),
            deity: None,
            class: Some(2),
            spawn_id: 7,
            race: 1,
            gender: 0,
            level: 1,
            position: eq_client_core::WorldPosition::default(),
            mana: 25,
            endurance: Some(20),
            skills: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
        }
    }

    #[test]
    fn harmless_stack_moves_keep_estimates_but_stat_food_and_equipment_do_not() {
        use eq_client_core::ItemBonuses;
        use eq_client_core::inventory::{Inventory, InventorySlot};
        let hud = HudState {
            buff_state: eq_client_core::buffs::BuffTracker::empty_snapshot(),
            ..default()
        };
        let names = SpellNames::default();
        let mut milk = super::super::inventory::demo_items().remove(0);
        milk.slot = InventorySlot(22);
        milk.rules.item_type = 15;
        milk.details.bonuses = Some(ItemBonuses::default());
        milk.details.equipment = Some(eq_client_core::EquipmentRules::default());
        milk.stack_count = Some(5);
        for mana_bonus in [0, 10] {
            milk.details.bonuses.as_mut().unwrap().mana = mana_bonus;
            let mut inventory = Inventory::default();
            inventory.apply(InventoryUpdate::Snapshot(vec![milk.clone()]));
            let mut held = milk.clone();
            held.slot = InventorySlot(30);
            inventory.apply(InventoryUpdate::Prediction(vec![held]));
            assert_eq!(
                estimate(&player(), &inventory, &hud, &names),
                (mana_bonus == 0).then_some((25, 20))
            );
            // Moving the item onward cannot erase the unresolved original food.
            inventory.apply(InventoryUpdate::Prediction(vec![]));
            assert_eq!(
                estimate(&player(), &inventory, &hud, &names),
                (mana_bonus == 0).then_some((25, 20))
            );
        }
        let mut equipment = milk;
        equipment.slot = InventorySlot(0);
        let mut inventory = Inventory::default();
        inventory.apply(InventoryUpdate::Snapshot(vec![equipment]));
        inventory.apply(InventoryUpdate::Prediction(vec![]));
        assert_eq!(estimate(&player(), &inventory, &hud, &names), None);
    }

    #[test]
    fn estimates_follow_buffs_and_invalidate_missing_or_contradictory_data() {
        let mut inventory = eq_client_core::inventory::Inventory::default();
        inventory.apply(InventoryUpdate::Snapshot(vec![]));
        let mut hud = HudState {
            buff_state: eq_client_core::buffs::BuffTracker::empty_snapshot(),
            mana: Some(25),
            endurance: Some(20),
            ..default()
        };
        let mut fields = vec!["0"; 183];
        fields[0] = "42";
        fields[1] = "Synthetic wisdom";
        fields[20] = "10";
        fields[70] = "100";
        fields[86] = "9";
        let names = SpellNames::parse(&fields.join("^"));
        assert_eq!(
            estimate(&player(), &inventory, &hud, &names),
            Some((25, 20))
        );
        hud.buff_state.apply(eq_client_core::BuffUpdate {
            entity_id: 7,
            slot: 0,
            spell_id: 42,
            buff: Some(eq_client_core::Buff {
                spell_id: 42,
                caster_level: 1,
                effect_type: 2,
                bard_modifier: 10,
                duration_ticks: 10,
                counters: 0,
                caster_id: 7,
            }),
        });
        assert_eq!(
            estimate(&player(), &inventory, &hud, &names),
            Some((27, 20))
        );
        assert_eq!(
            estimate(&player(), &inventory, &hud, &SpellNames::default()),
            None
        );
        let confirmed = hud.buff_state.slots().unwrap()[&0].clone();
        hud.spell_effect(
            eq_client_core::SpellEffect {
                target_id: 7,
                caster_id: 7,
                caster_level: 1,
                instrument_modifier: 10,
                spell_id: 42,
                spell_level: 1,
                effect_flag: 4,
            },
            None,
        );
        assert_eq!(estimate(&player(), &inventory, &hud, &names), None);
        hud.buff_update(eq_client_core::BuffUpdate {
            entity_id: 7,
            slot: 0,
            spell_id: 42,
            buff: Some(confirmed),
        });
        assert_eq!(
            estimate(&player(), &inventory, &hud, &names),
            Some((27, 20))
        );
        hud.buff_state
            .replace_snapshot(std::collections::BTreeMap::default());
        assert_eq!(
            estimate(&player(), &inventory, &hud, &names),
            Some((25, 20))
        );
        hud.mana = Some(26);
        assert_eq!(estimate(&player(), &inventory, &hud, &names), None);
        hud.mana = Some(25);
        hud.buff_state.clear();
        assert_eq!(estimate(&player(), &inventory, &hud, &names), None);
        hud.buff_state
            .replace_snapshot(std::collections::BTreeMap::default());
        inventory.apply(InventoryUpdate::Invalidated);
        assert_eq!(estimate(&player(), &inventory, &hud, &names), None);
    }

    #[test]
    fn harmless_effects_preserve_estimates_independently_of_duration() {
        let mut inventory = eq_client_core::inventory::Inventory::default();
        inventory.apply(InventoryUpdate::Snapshot(vec![]));
        for (primary, alternate, expected_instant) in
            [(0, 0, true), (1, 0, false), (0, 7, false), (99, 0, false)]
        {
            let mut fields = vec!["0".to_owned(); 183];
            fields[0] = "42".into();
            fields[1] = "Synthetic effect".into();
            fields[16] = primary.to_string();
            fields[181] = alternate.to_string();
            let names = SpellNames::parse(&fields.join("^"));
            let mut hud = HudState {
                buff_state: eq_client_core::buffs::BuffTracker::empty_snapshot(),
                ..default()
            };
            hud.spell_effect(
                eq_client_core::SpellEffect {
                    target_id: 7,
                    caster_id: 7,
                    caster_level: 1,
                    instrument_modifier: 10,
                    spell_id: 42,
                    spell_level: 1,
                    effect_flag: 4,
                },
                Some(&names),
            );
            assert_eq!(hud.buff_state.effects().is_empty(), expected_instant);
            assert_eq!(
                estimate(&player(), &inventory, &hud, &names),
                Some((25, 20))
            );
            if !expected_instant {
                // An unknown or resource-bearing existing buff could have been
                // replaced. Do not retain its old bonus or silently discard it.
                hud.buff_state.apply(eq_client_core::BuffUpdate {
                    entity_id: 7,
                    slot: 0,
                    spell_id: 43,
                    buff: Some(eq_client_core::Buff {
                        spell_id: 43,
                        caster_level: 1,
                        effect_type: 2,
                        bard_modifier: 10,
                        duration_ticks: 10,
                        counters: 0,
                        caster_id: 7,
                    }),
                });
                assert_eq!(estimate(&player(), &inventory, &hud, &names), None);
                for effect_id in ["9", "97", "190", "9999"] {
                    let mut other = fields.clone();
                    other[0] = "43".into();
                    other[20] = "10".into();
                    other[70] = "100".into();
                    other[86] = effect_id.into();
                    let combined =
                        SpellNames::parse(&format!("{}\n{}", fields.join("^"), other.join("^")));
                    assert_eq!(estimate(&player(), &inventory, &hud, &combined), None);
                }
            }
        }
    }

    #[test]
    fn scheduled_estimate_clears_on_disconnect_and_when_rules_are_disabled() {
        let mut app = App::new();
        let mut online = OnlineState::new(true);
        online.connected = true;
        online.session_id = Some(1);
        online.player = Some(player());
        let mut inventory = InventoryState::default();
        inventory.apply(InventoryUpdate::Snapshot(vec![]));
        app.insert_resource(ViewerSettings(super::super::ViewerConfig {
            estimate_titanium_resources: true,
            ..default()
        }))
        .insert_resource(online)
        .insert_resource(inventory)
        .insert_resource(HudState {
            buff_state: eq_client_core::buffs::BuffTracker::empty_snapshot(),
            ..default()
        })
        .init_resource::<SpellNames>()
        .add_systems(Update, update);
        app.update();
        assert_eq!(
            app.world().resource::<HudState>().resource_estimate,
            Some((25, 20))
        );
        app.world_mut().resource_mut::<OnlineState>().connected = false;
        app.update();
        assert_eq!(app.world().resource::<HudState>().resource_estimate, None);
        app.world_mut().resource_mut::<OnlineState>().connected = true;
        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .0
            .estimate_titanium_resources = false;
        app.update();
        assert_eq!(app.world().resource::<HudState>().resource_estimate, None);
    }
}
