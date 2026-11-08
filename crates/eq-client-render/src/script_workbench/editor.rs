//! A real code editor, embedded as one texture in the existing Bevy window.
//! egui owns text layout, editing, selection and undo; the workbench retains
//! document/file ownership and the executor remains the validation authority.

mod buffer;
mod input;
mod surface;

use bevy::{ecs::schedule::ScheduleLabel, prelude::*};
use bevy_egui::{EguiContext, EguiGlobalSettings, EguiPlugin, egui};
use egui_code_editor::{CodeEditor, ColorTheme, Syntax};

use super::{Tab, Workbench};
use crate::script::{ParseError, Program};

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
    checked: Option<String>,
    errors: Vec<ParseError>,
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

fn syntax() -> Syntax {
    Syntax::new("eqscript")
        .with_comment("#")
        .with_comment_multiline(["#", "#"])
        .with_quotes([])
        .with_hyperlinks([])
        .with_keywords([
            "login",
            "wait_servers",
            "server",
            "wait_select",
            "select",
            "create",
            "wait_online",
            "wait_zone",
            "wait_dead",
            "slash",
            "gm",
            "chat",
            "drag",
            "press",
            "hold",
            "wait",
            "camera",
            "trace",
            "face",
            "move",
            "approach",
            "chase",
            "walk",
            "give",
            "interact",
            "inspect_link",
            "compose_link",
            "click",
            "right_click",
            "hover",
            "hold_click",
            "report",
            "sound",
            "screenshot",
            "quit",
        ])
        .with_special([
            "CTRL", "ALT", "SHIFT", "ENTER", "ESCAPE", "SPACE", "UP", "DOWN", "LEFT", "RIGHT",
        ])
}

/// The editor renders once; diagnostics annotate its own row geometry.
#[allow(clippy::needless_pass_by_value)]
fn draw(
    mut contexts: Query<&mut EguiContext, With<EditorCamera>>,
    mut state: ResMut<State>,
    mut workbench: ResMut<Workbench>,
    mut language: Local<Option<Syntax>>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    surfaces: Query<(), With<Surface>>,
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
    }
    let syntax = language.get_or_insert_with(syntax);
    let mut viewport = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("eqscript-viewport"),
        egui::UiBuilder::new().max_rect(ctx.viewport_rect()),
    );
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(&mut viewport, |ui| {
            if workbench.busy() {
                ui.disable();
            }
            let mut buffer = buffer::Bounded(&mut workbench.document.text);
            let (output, _) = CodeEditor::default()
                .id_source("eqscript-source")
                .with_fontsize(14.0)
                .with_rows(14)
                .with_theme(ColorTheme::AYU_DARK)
                .with_numlines(true)
                .with_clickable_links(false)
                .show(ui, &mut buffer, syntax);
            // Escape releases the native keyboard gate along with egui's caret,
            // including a host click that focused only the gutter or scrollbar.
            if state.focused
                && (output.response.lost_focus()
                    || ctx.input(|input| input.key_pressed(egui::Key::Escape)))
            {
                state.focused = false;
                if focus.get().is_some_and(|entity| surfaces.contains(entity)) {
                    focus.clear();
                }
            }
            if state.checked.as_deref() != Some(&workbench.document.text) {
                state.errors =
                    Program::diagnostics(&workbench.document.text, std::path::Path::new("."));
                state.checked = Some(workbench.document.text.clone());
            }
            diagnostics(ui, &output, &state.errors);
        });
}

fn diagnostics(ui: &mut egui::Ui, output: &egui::text_edit::TextEditOutput, errors: &[ParseError]) {
    let red = egui::Color32::from_rgb(255, 105, 100);
    let painter = ui.painter().with_clip_rect(output.text_clip_rect);
    for error in errors {
        let Some(line) = error.line else {
            continue;
        };
        let Some(row) = output.galley.rows.get(line - 1) else {
            continue;
        };
        let rect = row.rect().translate(output.galley_pos.to_vec2());
        if !rect.intersects(output.text_clip_rect) {
            continue;
        }
        ui.painter().circle_filled(
            egui::pos2(output.text_clip_rect.left() - 5.0, rect.center().y),
            2.5,
            red,
        );
        let start = egui::pos2(
            rect.left().max(output.text_clip_rect.left()),
            rect.bottom() - 1.0,
        );
        let end = egui::pos2(
            rect.right()
                .max(start.x + 12.0)
                .min(output.text_clip_rect.right()),
            start.y,
        );
        painter.line_segment([start, end], egui::Stroke::new(1.5, red));
        ui.interact(
            rect.intersect(output.text_clip_rect),
            ui.id().with(("error", line)),
            egui::Sense::hover(),
        )
        .on_hover_text(error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_code_editor::{Token, TokenType};

    #[test]
    fn language_has_line_comments_and_no_quoted_strings() {
        let source = "wait 25\nreport \"# café comment\npress CTRL+I\n";
        let tokens = Token::default().tokens(&syntax(), source);
        assert_eq!(tokens.iter().map(Token::buffer).collect::<String>(), source);
        assert!(
            tokens
                .iter()
                .any(|token| token.ty() == TokenType::Keyword && token.buffer() == "wait")
        );
        assert!(tokens.iter().any(
            |token| token.ty() == TokenType::Comment(false) && token.buffer().contains("café")
        ));
        assert!(
            !tokens
                .iter()
                .any(|token| matches!(token.ty(), TokenType::Str(_) | TokenType::Comment(true)))
        );
    }

    #[test]
    fn editor_renders_and_reports_multiple_errors_without_a_window() {
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
        app.world_mut().resource_mut::<Workbench>().document.text =
            "# note\nwait nope\npress UNKNOWN\nreport café\n".into();
        let mut context = EguiContext::default();
        let ctx = context.get_mut().clone();
        app.world_mut().spawn((EditorCamera, context));
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: Some(egui::Key::Escape),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 280.0),
                )),
                ..default()
            },
            |_| app.world_mut().run_schedule(CodePass),
        );
        assert_ne!(output.shapes.len(), 0);
        // This headless test inspects geometry; there is no renderer to upload textures.
        output.textures_delta.clear();
        assert!(!app.world().resource::<State>().focused);
        assert!(
            app.world()
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                .is_none()
        );
        let errors = &app.world().resource::<State>().errors;
        assert_eq!(
            errors.iter().map(|error| error.line).collect::<Vec<_>>(),
            [Some(2), Some(3)]
        );
        assert!(
            app.world()
                .resource::<Workbench>()
                .document
                .text
                .ends_with("report café\n")
        );
    }
}
