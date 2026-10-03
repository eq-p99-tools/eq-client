//! The skin's frames drawn from pieces (`FrameTemplate`): around a tab, a
//! page, or a list's column heading. Each piece is anchored to the edges of
//! the node it is drawn in, so the frame fits the node at whatever size the
//! layout gives it, as a tab is as wide as its words.
use super::to_f32;
use bevy::prelude::*;
use eq_client_assets::sidl::{FrameLook, Piece};

/// How far in from each edge of its node a frame reaches.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Insets {
    pub(super) left: f32,
    pub(super) top: f32,
    pub(super) right: f32,
    pub(super) bottom: f32,
}

/// A piece's size; one with no area draws nothing and takes no room, as
/// the Velious skin leaves its tab frames.
fn size(piece: Option<&Piece>) -> (f32, f32) {
    piece
        .filter(|piece| piece.width > 0 && piece.height > 0)
        .map_or((0.0, 0.0), |piece| {
            (to_f32(piece.width), to_f32(piece.height))
        })
}

/// A node anchored to its parent's edges: each inset in pixels where it is
/// anchored to that edge, and its size where it has one of its own.
fn anchored(
    [left, top, right, bottom]: [Option<f32>; 4],
    [width, height]: [Option<f32>; 2],
) -> Node {
    let edge = |inset: Option<f32>| inset.map_or(Val::Auto, px);
    Node {
        position_type: PositionType::Absolute,
        left: edge(left),
        top: edge(top),
        right: edge(right),
        bottom: edge(bottom),
        width: edge(width),
        height: edge(height),
        ..default()
    }
}

/// How far in from each edge of its node a frame reaches: as far as its
/// widest pieces on each side.
pub(super) fn insets(look: &FrameLook) -> Insets {
    let width = |piece: &Option<Piece>| size(piece.as_ref()).0;
    let height = |piece: &Option<Piece>| size(piece.as_ref()).1;
    Insets {
        left: width(&look.left_top)
            .max(width(&look.left))
            .max(width(&look.left_bottom)),
        top: height(&look.top_left)
            .max(height(&look.top))
            .max(height(&look.top_right)),
        right: width(&look.right_top)
            .max(width(&look.right))
            .max(width(&look.right_bottom)),
        bottom: height(&look.bottom_left)
            .max(height(&look.bottom))
            .max(height(&look.bottom_right)),
    }
}

/// Draws a frame's pieces around the node they are spawned in: corners at
/// its corners, edges stretched between them, and each side's end caps.
#[allow(clippy::too_many_lines)] // One table of the frame's twelve pieces.
pub(super) fn around(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    look: &FrameLook,
) -> Insets {
    let (top_left_width, top_left_height) = size(look.top_left.as_ref());
    let (top_right_width, top_right_height) = size(look.top_right.as_ref());
    let (_, top_height) = size(look.top.as_ref());
    let (bottom_left_width, bottom_left_height) = size(look.bottom_left.as_ref());
    let (bottom_right_width, bottom_right_height) = size(look.bottom_right.as_ref());
    let (_, bottom_height) = size(look.bottom.as_ref());
    let (left_top_width, left_top_height) = size(look.left_top.as_ref());
    let (left_width, _) = size(look.left.as_ref());
    let (left_bottom_width, left_bottom_height) = size(look.left_bottom.as_ref());
    let (right_top_width, right_top_height) = size(look.right_top.as_ref());
    let (right_width, _) = size(look.right.as_ref());
    let (right_bottom_width, right_bottom_height) = size(look.right_bottom.as_ref());
    let insets = insets(look);
    let Insets { top, bottom, .. } = insets;
    for (piece, node) in [
        (
            &look.top_left,
            anchored(
                [Some(0.0), Some(0.0), None, None],
                [Some(top_left_width), Some(top_left_height)],
            ),
        ),
        (
            &look.top,
            anchored(
                [Some(top_left_width), Some(0.0), Some(top_right_width), None],
                [None, Some(top_height)],
            ),
        ),
        (
            &look.top_right,
            anchored(
                [None, Some(0.0), Some(0.0), None],
                [Some(top_right_width), Some(top_right_height)],
            ),
        ),
        (
            &look.left_top,
            anchored(
                [Some(0.0), Some(top), None, None],
                [Some(left_top_width), Some(left_top_height)],
            ),
        ),
        (
            &look.left,
            anchored(
                [
                    Some(0.0),
                    Some(top + left_top_height),
                    None,
                    Some(bottom + left_bottom_height),
                ],
                [Some(left_width), None],
            ),
        ),
        (
            &look.left_bottom,
            anchored(
                [Some(0.0), None, None, Some(bottom)],
                [Some(left_bottom_width), Some(left_bottom_height)],
            ),
        ),
        (
            &look.right_top,
            anchored(
                [None, Some(top), Some(0.0), None],
                [Some(right_top_width), Some(right_top_height)],
            ),
        ),
        (
            &look.right,
            anchored(
                [
                    None,
                    Some(top + right_top_height),
                    Some(0.0),
                    Some(bottom + right_bottom_height),
                ],
                [Some(right_width), None],
            ),
        ),
        (
            &look.right_bottom,
            anchored(
                [None, None, Some(0.0), Some(bottom)],
                [Some(right_bottom_width), Some(right_bottom_height)],
            ),
        ),
        (
            &look.bottom_left,
            anchored(
                [Some(0.0), None, None, Some(0.0)],
                [Some(bottom_left_width), Some(bottom_left_height)],
            ),
        ),
        (
            &look.bottom,
            anchored(
                [
                    Some(bottom_left_width),
                    None,
                    Some(bottom_right_width),
                    Some(0.0),
                ],
                [None, Some(bottom_height)],
            ),
        ),
        (
            &look.bottom_right,
            anchored(
                [None, None, Some(0.0), Some(0.0)],
                [Some(bottom_right_width), Some(bottom_right_height)],
            ),
        ),
    ] {
        draw(parent, art, piece.as_ref(), node);
    }
    insets
}

/// How high a list's column headings are drawn, where their frame gives
/// them a height.
pub(super) fn heading_height(look: &FrameLook) -> Option<f32> {
    let height = |piece: &Option<Piece>| size(piece.as_ref()).1;
    let height = height(&look.left)
        .max(height(&look.middle))
        .max(height(&look.right));
    (height > 0.0).then_some(height)
}

/// Draws a list's column heading's frame across the node it is spawned in:
/// its left end, its middle, stretched, and its right end.
pub(super) fn heading(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    look: &FrameLook,
) {
    let (left_width, left_height) = size(look.left.as_ref());
    let (right_width, right_height) = size(look.right.as_ref());
    let (_, middle_height) = size(look.middle.as_ref());
    for (piece, node) in [
        (
            &look.left,
            anchored(
                [Some(0.0), Some(0.0), None, None],
                [Some(left_width), Some(left_height)],
            ),
        ),
        (
            &look.middle,
            anchored(
                [Some(left_width), Some(0.0), Some(right_width), None],
                [None, Some(middle_height)],
            ),
        ),
        (
            &look.right,
            anchored(
                [None, Some(0.0), Some(0.0), None],
                [Some(right_width), Some(right_height)],
            ),
        ),
    ] {
        draw(parent, art, piece.as_ref(), node);
    }
}

/// A piece with an area, stretched over its node.
fn draw(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    piece: Option<&Piece>,
    node: Node,
) {
    if let Some(image) = piece
        .filter(|piece| piece.width > 0 && piece.height > 0)
        .and_then(|piece| art.cut(piece))
    {
        parent.spawn((image, node));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn a_frame_reaches_in_as_far_as_its_widest_pieces_and_empty_ones_take_no_room() {
        let piece = |width, height| Piece {
            texture: "frame.tga".into(),
            x: 0,
            y: 0,
            width,
            height,
        };
        let look = FrameLook {
            top_left: Some(piece(10, 4)),
            top: Some(piece(2, 4)),
            left_top: Some(piece(4, 8)),
            left: Some(piece(4, 2)),
            // As the Velious skin draws a page's left side: no height.
            right: Some(piece(9, 0)),
            ..FrameLook::default()
        };
        let mut app = App::new();
        app.init_resource::<crate::sheets::Sheets>()
            .init_resource::<Assets<Image>>()
            .init_resource::<super::super::super::skin::UiSkin>()
            .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()));
        let insets = app
            .world_mut()
            .run_system_once(move |mut commands: Commands, mut art: crate::sheets::Art| {
                let mut insets = Insets::default();
                commands.spawn(Node::default()).with_children(|node| {
                    insets = around(node, &mut art, &look);
                });
                insets
            })
            .unwrap();
        assert_eq!(
            insets,
            Insets {
                left: 4.0,
                top: 4.0,
                right: 0.0,
                bottom: 0.0
            }
        );
    }
}
