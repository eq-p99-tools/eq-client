//! The one floating tooltip: the text of the control under the pointer,
//! beside the pointer. A greyed-out control says why it is greyed out.
use crate::theme::{self, Size};
use bevy::{prelude::*, window::PrimaryWindow};

/// What a control says when the pointer rests on it.
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct Tooltip(pub String);

/// The floating box the tooltip shows in.
#[derive(Component)]
pub(crate) struct TooltipBox;

/// Spawns the hidden tooltip box, above every window.
pub(crate) fn spawn(mut commands: Commands) {
    commands.spawn((
        TooltipBox,
        Text::new(""),
        theme::font(Size::Body),
        TextColor(theme::INK_WARM),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(px(6), px(3)),
            max_width: px(320),
            display: Display::None,
            ..default()
        },
        BackgroundColor(theme::SCRIM),
        GlobalZIndex(1000),
        bevy::ui::FocusPolicy::Pass,
    ));
}

/// Shows the hovered control's tooltip beside the pointer; a control the
/// session does not offer says so instead.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    online: Res<super::online::OnlineState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    scale: Option<Res<UiScale>>,
    controls: Query<(
        &Interaction,
        Option<&Tooltip>,
        Option<&super::outbox::Needs>,
    )>,
    mut boxes: Query<(&mut Text, &mut Node), With<TooltipBox>>,
) {
    let Ok((mut text, mut node)) = boxes.single_mut() else {
        return;
    };
    let pointer = windows
        .single()
        .ok()
        .and_then(|window| window.cursor_position().map(|at| (at, window.size())));
    let hovered = controls
        .iter()
        .filter(|(interaction, ..)| **interaction != Interaction::None)
        .find_map(|(_, tooltip, needs)| match needs {
            Some(super::outbox::Needs(capability))
                if !super::outbox::offered(online.world(), *capability) =>
            {
                Some(super::outbox::UNAVAILABLE.to_owned())
            }
            _ => tooltip.map(|tooltip| tooltip.0.clone()),
        })
        .filter(|line| !line.is_empty());
    let (Some(line), Some((pointer, viewport))) = (hovered, pointer) else {
        if node.display != Display::None {
            node.display = Display::None;
        }
        return;
    };
    let factor = scale.as_ref().map_or(1.0, |scale| scale.0);
    let (pointer, viewport) = (pointer / factor, viewport / factor);
    // Below and right of the pointer, flipped near the window's edges.
    let flip_x = pointer.x > viewport.x - 240.0;
    let flip_y = pointer.y > viewport.y - 60.0;
    node.left = if flip_x {
        Val::Auto
    } else {
        px(pointer.x + 14.0)
    };
    node.right = if flip_x {
        px(viewport.x - pointer.x + 8.0)
    } else {
        Val::Auto
    };
    node.top = if flip_y {
        Val::Auto
    } else {
        px(pointer.y + 18.0)
    };
    node.bottom = if flip_y {
        px(viewport.y - pointer.y + 8.0)
    } else {
        Val::Auto
    };
    node.display = Display::Flex;
    if text.0 != line {
        text.0 = line;
    }
}
