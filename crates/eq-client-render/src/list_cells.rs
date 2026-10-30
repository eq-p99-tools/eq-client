//! Shared text cells for the skin's skills, training and merchant lists.
use crate::theme::{self, Size};
use bevy::prelude::*;

/// Appends non-wrapping cells using the skin's column widths. Row behavior and
/// optional icon columns stay with the feature that owns the list.
pub(crate) fn text(
    commands: &mut Commands,
    row: Entity,
    cells: impl IntoIterator<Item = String>,
    widths: impl IntoIterator<Item = f32>,
) {
    for (words, width) in cells.into_iter().zip(widths) {
        commands.spawn((
            theme::text(words, Size::Small, theme::INK_BRIGHT),
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
            Node {
                width: px(width),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                ..default()
            },
            ChildOf(row),
        ));
    }
}
