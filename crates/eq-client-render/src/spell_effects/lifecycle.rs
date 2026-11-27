//! Asset-free ECS lifetime coverage of real spell particle drawing.
//! No installed assets/GPU/game. Event reduction, real particle update, model
//! drawing and root replacement are exercised. Native death preserves the
//! actor; graphics-DLL despawn policy remains unread (R422/U1).
use super::*;
use crate::{
    character,
    online::{OnlineState, testing},
};
use bevy::ecs::system::RunSystemOnce;
use eq_client_core::{
    Death, SpawnKind, SpellEffect, WorldEvent,
    combat::{Damage, DamageOutcome},
};
use std::time::Duration;

fn effects(placement: u32) -> (SpellEffects, crate::spellbook::SpellNames) {
    let mut eff = vec![0; 268 * 2];
    eff[268 + 68..268 + 72].copy_from_slice(&1_u32.to_le_bytes());
    eff[268 + 80..268 + 84].copy_from_slice(&placement.to_le_bytes());
    eff[268 + 204..268 + 208].copy_from_slice(&1_u32.to_le_bytes());
    eff[268 + 216..268 + 220].copy_from_slice(&placement.to_le_bytes());
    let mut edd = b"EDD\0".to_vec();
    edd.extend(b"110\0");
    edd.resize(8 + 416 * 2, 0);
    let emitter = &mut edd[8 + 416..];
    emitter[64..72].copy_from_slice(b"test.tga");
    for (at, value) in [(0x88, 20.0_f32), (0x94, 0.2), (0xcc, 1.0)] {
        emitter[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    for at in [0xd4, 0xd8, 0xdc, 0xe0, 0xe4, 0xe8] {
        emitter[at..at + 4].copy_from_slice(&255_u32.to_le_bytes());
    }
    let mut spell = vec!["0"; 160];
    spell[0] = "42";
    spell[1] = "Synthetic killing spell";
    spell[145] = "1";
    (
        SpellEffects::parse(&eff, &edd).unwrap(),
        crate::spellbook::SpellNames::parse(&spell.join("^")),
    )
}

fn impact() -> WorldEvent {
    WorldEvent::SpellEffect(SpellEffect {
        target_id: 9,
        caster_id: 7,
        caster_level: 1,
        instrument_modifier: 10,
        spell_id: 42,
        spell_level: 1,
        effect_flag: 4,
        push: None,
    })
}
fn damage() -> WorldEvent {
    WorldEvent::Damage(Damage {
        target_id: 9,
        source_id: 7,
        kind: 231,
        spell_id: Some(42),
        outcome: DamageOutcome::Hit(100),
        push: None,
    })
}
fn death() -> WorldEvent {
    WorldEvent::Death(Death {
        spawn_id: 9,
        killer_id: 7,
        corpse_id: 9,
        bind_zone_id: 0,
        corpse_name: None,
    })
}
fn news(world: &mut World, events: impl IntoIterator<Item = WorldEvent>) {
    testing::news(&mut world.resource_mut::<OnlineState>(), events);
}
fn tick(world: &mut World) {
    world
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(50));
    world.run_system_once(update).unwrap();
}
fn visuals(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<Entity, (With<Mesh3d>, With<NoFrustumCulling>)>()
        .iter(world)
        .collect()
}
fn model(world: &mut World) -> Entity {
    let prepared = character::testing::prepared(world);
    let root = character::testing::drawn(world, &prepared);
    world
        .entity_mut(root)
        .insert(crate::entities::RemoteEntity::new(9, None, 6.0));
    root
}
fn fixture_for(placement: u32) -> (World, Entity) {
    let mut world = World::new();
    character::testing::stores(&mut world);
    world.init_resource::<Time>();
    world.init_resource::<crate::sounds::Sounds>();
    world.init_resource::<crate::options::OptionsState>();
    world
        .resource_mut::<crate::options::OptionsState>()
        .options
        .set(eq_client_core::options::Toggle::SpellEffects, true);
    let (effects, spells) = effects(placement);
    world.insert_resource(spells);
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    world.insert_resource(SpellParticles {
        directory: Some(PathBuf::from("synthetic-no-file-io")),
        effects: Some(Arc::new(effects)),
        images: [("test.tga".into(), Picture::Ready(material))]
            .into_iter()
            .collect(),
        ..SpellParticles::default()
    });
    world.insert_resource(OnlineState::new(true));
    let mut victim = testing::pet(9, 0);
    victim.pet_owner = None;
    news(
        &mut world,
        [
            WorldEvent::Entered {
                capabilities: vec![],
                choices: vec![],
                session_id: 1,
                zone: "synthetic".into(),
                zone_id: 0,
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: Some(4),
            },
            WorldEvent::Spawns(vec![victim]),
        ],
    );
    let root = model(&mut world);
    tick(&mut world); // initialize Running before the new landing
    (world, root)
}
fn fixture() -> (World, Entity) {
    fixture_for(4)
}

fn finish(world: &mut World) {
    // Synthetic emission1.5s + particle life0.2s, with ample room; no native
    // timing claim is made by this fixture.
    for _ in 0..50 {
        tick(world);
    }
    assert!(
        visuals(world).is_empty(),
        "an expired effect left a frozen render entity"
    );
}

#[test]
fn nonlethal_spell_target_particles_update_and_complete_their_finite_lifetime() {
    let (mut world, root) = fixture();
    news(&mut world, [impact()]);
    tick(&mut world);
    let drawn = visuals(&mut world);
    assert_eq!(drawn.len(), 1);
    let mesh = world.get::<Mesh3d>(drawn[0]).unwrap().0.clone();
    let before = world
        .resource::<Assets<Mesh>>()
        .get(&mesh)
        .unwrap()
        .count_vertices();
    tick(&mut world);
    assert!(
        world
            .resource::<Assets<Mesh>>()
            .get(&mesh)
            .unwrap()
            .count_vertices()
            > before
    );
    finish(&mut world);
    assert!(world.get_entity(root).is_ok());
}

/// Move the actual model and its hand attachment; no fake particle pose.
fn pose(world: &mut World, root: Entity, origin: Vec3, hand: Vec3) {
    let model = {
        let mut character = world.get_mut::<character::AnimatedCharacter>(root).unwrap();
        character.attachments[eq_client_assets::characters::Attachment::LeftHand.index()] =
            Some(Mat4::from_translation(hand));
        character.model
    };
    *world.get_mut::<GlobalTransform>(model).unwrap() = GlobalTransform::from_translation(origin);
}

fn centers(world: &World, visual: Entity) -> Vec<Vec3> {
    let mesh = &world.get::<Mesh3d>(visual).unwrap().0;
    world
        .resource::<Assets<Mesh>>()
        .get(mesh)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
        .as_float3()
        .unwrap()
        .chunks_exact(4)
        .map(|quad| quad.iter().map(|p| Vec3::from_array(*p)).sum::<Vec3>() / 4.0)
        .collect()
}

#[test]
fn a_spells_own_killing_blow_keeps_its_bone_attachment_and_expires_on_the_corpse() {
    for death_first in [false, true] {
        for replace_before_present in [false, true] {
            let (mut world, root) = fixture();
            pose(&mut world, root, Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0));
            let revision = world
                .resource::<OnlineState>()
                .world()
                .spawn(9)
                .unwrap()
                .revision;
            let events = if death_first {
                vec![damage(), death(), impact()]
            } else {
                vec![impact(), damage(), death()]
            };
            news(&mut world, events);
            let spawn = world.resource::<OnlineState>().world().spawn(9).unwrap();
            assert_eq!(spawn.state.kind, SpawnKind::NpcCorpse);
            assert_ne!(spawn.revision, revision);
            let prior = if replace_before_present {
                None
            } else {
                tick(&mut world); // real reconcile may retain the old root for100ms
                let visual = visuals(&mut world)[0];
                assert_eq!(centers(&world, visual)[0], Vec3::new(1.0, 2.0, 3.0));
                Some(visual)
            };
            world.despawn(root); // the root replacement operation, not full scheduled reconcile
            let corpse = model(&mut world);
            pose(
                &mut world,
                corpse,
                Vec3::new(20.0, 0.0, 0.0),
                Vec3::new(2.0, 4.0, 6.0),
            );
            tick(&mut world);
            let drawn = visuals(&mut world);
            assert_eq!(drawn.len(), 1);
            if let Some(prior) = prior {
                assert_eq!(
                    drawn[0], prior,
                    "same effect, rather than replayed on the corpse"
                );
                assert_eq!(centers(&world, prior)[0], Vec3::new(1.0, 2.0, 3.0));
            }
            assert_eq!(
                *centers(&world, drawn[0]).last().unwrap(),
                Vec3::new(22.0, 4.0, 6.0)
            );
            finish(&mut world);
            assert!(world.get_entity(corpse).is_ok());
        }
    }
}

#[test]
fn a_delayed_corpse_model_keeps_the_effects_original_deadline() {
    let (mut world, root) = fixture();
    news(&mut world, [impact(), damage(), death()]);
    tick(&mut world); // at0.10; existing inferred target duration ends at1.60
    let visual = visuals(&mut world)[0];
    world.despawn(root);
    for _ in 0..24 {
        tick(&mut world);
    } // at1.30, while the model is absent
    let corpse = model(&mut world);
    pose(&mut world, corpse, Vec3::X * 20.0, Vec3::Y * 4.0);
    tick(&mut world);
    assert_eq!(visuals(&mut world), vec![visual]);
    assert_eq!(
        *centers(&world, visual).last().unwrap(),
        Vec3::new(20.0, 4.0, 0.0)
    );
    for _ in 0..10 {
        tick(&mut world);
    } // at1.85, past original duration+life
    assert!(
        visuals(&mut world).is_empty(),
        "replacement restarted the effect lifetime"
    );
}

#[test]
fn floor_placements_stay_at_the_impact_point_when_the_corpse_moves() {
    // These are the existing inferred EFF floor placements. R422's classic
    // types5/6 do not establish a new EFF numeric mapping.
    for placement in [0, 3] {
        let (mut world, root) = fixture_for(placement);
        news(&mut world, [impact(), damage(), death()]);
        tick(&mut world);
        let visual = visuals(&mut world)[0];
        let floor = centers(&world, visual)[0];
        world.despawn(root);
        let corpse = model(&mut world);
        pose(
            &mut world,
            corpse,
            Vec3::new(20.0, 10.0, 0.0),
            Vec3::Y * 4.0,
        );
        tick(&mut world);
        assert_eq!(visuals(&mut world), vec![visual]);
        assert_eq!(*centers(&world, visual).last().unwrap(), floor);
        finish(&mut world);
    }
}

#[test]
fn a_killing_impact_followed_by_despawn_cannot_leave_an_unaged_particle_mesh() {
    let (mut world, root) = fixture();
    news(&mut world, [impact(), damage()]);
    tick(&mut world); // impact visible before a later batch/frame removes it
    assert!(!visuals(&mut world).is_empty());
    news(&mut world, [death(), WorldEvent::Despawn(9)]);
    assert!(world.resource::<OnlineState>().world().spawn(9).is_none());
    world.despawn(root);
    finish(&mut world);
}

#[test]
fn a_reused_spawn_id_does_not_inherit_the_previous_actors_emitter() {
    let (mut world, root) = fixture();
    news(&mut world, [impact()]);
    tick(&mut world);
    let old = visuals(&mut world)[0];
    let mut replacement = testing::pet(9, 0);
    replacement.pet_owner = None;
    news(&mut world, [WorldEvent::Spawns(vec![replacement])]);
    world.despawn(root);
    let next = model(&mut world);
    pose(&mut world, next, Vec3::X * 100.0, Vec3::Y * 4.0);
    tick(&mut world);
    assert_eq!(visuals(&mut world), vec![old]);
    assert!(centers(&world, old).iter().all(|point| point.x < 10.0));
    for _ in 0..5 {
        tick(&mut world);
    }
    assert!(visuals(&mut world).is_empty());
    news(&mut world, [impact()]);
    tick(&mut world);
    let drawn = visuals(&mut world);
    assert_eq!(drawn.len(), 1);
    assert_ne!(drawn[0], old);
    assert_eq!(
        *centers(&world, drawn[0]).last().unwrap(),
        Vec3::new(100.0, 4.0, 0.0)
    );
    finish(&mut world);
}

#[test]
fn a_missing_model_does_not_stop_the_particle_lifetime_clock() {
    let (mut world, root) = fixture();
    news(&mut world, [impact()]);
    tick(&mut world);
    assert_eq!(visuals(&mut world).len(), 1);
    let model = world
        .get::<character::AnimatedCharacter>(root)
        .unwrap()
        .model;
    world.despawn(model);
    finish(&mut world);
    assert!(world.get_entity(root).is_ok());
}

#[test]
fn turning_effects_off_also_clears_particles_without_a_drawn_body() {
    let (mut world, root) = fixture();
    news(&mut world, [impact()]);
    tick(&mut world);
    assert_eq!(visuals(&mut world).len(), 1);
    world.despawn(root);
    world
        .resource_mut::<crate::options::OptionsState>()
        .options
        .set(eq_client_core::options::Toggle::SpellEffects, false);
    tick(&mut world);
    assert!(visuals(&mut world).is_empty());
}

#[test]
fn admission_replacement_clears_the_prior_scenes_particles() {
    let (mut world, _) = fixture();
    news(&mut world, [impact()]);
    tick(&mut world);
    assert_eq!(visuals(&mut world).len(), 1);
    testing::admit(
        &mut world.resource_mut::<OnlineState>(),
        2,
        testing::player(7),
    );
    tick(&mut world);
    assert!(visuals(&mut world).is_empty());
}

#[test]
fn caster_particles_keep_aging_when_the_cast_ends_or_the_caster_dies() {
    for lethal in [false, true] {
        let (mut world, root) = fixture();
        testing::spell(
            &mut world.resource_mut::<OnlineState>(),
            eq_client_core::SpellUpdate::Began {
                caster_id: 9,
                spell_id: 42,
                duration_ms: 3_000,
            },
        );
        tick(&mut world);
        let visual = visuals(&mut world)[0];
        if lethal {
            news(&mut world, [death()]);
            world.despawn(root);
            model(&mut world);
        } else {
            let WorldEvent::SpellEffect(mut effect) = impact() else {
                unreachable!()
            };
            effect.caster_id = 9;
            effect.target_id = 7;
            news(&mut world, [WorldEvent::SpellEffect(effect)]);
        }
        tick(&mut world);
        assert_eq!(visuals(&mut world), vec![visual]);
        for _ in 0..5 {
            tick(&mut world);
        }
        assert!(visuals(&mut world).is_empty());
    }
}

#[test]
fn replacing_the_zone_within_one_admission_clears_the_prior_particles() {
    let (mut world, _) = fixture();
    news(&mut world, [impact()]);
    tick(&mut world);
    assert_eq!(visuals(&mut world).len(), 1);
    news(
        &mut world,
        [WorldEvent::Entered {
            capabilities: vec![],
            choices: vec![],
            session_id: 1,
            zone: "other".into(),
            zone_id: 0,
            player: Box::new(testing::player(7)),
            far_clip: None,
            spell_landing_flag: Some(4),
        }],
    );
    tick(&mut world);
    assert!(visuals(&mut world).is_empty());
}
