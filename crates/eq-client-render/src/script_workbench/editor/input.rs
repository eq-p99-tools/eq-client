//! Route input into the embedded editor only. The native window continues to
//! own hit testing, occlusion and the shared game-input exclusion flag.
use super::super::{Tab, Workbench};
use super::{EditorCamera, State, Surface};
use crate::{
    keys::modifiers::snapshots,
    windows::{self, Shown, WindowId},
};
use bevy::{
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::MouseScrollUnit,
    },
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    ui::ComputedStackIndex,
    window::{PrimaryWindow, WindowEvent},
};
use bevy_egui::{
    EguiClipboard, EguiContext, EguiInput, EguiInputSet, EguiPreUpdateSet, egui,
    input::{EguiInputEvent, FocusedNonWindowEguiContext},
};

pub(super) fn install(app: &mut App) {
    app.add_systems(
        PreUpdate,
        pointer
            .after(EguiPreUpdateSet::InitContexts)
            .after(bevy::input::InputSystems)
            .before(EguiInputSet::InitReading)
            .before(super::super::input::forward),
    )
    .add_systems(
        PreUpdate,
        keyboard
            .after(EguiInputSet::InitReading)
            .before(EguiInputSet::WriteEguiEvents),
    )
    .add_systems(
        PreUpdate,
        focus
            .after(EguiInputSet::WriteEguiEvents)
            .before(EguiPreUpdateSet::BeginPass),
    );
}

/// Uses ordered window events so a press/release in one frame stays a click.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::too_many_lines
)]
fn pointer(
    mut commands: Commands,
    window: Query<(Entity, &Window), With<PrimaryWindow>>,
    surface: Query<
        (
            Entity,
            &ComputedNode,
            &UiGlobalTransform,
            &ComputedStackIndex,
        ),
        With<Surface>,
    >,
    covers: Query<
        (&ComputedNode, &UiGlobalTransform, &ComputedStackIndex),
        windows::PointerSurface,
    >,
    frames: Query<&windows::Frame, With<super::super::view::Root>>,
    contexts: Query<Entity, With<EditorCamera>>,
    mut events: MessageReader<WindowEvent>,
    mut output: MessageWriter<EguiInputEvent>,
    keys: Res<ButtonInput<Key>>,
    mut state: ResMut<State>,
    workbench: Res<Workbench>,
    shown: Res<Shown>,
    script: Option<Res<crate::script::Script>>,
    mut native_focus: ResMut<InputFocus>,
) {
    let (Ok((window_id, window)), Ok((host, node, transform, stack)), Ok(context)) =
        (window.single(), surface.single(), contexts.single())
    else {
        events.clear();
        return;
    };
    state.visible = shown.displayed(WindowId::Scripts)
        && workbench.tab == Tab::Editor
        && script.is_none()
        && !frames.iter().any(windows::Frame::minimized);
    let enabled = state.visible && !workbench.busy() && window.focused;
    let modifiers = modifiers([
        keys.pressed(Key::Control),
        keys.pressed(Key::Shift),
        keys.pressed(Key::Alt),
        keys.pressed(Key::Super),
    ]);
    let local = |point: Vec2| {
        transform.try_inverse().map(|inverse| {
            (inverse.transform_point2(point) - node.content_box().min) * node.inverse_scale_factor()
        })
    };
    let hit = |point: Vec2| {
        enabled
            && node.size().cmpgt(Vec2::ZERO).all()
            && windows::contains(point, transform, node)
            && !covers.iter().any(|(other, transform, order)| {
                order.0 > stack.0 && windows::contains(point, transform, other)
            })
    };
    let mut point = state
        .pointer
        .position
        .or_else(|| window.physical_cursor_position());
    let send = |output: &mut MessageWriter<EguiInputEvent>, event| {
        output.write(EguiInputEvent { context, event });
    };
    if !enabled {
        if state.pointer.captured {
            send(
                &mut output,
                egui::Event::PointerButton {
                    pos: point
                        .and_then(local)
                        .map_or(egui::Pos2::ZERO, |point| egui::pos2(point.x, point.y)),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers,
                },
            );
        }
        state.focused = false;
        state.pointer.captured = false;
    }
    for event in events.read() {
        match event {
            WindowEvent::CursorMoved(event) if event.window == window_id => {
                let physical = event.position * window.scale_factor();
                point = Some(physical);
                if (hit(physical) || state.pointer.captured)
                    && let Some(position) = local(physical)
                {
                    send(
                        &mut output,
                        egui::Event::PointerMoved(egui::pos2(position.x, position.y)),
                    );
                }
            }
            WindowEvent::MouseButtonInput(event) if event.window == window_id => {
                let pressed = event.state.is_pressed();
                let inside = point.is_some_and(hit);
                if pressed && event.button == MouseButton::Left {
                    state.focused = inside;
                    state.pointer.captured = inside;
                    if inside {
                        native_focus.set(host, FocusCause::Pressed);
                    } else if native_focus.get() == Some(host) {
                        native_focus.clear();
                    }
                }
                let button = match event.button {
                    MouseButton::Left => Some(egui::PointerButton::Primary),
                    MouseButton::Right => Some(egui::PointerButton::Secondary),
                    MouseButton::Middle => Some(egui::PointerButton::Middle),
                    _ => None,
                };
                if (inside || state.pointer.captured)
                    && let Some(button) = button
                    && let Some(position) = point.and_then(local)
                {
                    send(
                        &mut output,
                        egui::Event::PointerButton {
                            pos: egui::pos2(position.x, position.y),
                            button,
                            pressed,
                            modifiers,
                        },
                    );
                }
                if !pressed && event.button == MouseButton::Left {
                    state.pointer.captured = false;
                }
            }
            WindowEvent::MouseWheel(event)
                if event.window == window_id && point.is_some_and(hit) =>
            {
                send(
                    &mut output,
                    egui::Event::MouseWheel {
                        phase: egui::TouchPhase::Move,
                        unit: match event.unit {
                            MouseScrollUnit::Line => egui::MouseWheelUnit::Line,
                            MouseScrollUnit::Pixel => egui::MouseWheelUnit::Point,
                        },
                        delta: egui::vec2(event.x, event.y),
                        modifiers,
                    },
                );
            }
            _ => (),
        }
    }
    state.pointer.position = point;
    let hovered = point.is_some_and(hit);
    if state.pointer.hovered && !hovered && !state.pointer.captured {
        send(&mut output, egui::Event::PointerGone);
    }
    state.pointer.hovered = hovered;
    if state.focused {
        commands.insert_resource(FocusedNonWindowEguiContext(context));
    } else {
        commands.remove_resource::<FocusedNonWindowEguiContext>();
        if native_focus.get() == Some(host) {
            native_focus.clear();
        }
    }
}

fn modifiers(held: [bool; 4]) -> egui::Modifiers {
    egui::Modifiers {
        ctrl: held[0],
        alt: held[2],
        shift: held[1],
        mac_cmd: cfg!(target_os = "macos") && held[3],
        command: if cfg!(target_os = "macos") {
            held[3]
        } else {
            held[0]
        },
    }
}

/// Preserve each key's modifiers, including complete shortcut chords in one frame.
#[allow(clippy::needless_pass_by_value)]
fn keyboard(
    mut events: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<Key>>,
    state: Res<State>,
    contexts: Query<Entity, With<EditorCamera>>,
    mut output: MessageWriter<EguiInputEvent>,
    mut clipboard: ResMut<EguiClipboard>,
) {
    let events: Vec<_> = events.read().cloned().collect();
    let Ok(context) = contexts.single() else {
        return;
    };
    if !state.focused {
        return;
    }
    for (event, held) in events.iter().zip(snapshots(&events, &keys)) {
        let modifiers = modifiers(held);
        let pressed = event.state.is_pressed();
        let key = bevy_egui::helpers::bevy_to_egui_key(&event.logical_key);
        if pressed && modifiers.command && !modifiers.alt {
            let action = match key {
                Some(egui::Key::C) => Some(egui::Event::Copy),
                Some(egui::Key::X) => Some(egui::Event::Cut),
                Some(egui::Key::V) => clipboard.get_text().map(egui::Event::Paste),
                _ => None,
            };
            if let Some(event) = action {
                output.write(EguiInputEvent { context, event });
            }
        } else if pressed
            && !held[3]
            && (!modifiers.ctrl || modifiers.alt)
            && let Some(text) = &event.text
        {
            let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
            if !text.is_empty() {
                output.write(EguiInputEvent {
                    context,
                    event: egui::Event::Text(text),
                });
            }
        }
        if let Some(key) = key {
            output.write(EguiInputEvent {
                context,
                event: egui::Event::Key {
                    key,
                    physical_key: bevy_egui::helpers::bevy_to_egui_physical_key(&event.key_code),
                    pressed,
                    repeat: event.repeat,
                    modifiers,
                },
            });
        }
    }
}

/// Losing the host focus must release egui focus as well as game-input capture.
#[allow(clippy::needless_pass_by_value)]
fn focus(
    state: Res<State>,
    mut contexts: Query<(&mut EguiContext, &mut EguiInput), With<EditorCamera>>,
) {
    for (mut context, mut input) in &mut contexts {
        input.focused = state.focused;
        if !state.focused {
            context.get_mut().memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{ecs::message::MessageCursor, input::ButtonState};

    fn event(key: Key, state: ButtonState, text: Option<&str>) -> KeyboardInput {
        KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: key,
            state,
            text: text.map(Into::into),
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    #[test]
    fn hiding_the_editor_cancels_a_captured_drag_and_native_focus() {
        let mut app = App::new();
        app.init_resource::<State>()
            .init_resource::<Workbench>()
            .init_resource::<Shown>()
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<Key>>()
            .add_message::<WindowEvent>()
            .add_message::<EguiInputEvent>()
            .add_systems(Update, pointer);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app.world_mut().spawn(EditorCamera);
        let host = app
            .world_mut()
            .spawn((
                Surface,
                Node::default(),
                ComputedNode {
                    size: Vec2::splat(100.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(host, FocusCause::Pressed);
        {
            let mut state = app.world_mut().resource_mut::<State>();
            state.focused = true;
            state.pointer.captured = true;
            state.pointer.hovered = true;
            state.pointer.position = Some(Vec2::ZERO);
        }
        app.update();
        let state = app.world().resource::<State>();
        assert!(!state.focused && !state.pointer.captured && !state.pointer.hovered);
        assert!(app.world().resource::<InputFocus>().get().is_none());
        let mut cursor = MessageCursor::<EguiInputEvent>::default();
        let events: Vec<_> = cursor
            .read(app.world().resource::<Messages<EguiInputEvent>>())
            .map(|event| &event.event)
            .collect();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, egui::Event::PointerButton { pressed: false, .. }))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, egui::Event::PointerGone))
        );
    }

    #[test]
    fn rapid_shortcuts_keep_modifiers_and_unfocused_input_is_ignored() {
        let mut app = App::new();
        app.init_resource::<State>()
            .init_resource::<EguiClipboard>()
            .init_resource::<ButtonInput<Key>>()
            .add_message::<KeyboardInput>()
            .add_message::<EguiInputEvent>()
            .add_systems(Update, keyboard);
        app.world_mut().spawn(EditorCamera);
        app.world_mut().resource_mut::<State>().focused = true;
        for event in [
            event(Key::Control, ButtonState::Pressed, None),
            event(Key::Character("a".into()), ButtonState::Pressed, Some("a")),
            event(Key::Control, ButtonState::Released, None),
            event(Key::Character("w".into()), ButtonState::Pressed, Some("w")),
        ] {
            app.world_mut().write_message(event);
        }
        app.update();
        let mut cursor = MessageCursor::<EguiInputEvent>::default();
        let events: Vec<_> = cursor
            .read(app.world().resource::<Messages<EguiInputEvent>>())
            .map(|event| &event.event)
            .collect();
        assert!(events.iter().any(|event| matches!(event, egui::Event::Key { key: egui::Key::A, modifiers, .. } if modifiers.ctrl)));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, egui::Event::Text(text) if text == "a"))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, egui::Event::Text(text) if text == "w"))
        );
        app.world_mut().resource_mut::<State>().focused = false;
        app.world_mut().write_message(event(
            Key::Character("x".into()),
            ButtonState::Pressed,
            Some("x"),
        ));
        app.update();
        assert_eq!(
            cursor
                .read(app.world().resource::<Messages<EguiInputEvent>>())
                .count(),
            0
        );
    }
}
