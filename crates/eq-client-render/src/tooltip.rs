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

/// Each control the pointer may rest on: whether it does, what it says,
/// what it needs and where it is.
type Controls<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        Option<&'static Tooltip>,
        Option<&'static super::outbox::Needs>,
        Option<(&'static ComputedNode, &'static UiGlobalTransform)>,
    ),
>;

/// Shows the hovered control's tooltip beside the pointer; a control the
/// session does not offer says so instead. A control hovered with no pointer
/// over the window, as a script hovers one, has it beside its middle.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    (online, options): (
        Res<super::online::OnlineState>,
        Res<super::options::OptionsState>,
    ),
    windows: Query<&Window, With<PrimaryWindow>>,
    scale: Option<Res<UiScale>>,
    controls: Controls,
    mut boxes: Query<(&mut Text, &mut Node), With<TooltipBox>>,
) {
    let Ok((mut text, mut node)) = boxes.single_mut() else {
        return;
    };
    let factor = scale.as_ref().map_or(1.0, |scale| scale.0);
    let greys = super::outbox::greys(&options);
    let hovered = controls
        .iter()
        .filter(|(interaction, ..)| **interaction != Interaction::None)
        .find_map(|(_, tooltip, needs, placed)| {
            let line = match needs {
                Some(needs) if greys && !needs.offered(online.world()) => {
                    Some(needs.reason(online.world()))
                }
                _ => tooltip.map(|tooltip| tooltip.0.clone()),
            }?;
            let middle =
                placed.map(|(computed, at)| at.translation * computed.inverse_scale_factor());
            Some((line, middle))
        })
        .filter(|(line, _)| !line.is_empty());
    // Where the pointer is and how big the window is, as the UI lays out.
    let window = windows.single().ok();
    let pointer = window
        .and_then(Window::cursor_position)
        .map(|at| at / factor)
        .or_else(|| hovered.as_ref().and_then(|(_, middle)| *middle));
    let (Some((line, _)), Some(pointer), Some(window)) = (hovered, pointer, window) else {
        if node.display != Display::None {
            node.display = Display::None;
        }
        return;
    };
    let viewport = window.size() / factor;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_hovered_without_a_pointer_gives_its_reason_beside_its_middle() {
        let mut app = crate::testing::app();
        app.add_systems(Startup, spawn).add_systems(Update, show);
        app.world_mut().spawn((
            Node::default(),
            Interaction::Hovered,
            crate::outbox::Needs::Missing,
            UiGlobalTransform::from_xy(100.0, 50.0),
        ));
        app.update();
        let mut boxes = app
            .world_mut()
            .query_filtered::<(&Text, &Node), With<TooltipBox>>();
        let (text, node) = boxes.single(app.world()).unwrap();
        assert_eq!(text.0, crate::outbox::MISSING);
        assert_eq!(node.display, Display::Flex);
        assert_eq!((node.left, node.top), (px(114), px(68)));
    }
}
