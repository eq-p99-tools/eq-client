//! Saved-file shortcuts share slot storage and the ordinary hotkey carry rule.
//! Only the exact runner started here accepts pause/resume/stop from a shortcut.
use super::{Action, Bindings, Slot};
use crate::{
    background::Background,
    profile_files::{CharacterProfile, Profile},
    script::{Phase, Program, Request, RunControl, Script},
};
use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Default)]
pub(super) struct Files(BTreeMap<u32, PathBuf>);

impl Files {
    pub(super) fn read(text: &str) -> Self {
        let mut files = BTreeMap::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let Some(id) = key
                .trim()
                .strip_prefix("script_path_")
                .and_then(|id| id.parse::<u32>().ok())
                .filter(|id| *id != 0)
            else {
                continue;
            };
            if let Ok(path) = serde_json::from_str::<PathBuf>(value.trim())
                && is_script(&path)
            {
                files.insert(id, path);
            }
        }
        Self(files)
    }
}

fn is_script(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("eqscript"))
}

impl Bindings {
    /// Register one saved file without hashing or changing any existing slot.
    pub(crate) fn script_binding(&mut self, path: PathBuf) -> Result<Action, &'static str> {
        if !is_script(&path) {
            return Err("Save this script with the .eqscript extension first.");
        }
        if serde_json::to_string(&path).is_err() {
            return Err("Cannot store this script's file name.");
        }
        if let Some((&id, _)) = self.2.0.iter().find(|(_, saved)| **saved == path) {
            return Ok(Action::Script(id));
        }
        let id = self
            .2
            .0
            .last_key_value()
            .map_or(Some(1), |(id, _)| id.checked_add(1))
            .ok_or("The script shortcut registry is full.")?;
        self.2.0.insert(id, path);
        Ok(Action::Script(id))
    }

    pub(crate) fn script_path(&self, id: u32) -> Option<&Path> {
        self.2.0.get(&id).map(PathBuf::as_path)
    }

    pub(super) fn text(&self) -> String {
        let mut text = self.snapshot().text();
        for (id, path) in &self.2.0 {
            if let Ok(path) = serde_json::to_string(path) {
                let _ = writeln!(text, "script_path_{id} = {path}");
            }
        }
        text
    }

    pub(super) fn script_name(&self, id: u32) -> String {
        self.script_path(id).and_then(Path::file_stem).map_or_else(
            || "Missing script".into(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    pub(super) fn script_detail(&self, id: u32) -> String {
        format!(
            "{}\nPress to start, pause or resume; right-click to stop. Runs the saved file.",
            self.script_name(id)
        )
    }
}

struct Scope {
    profile: CharacterProfile,
    admission: Option<u64>,
    id: u32,
}

impl Scope {
    fn current(&self, profile: &Profile, admission: Option<u64>) -> bool {
        profile.key_ref() == Some(&self.profile) && admission == self.admission
    }
}

struct Owner {
    scope: Scope,
    run: Arc<()>,
}

struct Loading {
    scope: Scope,
    path: PathBuf,
    root: PathBuf,
    pause: bool,
}

#[derive(Default, Resource)]
pub(crate) struct Runs {
    owner: Option<Owner>,
    loading: Option<Loading>,
    generation: u64,
    job: Background<Result<Program, String>>,
}

/// Lets the skin use its latched button art without faking pointer interaction.
#[derive(Component)]
pub(crate) struct Latched {
    outline: Option<Outline>,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum View {
    #[default]
    Idle,
    Loading,
    Running,
    Pausing,
    Paused,
}

impl View {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Idle => "Script",
            Self::Loading => "Loading",
            Self::Running => "Running",
            Self::Pausing => "Pausing",
            Self::Paused => "Paused",
        }
    }
}

impl Runs {
    pub(super) fn view(&self, id: u32, profile: &Profile, script: Option<&Script>) -> View {
        if self.loading.as_ref().is_some_and(|load| {
            load.scope.id == id && profile.key_ref() == Some(&load.scope.profile)
        }) {
            return View::Loading;
        }
        let Some(script) = script.filter(|script| self.owns(id, profile, script)) else {
            return View::Idle;
        };
        match script.status().phase() {
            Phase::Paused => View::Paused,
            Phase::Pausing => View::Pausing,
            phase if phase.active() => View::Running,
            _ => View::Idle,
        }
    }

    fn owns(&self, id: u32, profile: &Profile, script: &Script) -> bool {
        self.owner.as_ref().is_some_and(|owner| {
            owner.scope.id == id
                && profile.key_ref() == Some(&owner.scope.profile)
                && Arc::ptr_eq(&owner.run, script.status().run())
        })
    }

    fn cancel_loading(&mut self) {
        self.loading = None;
        self.job.clear();
    }

    fn begin(&mut self, scope: Scope, path: PathBuf, root: PathBuf) {
        self.generation = self.generation.wrapping_add(1);
        self.loading = Some(Loading {
            scope,
            path: path.clone(),
            root: root.clone(),
            pause: false,
        });
        self.job.start(self.generation, move || {
            crate::script_workbench::hotbutton_program(&path, &root)
        });
    }

    fn finish(
        &mut self,
        commands: &mut Commands,
        settings: &crate::ViewerConfig,
        online: bool,
        busy: bool,
        chat: &mut crate::chat::ChatState,
    ) -> bool {
        let Some(result) = self.job.poll(Some(self.generation)) else {
            return false;
        };
        let Some(load) = self.loading.take() else {
            return true;
        };
        match result {
            Ok(program) if !busy => {
                let script = Script::for_viewer(program, settings, online);
                self.owner = Some(Owner {
                    scope: load.scope,
                    run: script.status().run().clone(),
                });
                commands.insert_resource(script.status().clone());
                let mut control = RunControl::default();
                if load.pause {
                    control.request_for(script.status().run(), Request::Pause);
                }
                commands.insert_resource(control);
                commands.insert_resource(script);
            }
            Ok(_) => chat.refuse("Another script is running; this shortcut was not started."),
            Err(error) => chat.refuse(format!("Could not start script: {error}")),
        }
        true
    }

    fn activate(
        &mut self,
        id: u32,
        right: bool,
        (profile, admission, root): (&Profile, Option<u64>, Option<PathBuf>),
        (bindings, script): (&Bindings, Option<&Script>),
        (control, chat): (&mut RunControl, &mut crate::chat::ChatState),
    ) {
        if let Some(script) = script {
            if self.owns(id, profile, script) && script.status().phase().active() {
                let request = if right {
                    Request::Stop
                } else if script.status().phase() == Phase::Paused {
                    Request::Resume
                } else {
                    Request::Pause
                };
                control.request_for(script.status().run(), request);
            } else if !right {
                chat.refuse("Another script is running; stop it before starting this shortcut.");
            }
        } else if right {
            if self
                .loading
                .as_ref()
                .is_some_and(|load| load.scope.id == id)
            {
                self.cancel_loading();
            }
        } else if let Some(load) = &mut self.loading {
            if load.scope.id == id {
                load.pause = !load.pause;
            } else {
                chat.refuse("A script is loading. Wait, or right-click its hotbutton to cancel.");
            }
        } else {
            let Some(profile) = profile.key() else { return };
            let Some(root) = root else {
                chat.refuse("Choose the EQ installation containing this script's Scripts folder.");
                return;
            };
            match bindings.script_path(id) {
                Some(path) => self.begin(
                    Scope {
                        profile,
                        admission,
                        id,
                    },
                    path.to_owned(),
                    root,
                ),
                None => chat
                    .refuse("This script binding has no saved file. Create its hotbutton again."),
            }
        }
    }
}

/// Native slot clicks and bound hotkeys use the ordinary click/carry routing.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn input(
    mut commands: Commands,
    keys: crate::keys::Keys,
    clicks: crate::clicks::Clicks<Slot>,
    (mouse, windows): (
        Res<ButtonInput<MouseButton>>,
        Query<&Window, With<bevy::window::PrimaryWindow>>,
    ),
    slots: Query<(&Slot, &Interaction)>,
    (bindings, profile): (Res<Bindings>, Res<Profile>),
    (online, settings): (Res<crate::online::OnlineState>, Res<crate::ViewerSettings>),
    (script, authored): (
        Option<Res<Script>>,
        Option<Res<crate::script::AuthoredInput>>,
    ),
    (mut runs, mut control): (ResMut<Runs>, ResMut<RunControl>),
    mut chat: ResMut<crate::chat::ChatState>,
) {
    let admission = online.world().session_id();
    let root = settings
        .0
        .eq_directory
        .as_ref()
        .map(|directory| directory.join("Scripts"));
    if runs.loading.as_ref().is_some_and(|load| {
        !load.scope.current(&profile, admission)
            || bindings.script_path(load.scope.id) != Some(load.path.as_path())
            || root.as_ref() != Some(&load.root)
    }) {
        runs.cancel_loading();
    }
    if runs.finish(
        &mut commands,
        &settings.0,
        online.enabled,
        script.is_some(),
        &mut chat,
    ) {
        return;
    }
    if !windows.single().is_ok_and(|window| window.focused) {
        return;
    }
    let right = mouse.just_pressed(MouseButton::Right)
        && !authored
            .as_ref()
            .is_some_and(|input| input.button(MouseButton::Right));
    let asked = if right {
        slots
            .iter()
            .find(|(_, interaction)| **interaction != Interaction::None)
            .map(|(slot, _)| (slot.0, None))
    } else {
        super::requested_slot(&keys, &clicks)
    };
    let Some((slot, key)) = asked else { return };
    let Some(Action::Script(id)) = bindings.0.get(slot).copied().flatten() else {
        return;
    };
    if !right
        && authored.as_ref().is_some_and(|input| {
            key.map_or(input.left_button, |act| {
                keys.map.keys(act).any(|code| input.keys.contains(&code))
            })
        })
    {
        return;
    }
    runs.activate(
        id,
        right,
        (&profile, admission, root),
        (&bindings, script.as_deref()),
        (&mut control, &mut chat),
    );
}

/// Reflects the real executor after ordinary slot presentation, before text fit.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub(crate) fn presentation(
    mut commands: Commands,
    bindings: Res<Bindings>,
    (runs, profile, script): (Res<Runs>, Res<Profile>, Option<Res<Script>>),
    mut slots: Query<(
        Entity,
        &Slot,
        Option<&mut BackgroundColor>,
        Has<crate::skinned::SkinButton>,
        Option<&Latched>,
        Option<&Outline>,
    )>,
    mut words: Query<(&super::Words, &mut Text)>,
) {
    for (entity, slot, color, skinned, latched, outline) in &mut slots {
        let view = match bindings.0[slot.0] {
            Some(Action::Script(id)) => runs.view(id, &profile, script.as_deref()),
            _ => View::Idle,
        };
        if view == View::Idle {
            if let Some(latched) = latched {
                let mut frame = commands.entity(entity);
                frame.remove::<Latched>();
                if let Some(previous) = latched.outline {
                    frame.insert(previous);
                } else {
                    frame.remove::<Outline>();
                }
            }
        } else {
            if latched.is_none() {
                commands.entity(entity).insert(Latched {
                    outline: outline.copied(),
                });
            }
            let color = if view == View::Paused {
                Color::srgb(1.0, 0.72, 0.25)
            } else {
                Color::srgb(0.3, 0.85, 0.45)
            };
            let wanted = Outline::new(px(2), px(0), color);
            if outline != Some(&wanted) {
                commands.entity(entity).insert(wanted);
            }
        }
        if !skinned
            && view != View::Idle
            && let Some(mut color) = color
        {
            color.0 = if view == View::Paused {
                Color::srgb(0.28, 0.20, 0.06)
            } else {
                crate::theme::BUTTON_ON
            };
        }
    }
    for (word, mut text) in &mut words {
        if let Some(Action::Script(id)) = bindings.0[word.0] {
            let view = runs.view(id, &profile, script.as_deref());
            let wanted = format!("{}\n{}", view.label(), bindings.script_name(id));
            if text.0 != wanted {
                text.0 = wanted;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
