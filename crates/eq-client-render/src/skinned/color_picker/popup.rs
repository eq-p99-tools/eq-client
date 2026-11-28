//! One transient editor, anchored to the active swatch in screen space so scrolling cannot clip it.
use super::{Binding, ScriptChannel, SkinSlider, at, draw, height};
use crate::{
    options::OptionsState,
    profile_files::{CharacterProfile, Profile},
    theme,
    windows::{Shown, WindowId},
};
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition},
    window::PrimaryWindow,
};

#[derive(Component)]
pub(crate) struct Swatch {
    binding: Binding,
    font: theme::Size,
    owner: WindowId,
}

pub(crate) fn draw_swatch(
    ui: &mut ChildSpawnerCommands,
    owner: WindowId,
    binding: Binding,
    node: Node,
    font: theme::Size,
) {
    let mut field = ui.spawn((
        Button,
        Swatch {
            binding,
            font,
            owner,
        },
        Node {
            border: UiRect::all(px(1)),
            ..node
        },
        BorderColor::all(theme::EDGE_LIGHT),
        BackgroundColor(Color::BLACK),
    ));
    if let Binding::Combat(levels) = binding {
        // Script slider selectors keep their existing targets. Physical input reaches the parent swatch.
        field.with_children(|children| {
            for (i, level) in [0.0, 1.0, 2.0].into_iter().zip(levels) {
                children.spawn((
                    ScriptChannel,
                    Button,
                    FocusPolicy::Pass,
                    at(i * 5.0, 0.0, 5.0, 16.0),
                    SkinSlider::channel(level),
                    RelativeCursorPosition::default(),
                ));
            }
        });
    }
}

#[derive(Component)]
pub(crate) struct ResetColors;

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn show_swatches(
    reset: crate::clicks::Clicks<ResetColors>,
    mut state: ResMut<OptionsState>,
    look: Option<Res<crate::chat::ChatLook>>,
    mut colors: Query<(&Swatch, &mut BackgroundColor)>,
) {
    if reset.any() {
        let defaults = eq_client_core::options::Options::default();
        state.options.user_colors.fill(None);
        state.options.combat_text_incoming = defaults.combat_text_incoming;
        state.options.combat_text_outgoing = defaults.combat_text_outgoing;
    }
    for (swatch, mut bg) in &mut colors {
        bg.set_if_neq(BackgroundColor(
            swatch.binding.read(&state.options, look.as_deref()).into(),
        ));
    }
}

pub(crate) struct Open {
    anchor: Entity,
    panel: Entity,
    profile: Option<CharacterProfile>,
}

type Anchors<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Swatch,
        &'static Interaction,
        &'static ComputedNode,
        &'static UiGlobalTransform,
        &'static InheritedVisibility,
        Option<&'static CalculatedClip>,
    ),
>;

#[derive(SystemParam)]
pub(crate) struct Inputs<'w, 's> {
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    anchors: Anchors<'w, 's>,
    shown: Res<'w, Shown>,
    profile: Res<'w, Profile>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    keys: Res<'w, ButtonInput<KeyCode>>,
}

const FIELD_WIDTH: f32 = 108.0;
const PAD: f32 = 5.0;

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn manage(
    mut commands: Commands,
    mut open: Local<Option<Open>>,
    input: Inputs,
    mut typing: ResMut<crate::keys::Typing>,
    mut panels: Query<(&mut Node, &ComputedNode, &UiGlobalTransform), Without<Swatch>>,
) {
    let window = input.windows.single().ok().filter(|w| w.focused);
    let left = input.mouse.just_pressed(MouseButton::Left);
    let clicked = left || input.mouse.just_pressed(MouseButton::Right);
    let trigger = window.and_then(|_| {
        left.then(|| {
            input.anchors.iter().find(|(_, swatch, i, ..)| {
                input.shown.displayed(swatch.owner) && **i == Interaction::Pressed
            })
        })
        .flatten()
    });
    let previous = open.as_ref().map(|o| o.anchor);
    if let Some(active) = open.as_ref() {
        let anchor = input.anchors.get(active.anchor).ok();
        let escaped = input.keys.just_pressed(KeyCode::Escape);
        let mut keep = false;
        if let (
            Some(w),
            Some((_, swatch, _, node, transform, vis, clip)),
            Ok((mut panel, pn, pt)),
        ) = (window, anchor, panels.get_mut(active.panel))
        {
            let inside = w
                .physical_cursor_position()
                .is_some_and(|p| crate::windows::contains(p, pt, pn));
            keep = input.shown.displayed(swatch.owner)
                && vis.get()
                && node.size().min_element() > 0.0
                && clip.is_none_or(|c| c.clip.contains(transform.translation))
                && active.profile.as_ref() == input.profile.key_ref()
                && !escaped
                && (!clicked || (inside && trigger.is_none()));
            if keep {
                let at = position(w, node, transform);
                if panel.left != px(at.x) || panel.top != px(at.y) {
                    panel.left = px(at.x);
                    panel.top = px(at.y);
                }
            }
        }
        if keep {
            return;
        }
        if escaped {
            typing.escape_consumed = true;
        }
        commands.entity(active.panel).try_despawn();
        *open = None;
    }
    if let (Some(w), Some((id, swatch, _, node, transform, ..))) = (window, trigger)
        && Some(id) != previous
    {
        let p = position(w, node, transform);
        let panel = commands
            .spawn((
                Button,
                GlobalZIndex(150),
                BackgroundColor(theme::PANEL),
                at(
                    p.x,
                    p.y,
                    FIELD_WIDTH + 2.0 * PAD,
                    height(FIELD_WIDTH) + 2.0 * PAD,
                ),
            ))
            .with_children(|ui| {
                draw(
                    ui,
                    swatch.binding,
                    at(PAD, PAD, FIELD_WIDTH, height(FIELD_WIDTH)),
                    FIELD_WIDTH,
                    swatch.font,
                );
            })
            .id();
        *open = Some(Open {
            anchor: id,
            panel,
            profile: input.profile.key(),
        });
    }
}

fn position(window: &Window, node: &ComputedNode, transform: &UiGlobalTransform) -> Vec2 {
    let rect = crate::windows::logical_rect(node, transform);
    let screen = window.physical_size().as_vec2() * node.inverse_scale_factor();
    let size = Vec2::new(FIELD_WIDTH + 2.0 * PAD, height(FIELD_WIDTH) + 2.0 * PAD);
    let y = if rect.max.y + size.y <= screen.y {
        rect.max.y
    } else {
        rect.min.y - size.y
    };
    Vec2::new(rect.min.x, y).clamp(Vec2::ZERO, (screen - size).max(Vec2::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_restores_combat_defaults_and_removes_only_the_chat_overrides() {
        let mut app = App::new();
        crate::clicks::testing::install(&mut app);
        let defaults = eq_client_core::options::Options::default();
        let mut custom = defaults;
        custom.user_colors[2] = Some([18, 64, 221]);
        custom.combat_text_outgoing = [0, 60, 15];
        custom.combat_text_incoming = [80, 0, 0];
        custom.chat_font_size = Some(7);
        app.insert_resource(OptionsState::new(&custom))
            .add_systems(Update, show_swatches);
        let button = app.world_mut().spawn((ResetColors, Interaction::None)).id();
        crate::clicks::testing::click(&mut app, button);
        let saved = &app.world().resource::<OptionsState>().options;
        assert_eq!(saved.user_colors, defaults.user_colors);
        assert_eq!(saved.combat_text_outgoing, defaults.combat_text_outgoing);
        assert_eq!(saved.combat_text_incoming, defaults.combat_text_incoming);
        assert_eq!(saved.chat_font_size, Some(7));
    }
}
