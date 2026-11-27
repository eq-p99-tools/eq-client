use super::*;
use std::time::Duration;

struct Catalog;
impl SpellCatalog for Catalog {
    fn timing(&self, _: u32) -> Option<SpellTiming> {
        Some(SpellTiming {
            recast_ms: 10_000,
            recovery_ms: 1500,
        })
    }
    fn cast_duration(&self, _: u32) -> Option<Duration> {
        Some(Duration::from_secs(4))
    }
    fn p99_gem_start(&self) -> bool {
        true
    }
    fn instant_effect(&self, _: u32) -> bool {
        false
    }
    fn feigns(&self, _: u32) -> bool {
        false
    }
}

fn tell(world: &mut ClientWorld, event: WorldEvent, now: Instant) {
    world.apply(&WorldUpdate::Game(event), now, &Catalog);
    world.tick(now, &Catalog);
}

fn caster(now: Instant, dialect: crate::GameDialect) -> ClientWorld {
    let mut world = ClientWorld::default();
    world.carry_as(dialect);
    let mut own = player(9);
    own.class = Some(2);
    own.memorized_spells[..3].copy_from_slice(&[Some(42), Some(43), Some(42)]);
    tell(
        &mut world,
        WorldEvent::Entered {
            session_id: 1,
            zone: "qeytoqrg".into(),
            player: Box::new(own),
            capabilities: vec![crate::Capability::Casting],
            choices: vec![],
            far_clip: None,
            spell_landing_flag: None,
            zone_id: 0,
        },
        now,
    );
    world.apply(&WorldUpdate::Connection(Link::Connected), now, &Catalog);
    world
}

fn pending(world: &mut ClientWorld, spell: Option<u32>, gem: Option<u8>, now: Instant) {
    tell(
        world,
        WorldEvent::CastPending {
            session_id: 1,
            spell_id: spell,
            gem,
        },
        now,
    );
}

fn begin(world: &mut ClientWorld, spell: u16, now: Instant) {
    tell(
        world,
        WorldEvent::Spell(SpellUpdate::Began {
            caster_id: 9,
            spell_id: spell,
            duration_ms: 4000,
        }),
        now,
    );
}

fn success(world: &mut ClientWorld, now: Instant) {
    let start = now.checked_sub(Duration::from_secs(4)).unwrap();
    pending(world, Some(42), Some(0), start);
    begin(world, 42, start);
    pending(world, None, None, now);
    tell(
        world,
        WorldEvent::Spell(SpellUpdate::Mana {
            spell_id: 42,
            keep_casting: false,
        }),
        now,
    );
}

#[test]
fn p99_repeat_10s_reuse_4s_cast_opens_at_6s_for_paint_click_and_duplicate_slots() {
    let start = Instant::now();
    let completed = start + Duration::from_secs(4);
    let mut world = caster(start, crate::GameDialect::Titanium);
    success(&mut world, completed);
    // A delayed refresh describes the same success, not a new time origin.
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 42,
            reduction_ms: 0,
        }),
        completed + Duration::from_millis(250),
    );
    for (millis, expected) in [(5999, 1), (6000, 0), (6001, 0)] {
        let now = completed + Duration::from_millis(millis);
        for gem in [0, 2] {
            assert_eq!(
                world.casting().gem_wait(gem, 42, None, now),
                Some(Duration::from_millis(expected))
            );
            assert_eq!(
                world.casting().gem_cast_wait(gem, 42, None, now),
                Some(Duration::from_millis(expected))
            );
        }
        assert_eq!(
            world.casting().gem_wait(1, 43, None, now),
            Some(Duration::ZERO)
        );
    }
}

#[test]
fn p99_b_has_no_provisional_or_global_recovery_but_real_pending_and_cast_ownership_remain() {
    let start = Instant::now();
    let mut world = caster(start, crate::GameDialect::Titanium);
    pending(&mut world, Some(42), Some(0), start);
    assert!(world.casting().holds_actions());
    assert_eq!(
        world.casting().gem_wait(1, 43, None, start),
        Some(Duration::ZERO)
    );
    begin(&mut world, 42, start);
    assert!(world.casting().holds_actions());
    assert_eq!(
        world.casting().gem_wait(1, 43, None, start),
        Some(Duration::ZERO)
    );
    let completed = start + Duration::from_secs(4);
    pending(&mut world, None, None, completed);
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::Mana {
            spell_id: 42,
            keep_casting: false,
        }),
        completed,
    );
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 42,
            reduction_ms: 0,
        }),
        completed,
    );
    assert!(!world.casting().holds_actions());
    assert_eq!(
        world.casting().gem_cast_wait(1, 43, None, completed),
        Some(Duration::ZERO)
    );
    assert_eq!(
        world.casting().gem_cast_wait(0, 42, None, completed),
        Some(Duration::from_secs(6))
    );
}

#[test]
fn p99_early_queries_and_denied_attempts_do_not_restart_a_confirmed_deadline() {
    let start = Instant::now();
    let completed = start + Duration::from_secs(4);
    let mut world = caster(start, crate::GameDialect::Titanium);
    success(&mut world, completed);
    let early = completed + Duration::from_millis(5999);
    for _ in 0..10 {
        assert_eq!(
            world.casting().gem_cast_wait(0, 42, None, early),
            Some(Duration::from_millis(1))
        );
    }
    // A rejected submitted attempt still must not replace the prior reuse.
    pending(&mut world, Some(42), Some(0), early);
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::Interrupted {
            caster_id: 9,
            caster_name: None,
            message_id: 173,
        }),
        early,
    );
    pending(&mut world, None, None, early);
    assert_eq!(
        world
            .casting()
            .gem_cast_wait(0, 42, None, completed + Duration::from_secs(6)),
        Some(Duration::ZERO)
    );
}

#[test]
fn p99_policy_does_not_change_other_dialects_or_ignore_received_profile_reuse() {
    let now = Instant::now();
    for dialect in [crate::GameDialect::EqMac, crate::GameDialect::Rof2] {
        let world = caster(now, dialect);
        assert!(!world.casting().p99_gem_start());
    }
    let mut state = super::super::p99_gems::Gems::restore(&[Some(42); 8], Some([10_000; 8]), now);
    state.resolve(&Catalog, true);
    assert_eq!(
        state.wait(42, now + Duration::from_millis(5999)),
        Some(Duration::from_millis(1))
    );
    assert_eq!(
        state.wait(42, now + Duration::from_secs(6)),
        Some(Duration::ZERO)
    );
}

#[test]
fn p99_raw_refresh_credit_is_not_counted_again_as_cast_time() {
    let now = Instant::now();
    let mut world = caster(now, crate::GameDialect::Titanium);
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 42,
            reduction_ms: 4000,
        }),
        now,
    );
    assert_eq!(
        world.casting().gem_cast_wait(0, 42, None, now),
        Some(Duration::from_secs(6))
    );
    // Raw field semantics remain unverified: do not turn 4 s into 8 s credit.
}

#[test]
fn p99_received_shared_category_is_a_real_gate_without_stock_global_recovery() {
    let now = Instant::now();
    let mut world = caster(now, crate::GameDialect::Titanium);
    tell(
        &mut world,
        WorldEvent::LinkedReuse(crate::disciplines::Reuse {
            timer: 7,
            remaining: Duration::from_secs(2),
        }),
        now,
    );
    assert_eq!(
        world.casting().gem_cast_wait(1, 43, Some(7), now),
        Some(Duration::from_secs(2))
    );
    assert_eq!(
        world.casting().gem_wait(1, 43, Some(7), now),
        Some(Duration::from_secs(2))
    );
    assert_eq!(
        world
            .casting()
            .gem_cast_wait(1, 43, Some(7), now + Duration::from_secs(2)),
        Some(Duration::ZERO)
    );
    assert_eq!(
        world.casting().gem_cast_wait(0, 42, None, now),
        Some(Duration::ZERO)
    );
}

#[test]
fn p99_pre_begin_fizzle_mana_does_not_create_or_extend_reuse() {
    let now = Instant::now();
    let mut world = caster(now, crate::GameDialect::Titanium);
    pending(&mut world, Some(42), Some(0), now);
    pending(&mut world, None, None, now);
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::Mana {
            spell_id: 42,
            keep_casting: false,
        }),
        now,
    );
    assert_eq!(
        world.casting().gem_cast_wait(0, 42, None, now),
        Some(Duration::ZERO)
    );
}

#[test]
fn p99_later_table_changes_do_not_redefine_the_established_reuse_deadline() {
    struct Changed;
    impl SpellCatalog for Changed {
        fn timing(&self, _: u32) -> Option<SpellTiming> {
            Some(SpellTiming {
                recast_ms: 20_000,
                recovery_ms: 9000,
            })
        }
        fn p99_gem_start(&self) -> bool {
            true
        }
        fn cast_duration(&self, _: u32) -> Option<Duration> {
            Some(Duration::from_secs(4))
        }
        fn instant_effect(&self, _: u32) -> bool {
            false
        }
        fn feigns(&self, _: u32) -> bool {
            false
        }
    }
    let start = Instant::now();
    let at = start + Duration::from_secs(4);
    let mut world = caster(start, crate::GameDialect::Titanium);
    success(&mut world, at);
    world.tick(at + Duration::from_secs(1), &Changed);
    assert_eq!(
        world
            .casting()
            .gem_cast_wait(0, 42, None, at + Duration::from_secs(6)),
        Some(Duration::ZERO)
    );
    world.apply(
        &WorldUpdate::Game(WorldEvent::Spell(SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 42,
            reduction_ms: 0,
        })),
        at + Duration::from_secs(2),
        &Changed,
    );
    world.tick(at + Duration::from_secs(2), &Changed);
    assert_eq!(
        world
            .casting()
            .gem_cast_wait(0, 42, None, at + Duration::from_secs(6)),
        Some(Duration::ZERO)
    );
}

#[test]
fn p99_received_effective_cast_duration_overrides_the_base_and_credit_cannot_underflow() {
    let now = Instant::now();
    let at = now + Duration::from_secs(2);
    let mut world = caster(now, crate::GameDialect::Titanium);
    pending(&mut world, Some(42), Some(0), now);
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::Began {
            caster_id: 9,
            spell_id: 42,
            duration_ms: 2000,
        }),
        now,
    );
    pending(&mut world, None, None, at);
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::Mana {
            spell_id: 42,
            keep_casting: false,
        }),
        at,
    );
    assert_eq!(
        world
            .casting()
            .gem_cast_wait(0, 42, None, at + Duration::from_secs(6)),
        Some(Duration::from_secs(2))
    );
    assert_eq!(
        world
            .casting()
            .gem_cast_wait(0, 42, None, at + Duration::from_secs(8)),
        Some(Duration::ZERO)
    );
    tell(
        &mut world,
        WorldEvent::Spell(SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 42,
            reduction_ms: u32::MAX,
        }),
        at,
    );
    assert_eq!(
        world.casting().gem_cast_wait(0, 42, None, at),
        Some(Duration::ZERO)
    );
}
