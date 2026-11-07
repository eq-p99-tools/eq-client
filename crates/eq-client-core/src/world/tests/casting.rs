use super::*;

fn pending(session_id: u64, spell_id: Option<u32>) -> WorldEvent {
    WorldEvent::CastPending {
        session_id,
        spell_id,
    }
}

fn unconfirmed(session_id: u64, spell_id: u32) -> WorldEvent {
    WorldEvent::CastRequestUnconfirmed {
        session_id,
        spell_id,
    }
}

fn begin(caster_id: u16, spell_id: u16) -> WorldEvent {
    WorldEvent::Spell(SpellUpdate::Began {
        caster_id,
        spell_id,
        duration_ms: 3000,
    })
}

fn mana(spell_id: u32) -> WorldEvent {
    WorldEvent::Spell(SpellUpdate::Mana {
        spell_id,
        keep_casting: false,
    })
}

fn gem(slot: u32, spell_id: u32) -> WorldEvent {
    WorldEvent::Spell(SpellUpdate::Slot {
        slot,
        spell_id,
        mode: 1,
    })
}

fn refresh(slot: u32, spell_id: u32) -> WorldEvent {
    WorldEvent::Spell(SpellUpdate::BarRefresh {
        slot,
        spell_id,
        reduction_ms: 0,
    })
}

#[test]
fn requested_spell_ends_its_aliased_cast_in_either_notice_order() {
    for begin_first in [false, true] {
        for result in [mana(42), refresh(0, 42)] {
            let mut world = admitted();
            game(&mut world, gem(0, 42));
            game(&mut world, pending(1, Some(42)));
            assert_eq!(game(&mut world, begin(5, 2935)).cast, None);
            assert_eq!(world.casting().pending, Some(42));
            let began = begin(9, 2935);
            let cleared = pending(1, None);
            let notices = if begin_first {
                [began, cleared]
            } else {
                [cleared, began]
            };
            for notice in notices {
                game(&mut world, notice);
            }
            assert_eq!(world.casting().pending, None);
            assert_eq!(world.casting().cast.map(|(spell, ..)| spell), Some(2935));
            assert_eq!(game(&mut world, mana(43)).cast, None);
            assert!(world.zone.casts.contains_key(&9));

            assert!(matches!(
                game(&mut world, result).cast,
                Some(CastNews::Ended | CastNews::Refreshed)
            ));
            assert_eq!(world.casting().cast, None);
            assert!(!world.zone.casts.contains_key(&9));
            assert!(world.completed_cast_of(9).is_none());
        }
    }
}

#[test]
fn late_prior_results_do_not_clear_a_new_pending_request_or_aliased_cast() {
    let mut world = admitted();
    game(&mut world, gem(0, 42));
    game(&mut world, pending(1, Some(42)));
    game(&mut world, pending(1, None));
    game(&mut world, begin(9, 2935));
    game(&mut world, pending(1, Some(43)));

    assert_eq!(game(&mut world, mana(42)).cast, Some(CastNews::Ended));
    assert_eq!(world.casting().pending, Some(43));

    game(&mut world, begin(9, 2936));
    game(&mut world, pending(1, None));
    game(&mut world, begin(9, 2936));
    assert_eq!(game(&mut world, mana(42)).cast, None);
    assert_eq!(
        game(&mut world, refresh(0, 42)).cast,
        Some(CastNews::Refreshed)
    );
    assert_eq!(world.casting().cast.map(|(spell, ..)| spell), Some(2936));
    assert_eq!(
        world.zone.casts.get(&9).map(|cast| cast.spell_id),
        Some(2936)
    );

    assert_eq!(game(&mut world, mana(43)).cast, Some(CastNews::Ended));
    assert_eq!(world.casting().cast, None);
    assert!(!world.zone.casts.contains_key(&9));
}

#[test]
fn a_new_admission_drops_active_and_released_aliases() {
    let mut world = admitted();
    game(&mut world, pending(1, Some(42)));
    game(&mut world, pending(1, None));
    game(&mut world, begin(9, 2935));
    game(&mut world, pending(1, Some(43)));
    game(&mut world, pending(1, None));

    game(&mut world, entered(2));
    connection(&mut world, true, false);
    assert_eq!(world.casting().cast, None);
    assert!(game(&mut world, pending(1, Some(42))).ignored);
    assert!(game(&mut world, pending(1, None)).ignored);

    game(&mut world, begin(9, 2936));
    for spell in [42, 43] {
        assert_eq!(game(&mut world, mana(spell)).cast, None);
    }
    assert_eq!(world.casting().cast.map(|(spell, ..)| spell), Some(2936));
    assert_eq!(game(&mut world, mana(2936)).cast, Some(CastNews::Ended));
}

#[test]
fn an_unanswered_request_does_not_alias_a_later_own_begin() {
    let mut world = admitted();
    game(&mut world, pending(1, Some(42)));
    game(
        &mut world,
        WorldEvent::BookAction(crate::BookActionStatus::AwaitingReply),
    );
    assert!(game(&mut world, unconfirmed(1, 43)).ignored);
    assert_eq!(world.casting().pending, Some(42));

    let timeout = game(&mut world, unconfirmed(1, 42));
    assert!(!timeout.ignored);
    assert_eq!(timeout.cast, None);
    assert_eq!(timeout.notices, []);
    assert_eq!(world.casting().pending, None);
    assert!(matches!(
        world.book_action(),
        Some(crate::BookActionStatus::AwaitingReply)
    ));

    game(&mut world, begin(9, 2935));
    assert_eq!(game(&mut world, mana(42)).cast, None);
    assert_eq!(world.casting().cast.map(|(spell, ..)| spell), Some(2935));
    assert!(world.completed_cast_of(9).is_none());
    assert_eq!(game(&mut world, mana(2935)).cast, Some(CastNews::Ended));
}

#[test]
fn an_unconfirmed_notice_cannot_release_a_begun_or_new_admission_cast() {
    let mut world = admitted();
    game(&mut world, pending(1, Some(42)));
    game(&mut world, unconfirmed(1, 42));
    game(&mut world, pending(1, Some(42)));
    game(&mut world, begin(9, 2935));
    assert!(game(&mut world, unconfirmed(1, 42)).ignored);
    assert_eq!(world.casting().cast.map(|(spell, ..)| spell), Some(2935));
    assert_eq!(game(&mut world, mana(42)).cast, Some(CastNews::Ended));

    game(&mut world, entered(2));
    connection(&mut world, true, false);
    game(&mut world, pending(2, Some(42)));
    assert!(game(&mut world, unconfirmed(1, 42)).ignored);
    assert_eq!(world.casting().pending, Some(42));
    game(&mut world, pending(2, None));
    game(&mut world, begin(9, 2935));
    assert_eq!(game(&mut world, mana(42)).cast, Some(CastNews::Ended));
}
