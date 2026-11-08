use super::*;
use crate::buffs::cancel::{Progress, Update};

fn report(session_id: u64, progress: Progress) -> WorldEvent {
    WorldEvent::BuffCancellation(Update {
        session_id,
        revision: 1,
        slot: 3,
        spell_id: 42,
        progress,
    })
}

#[test]
fn staged_discipline_state_leaves_a_profile_buff_cancellable() {
    use crate::{buffs::cancel::Request, disciplines::Active};

    for state in [Active::Titanium(4499), Active::Inactive] {
        let mut world = admitted();
        let staged = |session_id| WorldEvent::DisciplineAdmission { session_id, state };
        assert!(game(&mut world, staged(2)).ignored);
        assert_eq!(world.casting().active_discipline(), Active::Unknown);
        game(&mut world, staged(1));
        assert_eq!(world.casting().active_discipline(), state);
        assert_eq!(world.buffs().revision(), 0);

        let ordinary = crate::Buff {
            spell_id: 42,
            ..buff(30)
        };
        game(
            &mut world,
            WorldEvent::BuffSnapshot(vec![None, Some(ordinary)]),
        );
        assert_eq!(world.casting().active_discipline(), state);
        let request = Request {
            session_id: 1,
            revision: world.buffs().revision(),
            slot: 1,
            spell_id: 42,
            beneficial: Some(true),
            cancellable: Some(true),
            created: Instant::now(),
        };
        request.check(1, world.buffs().slots()).unwrap();
    }
}

#[test]
fn cancellation_reports_are_scoped_progress_and_never_membership_authority() {
    let mut world = admitted();
    let buff = crate::Buff {
        spell_id: 42,
        caster_level: 12,
        effect_type: 2,
        bard_modifier: 10,
        duration_ticks: 30,
        counters: 0,
        caster_id: 7,
    };
    game(
        &mut world,
        WorldEvent::BuffSnapshot(vec![None, None, None, Some(buff.clone())]),
    );
    assert!(game(&mut world, report(2, Progress::Submitted)).ignored);
    assert!(world.buffs().cancellation().is_none());
    for progress in [
        Progress::Submitted,
        Progress::Changed,
        Progress::FadeObserved,
        Progress::Unresolved,
        Progress::Refused("Constructed refusal".into()),
    ] {
        let news = game(&mut world, report(1, progress.clone()));
        assert!(
            matches!(news.notices.as_slice(), [Notice::BuffCancellation(update)] if update.progress == progress)
        );
        assert_eq!(world.buffs().slots().unwrap()[&3], buff);
        assert_eq!(world.buffs().revision(), 1);
        assert_eq!(news.replies, Vec::<crate::world::Reply>::new());
    }
    game(&mut world, WorldEvent::DeathBind { session_id: 1 });
    assert!(world.buffs().cancellation().is_none());
    assert!(game(&mut world, report(1, Progress::Submitted)).ignored);
    game(&mut world, entered(3));
    assert!(world.buffs().cancellation().is_none());
    assert!(game(&mut world, report(1, Progress::Unresolved)).ignored);
}
