//! Explicitly approximate Titanium capacities, invalidated when inputs are incomplete.
use super::{ViewerSettings, hud::HudState, online::OnlineState, spellbook::SpellNames};
use bevy::prelude::*;

/// Recomputes from current admission data; never carries a maximum across a disconnect.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    settings: Res<ViewerSettings>,
    online: Res<OnlineState>,
    names: Res<SpellNames>,
    mut hud: ResMut<HudState>,
) {
    hud.resource_estimate = if settings.0.estimate_titanium_resources
        && online.world().connected()
        && online.world().death().is_none()
        && online.world().session_id().is_some()
    {
        online
            .world()
            .player()
            .and_then(|player| estimate(player, online.world().inventory(), online.world(), &names))
    } else {
        None
    };
}

/// Supplies complete spell modifiers to the engine-independent estimator.
impl eq_client_core::resources::ResourceSpells for SpellNames {
    fn preserves_capacities(&self, spell: u32) -> bool {
        self.mechanics(spell)
            .is_some_and(eq_client_assets::spells::Mechanics::preserves_mana_and_endurance_capacity)
    }
    fn resource_bonuses(
        &self,
        spell: u32,
        level: u16,
    ) -> Option<eq_client_core::resources::ResourceBonuses> {
        let projection = self.mechanics(spell)?.resource_projection(level);
        if !projection.unresolved.is_empty() {
            return None;
        }
        let b = projection.modifiers;
        Some(eq_client_core::resources::ResourceBonuses {
            strength: b.strength,
            stamina: b.stamina,
            dexterity: b.dexterity,
            agility: b.agility,
            intelligence: b.intelligence,
            wisdom: b.wisdom,
            mana: b.mana,
            endurance: b.endurance,
        })
    }
}

/// Adapts a complete estimate to the optional pair the HUD displays.
fn estimate(
    player: &eq_client_core::PlayerState,
    inventory: &eq_client_core::inventory::Inventory,
    world: &eq_client_core::world::ClientWorld,
    names: &SpellNames,
) -> Option<(u32, u32)> {
    eq_client_core::resources::estimate_titanium(player, inventory, world, names)
        .ok()
        .map(|capacity| (capacity.mana, capacity.endurance))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::{
        BaseAttributes, PlayerState, WorldEvent, WorldUpdate,
        inventory::InventoryUpdate,
        world::{ClientWorld, NoSpells, SpellCatalog},
    };

    /// Tells the world one piece of news, with this spell data to hand.
    fn tell(world: &mut ClientWorld, event: WorldEvent, spells: &dyn SpellCatalog) {
        world.apply(&WorldUpdate::Game(event), std::time::Instant::now(), spells);
    }

    /// The player admitted, with an empty buff table.
    fn admitted() -> ClientWorld {
        let mut world = ClientWorld::default();
        for event in [
            WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 1,
                zone: "qeytoqrg".into(),
                player: Box::new(player()),
                far_clip: None,
            },
            WorldEvent::BuffSnapshot(Vec::new()),
        ] {
            tell(&mut world, event, &NoSpells);
        }
        world
    }

    fn buff(spell_id: u32, buff: Option<eq_client_core::Buff>) -> WorldEvent {
        WorldEvent::Buff(eq_client_core::BuffUpdate {
            entity_id: 7,
            slot: 0,
            spell_id,
            buff,
        })
    }

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
            practice_points: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
        }
    }

    #[test]
    fn harmless_stack_moves_keep_estimates_but_stat_food_and_equipment_do_not() {
        use eq_client_core::ItemBonuses;
        use eq_client_core::inventory::{Inventory, InventorySlot};
        let world = admitted();
        let names = SpellNames::default();
        let mut milk = crate::preview::items().remove(0);
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
            held.slot = InventorySlot::CURSOR;
            inventory.apply(InventoryUpdate::Prediction(vec![held]));
            assert_eq!(
                estimate(&player(), &inventory, &world, &names),
                (mana_bonus == 0).then_some((25, 20))
            );
            // Moving the item onward cannot erase the unresolved original food.
            inventory.apply(InventoryUpdate::Prediction(vec![]));
            assert_eq!(
                estimate(&player(), &inventory, &world, &names),
                (mana_bonus == 0).then_some((25, 20))
            );
        }
        let mut equipment = milk;
        equipment.slot = InventorySlot(0);
        let mut inventory = Inventory::default();
        inventory.apply(InventoryUpdate::Snapshot(vec![equipment]));
        inventory.apply(InventoryUpdate::Prediction(vec![]));
        assert_eq!(estimate(&player(), &inventory, &world, &names), None);
    }

    #[test]
    fn estimates_follow_buffs_and_invalidate_missing_or_contradictory_data() {
        let mut inventory = eq_client_core::inventory::Inventory::default();
        inventory.apply(InventoryUpdate::Snapshot(vec![]));
        let mut world = admitted();
        let mut fields = vec!["0"; 183];
        fields[0] = "42";
        fields[1] = "Synthetic wisdom";
        fields[20] = "10";
        fields[70] = "100";
        fields[86] = "9";
        let names = SpellNames::parse(&fields.join("^"));
        assert_eq!(
            estimate(&player(), &inventory, &world, &names),
            Some((25, 20))
        );
        tell(
            &mut world,
            buff(
                42,
                Some(eq_client_core::Buff {
                    spell_id: 42,
                    caster_level: 1,
                    effect_type: 2,
                    bard_modifier: 10,
                    duration_ticks: 10,
                    counters: 0,
                    caster_id: 7,
                }),
            ),
            &NoSpells,
        );
        assert_eq!(
            estimate(&player(), &inventory, &world, &names),
            Some((27, 20))
        );
        assert_eq!(
            estimate(&player(), &inventory, &world, &SpellNames::default()),
            None
        );
        assert_eq!(
            eq_client_core::resources::estimate_titanium(
                &player(),
                &inventory,
                &world,
                &SpellNames::default(),
            ),
            Err(eq_client_core::resources::EstimateUnavailable::Spell(42))
        );
        let confirmed = world.buffs().slots().unwrap()[&0].clone();
        tell(
            &mut world,
            WorldEvent::SpellEffect(eq_client_core::SpellEffect {
                target_id: 7,
                caster_id: 7,
                caster_level: 1,
                instrument_modifier: 10,
                spell_id: 42,
                spell_level: 1,
                effect_flag: 4,
            }),
            &NoSpells,
        );
        assert_eq!(estimate(&player(), &inventory, &world, &names), None);
        tell(&mut world, buff(42, Some(confirmed)), &NoSpells);
        assert_eq!(
            estimate(&player(), &inventory, &world, &names),
            Some((27, 20))
        );
        tell(&mut world, WorldEvent::BuffSnapshot(Vec::new()), &NoSpells);
        assert_eq!(
            estimate(&player(), &inventory, &world, &names),
            Some((25, 20))
        );
        tell(&mut world, WorldEvent::Mana(26), &NoSpells);
        assert_eq!(estimate(&player(), &inventory, &world, &names), None);
        tell(&mut world, WorldEvent::Mana(25), &NoSpells);
        // A dropped connection forgets the buffs.
        world.apply(
            &WorldUpdate::Connection(eq_client_core::world::Link::Entering),
            std::time::Instant::now(),
            &NoSpells,
        );
        assert_eq!(estimate(&player(), &inventory, &world, &names), None);
        tell(&mut world, WorldEvent::BuffSnapshot(Vec::new()), &NoSpells);
        inventory.apply(InventoryUpdate::Invalidated);
        assert_eq!(estimate(&player(), &inventory, &world, &names), None);
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
            let mut world = admitted();
            tell(
                &mut world,
                WorldEvent::SpellEffect(eq_client_core::SpellEffect {
                    target_id: 7,
                    caster_id: 7,
                    caster_level: 1,
                    instrument_modifier: 10,
                    spell_id: 42,
                    spell_level: 1,
                    effect_flag: 4,
                }),
                &names,
            );
            assert_eq!(world.buffs().effects().is_empty(), expected_instant);
            assert_eq!(
                estimate(&player(), &inventory, &world, &names),
                Some((25, 20))
            );
            if !expected_instant {
                // An unknown or resource-bearing existing buff could have been
                // replaced. Do not retain its old bonus or silently discard it.
                tell(
                    &mut world,
                    buff(
                        43,
                        Some(eq_client_core::Buff {
                            spell_id: 43,
                            caster_level: 1,
                            effect_type: 2,
                            bard_modifier: 10,
                            duration_ticks: 10,
                            counters: 0,
                            caster_id: 7,
                        }),
                    ),
                    &NoSpells,
                );
                assert_eq!(estimate(&player(), &inventory, &world, &names), None);
                for effect_id in ["9", "97", "190", "9999"] {
                    let mut other = fields.clone();
                    other[0] = "43".into();
                    other[20] = "10".into();
                    other[70] = "100".into();
                    other[86] = effect_id.into();
                    let combined =
                        SpellNames::parse(&format!("{}\n{}", fields.join("^"), other.join("^")));
                    assert_eq!(estimate(&player(), &inventory, &world, &combined), None);
                }
            }
        }
    }

    #[test]
    fn scheduled_estimate_clears_on_disconnect_and_when_rules_are_disabled() {
        let mut app = App::new();
        let mut online = OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, player());
        crate::online::testing::buffs(&mut online, std::collections::BTreeMap::new());
        crate::online::testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![]));
        app.insert_resource(ViewerSettings(super::super::ViewerConfig {
            estimate_titanium_resources: true,
            ..default()
        }))
        .insert_resource(online)
        .init_resource::<HudState>()
        .init_resource::<SpellNames>()
        .add_systems(Update, update);
        app.update();
        assert_eq!(
            app.world().resource::<HudState>().resource_estimate,
            Some((25, 20))
        );
        crate::online::testing::connect(&mut app.world_mut().resource_mut::<OnlineState>(), false);
        app.update();
        assert_eq!(app.world().resource::<HudState>().resource_estimate, None);
        crate::online::testing::connect(&mut app.world_mut().resource_mut::<OnlineState>(), true);
        crate::online::testing::buffs(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            std::collections::BTreeMap::new(),
        );
        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .0
            .estimate_titanium_resources = false;
        app.update();
        assert_eq!(app.world().resource::<HudState>().resource_estimate, None);
    }
}
