//! Item shortcut artwork shares the inventory's installed-asset loader.
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct Artwork {
    slot: usize,
    shown: Option<u32>,
}

/// An item's picture at this place in its button: its left, top and size.
pub(super) fn artwork(slot: usize, (left, top, size): (f32, f32, f32)) -> impl Bundle {
    (
        Artwork { slot, shown: None },
        ImageNode::default(),
        Node {
            position_type: PositionType::Absolute,
            left: px(left),
            top: px(top),
            width: px(size),
            height: px(size),
            display: Display::None,
            ..default()
        },
        bevy::ui::FocusPolicy::Pass,
    )
}

/// Removes obsolete spell/item art as bindings or inventory contents change.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn update(
    bindings: Res<super::Bindings>,
    (online, options): (
        Res<crate::online::OnlineState>,
        Res<crate::options::OptionsState>,
    ),
    mut art: crate::sheets::Art,
    mut artwork: Query<(&mut Artwork, &mut ImageNode, &mut Node)>,
) {
    for (mut shown, mut image, mut node) in &mut artwork {
        let icon = match bindings.0[shown.slot] {
            Some(super::Action::Item { slot, id }) => {
                super::bound_item(online.world().inventory(), (slot, id), &options.options.qol)
                    .and_then(|item| item.details.icon)
            }
            _ => None,
        };
        if shown.shown == icon {
            continue;
        }
        shown.shown = icon;
        node.display = Display::None;
        if let Some(icon) = icon
            && let Some(next) = art.item(icon)
        {
            *image = next;
            node.display = Display::Flex;
        }
    }
}
