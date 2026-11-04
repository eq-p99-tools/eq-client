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
    pub(crate) fn ready(&self, session: Option<u64>, wanted: MovementOptions) -> bool {
        self.session == session && self.applied == Some(wanted) && self.pending.is_none()
    }

    fn disconnected(&mut self) {
        // A reply may have been lost while the world was suspended. A resumed
        // admission must reestablish the actual local policy, even for defaults.
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
            if self.applied != Some(wanted) || self.pending.is_some() {
                self.applied = None;
            }
            self.pending = None;
            self.retry_after = None;
        }
        if let Some((request_id, result)) = world.movement_options_result()
            && let Some(pending) = self
                .pending
                .as_ref()
                .filter(|pending| pending.request_id == *request_id)
        {
            match result {
                Ok(options) if *options == pending.options => self.applied = Some(*options),
                _ => {
                    self.retry_after = Some(Instant::now() + Duration::from_secs(1));
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
        movement_sync.disconnected();
        *told = Told::default();
        return;
    };
    let movement = eq_client_core::movement::options(&state.options.qol);
    if world.can(Capability::Moving) {
        movement_sync.tell(world, &outbox, movement);
    }
    let memorize_without_sitting = state.options.qol.on(Fix::MemorizeWhileStanding);
    if world.can(Capability::Spellbook)
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
            .qol
            .set(Fix::RunBoost, true);
        app.update();
        // Change the preference while an older choice occupies the queue.
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .qol
            .set(Fix::Fly, true);
        app.update();
        assert!(
            matches!(receiver.try_recv().unwrap(), ClientCommand::SetMovementOptions { options, .. } if options.run_boost && !options.fly)
        );
        app.update();
        assert!(
            matches!(receiver.try_recv().unwrap(), ClientCommand::SetMovementOptions { options, .. } if options.run_boost && options.fly)
        );
        app.update();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ClientCommand::SetBookOptions {
                memorize_without_sitting: false,
                ..
            }
        ));
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
            matches!(receiver.try_recv().unwrap(), ClientCommand::SetMovementOptions { session_id: 2, options, .. } if options.run_boost)
        );
    }

    fn ready(app: &App, options: MovementOptions) -> bool {
        app.world()
            .resource::<MovementSync>()
            .ready(Some(1), options)
    }

    #[test]
    fn only_matching_worker_feedback_releases_departures_and_refusals_retry_with_a_new_id() {
        let (mut app, receiver) = fixture(8);
        app.update();
        let normal = MovementOptions::default();
        assert!(ready(&app, normal));
        assert!(
            !receiver
                .try_iter()
                .any(|command| matches!(command, ClientCommand::SetMovementOptions { .. }))
        );
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .qol
            .set(Fix::RunBoost, true);
        app.update();
        let ClientCommand::SetMovementOptions {
            request_id,
            options,
            ..
        } = receiver.try_recv().unwrap()
        else {
            panic!("policy request")
        };
        assert!(!ready(&app, options));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsApplied {
                session_id: 99,
                request_id,
                options,
            }],
        );
        app.update();
        assert!(!ready(&app, options));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsApplied {
                session_id: 1,
                request_id: request_id + 1,
                options,
            }],
        );
        app.update();
        assert!(!ready(&app, options));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::MovementOptionsRefused {
                session_id: 1,
                request_id,
                reason: "Expired".into(),
            }],
        );
        app.update();
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
        assert!(ready(&app, options));
        assert!(
            !app.world()
                .resource::<MovementSync>()
                .ready(Some(99), options)
        );
        app.world_mut()
            .resource_mut::<super::super::OptionsState>()
            .options
            .qol
            .set(Fix::RunBoost, false);
        app.update();
        assert!(!ready(&app, normal));
        assert!(
            matches!(receiver.try_recv().unwrap(), ClientCommand::SetMovementOptions { options, .. } if options == normal)
        );
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
