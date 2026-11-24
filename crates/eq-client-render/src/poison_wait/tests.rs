//! Effect 234's total from synthetic buffs and AAs, and its report to the
//! session (checklist 6.493b, read R49-1f).
use super::*;
use crate::online::testing;
use eq_client_core::{
    Buff, Capability, WorldEvent,
    alternate_advancement::{Effect as AaEffect, Rank, Selection, Update},
    hazards::TitaniumEnvironment,
};
use std::collections::BTreeMap;

const POISON_SPELL: u32 = 4001;
const OTHER_SPELL: u32 = 4002;

fn effect(id: u32, base: i32, formula: u32) -> Effect {
    Effect {
        id,
        base,
        limit: 0,
        maximum: 0,
        formula,
    }
}

/// Spell data built in code: one spell carries effect 234 at -2000 ms,
/// another carries none; every other spell is unknown.
fn mechanics(id: u32) -> Option<Vec<Effect>> {
    match id {
        POISON_SPELL => Some(vec![effect(0, 10, 0), effect(POISON_APPLY_TIME, -2000, 0)]),
        OTHER_SPELL => Some(vec![effect(0, 10, 0)]),
        _ => None,
    }
}

fn buff(spell_id: u32, effect_type: u8) -> Buff {
    Buff {
        spell_id,
        caster_level: 30,
        effect_type,
        bard_modifier: 10,
        duration_ticks: 50,
        counters: 0,
        caster_id: 0,
    }
}

/// A level-28 class-2 player in an admission that offers these, holding
/// AAs when `holds_aa`.
fn online(capabilities: Vec<Capability>, holds_aa: bool) -> OnlineState {
    let mut player = testing::player(7);
    player.class = Some(2);
    player.level = 28;
    player.titanium_environment = Some(TitaniumEnvironment {
        zone_id: 27,
        base_fire_resistance: 25,
        base_cold_resistance: 25,
        has_alternate_advancement: holds_aa,
    });
    let mut online = OnlineState::new(true);
    testing::enter_with(&mut online, 5, player, None, capabilities);
    online
}

fn aa(online: &mut OnlineState, effects: &[(u32, i32)]) {
    let rank = Rank {
        id: 77,
        required_level: 28,
        classes: 1 << 2,
        sequence: 900,
        rank: 1,
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
            .map(|(slot, &(id, value))| AaEffect {
                id,
                value,
                limit: 0,
                slot: u32::try_from(slot).unwrap(),
            })
            .collect(),
    };
    testing::news(
        online,
        [WorldEvent::AlternateAdvancement {
            session_id: 5,
            updates: vec![
                Update::Selections(Some(vec![Selection { id: 77, value: 1 }])),
                Update::Definition {
                    id: 77,
                    rank: Some(rank),
                },
            ],
        }],
    );
}

fn total_of(online: &OnlineState) -> Option<i64> {
    let world = online.world();
    total(world, world.player().unwrap(), mechanics)
}

#[test]
fn buffs_add_their_effect_and_a_type_four_record_takes_it_away() {
    let mut online = online(vec![Capability::ApplyingPoison], false);
    // No buff report yet: unknown, so the session keeps its 10 s.
    assert_eq!(total_of(&online), None);
    testing::buffs(&mut online, BTreeMap::new());
    assert_eq!(total_of(&online), Some(0));
    testing::buffs(
        &mut online,
        BTreeMap::from([(0, buff(POISON_SPELL, 2)), (1, buff(OTHER_SPELL, 2))]),
    );
    assert_eq!(total_of(&online), Some(-2000));
    // 0x41002f negates a type-4 record's share.
    testing::buffs(
        &mut online,
        BTreeMap::from([(0, buff(POISON_SPELL, 2)), (1, buff(POISON_SPELL, 4))]),
    );
    assert_eq!(total_of(&online), Some(0));
}

#[test]
fn an_unknown_buff_spell_leaves_the_total_unknown() {
    let mut online = online(vec![Capability::ApplyingPoison], false);
    testing::buffs(&mut online, BTreeMap::from([(0, buff(9999, 2))]));
    assert_eq!(total_of(&online), None);
    // An instrument this client does not evaluate is unknown only where the
    // spell carries the effect.
    let mut sung = buff(POISON_SPELL, 2);
    sung.bard_modifier = 12;
    testing::buffs(&mut online, BTreeMap::from([(0, sung)]));
    assert_eq!(total_of(&online), None);
    let mut other = buff(OTHER_SPELL, 2);
    other.bard_modifier = 12;
    testing::buffs(&mut online, BTreeMap::from([(0, other)]));
    assert_eq!(total_of(&online), Some(0));
}

#[test]
fn without_aas_known_buffs_are_the_whole_total() {
    // A profile that holds no AA has nothing there to add, even before any
    // AA definition arrives.
    let mut held_none = online(
        vec![Capability::ApplyingPoison, Capability::AlternateAdvancement],
        false,
    );
    testing::buffs(&mut held_none, BTreeMap::from([(0, buff(POISON_SPELL, 2))]));
    assert_eq!(total_of(&held_none), Some(-2000));
    // A player who holds AAs needs every definition first.
    let mut holds = online(
        vec![Capability::ApplyingPoison, Capability::AlternateAdvancement],
        true,
    );
    testing::buffs(&mut holds, BTreeMap::from([(0, buff(POISON_SPELL, 2))]));
    assert_eq!(total_of(&holds), None);
    // 0x40feaa adds every usable AA's effect.
    aa(&mut holds, &[(POISON_APPLY_TIME, -1500), (5, 20)]);
    assert_eq!(total_of(&holds), Some(-3500));
}

#[test]
fn worn_items_do_not_count() {
    // Neither 0x41002f nor 0x40feaa reads the worn inventory, so an item
    // whose worn effect carries 234 changes nothing.
    let mut online = online(vec![Capability::ApplyingPoison], false);
    testing::buffs(&mut online, BTreeMap::new());
    let mut ring = crate::preview::items().into_iter().next().unwrap();
    ring.slot = eq_client_core::inventory::InventorySlot(4);
    ring.bag_slots = 0;
    ring.book = None;
    ring.details.equipment = Some(eq_network_game::items::EquipmentRules {
        required_level: 1,
        recommended_level: 0,
        worn: Some(eq_network_game::items::WornEffect {
            spell_id: POISON_SPELL,
            effect_type: 2,
            level: 30,
            level2: 0,
        }),
    });
    testing::inventory(
        &mut online,
        eq_client_core::inventory::InventoryUpdate::Snapshot(vec![ring]),
    );
    assert_eq!(total_of(&online), Some(0));
}

#[test]
fn an_unslotted_landing_that_carries_the_effect_leaves_it_unknown() {
    let mut online = online(vec![Capability::ApplyingPoison], false);
    testing::buffs(&mut online, BTreeMap::new());
    testing::news(
        &mut online,
        [WorldEvent::SpellEffect(eq_client_core::SpellEffect {
            target_id: 7,
            caster_id: 7,
            caster_level: 30,
            instrument_modifier: 10,
            spell_id: u16::try_from(POISON_SPELL).unwrap(),
            spell_level: 30,
            effect_flag: 4,
            push: None,
        })],
    );
    let landed = online.world().buffs().effects().len();
    assert_eq!(landed, 1, "the landing is kept without a slot");
    assert_eq!(total_of(&online), None);
}

fn app(online: OnlineState) -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
    let (sender, queue) = std::sync::mpsc::sync_channel(16);
    let mut app = App::new();
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(sender)))
        .init_resource::<crate::spellbook::SpellNames>()
        .add_systems(Update, report);
    (app, queue)
}

#[test]
fn the_total_is_told_once_per_value_and_admission_while_offered() {
    let mut state = online(vec![Capability::ApplyingPoison], false);
    testing::buffs(&mut state, BTreeMap::new());
    let (mut app, queue) = app(state);
    app.update();
    app.update();
    let sent: Vec<_> = queue.try_iter().collect();
    assert!(
        matches!(
            sent[..],
            [ClientCommand::PoisonWait {
                session_id: 5,
                total_ms: Some(0)
            }]
        ),
        "{sent:?}"
    );
    // A buff of a spell the client does not know makes the total unknown,
    // which is told too, so the session goes back to its 10 s.
    testing::buffs(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        BTreeMap::from([(0, buff(POISON_SPELL, 2))]),
    );
    app.update();
    let sent: Vec<_> = queue.try_iter().collect();
    assert!(
        matches!(
            sent[..],
            [ClientCommand::PoisonWait {
                session_id: 5,
                total_ms: None
            }]
        ),
        "{sent:?}"
    );
}

#[test]
fn nothing_is_told_where_apply_poison_is_not_offered() {
    let mut state = online(Vec::new(), false);
    testing::buffs(&mut state, BTreeMap::new());
    let (mut app, queue) = app(state);
    app.update();
    assert_eq!(queue.try_iter().count(), 0);
}

#[test]
fn no_script_or_typed_line_can_post_the_total() {
    // Reviewer H1: the report is the host's own.
    assert_eq!(
        crate::commands::resolve("poisonwait"),
        crate::commands::Resolved::Nothing
    );
    let errors = crate::script::Program::diagnostics("poison_wait(0)\n", std::path::Path::new("."));
    assert!(!errors.is_empty(), "{errors:?}");
}
