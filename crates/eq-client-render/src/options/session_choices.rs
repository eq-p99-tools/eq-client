//! Runtime option synchronization with the current admission's worker.
use bevy::prelude::*;
use eq_client_core::{
    Capability, ClientCommand, movement::MovementOptions, qol::Fix, world::ClientWorld,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

// The worker refuses a queued setting after one second. Leave time to hear
// that result before resubmitting an absolute policy on the same command queue.
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// Tracks local worker application separately from successful queue admission.
#[derive(Resource, Default)]
pub(crate) struct MovementSync {
    session: Option<u64>,
    wanted: Option<MovementOptions>,
    applied: Option<MovementOptions>,
    conservative: Option<MovementOptions>,
    pending: Option<Pending>,
    next_id: u64,
    retry_after: Option<Instant>,
    last_settlement: Option<Settlement>,
}

struct Pending {
    request_id: u64,
    options: MovementOptions,
    submitted: Instant,
}

struct Settlement {
    request_id: u64,
    observed: Instant,
    status: SettlementStatus,
}

enum SettlementStatus {
    Applied,
    WorkerRefused,
    UnexpectedPolicy,
    ReplyTimeout,
}

impl SettlementStatus {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::WorkerRefused => "worker_refused",
            Self::UnexpectedPolicy => "unexpected_policy",
            Self::ReplyTimeout => "reply_timeout",
        }
    }
}

impl MovementSync {
    fn common_policy(a: MovementOptions, b: MovementOptions) -> Option<MovementOptions> {
        a.same_permissions(b).then_some(MovementOptions {
            run_speed_multiplier: a.run_speed_multiplier.min(b.run_speed_multiplier),
            ..a
        })
    }

    /// A move after the pending option command can reach the worker before
    /// its reply reaches us. Use a pace valid under either worker policy.
    /// Permission changes still require confirmation before motion resumes.
    pub(crate) fn usable(
        &self,
        session: Option<u64>,
        wanted: MovementOptions,
    ) -> Option<MovementOptions> {
        if self.session != session {
            return None;
        }
        let policy = Self::common_policy(self.applied.or(self.conservative)?, wanted)?;
        match &self.pending {
            Some(pending) => Self::common_policy(policy, pending.options),
            None => Some(policy),
        }
    }

    /// Forgets the policy when a world reset can discard the pending reply.
    pub(crate) fn invalidate(&mut self) {
        // A resumed admission may still have the worker's previous choices.
        // Reestablish the actual policy even if the user wants the defaults.
        self.wanted = None;
        self.applied = None;
        self.conservative = None;
        self.pending = None;
        self.retry_after = None;
        self.last_settlement = None;
    }

    fn tell(
        &mut self,
        world: &ClientWorld,
        outbox: &crate::outbox::Outbox,
        wanted: MovementOptions,
        now: Instant,
    ) {
        let session = world.session_id();
        // Each worker starts at the defaults. A departure policy is fixed
        // for the admission, so a worker that withholds it stays at the
        // defaults even when a refused transfer resumes the same admission.
        if self.session != session
            || (self.applied.is_none() && !world.can(Capability::GameplayDepartures))
        {
            self.invalidate();
            self.session = session;
            self.applied = Some(MovementOptions::default());
        }
        if self.wanted != Some(wanted) {
            self.wanted = Some(wanted);
            self.retry_after = None;
        }
        self.settle(world, wanted, now);
        if self.pending.is_some()
            || self.applied == Some(wanted)
            || self.retry_after.is_some_and(|due| now < due)
        {
            return;
        }
        let Some(session_id) = session else { return };
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let request_id = self.next_id;
        if outbox.tell(
            world,
            ClientCommand::SetMovementOptions {
                session_id,
                request_id,
                options: wanted,
                created: now,
            },
        ) {
            self.pending = Some(Pending {
                request_id,
                options: wanted,
                submitted: now,
            });
            self.retry_after = None;
        }
    }

    fn settle(&mut self, world: &ClientWorld, wanted: MovementOptions, now: Instant) {
        let Some(pending) = &self.pending else {
            return;
        };
        let status = if let Some((_, result)) = world
            .movement_options_result()
            .filter(|(id, _)| *id == pending.request_id)
        {
            match result {
                Ok(options) if *options == pending.options => {
                    self.applied = Some(*options);
                    self.conservative = None;
                    SettlementStatus::Applied
                }
                _ => {
                    if pending.options == wanted {
                        self.retry_after = Some(now + Duration::from_secs(1));
                    }
                    if result.is_ok() {
                        self.applied = None;
                        self.conservative = None;
                        SettlementStatus::UnexpectedPolicy
                    } else {
                        SettlementStatus::WorkerRefused
                    }
                }
            }
        } else if now.saturating_duration_since(pending.submitted) >= REPLY_TIMEOUT {
            // The worker may already be using the unanswered choice. Keep
            // only the pace known safe under every possible policy. A
            // permission difference has no common policy and holds motion.
            self.conservative = self
                .applied
                .or(self.conservative)
                .and_then(|previous| Self::common_policy(previous, pending.options));
            self.applied = None;
            SettlementStatus::ReplyTimeout
        } else {
            return;
        };
        self.last_settlement = Some(Settlement {
            request_id: pending.request_id,
            observed: now,
            status,
        });
        self.pending = None;
    }

    /// Bounded renderer observations. A settlement age measures when the
    /// renderer consumed a reply or expired a request, without guessing when
    /// the worker acted. Worker feedback is reported separately.
    pub(crate) fn report(
        &self,
        session: Option<u64>,
        wanted: MovementOptions,
        now: Instant,
    ) -> Value {
        let age_ms = |instant| now.saturating_duration_since(instant).as_millis();
        let usable = self.usable(session, wanted);
        json!({
            "admission_id": session,
            "sync_admission_id": self.session,
            "wanted": wanted,
            "applied": self.applied,
            "conservative": self.conservative,
            "usable": usable,
            "waiting_for_policy": session.is_some() && usable.is_none(),
            "pending": self.pending.as_ref().map(|pending| json!({
                "request_id": pending.request_id,
                "options": pending.options,
                "age_ms": age_ms(pending.submitted),
            })),
            "last_settlement": self.last_settlement.as_ref().map(|settlement| json!({
                "request_id": settlement.request_id,
                "status": settlement.status.as_str(),
                "age_ms": age_ms(settlement.observed),
            })),
            "retry_in_ms": self.retry_after.map(|due| due.saturating_duration_since(now).as_millis()),
        })
    }
}

#[derive(Default)]
pub(crate) struct Told {
    book: Option<(u64, bool)>,
    polls: Option<(u64, bool)>,
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn tell(
    state: Res<super::OptionsState>,
    online: Res<crate::online::OnlineState>,
    outbox: Res<crate::outbox::Outbox>,
    mut movement_sync: ResMut<MovementSync>,
    mut told: Local<Told>,
) {
    let world = online.world();
    let Some(session_id) = world.session_id().filter(|_| world.connected()) else {
        movement_sync.invalidate();
        *told = Told::default();
        return;
    };
    // Where the session offers no departures the official movement is wanted,
    // which every admission starts with, so nothing is sent and input is
    // never held waiting for a choice the server type refuses.
    let movement = eq_client_core::movement::offered_options(&state.options.qol, world);
    movement_sync.tell(world, &outbox, movement, Instant::now());
    let memorize_without_sitting = state.options.qol.on(Fix::MemorizeWhileStanding);
    if world.can(Capability::GameplayDepartures)
        && told.book != Some((session_id, memorize_without_sitting))
        && outbox.tell(
            world,
            ClientCommand::SetBookOptions {
                session_id,
                memorize_without_sitting,
            },
        )
    {
        told.book = Some((session_id, memorize_without_sitting));
    }
    let enabled = state.options.qol.on(Fix::AnswerListPolls);
    if world.can(Capability::AnsweringListPolls)
        && told.polls != Some((session_id, enabled))
        && outbox.tell(
            world,
            ClientCommand::AnswerListPolls {
                session_id,
                enabled,
            },
        )
    {
        told.polls = Some((session_id, enabled));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::{WorldEvent, WorldUpdate, ZoneOffer, ZoneRejection, options::Level};
    use std::sync::{Mutex, mpsc};

    fn set_run_speed(app: &mut App, percent: u16) {
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .set_level(Level::RunSpeed, percent);
    }

    fn fixture(capacity: usize) -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let (sender, receiver) = std::sync::mpsc::sync_channel(capacity);
        let mut app = App::new();
        app.insert_resource(online)
            .init_resource::<super::super::OptionsState>()
            .init_resource::<MovementSync>()
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, tell);
        (app, receiver)
    }

    #[test]
    fn full_queues_retry_current_choices_and_each_admission_is_initialized() {
        let (mut app, receiver) = fixture(1);
        set_run_speed(&mut app, 120);
        app.update();
        // Change the preference while an older choice occupies the queue.
        set_run_speed(&mut app, 140);
        app.update();
        let ClientCommand::SetMovementOptions {
            session_id: 1,
            request_id,
            options,
            ..
        } = receiver.try_recv().unwrap()
        else {
            panic!("initial speed choice");
        };
        assert_eq!(options.run_speed_multiplier.percent(), 120);
        app.update();
        // The queued speed choice must resolve before the next one is sent.
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ClientCommand::SetBookOptions {
                memorize_without_sitting: false,
                ..
            }
        ));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsApplied {
                session_id: 1,
                request_id,
                options,
            }],
        );
        app.update();
        assert!(
            matches!(receiver.try_recv().unwrap(), ClientCommand::SetMovementOptions { session_id: 1, options, .. } if options.run_speed_multiplier.percent() == 140)
        );
        app.update();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ClientCommand::AnswerListPolls { enabled: false, .. }
        ));
        app.update();
        assert!(receiver.try_recv().is_err());
        testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            2,
            testing::player(7),
        );
        app.update();
        assert!(
            matches!(receiver.try_recv().unwrap(), ClientCommand::SetMovementOptions { session_id: 2, options, .. } if options.run_speed_multiplier.percent() == 140)
        );
    }

    #[test]
    fn a_transfer_refused_in_one_receive_batch_retries_a_discarded_application() {
        resync_after_refused_transfer(true);
    }

    #[test]
    fn a_transfer_refused_in_one_receive_batch_retries_an_ignored_refusal() {
        resync_after_refused_transfer(false);
    }

    fn receive_fixture(
        online: OnlineState,
    ) -> (
        App,
        mpsc::Sender<WorldUpdate>,
        mpsc::Receiver<ClientCommand>,
    ) {
        use crate::systems::{Stage, configure_stages, receive_world};
        use bevy::mesh::skinning::SkinnedMeshInverseBindposes;

        let (updates, incoming) = mpsc::channel();
        let (sender, commands) = mpsc::sync_channel(8);
        let mut app = crate::testing::app();
        app.insert_resource(online)
            .insert_resource(crate::online::Updates(Mutex::new(Some(incoming))))
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>();
        configure_stages(&mut app);
        app.add_systems(Update, (receive_world(), tell.in_set(Stage::Input)));
        (app, updates, commands)
    }

    fn refused_transfer(session_id: u64) -> [WorldEvent; 2] {
        [
            WorldEvent::ZoneTransfer(ZoneOffer {
                zone_id: 2,
                instance_id: 0,
                position: eq_client_core::WorldPosition::default(),
                reason: 0,
                to_bind: false,
                solicited: true,
            }),
            WorldEvent::ZoneTransferRejected {
                session_id,
                reason: ZoneRejection::Cancelled,
            },
        ]
    }

    fn resync_after_refused_transfer(applied_before_transfer: bool) {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let (mut app, updates, commands) = receive_fixture(online);
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .qol
            .set(Fix::Fly, true);
        let option_command = || {
            commands
                .try_iter()
                .find_map(|command| match command {
                    ClientCommand::SetMovementOptions {
                        session_id: 1,
                        request_id,
                        options,
                        ..
                    } => Some((request_id, options)),
                    _ => None,
                })
                .expect("movement choice for the current admission")
        };
        app.update();
        let (original, wanted) = option_command();
        assert!(wanted.fly);
        let applied = |request_id| WorldEvent::MovementOptionsApplied {
            session_id: 1,
            request_id,
            options: wanted,
        };
        let original_reply = || {
            if applied_before_transfer {
                applied(original)
            } else {
                WorldEvent::MovementOptionsRefused {
                    session_id: 1,
                    request_id: original,
                    reason: "Changing zones".into(),
                }
            }
        };
        let [offer, rejected] = refused_transfer(1);
        let batch = if applied_before_transfer {
            [original_reply(), offer, rejected]
        } else {
            [offer, original_reply(), rejected]
        };
        for event in batch {
            updates.send(WorldUpdate::Game(event)).unwrap();
        }
        app.update();
        let world = app.world().resource::<OnlineState>().world();
        assert_eq!(world.session_id(), Some(1));
        assert!(world.in_world());
        assert!(world.movement_options_result().is_none());
        let (retry, options) = option_command();
        assert_ne!(retry, original);
        assert_eq!(options, wanted);
        let usable = |app: &App| {
            app.world()
                .resource::<MovementSync>()
                .usable(Some(1), wanted)
        };
        assert_eq!(usable(&app), None);

        updates.send(WorldUpdate::Game(original_reply())).unwrap();
        app.update();
        assert_eq!(usable(&app), None);
        updates.send(WorldUpdate::Game(applied(retry))).unwrap();
        app.update();
        assert_eq!(usable(&app), Some(wanted));
    }

    #[test]
    fn ordinary_movement_needs_no_option_receipt_after_a_refused_transfer() {
        let mut online = OnlineState::new(true);
        let capabilities: Vec<_> = Capability::ALL
            .into_iter()
            .filter(|capability| *capability != Capability::GameplayDepartures)
            .collect();
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities,
                choices: Vec::new(),
                session_id: 3,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
                zone_id: 0,
            }],
        );
        testing::connect(&mut online, true);
        let (mut app, updates, commands) = receive_fixture(online);
        {
            let mut state = app.world_mut().resource_mut::<super::super::OptionsState>();
            state.options.set_level(Level::RunSpeed, 140);
            for fix in [Fix::Fly, Fix::MemorizeWhileStanding] {
                state.options.qol.set(fix, true);
            }
        }
        app.update();
        let check_ordinary_policy = |app: &App| {
            let world = app.world().resource::<OnlineState>().world();
            assert!(world.in_world());
            assert_eq!(world.session_id(), Some(3));
            let wanted = eq_client_core::movement::offered_options(
                &app.world()
                    .resource::<super::super::OptionsState>()
                    .options
                    .qol,
                world,
            );
            assert_eq!(wanted, MovementOptions::default());
            let sync = app.world().resource::<MovementSync>();
            assert_eq!(sync.usable(Some(3), wanted), Some(wanted));
            assert_eq!(sync.next_id, 0, "no movement option submission attempted");
            assert!(!commands.try_iter().any(|command| matches!(
                command,
                ClientCommand::SetMovementOptions { .. } | ClientCommand::SetBookOptions { .. }
            )));
        };
        check_ordinary_policy(&app);

        for event in refused_transfer(3) {
            updates.send(WorldUpdate::Game(event)).unwrap();
        }
        app.update();
        check_ordinary_policy(&app);
    }

    #[test]
    fn unanswered_fly_choice_resubmits_latest_policy_and_waits_for_its_reply() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let (sender, commands) = mpsc::sync_channel(8);
        let outbox = crate::outbox::Outbox::new(Some(sender));
        let mut sync = MovementSync::default();
        let normal = MovementOptions::default();
        let fly = MovementOptions {
            fly: true,
            ..normal
        };
        let start = Instant::now();
        let deadline = start + REPLY_TIMEOUT;
        let queued = |wanted, now| {
            let ClientCommand::SetMovementOptions {
                session_id: 1,
                request_id,
                options,
                created,
            } = commands.try_recv().unwrap()
            else {
                panic!("movement choice for the current admission");
            };
            assert_eq!(options, wanted);
            assert_eq!(created, now);
            request_id
        };

        sync.tell(online.world(), &outbox, fly, start);
        let original = queued(fly, start);
        assert_eq!(sync.usable(Some(1), fly), None);
        sync.tell(
            online.world(),
            &outbox,
            normal,
            deadline.checked_sub(Duration::from_millis(1)).unwrap(),
        );
        assert!(commands.try_recv().is_err());
        assert_eq!(sync.usable(Some(1), normal), None);

        // The first choice could have enabled flight already. Returning to
        // the last acknowledged setting still requires a worker reply.
        sync.tell(online.world(), &outbox, normal, deadline);
        let replacement = queued(normal, deadline);
        assert_ne!(replacement, original);
        let report = sync.report(Some(1), normal, deadline + Duration::from_millis(10));
        assert_eq!(report["pending"]["request_id"], json!(replacement));
        assert_eq!(report["pending"]["age_ms"], json!(10));
        assert_eq!(report["last_settlement"]["request_id"], json!(original));
        assert_eq!(report["last_settlement"]["status"], "reply_timeout");
        assert_eq!(report["waiting_for_policy"], true);
        assert!(report["applied"].is_null());

        let applied = |session_id, request_id, options| WorldEvent::MovementOptionsApplied {
            session_id,
            request_id,
            options,
        };
        for feedback in [applied(1, original, fly), applied(2, replacement, normal)] {
            testing::news(&mut online, [feedback]);
            sync.tell(online.world(), &outbox, normal, deadline);
            assert_eq!(sync.usable(Some(1), normal), None);
            assert!(commands.try_recv().is_err());
        }
        testing::news(
            &mut online,
            [WorldEvent::MovementOptionsRefused {
                session_id: 1,
                request_id: replacement,
                reason: "Expired".into(),
            }],
        );
        sync.tell(online.world(), &outbox, normal, deadline);
        assert_eq!(sync.usable(Some(1), normal), None);
        assert!(commands.try_recv().is_err());
        let retry_at = deadline + Duration::from_secs(1);
        sync.tell(online.world(), &outbox, normal, retry_at);
        let retry = queued(normal, retry_at);
        testing::news(&mut online, [applied(1, retry, normal)]);
        sync.tell(online.world(), &outbox, normal, retry_at);
        assert_eq!(sync.usable(Some(1), normal), Some(normal));
        assert!(commands.try_recv().is_err());

        let fresh_start = retry_at + Duration::from_millis(1);
        sync.tell(online.world(), &outbox, fly, fresh_start);
        let fresh = queued(fly, fresh_start);
        testing::news(&mut online, [applied(1, fresh, fly)]);
        // A matching result already in the world wins over the deadline.
        sync.tell(online.world(), &outbox, fly, fresh_start + REPLY_TIMEOUT);
        assert_eq!(sync.usable(Some(1), fly), Some(fly));
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn unanswered_speed_choices_keep_motion_at_the_lowest_possible_pace() {
        use eq_client_core::movement::RunSpeedMultiplier;

        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let (sender, commands) = mpsc::sync_channel(8);
        let outbox = crate::outbox::Outbox::new(Some(sender));
        let mut sync = MovementSync::default();
        let options = |percent| MovementOptions {
            run_speed_multiplier: RunSpeedMultiplier::from_percent(percent).unwrap(),
            ..MovementOptions::default()
        };
        let (low, middle, high) = (options(100), options(120), options(140));
        let submit = |sync: &mut MovementSync, online: &OnlineState, wanted, now| {
            sync.tell(online.world(), &outbox, wanted, now);
            let ClientCommand::SetMovementOptions {
                request_id,
                options,
                ..
            } = commands.try_recv().unwrap()
            else {
                panic!("movement choice");
            };
            assert_eq!(options, wanted);
            request_id
        };
        let acknowledge = |online: &mut OnlineState, request_id, options| {
            testing::news(
                online,
                [WorldEvent::MovementOptionsApplied {
                    session_id: 1,
                    request_id,
                    options,
                }],
            );
        };
        let start = Instant::now();
        let first = submit(&mut sync, &online, high, start);
        acknowledge(&mut online, first, high);
        let expired = submit(&mut sync, &online, low, start);
        assert_eq!(sync.usable(Some(1), low), Some(low));

        submit(&mut sync, &online, middle, start + REPLY_TIMEOUT);
        assert_eq!(sync.applied, None);
        assert_eq!(sync.usable(Some(1), middle), Some(low));
        let final_at = start + REPLY_TIMEOUT * 2;
        let replacement = submit(&mut sync, &online, high, final_at);
        assert_eq!(sync.usable(Some(1), high), Some(low));

        acknowledge(&mut online, expired, low);
        sync.tell(online.world(), &outbox, high, final_at);
        assert_eq!(sync.usable(Some(1), high), Some(low));
        assert!(commands.try_recv().is_err());
        acknowledge(&mut online, replacement, high);
        sync.tell(online.world(), &outbox, high, final_at);
        assert_eq!(sync.usable(Some(1), high), Some(high));
        assert_eq!(sync.conservative, None);
    }

    #[test]
    fn only_matching_worker_feedback_updates_speed_and_refusals_retry_with_a_new_id() {
        let (mut app, receiver) = fixture(8);
        app.update();
        let normal = MovementOptions::default();
        let applied = |app: &App| app.world().resource::<MovementSync>().applied;
        let usable = |app: &App, wanted| {
            app.world()
                .resource::<MovementSync>()
                .usable(Some(1), wanted)
        };
        assert_eq!(applied(&app), Some(normal));
        assert!(
            !receiver
                .try_iter()
                .any(|command| matches!(command, ClientCommand::SetMovementOptions { .. }))
        );
        set_run_speed(&mut app, 120);
        app.update();
        let ClientCommand::SetMovementOptions {
            request_id,
            options,
            ..
        } = receiver.try_recv().unwrap()
        else {
            panic!("policy request")
        };
        assert_eq!(usable(&app, options), Some(normal));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsApplied {
                session_id: 99,
                request_id,
                options,
            }],
        );
        app.update();
        assert_eq!(usable(&app, options), Some(normal));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsApplied {
                session_id: 1,
                request_id: request_id + 1,
                options,
            }],
        );
        app.update();
        assert_eq!(usable(&app, options), Some(normal));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsRefused {
                session_id: 1,
                request_id,
                reason: "Expired".into(),
            }],
        );
        app.update();
        assert_eq!(applied(&app), Some(normal));
        assert_eq!(usable(&app, options), Some(normal));
        assert!(receiver.try_recv().is_err());
        app.world_mut().resource_mut::<MovementSync>().retry_after = None;
        app.update();
        let ClientCommand::SetMovementOptions {
            request_id: retry,
            options,
            ..
        } = receiver.try_recv().unwrap()
        else {
            panic!("retry")
        };
        assert_ne!(retry, request_id);
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsApplied {
                session_id: 1,
                request_id: retry,
                options,
            }],
        );
        app.update();
        assert_eq!(applied(&app), Some(options));
        assert_eq!(usable(&app, options), Some(options));
        assert_eq!(
            app.world()
                .resource::<MovementSync>()
                .usable(Some(99), options),
            None
        );
        set_run_speed(&mut app, 100);
        app.update();
        assert_eq!(applied(&app), Some(options));
        assert_eq!(usable(&app, normal), Some(normal));
        assert!(
            matches!(receiver.try_recv().unwrap(), ClientCommand::SetMovementOptions { options, .. } if options == normal)
        );
        // Returning the slider upward while the lower choice is in flight
        // cannot let a movement sample overtake the worker's lower budget.
        set_run_speed(&mut app, 120);
        app.update();
        assert_eq!(applied(&app), Some(options));
        assert_eq!(usable(&app, options), Some(normal));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn disabling_poll_replies_can_reach_the_session_while_dead() {
        let (mut app, receiver) = fixture(8);
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .qol
            .set(Fix::AnswerListPolls, true);
        app.update();
        assert!(receiver.try_iter().any(|command| matches!(
            command,
            ClientCommand::AnswerListPolls { enabled: true, .. }
        )));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::Death(eq_client_core::Death {
                spawn_id: 7,
                killer_id: 9,
                corpse_id: 7,
                bind_zone_id: 0,
                corpse_name: None,
            })],
        );
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .qol
            .set(Fix::AnswerListPolls, false);
        app.update();
        assert!(receiver.try_iter().any(|command| matches!(
            command,
            ClientCommand::AnswerListPolls { enabled: false, .. }
        )));
    }
}
