//! Full submerged decisions with synthetic received AA and buff inputs.
use super::*;
use crate::online::testing;
use eq_client_core::{
    BaseAttributes, Buff, WorldEvent,
    alternate_advancement::{Effect, Rank, Selection, Update},
    breath::{SET_BREATH_LEVEL, WATER_BREATHING},
    hazards::TitaniumEnvironment,
    inventory::InventoryUpdate,
};
use std::collections::BTreeMap;

fn fixture() -> OnlineState {
    let mut player = testing::player(7);
    player.class = Some(2);
    player.level = 28;
    player.base_attributes = Some(BaseAttributes {
        stamina: 75,
        ..BaseAttributes::default()
    });
    player.titanium_environment = Some(TitaniumEnvironment {
        zone_id: 27,
        base_fire_resistance: 25,
        has_alternate_advancement: true,
    });
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 5, player.clone());
    // Native landing records use the caster spawn's level, not the action's.
    testing::spawns(
        &mut online,
        vec![eq_client_core::SpawnState {
            body_type: None,
            spawn_id: player.spawn_id,
            class: player.class.map(|class| u8::try_from(class).unwrap()),
            name: player.name,
            kind: eq_client_core::SpawnKind::Player,
            race: player.race,
            gender: player.gender,
            position: player.position,
            velocity: [0.0; 3],
            forward: 0.0,
            walk_speed: player.walk_speed,
            run_speed: player.run_speed,
            size: player.size,
            invisible: false,
            appearance: player.appearance,
            level: player.level,
            listing: player.listing,
            name_parts: player.name_parts,
            pet_owner: None,
            hp_percent: player.hp_percent,
            posture: player.posture,
        }],
    );
    testing::inventory(&mut online, InventoryUpdate::Snapshot(Vec::new()));
    testing::buffs(&mut online, BTreeMap::new());
    testing::news(
        &mut online,
        [WorldEvent::HitPoints {
            spawn_id: 7,
            current: 80,
            maximum: 80,
            items: eq_client_core::ItemHitPoints::LeftOutOfCurrent,
        }],
    );
    online
}

fn receive(online: &mut OnlineState, session_id: u64, update: Update) {
    testing::news(
        online,
        [WorldEvent::AlternateAdvancement {
            session_id,
            updates: vec![update],
        }],
    );
}

fn selected(ids: &[u32]) -> Update {
    Update::Selections(Some(
        ids.iter().map(|&id| Selection { id, value: 99 }).collect(),
    ))
}

fn rank(id: u32, effects: &[(u32, i32)]) -> Update {
    Update::Definition {
        id,
        rank: Some(Rank {
            id,
            required_level: 28,
            classes: 1 << 2,
            sequence: 900,
            rank: 3,
            hotkey_sid: 0,
            hotkey_sid2: 0,
            title_sid: 0,
            desc_sid: 0,
            cost: 0,
            prereq_skill: 0,
            prereq_minpoints: 0,
            kind: 0,
            spellid: 0,
            spell_type: 0,
            spell_refresh: 0,
            max_level: 0,
            last_id: 0,
            next_id: 0,
            cost2: 0,
            effects: effects
                .iter()
                .enumerate()
                .map(|(slot, &(id, value))| Effect {
                    id,
                    value,
                    limit: 0,
                    slot: u32::try_from(slot).unwrap(),
                })
                .collect(),
        }),
    }
}

fn spells(id: u32, effect: u32, value: i32) -> SpellNames {
    spells_with_effects(id, &[(effect, value)])
}

fn spells_with_effects(id: u32, effects: &[(u32, i32)]) -> SpellNames {
    let mut fields = vec!["0".to_owned(); 183];
    fields[0] = id.to_string();
    fields[1] = "Synthetic environmental effect".to_owned();
    fields[16] = "7".to_owned();
    fields[17] = "10".to_owned();
    fields[83] = "1".to_owned();
    fields[98] = "6".to_owned();
    for (slot, &(effect, value)) in effects.iter().enumerate() {
        fields[20 + slot] = value.to_string();
        fields[70 + slot] = "100".to_owned();
        fields[86 + slot] = effect.to_string();
    }
    SpellNames::parse(&fields.join("^"))
}

fn buff(id: u32) -> Buff {
    Buff {
        spell_id: id,
        caster_level: 28,
        effect_type: 2,
        bard_modifier: 10,
        duration_ticks: 10,
        counters: 0,
        caster_id: 7,
    }
}

fn evaluate(
    online: &OnlineState,
    spells: &SpellNames,
) -> Result<(Sample, Option<u32>), Unavailable> {
    submerged(online.world(), spells, online.world().player().unwrap())
}

#[test]
fn a_received_buff_needs_mechanics_and_every_selected_aa_definition() {
    let mut online = fixture();
    testing::buffs(&mut online, BTreeMap::from([(0, buff(1000))]));
    let spells = spells(1000, WATER_BREATHING, 1);
    assert_eq!(
        evaluate(&online, &spells),
        Err(Unavailable::AlternateAdvancement)
    );
    receive(&mut online, 5, selected(&[42]));
    assert_eq!(
        evaluate(&online, &spells),
        Err(Unavailable::AlternateAdvancement)
    );
    // Knowing another rank, even one from the same sequence, supplies no ownership.
    receive(&mut online, 5, rank(43, &[]));
    assert_eq!(
        evaluate(&online, &spells),
        Err(Unavailable::AlternateAdvancement)
    );
    receive(&mut online, 5, rank(42, &[(11, 5)]));
    assert_eq!(
        evaluate(&online, &SpellNames::default()),
        Err(Unavailable::Effects)
    );
    assert_eq!(evaluate(&online, &spells), Ok((Sample::CanBreathe, None)));
    receive(&mut online, 5, Update::Definition { id: 42, rank: None });
    assert_eq!(
        evaluate(&online, &spells),
        Err(Unavailable::AlternateAdvancement)
    );
}

#[test]
fn signed_aa_values_and_eligibility_control_the_full_breathing_decision() {
    let mut online = fixture();
    testing::buffs(&mut online, BTreeMap::from([(0, buff(1000))]));
    let spells = spells(1000, WATER_BREATHING, 1);
    receive(&mut online, 5, selected(&[42]));
    receive(&mut online, 5, rank(42, &[(WATER_BREATHING, -1)]));
    let wet = Sample::Submerged(std::num::NonZeroU32::new(45).unwrap());
    assert_eq!(evaluate(&online, &spells), Ok((wet, Some(80))));
    receive(&mut online, 5, rank(42, &[(WATER_BREATHING, -2)]));
    assert_eq!(evaluate(&online, &spells), Ok((wet, Some(80))));
    // The selected value 99 never multiplies a definition's effect.
    receive(&mut online, 5, rank(42, &[(WATER_BREATHING, 1)]));
    testing::buffs(&mut online, BTreeMap::new());
    assert_eq!(evaluate(&online, &spells), Ok((Sample::CanBreathe, None)));
    let mut reversed = buff(1000);
    reversed.effect_type = 4;
    testing::buffs(&mut online, BTreeMap::from([(0, reversed)]));
    assert_eq!(evaluate(&online, &spells), Ok((wet, Some(80))));
    testing::buffs(&mut online, BTreeMap::new());
    let Update::Definition {
        rank: Some(mut wrong_class),
        ..
    } = rank(42, &[(WATER_BREATHING, 1)])
    else {
        panic!()
    };
    wrong_class.classes = 1 << 1;
    receive(
        &mut online,
        5,
        Update::Definition {
            id: 42,
            rank: Some(wrong_class),
        },
    );
    assert_eq!(evaluate(&online, &spells), Ok((wet, Some(80))));
    let Update::Definition {
        rank: Some(mut too_high),
        ..
    } = rank(42, &[(WATER_BREATHING, 1)])
    else {
        panic!()
    };
    too_high.required_level = 29;
    receive(
        &mut online,
        5,
        Update::Definition {
            id: 42,
            rank: Some(too_high),
        },
    );
    assert_eq!(evaluate(&online, &spells), Ok((wet, Some(80))));
    testing::news(
        &mut online,
        [WorldEvent::Level {
            current: 29,
            previous: 28,
            experience: 0,
        }],
    );
    assert_eq!(evaluate(&online, &spells), Ok((Sample::CanBreathe, None)));
}

#[test]
fn unprotected_capacity_needs_a_known_selection_and_preserves_unknown_modifiers() {
    let mut online = fixture();
    let spells = SpellNames::default();
    receive(&mut online, 5, selected(&[]));
    assert_eq!(
        evaluate(&online, &spells).unwrap().0,
        Sample::Submerged(std::num::NonZeroU32::new(45).unwrap())
    );
    receive(&mut online, 5, selected(&[42]));
    receive(&mut online, 5, rank(42, &[(SET_BREATH_LEVEL, 150)]));
    assert_eq!(
        evaluate(&online, &spells).unwrap().0,
        Sample::Submerged(std::num::NonZeroU32::new(67).unwrap())
    );
    receive(&mut online, 5, rank(42, &[(SET_BREATH_LEVEL, -150)]));
    assert_eq!(
        evaluate(&online, &spells).unwrap().0,
        Sample::Submerged(std::num::NonZeroU32::new(45).unwrap())
    );
    for effect in [7, 159, 262, 40] {
        receive(&mut online, 5, rank(42, &[(effect, 5)]));
        assert_eq!(
            evaluate(&online, &spells),
            Err(Unavailable::AlternateAdvancement)
        );
    }
    // An unresolved effective-STA path is unnecessary once the signed predicate is known.
    receive(&mut online, 5, rank(42, &[(7, 5), (WATER_BREATHING, 1)]));
    assert_eq!(evaluate(&online, &spells), Ok((Sample::CanBreathe, None)));
    receive(&mut online, 5, Update::Unknown);
    assert_eq!(
        evaluate(&online, &spells),
        Err(Unavailable::AlternateAdvancement)
    );
}

#[test]
fn admission_replacement_and_unconfirmed_buff_replacement_cannot_reuse_a_total() {
    let mut online = fixture();
    let spells = spells(1000, WATER_BREATHING, 1);
    receive(&mut online, 5, selected(&[42]));
    receive(&mut online, 5, rank(42, &[(WATER_BREATHING, 1)]));
    assert_eq!(evaluate(&online, &spells), Ok((Sample::CanBreathe, None)));
    let player = online.world().player().unwrap().clone();
    testing::admit(&mut online, 6, player);
    testing::inventory(&mut online, InventoryUpdate::Snapshot(Vec::new()));
    testing::buffs(&mut online, BTreeMap::new());
    receive(&mut online, 5, rank(42, &[(WATER_BREATHING, 1)]));
    receive(&mut online, 6, selected(&[42]));
    assert_eq!(
        evaluate(&online, &spells),
        Err(Unavailable::AlternateAdvancement)
    );
    receive(&mut online, 6, rank(42, &[(WATER_BREATHING, -1)]));
    testing::buffs(&mut online, BTreeMap::from([(0, buff(1000))]));
    testing::news(
        &mut online,
        [WorldEvent::SpellEffect(eq_client_core::SpellEffect {
            spell_id: 1000,
            caster_level: 28,
            effect_flag: 4,
            instrument_modifier: 10,
            target_id: 7,
            caster_id: 7,
            spell_level: 1,
            push: None,
        })],
    );
    assert_eq!(
        evaluate(&online, &spells),
        Err(Unavailable::UnslottedModifier)
    );
}

#[test]
fn the_native_special_refresh_does_not_exclude_its_own_blocker() {
    for id in [2751, 1000] {
        let mut online = fixture();
        let spells = spells_with_effects(id, &[(WATER_BREATHING, 1), (148, 1)]);
        receive(&mut online, 5, selected(&[]));
        testing::buffs(&mut online, BTreeMap::from([(0, buff(id))]));
        testing::news_with(
            &mut online,
            [WorldEvent::SpellEffect(eq_client_core::SpellEffect {
                spell_id: u16::try_from(id).unwrap(),
                caster_level: 28,
                effect_flag: 4,
                instrument_modifier: 10,
                target_id: 7,
                caster_id: 7,
                spell_level: 1,
                push: None,
            })],
            &spells,
        );
        assert_eq!(
            evaluate(&online, &spells),
            if id == 2751 {
                Err(Unavailable::UnslottedModifier)
            } else {
                Ok((Sample::CanBreathe, None))
            }
        );
    }
}

#[test]
fn a_known_native_landing_record_projects_without_inventing_received_duration() {
    let mut online = fixture();
    let spells = spells(1000, WATER_BREATHING, 1);
    receive(&mut online, 5, selected(&[]));
    testing::news_with(
        &mut online,
        [WorldEvent::SpellEffect(eq_client_core::SpellEffect {
            spell_id: 1000,
            caster_level: 28,
            effect_flag: 4,
            instrument_modifier: 12,
            target_id: 7,
            caster_id: 7,
            spell_level: 1,
            push: None,
        })],
        &spells,
    );
    assert_eq!(online.world().buffs().slot_of(1000), Some(0));
    assert!(
        online
            .world()
            .buffs()
            .modelled_effect_record(1000)
            .is_some()
    );
    assert_eq!(evaluate(&online, &spells), Ok((Sample::CanBreathe, None)));
    assert!(
        online
            .world()
            .buffs()
            .received_duration(0, Instant::now())
            .is_none()
    );
}
