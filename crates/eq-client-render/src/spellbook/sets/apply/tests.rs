use super::*;
use crate::{
    online::{OnlineState, testing},
    spellbook::SpellNames,
};
use eq_client_core::{SpellUpdate, WorldEvent};
use std::sync::mpsc::{self, Receiver};

struct Harness {
    online: OnlineState,
    outbox: Outbox,
    sent: Receiver<ClientCommand>,
    now: Instant,
}

impl Harness {
    fn new(gems: Gems) -> Self {
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.memorized_spells = gems;
        testing::admit(&mut online, 9, player);
        let mut book = SpellBook::default();
        for (slot, spell_id) in [42, 73, 99].into_iter().enumerate() {
            book.apply(&SpellUpdate::Slot {
                slot: u32::try_from(slot).unwrap(),
                spell_id,
                mode: 0,
            });
        }
        testing::book(&mut online, book);
        let (queue, sent) = mpsc::sync_channel(4);
        Self {
            online,
            outbox: Outbox::new(Some(queue)),
            sent,
            now: Instant::now(),
        }
    }

    fn start(&self, target: Gems) -> anyhow::Result<Apply> {
        Apply::start("Travel".into(), target, self.online.world(), &self.outbox)
    }

    fn tick(&self, active: &mut Apply) -> Option<String> {
        let result = active.tick(
            self.online.world(),
            &self.outbox,
            None,
            &SpellNames::default(),
            self.now,
        );
        if result.is_some() {
            self.outbox.end_spell_set();
        }
        result
    }

    fn status(&mut self, status: BookActionStatus) {
        testing::book_news(
            &mut self.online,
            &self.outbox,
            WorldEvent::BookAction(status),
        );
    }

    fn update(&mut self, gem: u8, id: u32, mode: u32) {
        testing::book_news(
            &mut self.online,
            &self.outbox,
            WorldEvent::Spell(SpellUpdate::Slot {
                slot: u32::from(gem),
                spell_id: id,
                mode,
            }),
        );
    }

    fn forget(&mut self, gem: u8, id: u32) {
        self.status(BookActionStatus::Submitted);
        self.update(gem, id, 2);
    }

    fn memorize(&mut self, gem: u8, id: u32) {
        self.update(gem, id, 1);
        self.status(BookActionStatus::Confirmed);
    }

    fn expect(&self, step: Step) {
        match (step, self.sent.try_recv().unwrap()) {
            (
                Step::Forget(gem, spell),
                ClientCommand::ForgetSpell {
                    session_id: 9,
                    gem: slot,
                    spell_id,
                    ..
                },
            )
            | (
                Step::Memorize(gem, spell),
                ClientCommand::MemorizeSpell {
                    session_id: 9,
                    gem: slot,
                    spell_id,
                    ..
                },
            ) => assert_eq!((gem, spell), (slot, spell_id)),
            (_, other) => panic!("Unexpected command {other:?}"),
        }
        self.quiet();
    }

    fn quiet(&self) {
        assert!(matches!(
            self.sent.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }
}

fn layout(first: Option<u32>, second: Option<u32>, third: Option<u32>) -> Gems {
    [first, second, third, None, None, None, None, Some(99)]
}

#[test]
fn swaps_slots_and_deliberately_clears_empties_one_confirmed_step_at_a_time() {
    let mut h = Harness::new(layout(Some(42), Some(73), Some(42)));
    let mut active = h.start(layout(Some(73), Some(42), None)).unwrap();
    assert_eq!(active.progress(), "5/8 match");
    assert_eq!(h.tick(&mut active), None);
    h.expect(Step::Forget(1, 73));
    h.status(BookActionStatus::Submitted);
    h.update(5, 42, 2); // A different, already-empty gem cannot advance the set.
    assert_eq!(h.tick(&mut active), None);
    assert!(active.progress().starts_with("5/8 match"));
    h.quiet();
    h.update(1, 73, 2);
    assert_eq!(h.tick(&mut active), None);
    h.expect(Step::Forget(0, 42));
    h.forget(0, 42);
    assert_eq!(h.tick(&mut active), None);
    h.expect(Step::Memorize(0, 73));
    h.status(BookActionStatus::Preparing);
    assert_eq!(h.tick(&mut active), None);
    h.quiet();
    h.status(BookActionStatus::AwaitingReply);
    h.update(0, 73, 1);
    assert_eq!(h.tick(&mut active), None);
    h.quiet();
    h.status(BookActionStatus::Confirmed);
    assert_eq!(h.tick(&mut active), None);
    h.expect(Step::Forget(2, 42));
    h.forget(2, 42);
    assert_eq!(h.tick(&mut active), None);
    h.expect(Step::Memorize(1, 42));
    h.memorize(1, 42);
    assert_eq!(
        h.tick(&mut active).as_deref(),
        Some("Applied Travel · 8/8 gems match")
    );
    h.quiet();
}

#[test]
fn preflights_unlearned_spells_before_clearing_gems_and_skips_a_matching_layout() {
    let current = layout(Some(42), None, None);
    let h = Harness::new(current);
    let error = h.start(layout(None, Some(115), None)).err().unwrap();
    assert!(error.to_string().contains("Spell #115 for gem 2"));
    let mut active = h.start(current).unwrap();
    assert!(h.tick(&mut active).unwrap().contains("8/8"));
    h.quiet();
}

#[test]
fn cancellation_stops_the_sequence_but_counts_an_in_flight_result() {
    let mut h = Harness::new(layout(Some(42), None, None));
    let mut active = h.start(layout(None, Some(73), None)).unwrap();
    assert_eq!(h.tick(&mut active), None);
    h.expect(Step::Forget(0, 42));
    active.stop("Cancelled.");
    assert!(h.tick(&mut active).is_none());
    assert!(active.progress().starts_with("Stopping · 6/8 match"));
    let error = h.outbox.post(h.online.world(), |stamp| {
        Step::Memorize(4, 99).command(stamp)
    });
    assert_eq!(error, Err(crate::outbox::Refusal::SpellSet));
    h.forget(0, 42);
    assert_eq!(
        h.tick(&mut active).as_deref(),
        Some("Travel stopped. 7/8 match. Cancelled.")
    );
    h.quiet();
    // Reapply plans from the partial authoritative layout.
    let mut retry = h.start(layout(None, Some(73), None)).unwrap();
    assert_eq!(h.tick(&mut retry), None);
    h.expect(Step::Memorize(1, 73));
}

#[test]
fn refusal_timeout_and_resets_end_without_another_mutation() {
    for case in ["refusal", "silence", "reset", "reentry"] {
        let mut h = Harness::new(layout(None, None, None));
        let mut active = h.start(layout(Some(42), Some(73), None)).unwrap();
        assert_eq!(h.tick(&mut active), None);
        h.expect(Step::Memorize(0, 42));
        h.memorize(0, 42);
        assert_eq!(h.tick(&mut active), None);
        h.expect(Step::Memorize(1, 73));
        let reason = match case {
            "refusal" => {
                h.status(BookActionStatus::Rejected("Server refused".into()));
                "Server refused"
            }
            "silence" => {
                h.now += STEP_TIMEOUT;
                "No confirmation for gem 2."
            }
            "reset" => {
                active.invalidate(None);
                "The admission ended."
            }
            _ => {
                testing::admit(&mut h.online, 10, testing::player(7));
                "The admission ended."
            }
        };
        assert_eq!(
            h.tick(&mut active).unwrap(),
            format!("Travel stopped. 7/8 match. {reason}")
        );
        h.quiet();
    }
}

#[test]
fn rechecks_the_book_before_each_step() {
    let mut h = Harness::new(layout(Some(42), None, None));
    let mut active = h.start(layout(Some(73), None, None)).unwrap();
    assert_eq!(h.tick(&mut active), None);
    h.expect(Step::Forget(0, 42));
    h.forget(0, 42);
    testing::book(&mut h.online, SpellBook::default());
    assert_eq!(
        h.tick(&mut active).as_deref(),
        Some("Travel stopped. 7/8 match. Spell #73 for gem 1 is no longer learned")
    );
    h.quiet();
}

#[test]
fn stops_on_player_interruption_even_when_the_last_step_succeeded() {
    let mut h = Harness::new(layout(Some(42), None, None));
    let mut active = h.start(layout(None, Some(73), None)).unwrap();
    assert_eq!(h.tick(&mut active), None);
    h.expect(Step::Forget(0, 42));
    h.outbox
        .post(h.online.world(), |stamp| ClientCommand::SetPosture {
            session_id: stamp.session_id,
            spawn_id: 7,
            posture: eq_client_core::Posture::Standing,
            created: stamp.created,
        })
        .unwrap();
    assert!(matches!(
        h.sent.try_recv(),
        Ok(ClientCommand::SetPosture { .. })
    ));
    h.forget(0, 42);
    assert_eq!(
        h.tick(&mut active).as_deref(),
        Some("Travel stopped. 7/8 match. Character actions were interrupted.")
    );
    h.quiet();
}

#[test]
fn manual_forgets_serialize_and_apply_waits_for_the_exact_gem() {
    use crate::outbox::Refusal;
    let mut h = Harness::new(layout(Some(42), Some(73), None));
    let target = layout(None, None, Some(42));
    h.outbox
        .post(h.online.world(), |stamp| Step::Forget(0, 42).command(stamp))
        .unwrap();
    h.expect(Step::Forget(0, 42));
    let second_forget = |h: &Harness| {
        h.outbox
            .post(h.online.world(), |stamp| Step::Forget(1, 73).command(stamp))
    };
    assert_eq!(second_forget(&h), Err(Refusal::BookPending));
    assert_eq!(h.outbox.take_refused(), [Refusal::BookPending]);
    assert!(h.start(target).is_err());
    h.forget(0, 42);
    second_forget(&h).unwrap();
    h.expect(Step::Forget(1, 73));
    h.status(BookActionStatus::Submitted);
    h.update(0, 42, 2); // Delayed/repeated update for the first gem.
    assert_eq!(h.online.world().book_action(), None);
    assert!(h.start(target).is_err());
    h.quiet();
    h.update(1, 73, 2);
    let mut active = h.start(target).unwrap();
    assert_eq!(h.tick(&mut active), None);
    h.expect(Step::Memorize(2, 42));
}

#[test]
fn an_expired_forget_still_blocks_edits_until_the_late_reply_or_admission() {
    use crate::outbox::Refusal;
    use std::time::Duration;
    for new_admission in [false, true] {
        let mut h = Harness::new(layout(Some(42), Some(73), None));
        let target = layout(None, None, Some(42));
        h.outbox
            .post(h.online.world(), |stamp| Step::Forget(0, 42).command(stamp))
            .unwrap();
        h.expect(Step::Forget(0, 42));
        h.status(BookActionStatus::Submitted);
        if !new_admission {
            h.update(3, 42, 2);
        }
        assert_eq!(
            h.outbox.begin_spell_set(
                h.online.world(),
                h.now + STEP_TIMEOUT + Duration::from_secs(1)
            ),
            Err(Refusal::BookUnconfirmed),
        );
        h.quiet();
        if new_admission {
            let book = h.online.world().spell_book().unwrap().clone();
            testing::admit(&mut h.online, 10, testing::player(7));
            testing::book(&mut h.online, book);
        } else {
            h.update(0, 42, 2);
        }
        assert!(h.start(target).is_ok());
    }
}

#[test]
fn a_manual_rejection_releases_the_queue_but_confirmation_waits_for_its_effect() {
    let mut h = Harness::new(layout(Some(42), None, None));
    let target = layout(None, Some(73), None);
    h.outbox
        .post(h.online.world(), |stamp| Step::Forget(0, 42).command(stamp))
        .unwrap();
    h.expect(Step::Forget(0, 42));
    h.status(BookActionStatus::Rejected("Busy".into()));
    h.outbox
        .post(h.online.world(), |stamp| {
            Step::Memorize(1, 73).command(stamp)
        })
        .unwrap();
    h.expect(Step::Memorize(1, 73));
    h.status(BookActionStatus::Confirmed);
    h.update(1, 42, 1);
    assert!(h.start(target).is_err());
    h.update(1, 73, 1);
    let mut active = h.start(target).unwrap();
    assert_eq!(h.tick(&mut active), None);
    h.expect(Step::Forget(0, 42));
}
