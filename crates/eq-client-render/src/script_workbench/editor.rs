//! A real code editor, embedded as one texture in the existing Bevy window.
//! egui owns text layout, editing, selection and undo; the workbench retains
//! document/file ownership and the executor's parser remains the validation
//! authority: the editor only shows the errors it reports.

mod buffer;
mod code;
mod input;
mod surface;

use bevy::{ecs::schedule::ScheduleLabel, prelude::*};
use bevy_egui::{EguiContext, EguiGlobalSettings, EguiPlugin, egui};

use super::{Tab, Workbench};

#[derive(Component)]
pub(super) struct Surface;

#[derive(Component)]
struct EditorCamera;

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct CodePass;

#[derive(Resource, Default)]
struct State {
    image: Handle<Image>,
    visible: bool,
    focused: bool,
    pointer: Pointer,
    ime_owned: bool,
    revision: Option<u64>,
    /// The check generation `marks` were placed from.
    marked: Option<u64>,
    marks: Vec<code::Mark>,
    code: code::Memory,
    /// The text the breakpoint dots were last placed against.
    shadow: String,
}

#[derive(Default)]
struct Pointer {
    captured: bool,
    hovered: bool,
    position: Option<Vec2>,
}

pub(super) fn install(app: &mut App) {
    app.add_plugins(EguiPlugin::default())
        .insert_resource(EguiGlobalSettings {
            auto_create_primary_context: false,
            enable_focused_non_window_context_updates: false,
            enable_cursor_icon_updates: false,
            ..default()
        })
        .init_resource::<State>()
        .add_systems(Startup, surface::spawn.after(super::view::spawn))
        .add_systems(
            PostUpdate,
            surface::resize
                .after(bevy::ui::UiSystems::Layout)
                .before(bevy_egui::EguiPostUpdateSet::EndPass),
        )
        .add_systems(
            PostUpdate,
            surface::platform.after(bevy_egui::input::process_ime_system),
        )
        .add_systems(CodePass, draw);
    input::install(app);
}

/// The run the editor's source may describe: the script, its control and
/// its status.
type Run<'w> = (
    Option<Res<'w, crate::script::Script>>,
    Option<ResMut<'w, crate::script::RunControl>>,
    Option<Res<'w, crate::script::RunStatus>>,
);

/// The editor renders once; the parser's marks annotate its own geometry.
#[allow(clippy::needless_pass_by_value)]
fn draw(
    mut contexts: Query<&mut EguiContext, With<EditorCamera>>,
    mut state: ResMut<State>,
    mut workbench: ResMut<Workbench>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    surfaces: Query<(), With<Surface>>,
    (script, mut control, status): Run,
) {
    let Ok(mut context) = contexts.single_mut() else {
        return;
    };
    let ctx = context.get_mut();
    if !state.visible || workbench.tab != Tab::Editor {
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        return;
    }
    let revision = workbench.revision;
    if state.revision != Some(revision) {
        // This context holds only this editor. Replacement resets undo, cursor
        // and scrolling without retaining state for every file ever opened.
        ctx.data_mut(egui::util::IdTypeMap::clear);
        state.revision = Some(revision);
        state.code.jump = None;
        state.shadow.clone_from(&workbench.document.text);
    }
    let mut viewport = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("eqscript-viewport"),
        egui::UiBuilder::new().max_rect(ctx.viewport_rect()),
    );
    let state = &mut *state;
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(code::BACKGROUND))
        .show(&mut viewport, |ui| {
            if workbench.busy() {
                ui.disable();
            }
            let visuals = ui.visuals_mut();
            visuals.extreme_bg_color = code::BACKGROUND;
            visuals.text_cursor.stroke.color = egui::Color32::from_rgb(255, 204, 102);
            refresh_marks(state, &mut workbench);
            let statements = workbench.document.check().statements.clone();
            let dots = std::mem::take(&mut workbench.document.breakpoints);
            let before = dots.clone();
            let running = script.is_some();
            let shown = code::show(
                ui,
                &mut buffer::Bounded(&mut workbench.document.text, !running),
                &state.marks,
                &mut state.code,
                &code::Dots {
                    lines: &dots,
                    statements: statements.as_ref(),
                },
            );
            workbench.document.breakpoints = dots;
            breakpoints(state, &mut workbench, &shown, ctx);
            // A paused run takes the dots changed here when it continues.
            if running
                && status
                    .as_deref()
                    .is_some_and(|status| workbench.describes(status))
                && workbench.document.breakpoints != before
                && let Some(control) = control.as_mut()
                && let Some(statements) = &statements
            {
                control.set_breakpoints(
                    workbench
                        .document
                        .breakpoints
                        .intersection(statements)
                        .copied()
                        .collect(),
                );
            }
            // Escape releases the native keyboard gate along with egui's caret,
            // including a host click that focused only the gutter or scrollbar.
            // Choosing a problem moves the caret instead, keeping the keyboard.
            if state.focused
                && !shown.chose
                && (shown.output.response.lost_focus()
                    || ctx.input(|input| input.key_pressed(egui::Key::Escape)))
            {
                state.focused = false;
                if focus.get().is_some_and(|entity| surfaces.contains(entity)) {
                    focus.clear();
                }
            }
            // Typing this frame is marked this frame, from the same cached check.
            refresh_marks(state, &mut workbench);
        });
}

/// Keep the dots with their lines through this frame's edit, toggle one for a
/// click on a line number or F9 on the caret's line, and note the caret's
/// line for Run to cursor.
fn breakpoints(
    state: &mut State,
    workbench: &mut Workbench,
    shown: &code::Shown,
    ctx: &egui::Context,
) {
    let document = &mut workbench.document;
    if state.shadow != document.text {
        document.breakpoints =
            super::breakpoints::follow(&document.breakpoints, &state.shadow, &document.text);
        state.shadow.clone_from(&document.text);
    }
    let caret = shown.output.cursor_range.map(|range| {
        let index = range.primary.index.0;
        1 + document
            .text
            .chars()
            .take(index)
            .filter(|ch| *ch == '\n')
            .count()
    });
    if caret.is_some() {
        // A caret moved after a line was picked in the Execution view is
        // the later choice for Run to cursor.
        if workbench.caret_line.is_some() && workbench.caret_line != caret {
            workbench.run_target = None;
        }
        workbench.caret_line = caret;
    }
    // The check of the text as it is now, after this frame's edit.
    let statements = document.check().statements.clone();
    let line = shown.clicked.or_else(|| {
        (shown.output.response.has_focus() && ctx.input(|input| input.key_pressed(egui::Key::F9)))
            .then_some(caret)
            .flatten()
    });
    if let Some(line) = line {
        super::breakpoints::toggle(&mut document.breakpoints, line, statements.as_ref());
    }
}

/// Rebuild the character marks only when the document's check changes.
fn refresh_marks(state: &mut State, workbench: &mut Workbench) {
    let check = workbench.document.check();
    if state.marked != Some(check.generation) {
        state.marked = Some(check.generation);
        let errors = check.errors.clone();
        state.marks = code::marks(&workbench.document.text, &errors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(text: &str) -> (App, egui::Context) {
        let mut app = App::new();
        app.insert_resource(State {
            visible: true,
            focused: true,
            ..default()
        })
        .init_resource::<Workbench>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .add_systems(CodePass, draw);
        let host = app.world_mut().spawn(Surface).id();
        app.world_mut()
            .resource_mut::<bevy::input_focus::InputFocus>()
            .set(host, bevy::input_focus::FocusCause::Pressed);
        app.world_mut().resource_mut::<Workbench>().document.text = text.into();
        let mut context = EguiContext::default();
        let ctx = context.get_mut().clone();
        app.world_mut().spawn((EditorCamera, context));
        (app, ctx)
    }

    fn frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 280.0),
                )),
                ..default()
            },
            |_| app.world_mut().run_schedule(CodePass),
        );
        // This headless test inspects geometry; there is no renderer to upload textures.
        output.textures_delta.clear();
        output
    }

    #[test]
    fn editor_renders_and_reports_multiple_errors_without_a_window() {
        let (mut app, ctx) = app("# note\nwait(nope)\npress(\"UNKNOWN\")\nreport(\"café\")\n");
        let output = frame(
            &mut app,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_ne!(output.shapes.len(), 0);
        assert!(!app.world().resource::<State>().focused);
        assert!(
            app.world()
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                .is_none()
        );
        let marks = &app.world().resource::<State>().marks;
        assert_eq!(
            marks.iter().map(|mark| mark.line).collect::<Vec<_>>(),
            [Some(2), Some(3)]
        );
        // The variable given as an argument, and the call's argument list, in characters.
        assert_eq!(
            marks
                .iter()
                .map(|mark| mark.chars.clone())
                .collect::<Vec<_>>(),
            [Some(12..16), Some(24..33)]
        );
        assert!(
            app.world()
                .resource::<Workbench>()
                .document
                .text
                .ends_with("report(\"café\")\n")
        );
    }

    #[test]
    fn document_level_errors_are_listed_without_a_line() {
        let (mut app, ctx) = app("wait(1)\n");
        frame(&mut app, &ctx, Vec::new());
        assert_eq!(app.world().resource::<State>().marks, []);
        app.world_mut().resource_mut::<Workbench>().document.text =
            "x".repeat(crate::script::MAX_SOURCE_BYTES + 1);
        frame(&mut app, &ctx, Vec::new());
        let marks = &app.world().resource::<State>().marks;
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].line, None);
        assert!(marks[0].message.contains("source limit"));
    }

    #[test]
    fn choosing_a_problem_moves_the_caret_to_its_line() {
        let (mut app, ctx) = app("wait(1)\nwait(2)\npres(\"I\")\n");
        frame(&mut app, &ctx, Vec::new());
        app.world_mut().resource_mut::<State>().code.jump = Some(3);
        frame(&mut app, &ctx, Vec::new());
        let state = egui::text_edit::TextEditState::load(&ctx, code::id()).unwrap();
        let cursor = state.cursor.char_range().unwrap();
        assert_eq!(cursor.primary.index.0, 16);
        assert!(ctx.memory(|memory| memory.has_focus(code::id())));
        assert!(app.world().resource::<State>().code.jump.is_none());
    }

    #[test]
    fn a_runs_source_is_read_only_and_its_dots_go_to_the_run() {
        let source = "wait(1)\nwait(2)\n";
        let (mut app, ctx) = app(source);
        app.insert_resource(crate::script::Script::from_program(
            crate::script::Program::parse(source, std::path::Path::new(".")).unwrap(),
        ))
        .insert_resource(crate::script::RunStatus::paused_at(source, 1))
        .init_resource::<crate::script::RunControl>();
        frame(&mut app, &ctx, Vec::new());
        app.world_mut().resource_mut::<State>().code.jump = Some(2);
        frame(&mut app, &ctx, Vec::new());
        let key = |key| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::Text("x".into()),
                key(egui::Key::Backspace),
                key(egui::Key::F9),
            ],
        );
        let workbench = app.world().resource::<Workbench>();
        assert_eq!(workbench.document.text, source);
        assert_eq!(workbench.caret_line, Some(2));
        assert_eq!(workbench.document.breakpoints, [2].into());
        assert_eq!(
            app.world()
                .resource::<crate::script::RunControl>()
                .pending_breakpoints(),
            Some(&[2].into())
        );
    }

    fn f9() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::F9,
            physical_key: Some(egui::Key::F9),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn dots_set_during_a_run_started_elsewhere_stay_in_the_editor() {
        let source = "wait(1)\nwait(2)\n";
        let (mut app, ctx) = app(source);
        // A run from the command line, whose source is not the document.
        let foreign = "wait(3)\nwait(4)\n";
        app.insert_resource(crate::script::Script::from_program(
            crate::script::Program::parse(foreign, std::path::Path::new(".")).unwrap(),
        ))
        .insert_resource(crate::script::RunStatus::paused_at(foreign, 1))
        .init_resource::<crate::script::RunControl>();
        frame(&mut app, &ctx, Vec::new());
        app.world_mut().resource_mut::<State>().code.jump = Some(2);
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, vec![f9()]);
        assert_eq!(
            app.world().resource::<Workbench>().document.breakpoints,
            [2].into()
        );
        assert_eq!(
            app.world()
                .resource::<crate::script::RunControl>()
                .pending_breakpoints(),
            None
        );
    }

    #[test]
    fn moving_the_caret_drops_a_line_picked_in_the_execution_view() {
        let (mut app, ctx) = app("wait(1)\nwait(2)\nwait(3)\n");
        frame(&mut app, &ctx, Vec::new());
        app.world_mut().resource_mut::<State>().code.jump = Some(1);
        frame(&mut app, &ctx, Vec::new());
        assert_eq!(app.world().resource::<Workbench>().caret_line, Some(1));
        app.world_mut().resource_mut::<Workbench>().run_target = Some(3);
        // A frame without a caret move keeps the pick.
        frame(&mut app, &ctx, Vec::new());
        assert_eq!(app.world().resource::<Workbench>().run_target, Some(3));
        app.world_mut().resource_mut::<State>().code.jump = Some(2);
        frame(&mut app, &ctx, Vec::new());
        let workbench = app.world().resource::<Workbench>();
        assert_eq!(workbench.caret_line, Some(2));
        assert_eq!(workbench.run_target, None);
    }
}
