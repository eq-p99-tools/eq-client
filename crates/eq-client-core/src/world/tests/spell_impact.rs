use super::*;
use crate::{
    GameDialect,
    combat::{Damage, DamageOutcome, SPELL_DAMAGE_KIND},
};

#[derive(Clone, Copy)]
struct Catalog {
    known: bool,
    on_you: Option<&'static str>,
    on_other: Option<&'static str>,
    target: Option<u32>,
    discipline: Option<bool>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            known: true,
            on_you: Some("synthetic own impact"),
            on_other: Some(" has a synthetic impact."),
            target: Some(5),
            discipline: Some(false),
        }
    }
}
impl SpellCatalog for Catalog {
    fn impact(&self, _: u32) -> Option<SpellImpact<'_>> {
        self.known.then_some(SpellImpact {
            on_you: self.on_you,
            on_other: self.on_other,
            discipline: self.discipline,
        })
    }
    fn timing(&self, _: u32) -> Option<SpellTiming> {
        None
    }
    fn instant_effect(&self, _: u32) -> bool {
        false
    }
    fn target_type(&self, _: u32) -> Option<u32> {
        self.target
    }
    fn feigns(&self, _: u32) -> bool {
        false
    }
}

fn position(x: f32, y: f32, z: f32) -> WorldPosition {
    WorldPosition {
        x,
        y,
        z,
        ..WorldPosition::default()
    }
}

fn fixture() -> ClientWorld {
    let mut world = admitted();
    world.carry_as(GameDialect::Titanium);
    for (id, name, kind) in [
        (5, "a_rat", SpawnKind::Npc),
        (6, "Caster", SpawnKind::Player),
    ] {
        let mut actor = spawn(id);
        actor.name = name.into();
        actor.kind = kind;
        actor.class = Some(1);
        game(&mut world, WorldEvent::Spawns(vec![actor]));
    }
    world
}

fn damage(source_id: u16, target_id: u16, amount: u32) -> Damage {
    Damage {
        source_id,
        target_id,
        kind: SPELL_DAMAGE_KIND,
        spell_id: Some(14),
        outcome: DamageOutcome::Hit(amount),
        push: None,
    }
}

fn receive(world: &mut ClientWorld, record: Damage, catalog: &Catalog) -> Changes {
    let changes = world.apply(
        &WorldUpdate::Game(WorldEvent::Damage(record)),
        Instant::now(),
        catalog,
    );
    assert_eq!(
        changes.damage,
        [record],
        "presentation never changes the numeric input"
    );
    changes
}

fn impact(changes: &Changes) -> (&SpellImpactLine, SpellImpactFilter, Option<u32>) {
    let [
        Notice::SpellImpact {
            line,
            filter,
            message_type,
        },
    ] = changes.notices.as_slice()
    else {
        panic!("expected one native description: {:?}", changes.notices);
    };
    (line, *filter, *message_type)
}

#[test]
fn caster_gets_one_other_description_without_amount_and_server_numeric_text_stays_independent() {
    let mut world = fixture();
    world.zone.spawns.get_mut(&5).unwrap().state.position = position(1000.0, 0.0, 0.0);
    let changes = receive(&mut world, damage(9, 5, 33), &Catalog::default());
    assert_eq!(
        impact(&changes),
        (
            &SpellImpactLine::Other {
                target: "a_rat".into(),
                text: " has a synthetic impact.".into()
            },
            SpellImpactFilter::Own,
            Some(0x108)
        )
    );
    let server = world.apply(
        &WorldUpdate::ServerMessage {
            string_id: 9073,
            arguments: vec!["caster".into(), "target".into(), "33".into()],
            message_type: Some(283),
        },
        Instant::now(),
        &Catalog::default(),
    );
    assert!(matches!(
        server.notices.as_slice(),
        [Notice::ServerString {
            id: 9073,
            message_type: Some(283),
            ..
        }]
    ));
}

#[test]
fn target_gets_one_own_description_with_each_positive_amount_and_plain_text_at_zero() {
    let mut world = fixture();
    world.zone.spawns.get_mut(&6).unwrap().state.position = position(1000.0, 0.0, 0.0);
    for amount in [1, 2, 33] {
        let changes = receive(&mut world, damage(6, 9, amount), &Catalog::default());
        assert_eq!(
            impact(&changes).0,
            &SpellImpactLine::Own {
                text: "synthetic own impact".into(),
                amount: Some(amount)
            }
        );
    }
    let mut landed = damage(6, 9, 0);
    landed.outcome = DamageOutcome::Landed;
    let changes = receive(&mut world, landed, &Catalog::default());
    assert_eq!(
        impact(&changes).0,
        &SpellImpactLine::Own {
            text: "synthetic own impact".into(),
            amount: None
        }
    );
}

#[test]
fn bystander_range_uses_target_taxicab_distance_with_inclusive_75_and_role_bypasses() {
    let mut world = fixture();
    for (source, target, shown) in [
        (position(1000.0, 0.0, 0.0), position(25.0, 25.0, 25.0), true),
        (position(0.0, 0.0, 0.0), position(50.0, 50.0, 0.0), false),
        (
            position(0.0, 0.0, 0.0),
            position(25.0, 25.0, f32::from_bits(25.0_f32.to_bits() + 1)),
            false,
        ),
        (position(1000.0, 0.0, 0.0), position(75.0, 0.0, 0.0), true),
        (position(0.0, 0.0, 0.0), position(75.001, 0.0, 0.0), false),
        (position(0.0, 0.0, 0.0), position(0.0, 0.0, 75.0), true),
        (position(0.0, 0.0, 0.0), position(0.0, 0.0, 75.001), false),
    ] {
        world.zone.spawns.get_mut(&6).unwrap().state.position = source;
        world.zone.spawns.get_mut(&5).unwrap().state.position = target;
        let changes = receive(&mut world, damage(6, 5, 2), &Catalog::default());
        assert_eq!(
            !changes.notices.is_empty(),
            shown,
            "source={source:?} target={target:?}"
        );
    }
}

#[test]
fn missing_spawns_and_missing_catalog_or_fields_do_not_invent_impact_text() {
    for missing in [5, 6] {
        let mut world = fixture();
        world.zone.spawns.remove(&missing);
        assert!(
            receive(&mut world, damage(6, 5, 2), &Catalog::default())
                .notices
                .is_empty()
        );
    }
    let mut world = fixture();
    for catalog in [
        Catalog {
            known: false,
            ..Catalog::default()
        },
        Catalog {
            on_other: None,
            ..Catalog::default()
        },
        Catalog {
            on_other: Some(""),
            ..Catalog::default()
        },
    ] {
        assert!(
            receive(&mut world, damage(9, 5, 2), &catalog)
                .notices
                .is_empty()
        );
    }
    assert!(
        receive(
            &mut world,
            damage(6, 9, 2),
            &Catalog {
                on_you: None,
                ..Catalog::default()
            }
        )
        .notices
        .is_empty()
    );
    let empty_target = receive(
        &mut world,
        damage(6, 9, 2),
        &Catalog {
            on_you: Some(""),
            ..Catalog::default()
        },
    );
    assert_eq!(
        impact(&empty_target).0,
        &SpellImpactLine::Own {
            text: String::new(),
            amount: Some(2)
        }
    );
    for id in [None, Some(0), Some(10_000), Some(u16::MAX)] {
        let mut record = damage(9, 5, 2);
        record.spell_id = id;
        assert!(
            receive(&mut world, record, &Catalog::default())
                .notices
                .is_empty()
        );
    }
}

#[test]
fn native_filter_roles_use_actor_class_and_either_group_endpoint() {
    let mut world = fixture();
    assert_eq!(
        impact(&receive(&mut world, damage(6, 5, 2), &Catalog::default())).1,
        SpellImpactFilter::Player
    );
    world.zone.spawns.get_mut(&6).unwrap().state.kind = SpawnKind::Npc;
    assert_eq!(
        impact(&receive(&mut world, damage(6, 5, 2), &Catalog::default())).1,
        SpellImpactFilter::Npc
    );
    world.zone.spawns.get_mut(&6).unwrap().state.class = Some(8);
    assert_eq!(
        impact(&receive(&mut world, damage(6, 9, 2), &Catalog::default())).1,
        SpellImpactFilter::Bard
    );
    world.player.as_mut().unwrap().class = Some(8);
    assert_eq!(
        impact(&receive(&mut world, damage(9, 5, 2), &Catalog::default())).1,
        SpellImpactFilter::OwnBard
    );
    world.player.as_mut().unwrap().class = Some(1);
    world.zone.spawns.get_mut(&6).unwrap().state.class = Some(1);
    world.zone.spawns.get_mut(&5).unwrap().state.kind = SpawnKind::Player;
    world.group = Some(Group {
        members: vec!["A_RAT".into()],
        ..Group::default()
    });
    assert_eq!(
        impact(&receive(&mut world, damage(6, 5, 2), &Catalog::default())).1,
        SpellImpactFilter::Grouped
    );
    assert_eq!(
        impact(&receive(&mut world, damage(6, 9, 2), &Catalog::default())).1,
        SpellImpactFilter::Own
    );
}

#[test]
fn source_zero_uses_the_known_target_actor_and_unknown_class_is_not_guessed() {
    let mut world = fixture();
    assert_eq!(
        impact(&receive(&mut world, damage(0, 5, 2), &Catalog::default())).1,
        SpellImpactFilter::Npc
    );
    world.zone.spawns.get_mut(&6).unwrap().state.class = None;
    assert!(
        receive(&mut world, damage(6, 5, 2), &Catalog::default())
            .notices
            .is_empty()
    );
}

#[test]
fn other_dialects_and_ordinary_melee_keep_their_existing_damage_notices() {
    for generation in [GameDialect::EqMac, GameDialect::Rof2] {
        let mut world = fixture();
        world.carry_as(generation);
        assert!(
            matches!(receive(&mut world, damage(9, 5, 2), &Catalog::default()).notices.as_slice(), [Notice::Damage { generation: observed, .. }] if *observed == generation)
        );
    }
    let mut world = fixture();
    let mut record = damage(9, 9, 33);
    record.kind = 4;
    record.spell_id = None;
    for _ in 0..2 {
        assert!(matches!(
            receive(&mut world, record, &Catalog::default())
                .notices
                .as_slice(),
            [Notice::Damage {
                generation: GameDialect::Titanium,
                ..
            }]
        ));
    }
    assert!(
        receive(
            &mut world,
            damage(9, 9, 2),
            &Catalog {
                target: Some(13),
                ..Catalog::default()
            }
        )
        .notices
        .is_empty()
    );
}
