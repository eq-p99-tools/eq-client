use super::*;
use std::time::Duration;

struct Catalog;
impl SpellCatalog for Catalog {
    fn timing(&self, spell: u32) -> Option<SpellTiming> {
        matches!(spell, 42 | 43).then_some(SpellTiming {
            recovery_ms: 1500,
            recast_ms: 6000,
        })
    }
    fn instant_effect(&self, _: u32) -> bool {
        false
    }
    fn feigns(&self, _: u32) -> bool {
        false
    }
}

fn caster(dialect: crate::GameDialect, class: u32, now: Instant) -> ClientWorld {
    let mut world = ClientWorld::default();
    world.carry_as(dialect);
    let mut own = player(9);
    own.class = Some(class);
    own.memorized_spells[0] = Some(42);
    own.memorized_spells[1] = Some(43);
    for update in [
        WorldUpdate::Game(WorldEvent::Entered {
            session_id: 1,
            zone: "qeytoqrg".into(),
            player: Box::new(own),
            capabilities: vec![crate::Capability::Casting, crate::Capability::Spellbook],
            choices: vec![],
            far_clip: None,
            spell_landing_flag: None,
            zone_id: 0,
        }),
        WorldUpdate::Connection(Link::Connected),
    ] {
        world.apply(&update, now, &Catalog);
    }
    world
}

fn tell(world: &mut ClientWorld, event: WorldEvent, now: Instant) {
    world.apply(&WorldUpdate::Game(event), now, &Catalog);
    world.tick(now, &Catalog);
}

#[test]
fn confirmed_memorize_uses_local_full_recast_and_does_not_lock_other_gems_or_other_dialects() {
    let now = Instant::now();
    for dialect in [
        crate::GameDialect::Titanium,
        crate::GameDialect::EqMac,
        crate::GameDialect::Rof2,
    ] {
        let mut world = caster(dialect, 2, now);
        tell(
            &mut world,
            WorldEvent::Spell(SpellUpdate::Slot {
                slot: 0,
                spell_id: 42,
                mode: 1,
            }),
            now,
        );
        let expected = if dialect == crate::GameDialect::Titanium {
            Duration::from_secs(7)
        } else {
            Duration::ZERO
        };
        assert_eq!(world.casting().gem_wait(0, 42, None, now), Some(expected));
        assert_eq!(
            world.casting().gem_wait(1, 43, None, now),
            Some(Duration::ZERO)
        );
    }
}

#[test]
fn cast_send_context_and_server_completion_do_not_replace_native_refresh_readiness() {
    let now = Instant::now();
    let mut world = caster(crate::GameDialect::Titanium, 2, now);
    tell(
        &mut world,
        WorldEvent::CastPending {
            session_id: 1,
            spell_id: Some(42),
            gem: Some(0),
        },
        now,
    );
    assert_eq!(
        world.casting().gem_wait(1, 43, None, now),
        Some(Duration::from_secs(10))
    );
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::Began {
            caster_id: 9,
            spell_id: 42,
            duration_ms: 3000,
        }),
        now,
    );
    let ended = now + Duration::from_secs(3);
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::Mana {
            spell_id: 42,
            keep_casting: false,
        }),
        ended,
    );
    assert_eq!(
        world.casting().gem_wait(0, 42, None, ended),
        Some(Duration::ZERO)
    );
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 42,
            reduction_ms: 1000,
        }),
        ended,
    );
    assert_eq!(
        world.casting().gem_wait(0, 42, None, ended),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        world.casting().gem_wait(1, 43, None, ended),
        Some(Duration::from_millis(1500))
    );
    assert!(world.casting().cast.is_none());
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::Mana {
            spell_id: 42,
            keep_casting: false,
        }),
        ended,
    );
    assert_eq!(
        world.casting().gem_wait(0, 42, None, ended),
        Some(Duration::from_secs(5))
    );
}

#[test]
fn item_cast_of_the_same_spell_does_not_create_a_provisional_gem_lock() {
    let now = Instant::now();
    let mut world = caster(crate::GameDialect::Titanium, 2, now);
    tell(
        &mut world,
        WorldEvent::CastPending {
            session_id: 1,
            spell_id: Some(42),
            gem: None,
        },
        now,
    );
    assert_eq!(
        world.casting().gem_wait(0, 42, None, now),
        Some(Duration::ZERO)
    );
}

#[test]
fn titanium_bard_global_recovery_is_a_program_rule_without_a_server_capability_override() {
    let now = Instant::now();
    let mut world = caster(crate::GameDialect::Titanium, 8, now);
    assert!(!world.can(crate::Capability::BardRefreshTimers));
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 42,
            reduction_ms: 500,
        }),
        now,
    );
    assert_eq!(
        world.casting().gem_wait(0, 42, None, now),
        Some(Duration::from_millis(5500))
    );
    assert_eq!(
        world.casting().gem_wait(1, 43, None, now),
        Some(Duration::ZERO)
    );
}

#[test]
fn preparation_progress_is_admission_scoped_and_cancelled_status_clears_it() {
    let now = Instant::now();
    let mut world = caster(crate::GameDialect::Titanium, 2, now);
    let progress = crate::MemorizationProgress {
        gem: 0,
        spell_id: 42,
        remaining: 69,
        approximate: false,
    };
    tell(
        &mut world,
        WorldEvent::BookAction(crate::BookActionStatus::Preparing),
        now,
    );
    tell(
        &mut world,
        WorldEvent::MemorizationProgress {
            session_id: 2,
            progress: Some(progress),
        },
        now,
    );
    assert!(world.memorization_progress().is_none());
    tell(
        &mut world,
        WorldEvent::MemorizationProgress {
            session_id: 1,
            progress: Some(progress),
        },
        now,
    );
    assert_eq!(world.memorization_progress(), Some(&progress));
    tell(
        &mut world,
        WorldEvent::BookAction(crate::BookActionStatus::Cancelled("Moved".into())),
        now,
    );
    assert!(world.memorization_progress().is_none());
}
