//! Runtime option synchronization, retried until queued for the current admission.
use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand, movement::MovementOptions, qol::Fix};
use std::time::{Duration, Instant};

/// Tracks local worker application separately from successful queue admission.
#[derive(Resource, Default)]
pub(crate) struct MovementSync {
    session: Option<u64>,
    wanted: Option<MovementOptions>,
    applied: Option<MovementOptions>,
    pending: Option<Pending>,
    next_id: u64,
    retry_after: Option<Instant>,
}

struct Pending {
    request_id: u64,
    options: MovementOptions,
}

impl MovementSync {
    /// A move after the pending option command can reach the worker before
    /// its reply reaches us. Use a pace valid under either worker policy.
    /// Permission changes still require confirmation before motion resumes.
    pub(crate) fn usable(
        &self,
        session: Option<u64>,
        wanted: MovementOptions,
    ) -> Option<MovementOptions> {
        let applied = self.applied?;
        if self.session != session || !applied.same_permissions(wanted) {
            return None;
        }
        let mut limit = applied
            .run_speed_multiplier
            .min(wanted.run_speed_multiplier);
        if let Some(pending) = &self.pending {
            if !applied.same_permissions(pending.options) {
                return None;
            }
            limit = limit.min(pending.options.run_speed_multiplier);
        }
        Some(MovementOptions {
            run_speed_multiplier: limit,
            ..applied
        })
    }

    /// Forgets the policy when a world reset can discard the pending reply.
    pub(crate) fn invalidate(&mut self) {
        // A resumed admission may still have the worker's previous choices.
        // Reestablish the actual policy even if the user wants the defaults.
        self.wanted = None;
        self.applied = None;
        self.pending = None;
        self.retry_after = None;
    }

    fn tell(
        &mut self,
        world: &eq_client_core::world::ClientWorld,
        outbox: &crate::outbox::Outbox,
        wanted: MovementOptions,
    ) {
        let session = world.session_id();
        if self.session != session {
            self.session = session;
            self.wanted = None;
            // A fresh admission starts with the worker's ordinary defaults.
            self.applied = Some(MovementOptions::default());
            self.pending = None;
            self.retry_after = None;
        }
        if self.wanted != Some(wanted) {
            self.wanted = Some(wanted);
            self.retry_after = None;
        }
        // Keep one choice in flight. If the slider moves again, the reply
        // still establishes the worker's policy before we send the latest choice.
        if let Some((request_id, result)) = world.movement_options_result()
            && let Some(pending) = self
                .pending
                .as_ref()
                .filter(|pending| pending.request_id == *request_id)
        {
            match result {
                Ok(options) if *options == pending.options => self.applied = Some(*options),
                _ => {
                    if result.is_ok() {
                        self.applied = None;
                    }
                    if pending.options == wanted {
                        self.retry_after = Some(Instant::now() + Duration::from_secs(1));
                    }
                }
            }
            self.pending = None;
        }
        if self.pending.is_some()
            || self.applied == Some(wanted)
            || self.retry_after.is_some_and(|due| Instant::now() < due)
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
                created: Instant::now(),
            },
        ) {
            self.pending = Some(Pending {
                request_id,
                options: wanted,
            });
        }
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
    movement_sync.tell(world, &outbox, movement);
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
    use eq_client_core::options::Level;

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
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .set_level(Level::RunSpeed, 120);
        app.update();
        // Change the preference while an older choice occupies the queue.
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .set_level(Level::RunSpeed, 140);
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

    fn resync_after_refused_transfer(applied_before_transfer: bool) {
        use crate::systems::{Stage, configure_stages, receive_world};
        use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
        use eq_client_core::{WorldEvent, WorldUpdate, ZoneOffer, ZoneRejection};
        use std::sync::{Mutex, mpsc};

        let (updates, incoming) = mpsc::channel();
        let (sender, commands) = mpsc::sync_channel(8);
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let mut app = crate::testing::app();
        app.insert_resource(online)
            .insert_resource(crate::online::Updates(Mutex::new(Some(incoming))))
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>();
        configure_stages(&mut app);
        app.add_systems(Update, (receive_world(), tell.in_set(Stage::Input)));
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
        let offer = WorldEvent::ZoneTransfer(ZoneOffer {
            zone_id: 2,
            instance_id: 0,
            position: Default::default(),
            reason: 0,
            to_bind: false,
            solicited: true,
        });
        let rejected = WorldEvent::ZoneTransferRejected {
            session_id: 1,
            reason: ZoneRejection::Cancelled,
        };
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
    fn a_session_without_departures_sends_no_options_and_holds_no_input() {
        let mut online = OnlineState::new(true);
        let capabilities: Vec<_> = Capability::ALL
            .into_iter()
            .filter(|capability| *capability != Capability::GameplayDepartures)
            .collect();
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::Entered {
                capabilities,
                choices: Vec::new(),
                session_id: 3,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
            }],
        );
        testing::connect(&mut online, true);
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let mut app = App::new();
        app.insert_resource(online)
            .init_resource::<super::super::OptionsState>()
            .init_resource::<MovementSync>()
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, tell);
        {
            let mut state = app.world_mut().resource_mut::<super::super::OptionsState>();
            state.options.set_level(Level::RunSpeed, 140);
            for fix in [Fix::Fly, Fix::MemorizeWhileStanding] {
                state.options.qol.set(fix, true);
            }
        }
        app.update();
        app.update();
        // The official client's movement is what this session takes, and
        // it is applied from admission, so input is never gated on it.
        let world = app.world().resource::<OnlineState>();
        let wanted = eq_client_core::movement::offered_options(
            &app.world()
                .resource::<super::super::OptionsState>()
                .options
                .qol,
            world.world(),
        );
        assert_eq!(wanted, MovementOptions::default());
        assert_eq!(
            app.world()
                .resource::<MovementSync>()
                .usable(Some(3), wanted),
            Some(wanted)
        );
        for command in receiver.try_iter() {
            assert!(
                !matches!(
                    command,
                    ClientCommand::SetMovementOptions { .. } | ClientCommand::SetBookOptions { .. }
                ),
                "{command:?} sent where departures are not offered"
            );
        }
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
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .set_level(Level::RunSpeed, 120);
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
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .set_level(Level::RunSpeed, 100);
        app.update();
        assert_eq!(applied(&app), Some(options));
        assert_eq!(usable(&app, normal), Some(normal));
        assert!(
            matches!(receiver.try_recv().unwrap(), ClientCommand::SetMovementOptions { options, .. } if options == normal)
        );
        // Returning the slider upward while the lower choice is in flight
        // cannot let a movement sample overtake the worker's lower budget.
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .set_level(Level::RunSpeed, 120);
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
