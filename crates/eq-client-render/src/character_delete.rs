//! Shared ordinary confirmation and sender state for character deletion.
use eq_client_core::{
    deletion::{Presence, Progress, Refusal, Request, Scope, Unconfirmed, Update},
    world::CharacterList,
};

#[derive(Default)]
pub(super) struct State {
    next_id: u64,
    current: Option<Scope>,
    ended_connection: u64,
    prompt: Option<Request>,
    update: Option<Update>,
    error: Option<String>,
}

impl State {
    pub fn blocked(&self) -> bool {
        self.prompt.is_some()
            || self.update.as_ref().is_some_and(|update| {
                self.current.is_some_and(|scope| {
                    scope.connection_id == update.request.scope().connection_id
                }) && update.progress.unresolved()
            })
    }

    pub fn prompt(&self) -> Option<&Request> {
        self.prompt.as_ref()
    }

    /// Any new list voids an unanswered confirmation. An in-flight request
    /// survives a same-connection refresh, but never moves to a new connection.
    pub fn listed(&mut self, list: &CharacterList) {
        let scope = list.scope();
        if scope.connection_id <= self.ended_connection {
            return;
        }
        if self.current != Some(scope) {
            self.prompt = None;
        }
        if self
            .current
            .is_some_and(|old| old.connection_id != scope.connection_id)
        {
            self.ended();
        }
        self.current = Some(scope);
    }

    pub fn ended(&mut self) {
        if let Some(scope) = self.current {
            self.ended_connection = self.ended_connection.max(scope.connection_id);
        }
        self.prompt = None;
        self.current = None;
        if let Some(update) = self
            .update
            .as_mut()
            .filter(|update| update.progress.unresolved())
        {
            update.progress = Progress::Unconfirmed {
                reason: Unconfirmed::ConnectionEnded,
                observation: update.progress.observation(),
            };
        }
    }

    pub fn prepare(&mut self, list: &CharacterList, name: &str) -> Result<Request, String> {
        if self.blocked() {
            return Err("A character deletion is awaiting confirmation or a refreshed list. Back / Disconnect is available.".into());
        }
        self.validate(list, name)?;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Character-deletion request identifiers are exhausted")?;
        let request = Request::new(list.scope(), self.next_id, name, name)
            .map_err(|error| error.to_string())?;
        self.prompt = Some(request.clone());
        self.error = None;
        Ok(request)
    }

    pub fn cancel(&mut self, request: &Request) {
        if self.prompt.as_ref() == Some(request) {
            self.prompt = None;
        }
    }

    /// Both the dialog's Yes and the explicit script confirmation use this
    /// sender, with the current list and selected identity checked again.
    pub fn submit(
        &mut self,
        request: &Request,
        confirmation: &str,
        list: &CharacterList,
        selected: Option<&str>,
        send: impl FnOnce(Request) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.prompt.as_ref() != Some(request) {
            return Err("Character deletion confirmation is stale".into());
        }
        let checked = self.validate(list, request.name()).and_then(|()| {
            if request.scope() != list.scope() || selected != Some(request.name()) {
                return Err("Character list or selected identity changed; confirm again".into());
            }
            Request::new(
                request.scope(),
                request.request_id(),
                request.name(),
                confirmation,
            )
            .map_err(|error| error.to_string())
        });
        self.prompt = None;
        let checked = checked.inspect_err(|error| self.error = Some(error.clone()))?;
        self.update = Some(Update {
            request: checked.clone(),
            progress: Progress::Pending(None),
        });
        if let Err(error) = send(checked) {
            self.update
                .as_mut()
                .expect("recorded before queueing")
                .progress = Progress::NotSent(Refusal::QueueUnavailable);
            self.error = Some(error.clone());
            return Err(error);
        }
        Ok(())
    }

    fn validate(&self, list: &CharacterList, name: &str) -> Result<(), String> {
        if self.current != Some(list.scope()) || list.connection_id == 0 {
            return Err("Character list is no longer current".into());
        }
        if list.deletion.is_none() {
            return Err("Character deletion is unavailable on this server".into());
        }
        if list
            .characters
            .iter()
            .filter(|entry| entry.name == name)
            .count()
            != 1
        {
            return Err("Deletion requires one exact listed character name".into());
        }
        Ok(())
    }

    pub fn hear(&mut self, update: &Update, list: Option<&CharacterList>) {
        if self
            .update
            .as_ref()
            .is_none_or(|last| last.request != update.request)
        {
            return;
        }
        if self
            .current
            .is_none_or(|scope| scope.connection_id != update.request.scope().connection_id)
        {
            return;
        }
        if let Some(observation) = update.progress.observation() {
            let Some(list) = list.filter(|list| list.scope() == observation.scope) else {
                return;
            };
            let present = list
                .characters
                .iter()
                .any(|entry| entry.name == update.request.name());
            if present != (observation.presence == Presence::Present) {
                return;
            }
        }
        // Terminal absence is a historical snapshot; late pending/duplicate
        // replies cannot turn it into a new attempt or a new completion.
        if self
            .update
            .as_ref()
            .is_some_and(|last| !last.progress.unresolved())
        {
            return;
        }
        if let Some(last) = &self.update {
            if matches!(last.progress, Progress::Unconfirmed { .. })
                && matches!(update.progress, Progress::Pending(_))
            {
                return;
            }
            if last.progress.observation().is_some()
                && update.progress.observation().is_none()
                && matches!(update.progress, Progress::Pending(_))
            {
                return;
            }
        }
        self.update = Some(update.clone());
        self.error = None;
    }

    pub fn message(&self) -> Option<String> {
        if let Some(error) = &self.error {
            return Some(error.clone());
        }
        if let Some(request) = &self.prompt {
            return Some(format!(
                "Confirm deletion of {} in the dialog, or choose No.",
                request.name()
            ));
        }
        let update = self.update.as_ref()?;
        Some(match &update.progress {
            Progress::NotSent(reason) => format!("Deletion was not sent: {reason:?}."),
            Progress::Pending(_) => format!(
                "Deletion of {} is awaiting a refreshed character list. Back / Disconnect is available.",
                update.request.name()
            ),
            Progress::Unconfirmed { .. } => format!(
                "Deletion of {} is unconfirmed. Back / Disconnect, then reconnect to inspect the list.",
                update.request.name()
            ),
            Progress::ObservedAbsent { .. } => format!(
                "{} was absent in the refreshed character list. This does not confirm permanent deletion.",
                update.request.name()
            ),
        })
    }

    /// Versioned JSON for private script reports, without a deletion-success flag.
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({ "version": 1, "current_scope": self.current, "update": self.update })
    }

    /// The script advances only for a matching absence observation on the current
    /// list. Refusals, session changes and ambiguous outcomes stop it without retry.
    pub fn observed(&self, request: &Request) -> Result<bool, String> {
        let update = self
            .update
            .as_ref()
            .filter(|update| &update.request == request)
            .ok_or("Character deletion lost its request identity")?;
        if self
            .current
            .is_none_or(|scope| scope.connection_id != request.scope().connection_id)
        {
            return Err("Character deletion connection ended; outcome is unconfirmed".into());
        }
        match &update.progress {
            Progress::ObservedAbsent { scope } if self.current == Some(*scope) => Ok(true),
            Progress::ObservedAbsent { .. } => {
                Err("The character-deletion absence observation belongs to an older list".into())
            }
            Progress::NotSent(reason) => {
                Err(format!("Character deletion was not sent: {reason:?}"))
            }
            Progress::Unconfirmed { reason, .. } => Err(format!(
                "Character deletion is unconfirmed: {reason:?}; reconnect to inspect the list"
            )),
            Progress::Pending(_) => Ok(false),
        }
    }
}

pub(super) fn question(request: &Request) -> crate::confirm::Question {
    crate::confirm::Question::DeleteCharacter {
        scope: request.scope(),
        request_id: request.request_id(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn list(connection_id: u64, selection_id: u64, names: &[&str]) -> CharacterList {
        CharacterList {
            connection_id,
            selection_id,
            creation: None,
            deletion: Some(eq_client_core::deletion::Rules::Titanium),
            characters: names
                .iter()
                .enumerate()
                .map(|(slot, name)| eq_client_core::CharacterChoice {
                    slot: u8::try_from(slot).unwrap(),
                    name: (*name).into(),
                    level: None,
                    zone_id: None,
                    look: None,
                })
                .collect(),
        }
    }
    fn pending(state: &mut State, list: &CharacterList) -> Request {
        state.listed(list);
        let request = state.prepare(list, "Disposable").unwrap();
        state
            .submit(&request, "Disposable", list, Some("Disposable"), |_| Ok(()))
            .unwrap();
        request
    }
    #[test]
    fn changed_list_or_identity_and_no_confirmation_never_reach_sender() {
        let mut state = State::default();
        let first = list(1, 1, &["Disposable", "Other"]);
        state.listed(&first);
        assert!(state.prepare(&first, "disposable").is_err());
        let request = state.prepare(&first, "Disposable").unwrap();
        assert!(
            state
                .submit(&request, "Disposable", &first, Some("Other"), |_| panic!(
                    "must not send"
                ))
                .is_err()
        );
        let request = state.prepare(&first, "Disposable").unwrap();
        assert!(
            state
                .submit(&request, "Other", &first, Some("Disposable"), |_| panic!(
                    "must not send"
                ))
                .is_err()
        );
        let request = state.prepare(&first, "Disposable").unwrap();
        let next = list(1, 2, &["Disposable"]);
        state.listed(&next);
        assert!(
            state
                .submit(
                    &request,
                    "Disposable",
                    &next,
                    Some("Disposable"),
                    |_| panic!("must not send")
                )
                .is_err()
        );
    }
    #[test]
    fn duplicate_send_busy_and_queue_failure_are_local() {
        let mut state = State::default();
        let list = list(1, 1, &["Disposable"]);
        let request = pending(&mut state, &list);
        assert!(state.blocked());
        assert!(state.prepare(&list, "Disposable").is_err());
        assert!(
            state
                .submit(
                    &request,
                    "Disposable",
                    &list,
                    Some("Disposable"),
                    |_| panic!("must not resend")
                )
                .is_err()
        );
        let mut state = State::default();
        state.listed(&list);
        let request = state.prepare(&list, "Disposable").unwrap();
        assert!(
            state
                .submit(&request, "Disposable", &list, Some("Disposable"), |_| Err(
                    "queue closed".into()
                ))
                .is_err()
        );
        assert!(!state.blocked());
        assert!(matches!(
            state.update.unwrap().progress,
            Progress::NotSent(Refusal::QueueUnavailable)
        ));
    }
    #[test]
    fn present_absent_and_older_observations_have_distinct_meanings() {
        let mut state = State::default();
        let first = list(1, 1, &["Disposable"]);
        let request = pending(&mut state, &first);
        let present = list(1, 2, &["Disposable"]);
        state.listed(&present);
        state.hear(
            &Update {
                request: request.clone(),
                progress: Progress::Pending(Some(eq_client_core::deletion::Observation {
                    scope: present.scope(),
                    presence: Presence::Present,
                })),
            },
            Some(&present),
        );
        assert_eq!(state.observed(&request), Ok(false));
        let absent = list(1, 3, &[]);
        let update = Update {
            request: request.clone(),
            progress: Progress::ObservedAbsent {
                scope: absent.scope(),
            },
        };
        state.hear(&update, Some(&present));
        assert!(state.blocked());
        state.listed(&absent);
        state.hear(&update, Some(&absent));
        assert_eq!(state.observed(&request), Ok(true));
        assert!(!state.blocked());
        state.hear(
            &Update {
                request: request.clone(),
                progress: Progress::Pending(None),
            },
            Some(&absent),
        );
        assert_eq!(state.observed(&request), Ok(true));
        state.listed(&list(1, 4, &[]));
        assert!(state.observed(&request).is_err());
    }
    #[test]
    fn reconnect_with_identical_name_and_small_ids_cannot_reuse_intent() {
        let mut state = State::default();
        let first = list(1, 1, &["Disposable"]);
        let old = pending(&mut state, &first);
        let fresh = list(2, 1, &["Disposable"]);
        state.listed(&fresh);
        assert!(state.observed(&old).is_err());
        assert!(!state.blocked());
        state.hear(
            &Update {
                request: old.clone(),
                progress: Progress::ObservedAbsent {
                    scope: fresh.scope(),
                },
            },
            Some(&fresh),
        );
        assert!(matches!(
            state.update.as_ref().unwrap().progress,
            Progress::Unconfirmed {
                reason: Unconfirmed::ConnectionEnded,
                ..
            }
        ));
        assert!(
            state
                .submit(&old, "Disposable", &fresh, Some("Disposable"), |_| panic!(
                    "old intent"
                ))
                .is_err()
        );
        let request = state.prepare(&fresh, "Disposable").unwrap();
        assert_ne!(request, old);
    }
    #[test]
    fn unsupported_ambiguous_and_exhausted_intents_are_rejected() {
        let mut state = State::default();
        let mut first = list(1, 1, &["Disposable"]);
        first.deletion = None;
        state.listed(&first);
        assert!(state.prepare(&first, "Disposable").is_err());
        let repeated = list(1, 2, &["Disposable", "Disposable"]);
        state.listed(&repeated);
        assert!(state.prepare(&repeated, "Disposable").is_err());
        first.deletion = Some(eq_client_core::deletion::Rules::Titanium);
        state.listed(&first);
        state.next_id = u64::MAX;
        assert!(state.prepare(&first, "Disposable").is_err());
        assert_eq!(state.next_id, u64::MAX);
    }
    #[test]
    fn a_late_old_list_cannot_revive_a_terminated_connection() {
        let mut state = State::default();
        let first = list(7, 1, &["Disposable"]);
        let request = pending(&mut state, &first);
        state.ended();
        let late = list(7, 2, &[]);
        state.listed(&late);
        state.hear(
            &Update {
                request: request.clone(),
                progress: Progress::ObservedAbsent {
                    scope: late.scope(),
                },
            },
            Some(&late),
        );
        assert!(state.current.is_none());
        assert!(state.observed(&request).is_err());
        assert!(matches!(
            state.update.as_ref().unwrap().progress,
            Progress::Unconfirmed {
                reason: Unconfirmed::ConnectionEnded,
                ..
            }
        ));
    }

    #[test]
    fn report_is_versioned_and_carries_both_request_and_observation_scopes() {
        let mut state = State::default();
        let first = list(1, 1, &["Disposable"]);
        let request = pending(&mut state, &first);
        let absent = list(1, 2, &[]);
        state.listed(&absent);
        state.hear(
            &Update {
                request,
                progress: Progress::ObservedAbsent {
                    scope: absent.scope(),
                },
            },
            Some(&absent),
        );
        let json = state.report();
        assert_eq!(json["version"], 1);
        assert_eq!(json["current_scope"]["selection_id"], 2);
        assert_eq!(json["update"]["request"]["scope"]["selection_id"], 1);
        assert_eq!(
            json["update"]["progress"]["ObservedAbsent"]["scope"]["selection_id"],
            2
        );
    }
}
