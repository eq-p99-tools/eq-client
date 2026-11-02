//! The in-client scenario workbench. Documents, execution and presentation stay
//! separate; the command-line runner and this window use the same `Script`.

mod document;
mod editor;
mod help;
mod input;
mod library;
mod syntax;
mod view;

use std::path::PathBuf;

use bevy::{input_focus::InputFocus, prelude::*, text::EditableText};

use crate::{
    background::Background,
    script::{Phase, Request, RunControl, RunStatus, Script, Step},
    windows::{Shown, WindowId},
};
use document::Document;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Editor,
    Execution,
    Help,
}

#[derive(Clone, Debug)]
enum Replace {
    New,
    Open(PathBuf),
}

enum FileResult {
    Open(Result<(PathBuf, String), String>),
    Save(Result<(PathBuf, String), String>),
}

#[derive(Resource, Default)]
struct Workbench {
    library: library::Library,
    library_collapsed: bool,
    document: Document,
    path: String,
    tab: Tab,
    help_page: usize,
    message: String,
    confirm: Option<Replace>,
    job: Background<FileResult>,
    generation: u64,
    /// Replaces the widgets only when the document itself was loaded or reset.
    revision: u64,
}

impl Workbench {
    fn busy(&self) -> bool {
        self.job.pending(Some(self.generation))
    }

    fn replace(&mut self, action: Replace) {
        if self.document.dirty() {
            self.confirm = Some(action);
            self.message = "Unsaved edits. Choose Discard edits to continue, or Cancel.".into();
        } else {
            self.replace_now(action);
        }
    }

    fn replace_now(&mut self, action: Replace) {
        self.generation = self.generation.wrapping_add(1);
        self.confirm = None;
        match action {
            Replace::New => {
                self.document = Document::new(String::new(), None);
                self.path.clear();
                self.revision = self.revision.wrapping_add(1);
                self.message = "New document. Enter a file path to save it.".into();
            }
            Replace::Open(path) => {
                self.message = "Reading script…".into();
                let root = self.library.root.clone();
                self.job.start(self.generation, move || {
                    FileResult::Open((|| {
                        if let Some(root) = root.filter(|root| path.starts_with(root)) {
                            library::prepare(&root, &path, false)?;
                        }
                        document::read(&path).map(|text| (path, text))
                    })())
                });
            }
        }
    }

    fn save(&mut self) {
        if self.path.trim().is_empty() {
            self.message = "Enter a script name, such as tests/camera.eqscript.".into();
            return;
        }
        let path = match self.library.path(&self.path) {
            Ok(path) => path,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        let Some(root) = self.library.root.clone() else {
            return;
        };
        let save = self.document.save(path.clone());
        self.generation = self.generation.wrapping_add(1);
        self.message = "Saving script…".into();
        self.job.start(self.generation, move || {
            FileResult::Save(library::prepare(&root, &path, true).and_then(|()| save.write()))
        });
    }
}

/// Installs the workbench on every launch, independent of CLI script options.
pub(crate) fn install(app: &mut App) {
    let config = &app.world().resource::<crate::ViewerSettings>().0;
    let mut workbench = Workbench::default();
    workbench.library.root = config
        .eq_directory
        .as_ref()
        .map(|directory| directory.join("Scripts"));
    if let Some(program) = &config.script {
        workbench.document = Document::new(program.source().to_owned(), config.script_path.clone());
        workbench.path = config
            .script_path
            .as_ref()
            .map_or_else(String::new, |path| workbench.library.relative(path));
        workbench.tab = Tab::Execution;
    }
    if config.script_editor {
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Scripts);
    }
    editor::install(app);
    app.insert_resource(workbench)
        .add_systems(Startup, view::spawn)
        .add_systems(
            PreUpdate,
            (release_running_editor, input::forward)
                .chain()
                .after(bevy::input::InputSystems)
                .before(bevy::input_focus::InputFocusSystems::Dispatch)
                .before(crate::script::drive),
        )
        .add_systems(
            Update,
            claim_keyboard
                .before(crate::character_create::input)
                .before(crate::chat::input)
                .in_set(crate::Stage::Typing),
        )
        .add_systems(
            PostUpdate,
            (read_fields, library::actions, controls)
                .chain()
                .after(bevy::text::EditableTextSystems)
                .after(bevy_egui::EguiPostUpdateSet::EndPass),
        )
        .add_systems(
            Update,
            (
                library::poll,
                poll,
                library::draw,
                view::refresh,
                view::scroll,
                library::scroll,
            )
                .chain()
                .in_set(crate::Stage::Present),
        );
}

/// Script-injected keys must never be interpreted as edits to their own source.
#[allow(clippy::needless_pass_by_value)]
fn release_running_editor(
    script: Option<Res<Script>>,
    state: Res<Workbench>,
    fields: Query<(), With<view::Field>>,
    mut focus: ResMut<InputFocus>,
) {
    if (script.is_some() || state.busy())
        && focus.get().is_some_and(|entity| fields.contains(entity))
    {
        focus.clear();
    }
}

#[allow(clippy::needless_pass_by_value)]
fn read_fields(
    fields: Query<(&view::Field, &EditableText), Changed<EditableText>>,
    mut state: ResMut<Workbench>,
    script: Option<Res<Script>>,
) {
    if script.is_some() || state.busy() {
        return;
    }
    for (field, editor) in &fields {
        let text = editor.value().to_string();
        match field {
            view::Field::Path => state.path = text,
            view::Field::Source => state.document.text = text,
        }
    }
}

/// Prevent editing keys leaking into movement, chat, inventory or login.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn claim_keyboard(
    mut typing: ResMut<crate::keys::Typing>,
    mut focus: ResMut<InputFocus>,
    fields: Query<(&view::Field, &Interaction)>,
    frames: Query<&crate::windows::Frame, With<view::Root>>,
    shown: Res<Shown>,
    state: Res<Workbench>,
    script: Option<Res<Script>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    let ours = focus.get().is_some_and(|entity| fields.contains(entity));
    let hidden = !shown.displayed(WindowId::Scripts)
        || state.tab != Tab::Editor
        || frames.iter().any(crate::windows::Frame::minimized);
    // The source editor routes ordered pointer events itself, including a
    // complete click between frames. Bevy may already report Hovered here.
    let source = focus.get().is_some_and(|entity| {
        fields
            .get(entity)
            .is_ok_and(|(field, _)| matches!(field, view::Field::Source))
    });
    let outside = !source
        && mouse.just_pressed(MouseButton::Left)
        && !fields
            .iter()
            .any(|(_, interaction)| *interaction == Interaction::Pressed);
    if ours && (hidden || outside || script.is_some() || state.busy()) {
        focus.clear();
    }
    typing.editing_script = focus.get().is_some_and(|entity| fields.contains(entity));
    if typing.editing_script {
        typing.composing = false;
        typing.counting = false;
    }
}

/// Route explicit user actions; loading, saving and opening Help never run code.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
fn controls(
    mut commands: Commands,
    buttons: crate::clicks::Clicks<view::Action>,
    mut state: ResMut<Workbench>,
    (status, mut control): (Res<RunStatus>, ResMut<RunControl>),
    script: Option<Res<Script>>,
    settings: Res<crate::ViewerSettings>,
    online: Res<crate::online::OnlineState>,
    mut focus: ResMut<InputFocus>,
) {
    for action in buttons.iter() {
        use view::Action;
        match action {
            Action::Tab(Tab::Editor) if script.is_some() => {
                state.tab = Tab::Execution;
                state.message = "Stop the current run before editing its source.".into();
            }
            Action::Tab(tab) => state.tab = *tab,
            Action::ToggleLibrary => state.library_collapsed = !state.library_collapsed,
            Action::Help(page) => {
                state.help_page = *page;
                state.tab = Tab::Help;
            }
            Action::Pause if script.is_some() => control.request(Request::Pause),
            Action::Resume if status.phase() == Phase::Paused => control.request(Request::Resume),
            Action::Step if status.phase() == Phase::Paused => control.request(Request::Step),
            Action::Stop if script.is_some() => control.request(Request::Stop),
            Action::Cancel => {
                state.confirm = None;
                state.message.clear();
            }
            _ if script.is_some() || state.busy() => (),
            Action::Discard => {
                if let Some(action) = state.confirm.take() {
                    state.replace_now(action);
                }
            }
            Action::New => state.replace(Replace::New),
            Action::Open => match state.library.path(&state.path) {
                Ok(path) => state.replace(Replace::Open(path)),
                Err(error) => state.message = error,
            },
            Action::Reload => {
                if let Some(path) = state.document.path.clone() {
                    state.replace(Replace::Open(path));
                } else {
                    state.message = "This document has not been loaded or saved yet.".into();
                }
            }
            Action::Save => state.save(),
            Action::Run => match state.document.program() {
                Ok(program) if program.instructions().is_empty() => {
                    state.message = "Add an action before running.".into();
                }
                Ok(program) => {
                    if !settings.0.local_session
                        && program.instructions().iter().any(|instruction| {
                            matches!(instruction.step(), Step::Gm(_) | Step::Chat(_))
                        })
                    {
                        state.message =
                            "gm and chat require a network-enforced local test session.".into();
                        continue;
                    }
                    let script = Script::from_program(program)
                        .local_session(settings.0.local_session)
                        .offline_preview(!online.enabled);
                    commands.insert_resource(script.status().clone());
                    commands.insert_resource(RunControl::default());
                    commands.insert_resource(script);
                    focus.clear();
                    state.tab = Tab::Execution;
                    state.message = "Running the current buffer; the file is unchanged.".into();
                }
                Err(error) => state.message = error.to_string(),
            },
            Action::Pause | Action::Resume | Action::Step | Action::Stop => (),
        }
    }
}

/// Install only this document's result, preserving the buffer on every failure.
fn poll(mut state: ResMut<Workbench>) {
    let generation = state.generation;
    let Some(result) = state.job.poll(Some(generation)) else {
        return;
    };
    match result {
        FileResult::Open(Ok((path, text))) => {
            state.path = state.library.relative(&path);
            state.document = Document::loaded(path, text);
            state.revision = state.revision.wrapping_add(1);
            state.message = "Loaded. Run starts the scenario.".into();
        }
        FileResult::Save(Ok((path, text))) => {
            state.document.saved(path, text);
            state.library.invalidate();
            state.message = "Saved.".into();
        }
        FileResult::Open(Err(error)) | FileResult::Save(Err(error)) => state.message = error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input_focus::FocusCause;

    #[test]
    fn views_build_and_run_uses_an_immutable_buffer_snapshot() {
        let mut app = crate::testing::app();
        app.init_resource::<Workbench>()
            .init_resource::<InputFocus>()
            .init_resource::<Assets<Font>>()
            .init_resource::<RunStatus>()
            .init_resource::<RunControl>()
            .init_resource::<crate::escape::Escape>()
            .add_systems(Startup, view::spawn)
            .add_systems(
                Update,
                (crate::skinned::close, controls, view::refresh, view::scroll).chain(),
            );
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Scripts);
        app.update();
        let files = app
            .world_mut()
            .query::<(Entity, &view::Action)>()
            .iter(app.world())
            .find_map(|(entity, action)| {
                matches!(action, view::Action::ToggleLibrary).then_some(entity)
            })
            .unwrap();
        crate::clicks::testing::click(&mut app, files);
        assert_eq!(
            app.world_mut()
                .query_filtered::<&Node, With<view::LibraryPane>>()
                .single(app.world())
                .unwrap()
                .display,
            Display::None
        );
        crate::clicks::testing::click(&mut app, files);
        assert_eq!(
            app.world_mut()
                .query_filtered::<&Node, With<view::LibraryPane>>()
                .single(app.world())
                .unwrap()
                .display,
            Display::Flex
        );
        let run = app
            .world_mut()
            .query::<(Entity, &view::Action)>()
            .iter(app.world())
            .find_map(|(entity, action)| matches!(action, view::Action::Run).then_some(entity))
            .unwrap();
        let source = app.world().resource::<Workbench>().document.text.clone();
        crate::clicks::testing::click(&mut app, run);
        assert_eq!(app.world().resource::<Script>().status().source(), source);
        app.world_mut().resource_mut::<Workbench>().document.text = "wait 10\n".into();
        assert_eq!(app.world().resource::<Script>().status().source(), source);
        app.world_mut().resource_mut::<Workbench>().tab = Tab::Help;
        app.update();
        assert!(app.world().contains_resource::<Script>());
        let close = app
            .world_mut()
            .query_filtered::<Entity, With<crate::skinned::Closes>>()
            .single(app.world())
            .unwrap();
        assert!(app.world().get::<crate::windows::Minimize>(close).is_none());
        crate::clicks::testing::click(&mut app, close);
        assert!(!app.world().resource::<Shown>().displayed(WindowId::Scripts));
        assert_eq!(app.world().resource::<Script>().status().source(), source);
        assert_eq!(
            app.world().resource::<Workbench>().document.text,
            "wait 10\n"
        );
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Scripts);
        app.update();
        let root = app
            .world_mut()
            .query_filtered::<&Node, With<view::Root>>()
            .single(app.world())
            .unwrap();
        assert_eq!(root.display, Display::Flex);
    }

    #[test]
    fn replacing_a_dirty_document_requires_an_explicit_discard() {
        let mut state = Workbench::default();
        state.document.text.push_str("wait 50\n");
        let text = state.document.text.clone();
        state.replace(Replace::New);
        assert!(state.confirm.is_some());
        assert_eq!(state.document.text, text);
        state.confirm = None; // Cancel is lossless.
        assert_eq!(state.document.text, text);
        state.replace_now(Replace::New);
        assert_eq!(state.document.text, "");
    }

    #[test]
    fn failed_or_stale_file_loads_do_not_replace_the_document() {
        let mut app = App::new();
        app.init_resource::<Workbench>().add_systems(Update, poll);
        let text = app.world().resource::<Workbench>().document.text.clone();
        app.world_mut()
            .resource_mut::<Workbench>()
            .job
            .start(0, || FileResult::Open(Err("missing".into())));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.world().resource::<Workbench>().busy() {
            app.update();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let state = app.world().resource::<Workbench>();
        assert_eq!(state.document.text, text);
        assert_eq!(state.message, "missing");
        let mut state = app.world_mut().resource_mut::<Workbench>();
        state.job.start(0, || {
            FileResult::Open(Ok(("old.eqscript".into(), "wait 1".into())))
        });
        state.generation = 1;
        app.update();
        assert_eq!(app.world().resource::<Workbench>().document.text, text);
    }

    #[test]
    fn an_editor_has_exclusive_keyboard_focus_and_hiding_releases_it() {
        let mut app = crate::testing::app();
        app.init_resource::<Workbench>()
            .init_resource::<InputFocus>()
            .add_systems(Update, claim_keyboard);
        let field = app
            .world_mut()
            .spawn((view::Field::Source, Interaction::None))
            .id();
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Scripts);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Pressed);
        app.update();
        assert!(app.world().resource::<crate::keys::Typing>().editing_script);
        *app.world_mut().get_mut::<Interaction>(field).unwrap() = Interaction::Hovered;
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.press(MouseButton::Left);
            mouse.release(MouseButton::Left);
        }
        app.update();
        assert!(app.world().resource::<crate::keys::Typing>().editing_script);
        app.world_mut()
            .resource_mut::<Shown>()
            .close(WindowId::Scripts);
        app.update();
        assert!(!app.world().resource::<crate::keys::Typing>().editing_script);
        assert!(app.world().resource::<InputFocus>().get().is_none());
    }

    #[test]
    fn a_running_script_cannot_type_into_its_source() {
        let mut app = App::new();
        app.init_resource::<Workbench>()
            .init_resource::<InputFocus>()
            .insert_resource(Script::new(vec![Step::Wait(
                std::time::Duration::from_secs(1),
            )]))
            .add_systems(Update, release_running_editor);
        let field = app.world_mut().spawn(view::Field::Source).id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Pressed);
        app.update();
        assert!(app.world().resource::<InputFocus>().get().is_none());
    }
}
