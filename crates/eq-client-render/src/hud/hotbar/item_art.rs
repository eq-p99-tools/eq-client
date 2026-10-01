//! Item shortcut artwork shares the inventory's installed-asset loader.
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct Artwork {
    slot: usize,
    shown: Option<u32>,
}

pub(super) fn artwork(slot: usize) -> impl Bundle {
    (
        Artwork { slot, shown: None },
        ImageNode::default(),
        Node {
            position_type: PositionType::Absolute,
            left: px(4),
            top: px(4),
            width: px(30),
            height: px(30),
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
    online: Res<crate::online::OnlineState>,
    mut art: crate::sheets::Art,
    mut artwork: Query<(&mut Artwork, &mut ImageNode, &mut Node)>,
) {
    for (mut shown, mut image, mut node) in &mut artwork {
        let icon = match bindings.0[shown.slot] {
            Some(super::Action::Item { slot, id }) => {
                super::bound_item(online.world.inventory(), slot, id).map(|item| item.icon)
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
