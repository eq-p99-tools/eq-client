//! Per-character social hotbuttons. Each due line uses ordinary chat input.
mod definition;

pub(crate) use definition::Request;
use definition::{Catalog, Line};

use crate::{
    chat::{ChatState, system_line},
    hud::hotbar::{self, Action, Bindings, Slot},
    online::OnlineState,
    outbox::Outbox,
    profile_files::{self, Profile, Store},
};
use bevy::prelude::*;
use std::{sync::Arc, time::Instant};

/// A named step's authority to dispatch work in the social runner.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptDispatch {
    Continue,
    OneLine,
}

pub(crate) struct Queued {
    pub(crate) request: Request,
    pub(crate) owner: Option<Arc<()>>,
}

#[derive(Default)]
pub(super) struct Inbox {
    pub(super) request: Option<Queued>,
    /// A departure supersedes a pending social command or active sequence.
    pub(super) stop: bool,
}

#[derive(Resource, Default)]
pub(crate) struct State {
    file: Store,
    catalog: Catalog,
    load_error: Option<String>,
    run: Option<Run>,
    last: Option<&'static str>,
}

struct Run {
    id: u32,
    owner: Option<Arc<()>>,
    admission: u64,
    lines: Vec<Line>,
    next: usize,
    due: Instant,
}

impl State {
    pub(crate) fn name(&self, id: u32) -> Option<&str> {
        self.catalog
            .entries
            .get(&id)
            .map(|entry| entry.name.as_str())
    }

    fn cancel(&mut self, reason: &'static str) -> bool {
        if self.run.take().is_some() {
            self.last = Some(reason);
            true
        } else {
            false
        }
    }

    /// A world reset stops the sequence even if an admission is then restored.
    pub(crate) fn reset(&mut self) {
        self.cancel("world reset");
    }

    /// A script can lose its executor between any two due lines.
    fn retire_script(&mut self, script: Option<&crate::script::Script>) {
        if self
            .run
            .as_ref()
            .and_then(|run| run.owner.as_ref())
            .is_some_and(|owner| {
                script.and_then(|script| script.social_dispatch(owner))
                    != Some(ScriptDispatch::Continue)
            })
        {
            self.cancel("script ownership ended");
        }
    }

    fn start(
        &mut self,
        id: u32,
        admission: u64,
        now: Instant,
        owner: Option<Arc<()>>,
    ) -> Result<(), String> {
        if self.run.is_some() {
            return Err("A social is already running. Use /social cancel to stop it.".into());
        }
        let social = self
            .catalog
            .entries
            .get(&id)
            .ok_or("This social is unavailable")?;
        self.run = Some(Run {
            id,
            owner,
            admission,
            lines: social.program.clone(),
            next: 0,
            due: now,
        });
        self.last = Some("running");
        Ok(())
    }

    /// Runs at most one due line. A pause starts after submission and never
    /// stands for the result of the command. Refusals end the run.
    fn tick(
        &mut self,
        admission: Option<u64>,
        now: Instant,
        submit: impl FnOnce(&str) -> Result<(), String>,
    ) -> Option<String> {
        let run = self.run.as_mut()?;
        if Some(run.admission) != admission {
            self.cancel("admission changed");
            return None;
        }
        if now < run.due {
            return None;
        }
        let Some(line) = run.lines.get(run.next) else {
            self.cancel("completed");
            return None;
        };
        if let Some(command) = line.command.as_deref()
            && let Err(error) = submit(command)
        {
            let line_number = run.next + 1;
            self.cancel("refused");
            return Some(format!("Social stopped at line {line_number}. {error}"));
        }
        run.next += 1;
        run.due = now + line.pause;
        if run.next == run.lines.len() && line.pause.is_zero() {
            self.cancel("completed");
        }
        None
    }

    fn require_profile(&self) -> Result<(), String> {
        if self.file.profile.is_none() {
            return Err("Socials need an active character profile".into());
        }
        if let Some(error) = &self.load_error {
            return Err(error.clone());
        }
        Ok(())
    }

    fn id(&self, name: &str) -> Result<u32, String> {
        self.catalog
            .named(name)
            .ok_or_else(|| format!("Unknown social {name}"))
    }

    fn save(&mut self, now: Instant) {
        if let Some(profile) = self.file.profile.as_ref() {
            self.file.save(
                profile_files::exact_name("socials", profile),
                self.catalog.text(),
                now,
            );
        }
    }

    fn changed(&mut self, id: u32, now: Instant) {
        if self.run.as_ref().is_some_and(|run| run.id == id) {
            self.cancel("definition changed");
        }
        self.save(now);
    }

    fn request(
        &mut self,
        Queued { request, owner }: Queued,
        admission: Option<u64>,
        now: Instant,
        bindings: &mut Bindings,
    ) -> Result<Option<String>, String> {
        if matches!(request, Request::Cancel) {
            if let Some(owner) = &owner
                && self.run.as_ref().is_some_and(|run| {
                    !run.owner
                        .as_ref()
                        .is_some_and(|running| Arc::ptr_eq(owner, running))
                })
            {
                return Err("This script does not own the running social".into());
            }
            return Ok(Some(if self.cancel("cancelled") {
                "Social cancelled.".into()
            } else {
                "No social is running.".into()
            }));
        }
        self.require_profile()?;
        let admission = admission.ok_or("Socials need a character in the world")?;
        Ok(Some(match request {
            Request::List => {
                if self.catalog.entries.is_empty() {
                    "No socials saved. Use /social set Name | /command".into()
                } else {
                    let names = self
                        .catalog
                        .entries
                        .values()
                        .map(|entry| entry.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("Socials\n{names}")
                }
            }
            Request::Show(name) => {
                let entry = &self.catalog.entries[&self.id(&name)?];
                format!("{}\n{}", entry.name, entry.lines.join("\n"))
            }
            Request::Set(entry) => {
                let name = entry.name.clone();
                let edited = self.catalog.named(&name);
                self.catalog.set(entry)?;
                if let Some(id) = edited {
                    self.changed(id, now);
                } else {
                    self.save(now);
                }
                format!("Saved social {name}.")
            }
            Request::Bind { slot, name } => {
                let id = self.id(&name)?;
                if self.file.profile.as_ref().is_some_and(|profile| {
                    self.file
                        .pending(&profile_files::exact_name("socials", profile))
                }) {
                    return Err(
                        "Wait for the social settings to finish saving before binding".into(),
                    );
                }
                let binding = bindings
                    .0
                    .get_mut(slot)
                    .ok_or("Use a hotbar slot from 1 to 10")?;
                *binding = Some(Action::Social(id));
                format!(
                    "Bound {name} to slot {} on page {}.",
                    slot + 1,
                    bindings.page() + 1
                )
            }
            Request::Play(name) => {
                self.start(self.id(&name)?, admission, now, owner)?;
                return Ok(None);
            }
            Request::Delete(name) => {
                let id = self.id(&name)?;
                self.catalog.entries.remove(&id);
                self.changed(id, now);
                format!("Deleted social {name}.")
            }
            Request::Cancel => unreachable!("cancellation is handled before the profile check"),
        }))
    }

    /// Log local scheduling state for script acceptance without logging macro text.
    pub(crate) fn report(&self, label: &str) {
        let now = Instant::now();
        let run = self.run.as_ref().map(|run| {
            (
                run.admission,
                run.id,
                run.next,
                run.lines.len(),
                run.due.saturating_duration_since(now).as_millis(),
            )
        });
        info!(label, ?run, last = ?self.last, definitions = self.catalog.entries.len(), "Script social report");
    }
}

/// Follow the same profile owner and retrying settings store as the hotbar.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn prepare(
    settings: Res<crate::ViewerSettings>,
    profile: Res<Profile>,
    mut state: ResMut<State>,
) {
    state.file.retry(Instant::now());
    if !state.file.loaded {
        state.file.open(settings.0.settings_directory.as_ref());
    }
    if state.file.profile.as_ref() == profile.key_ref() {
        return;
    }
    state.cancel("profile changed");
    state.file.profile = profile.key();
    state.catalog = Catalog::default();
    state.load_error = None;
    let Some(profile) = state.file.profile.as_ref() else {
        state.file.loaded_text(String::new());
        return;
    };
    let name = profile_files::exact_name("socials", profile);
    let saved = state
        .file
        .read_result(&name)
        .map_err(|error| format!("Could not read social settings. {error}"))
        .and_then(|text| text.map_or_else(|| Ok(Catalog::default()), |text| Catalog::read(&text)));
    match saved {
        Ok(catalog) => state.catalog = catalog,
        Err(error) => {
            warn!("{error}");
            state.load_error = Some(error);
        }
    }
    let text = state.catalog.text();
    state.file.loaded_text(text);
}

/// Consume an explicit editor request or a normal hotbutton activation,
/// then submit one due command through the same path as typed input.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    mut state: ResMut<State>,
    mut bindings: ResMut<Bindings>,
    mut online: ResMut<OnlineState>,
    mut chat: ResMut<ChatState>,
    keys: crate::keys::Keys,
    clicks: crate::clicks::Clicks<Slot>,
    (outbox, options): (Res<Outbox>, Res<crate::options::OptionsState>),
    script: Option<Res<crate::script::Script>>,
    authored: Option<Res<crate::script::AuthoredInput>>,
) {
    let now = Instant::now();
    state.retire_script(script.as_deref());
    let admission = online
        .world()
        .session_id()
        .filter(|_| online.in_world() && online.world().camp().is_none());
    if std::mem::take(&mut chat.socials.stop) {
        state.cancel("departure requested");
        chat.socials.request = None;
        return;
    }
    let mut stepped_play = false;
    if let Some(request) = chat.socials.request.take() {
        let dispatch = request
            .owner
            .as_ref()
            .map_or(Some(ScriptDispatch::Continue), |owner| {
                script
                    .as_ref()
                    .and_then(|script| script.social_dispatch(owner))
            });
        if let Some(dispatch) = dispatch {
            match state.request(request, admission, now, &mut bindings) {
                Ok(Some(message)) => chat.history.push(system_line(message)),
                Ok(None) => stepped_play = dispatch == ScriptDispatch::OneLine,
                Err(error) => chat.refuse(error),
            }
        }
    } else if let Some((slot, key)) = hotbar::requested_slot(&keys, &clicks)
        && let Some(Action::Social(id)) = bindings.0.get(slot).copied().flatten()
    {
        let synthetic = authored.as_ref().is_some_and(|authored| {
            key.map_or(authored.left_button, |act| {
                keys.map.keys(act).any(|code| authored.keys.contains(&code))
            })
        });
        if synthetic {
            chat.refuse("Use a named social step in a local script.");
        } else {
            let result = state.require_profile().and_then(|()| {
                let admission = admission.ok_or("Socials need a character in the world")?;
                state.start(id, admission, now, None)
            });
            if let Err(error) = result {
                chat.refuse(error);
            }
        }
    }

    if let Some(error) = state.tick(admission, now, |command| {
        crate::chat::submit_social_line(
            command,
            &mut chat,
            &mut online,
            &outbox,
            &options.options.qol,
        )
    }) {
        chat.refuse(error);
    }
    if stepped_play {
        // Even an immediate Resume must leave the stepped run's later lines retired.
        state.cancel("script step ended");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn with_social(text: &str) -> (State, u32) {
        let mut state = State::default();
        let Request::Set(entry) = Request::parse(text).unwrap().unwrap() else {
            panic!("expected social definition");
        };
        state.catalog.set(entry).unwrap();
        let id = state.catalog.named("Test").unwrap();
        (state, id)
    }

    #[test]
    fn timed_lines_wait_after_the_command_and_run_once() {
        let (mut state, id) =
            with_social("/social set Test | /pause 4,/say first | /pause 2 | /sit");
        let now = Instant::now();
        state.start(id, 1, now, None).unwrap();
        assert!(state.start(id, 1, now, None).is_err());
        let mut sent = Vec::new();
        for ms in [0, 399, 400, 599, 600, 601] {
            state.tick(Some(1), now + Duration::from_millis(ms), |line| {
                sent.push((ms, line.to_owned()));
                Ok(())
            });
        }
        assert_eq!(sent, [(0, "/say first".into()), (600, "/sit".into())]);
        assert!(state.run.is_none());
        assert_eq!(state.last, Some("completed"));
    }

    #[test]
    fn a_bound_social_dispatches_ordinary_posture_and_chat_requests() {
        use eq_client_core::{ClientCommand, OutboundChat, Posture};

        let (mut state, id) = with_social("/social set Test | /sit | /say Synthetic");
        state.file.profile = Some(profile_files::CharacterProfile {
            character: "Example".into(),
            world: "Fixture".into(),
        });
        let mut bindings = Bindings::default();
        let now = Instant::now();
        let bind = Request::parse("/social bind 10 Test").unwrap().unwrap();
        state
            .request(
                Queued {
                    request: bind,
                    owner: None,
                },
                Some(1),
                now,
                &mut bindings,
            )
            .unwrap();
        assert_eq!(bindings.0[9], Some(Action::Social(id)));
        let mut online = OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        let mut chat = ChatState::default();
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(tx));
        state.start(id, 1, now, None).unwrap();
        for _ in 0..2 {
            assert!(
                state
                    .tick(Some(1), now, |command| {
                        crate::chat::submit_social_line(
                            command,
                            &mut chat,
                            &mut online,
                            &outbox,
                            &eq_client_core::qol::Settings::default(),
                        )
                    })
                    .is_none()
            );
        }
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::SetPosture {
                session_id: 1,
                spawn_id: 1,
                posture: Posture::Sitting,
                ..
            })
        ));
        assert_eq!(
            rx.try_recv().unwrap(),
            ClientCommand::SendChat(OutboundChat::Say("Synthetic".into()))
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn reset_cancel_and_immediate_refusal_end_the_sequence() {
        let (mut state, id) = with_social("/social set Test | /sit | /say next");
        let now = Instant::now();
        state.start(id, 1, now, None).unwrap();
        let error = state
            .tick(Some(1), now, |_| Err("Unavailable".into()))
            .unwrap();
        assert_eq!(error, "Social stopped at line 1. Unavailable");
        state.tick(Some(1), now, |_| panic!("refused run resumed"));
        state.start(id, 1, now, None).unwrap();
        state.tick(Some(2), now, |_| panic!("stale run submitted"));
        assert!(state.run.is_none());
        state.start(id, 1, now, None).unwrap();
        state.reset();
        state.tick(Some(1), now, |_| panic!("reset run resumed"));
        state.start(id, 1, now, None).unwrap();
        assert!(state.cancel("cancelled"));
        state.tick(Some(1), now, |_| panic!("cancelled run resumed"));
    }
}
